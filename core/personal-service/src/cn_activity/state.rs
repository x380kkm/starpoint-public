// audience: internal
// # personal-service-cn-activity-state
//
// 该模块将 CN 活动进度保存在玩家快照中, 并将活动战斗写入现有活跃战斗表.

use crate::cn_tutorial::{decode_player_data, encode_player_data, player_snapshot, require_root};
use crate::database::{ActiveSingleQuest, ServiceDatabase};
use crate::http::HttpResponse;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};

const ACTIVITY_STATE_KEY: &str = "cn_activity_state";
const CURRENT_EVENT_IDS_KEY: &str = "current_event_ids";
const EVENT_FAMILIES_KEY: &str = "event_families";

pub(super) struct ActivityPlayer {
    pub(super) account_id: i64,
    pub(super) data: Value,
}

// //// 读取活动玩家快照 [@x380kkm 2026-08-22] ////
pub(super) fn load_player(
    database: &ServiceDatabase,
    viewer_id: i64,
) -> Result<Result<ActivityPlayer, HttpResponse>, PersonalServiceError> {
    let snapshot = match player_snapshot(database, viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(Err(response)),
    };
    Ok(Ok(ActivityPlayer {
        account_id: snapshot.account_id,
        data: decode_player_data(&snapshot.data)?,
    }))
}
// //// /读取活动玩家快照 ////

impl ActivityPlayer {
    pub(super) fn root(&self) -> Result<&Map<String, Value>, PersonalServiceError> {
        self.data
            .as_object()
            .ok_or_else(|| PersonalServiceError::new("stored CN player data is not an object"))
    }

    pub(super) fn root_mut(&mut self) -> Result<&mut Map<String, Value>, PersonalServiceError> {
        require_root(&mut self.data)
    }

    pub(super) fn save(&self, database: &mut ServiceDatabase) -> Result<(), PersonalServiceError> {
        database.save_player_snapshot(self.account_id, &encode_player_data(&self.data)?)
    }

    pub(super) fn start_battle(
        &self,
        database: &mut ServiceDatabase,
        active_quest: &ActiveSingleQuest,
    ) -> Result<(), PersonalServiceError> {
        database.start_active_single_quest(
            self.account_id,
            &encode_player_data(&self.data)?,
            active_quest,
        )
    }
}

pub(super) fn current_event_id(root: &Map<String, Value>, family: &str) -> Option<i64> {
    root.get(ACTIVITY_STATE_KEY)
        .and_then(Value::as_object)
        .and_then(|state| state.get(CURRENT_EVENT_IDS_KEY))
        .and_then(Value::as_object)
        .and_then(|ids| ids.get(family))
        .and_then(Value::as_i64)
}

pub(super) fn managed_event_id(
    database: &ServiceDatabase,
    family: &str,
) -> Result<Option<i64>, PersonalServiceError> {
    let prefix = format!("{family}:");
    let mut event_ids = database
        .list_activity_schedules()?
        .into_iter()
        .filter_map(|schedule| {
            schedule
                .activity_id
                .strip_prefix(&prefix)
                .map(str::to_owned)
        })
        .filter_map(|event_id| event_id.parse::<i64>().ok())
        .filter(|event_id| *event_id > 0);
    let first = event_ids.next();
    Ok(if event_ids.next().is_none() {
        first
    } else {
        None
    })
}

pub(super) fn set_current_event(
    root: &mut Map<String, Value>,
    family: &str,
    event_id: i64,
) -> Result<(), PersonalServiceError> {
    let legacy = legacy_event_state(root, family, event_id);
    let activity = activity_state_mut(root)?;
    let current = required_object_mut(activity, CURRENT_EVENT_IDS_KEY)?;
    current.insert(family.to_owned(), Value::from(event_id));
    let event = event_state_mut(root, family, event_id)?;
    if !event
        .get("legacy_state_imported")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        if let Some(legacy) = legacy {
            merge_legacy_event_state(event, legacy);
        }
        event.insert("legacy_state_imported".to_owned(), Value::Bool(true));
    }
    Ok(())
}

pub(super) fn event_state<'a>(
    root: &'a Map<String, Value>,
    family: &str,
    event_id: i64,
) -> Option<&'a Map<String, Value>> {
    root.get(ACTIVITY_STATE_KEY)
        .and_then(Value::as_object)
        .and_then(|state| state.get(EVENT_FAMILIES_KEY))
        .and_then(Value::as_object)
        .and_then(|families| families.get(family))
        .and_then(Value::as_object)
        .and_then(|events| events.get(&event_id.to_string()))
        .and_then(Value::as_object)
}

pub(super) fn event_state_mut<'a>(
    root: &'a mut Map<String, Value>,
    family: &str,
    event_id: i64,
) -> Result<&'a mut Map<String, Value>, PersonalServiceError> {
    let activity = activity_state_mut(root)?;
    let families = required_object_mut(activity, EVENT_FAMILIES_KEY)?;
    let family_events = families
        .entry(family.to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN activity family state is invalid"))?;
    family_events
        .entry(event_id.to_string())
        .or_insert_with(default_event_state)
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN activity event state is invalid"))
}

pub(super) fn record_battle(
    event: &mut Map<String, Value>,
    quest_id: i64,
    party_id: i64,
    play_id: &str,
    started_at: i64,
) {
    event.insert(
        "last_battle".to_owned(),
        json!({
            "party_id": party_id,
            "play_id": play_id,
            "quest_id": quest_id,
            "started_at": started_at,
        }),
    );
}

pub(super) fn reset_progress(
    event: &mut Map<String, Value>,
    quest_type: i64,
    reset_target_id: Option<i64>,
    is_reset_after_target_round: bool,
) -> Result<(), PersonalServiceError> {
    match quest_type {
        1 => {
            if let Some(round) = reset_target_id {
                remove_rounds_at_or_after(event, "folder_played_party_list", round)?;
            } else {
                event.insert("active_folder_id".to_owned(), Value::Null);
                event.insert(
                    "folder_played_party_list".to_owned(),
                    Value::Object(Map::new()),
                );
            }
        }
        2 => {
            if let Some(round) = reset_target_id {
                if is_reset_after_target_round {
                    remove_rounds_at_or_after(event, "endless_played_party_list", round)?;
                } else {
                    required_object_mut(event, "endless_played_party_list")?
                        .remove(&round.to_string());
                }
                let next_round = smallest_missing_round(required_object_mut(
                    event,
                    "endless_played_party_list",
                )?);
                event.insert(
                    "endless_battle_next_round".to_owned(),
                    Value::from(next_round),
                );
            }
        }
        0 => {}
        _ => {
            return Err(PersonalServiceError::new(
                "CN activity reset type is invalid",
            ))
        }
    }
    Ok(())
}

// //// 读取既有玩家快照中的活动进度 [@x380kkm 2026-08-25] ////
fn legacy_event_state(
    root: &Map<String, Value>,
    family: &str,
    event_id: i64,
) -> Option<Map<String, Value>> {
    match family {
        "rush" => legacy_rush_event_state(root, event_id),
        "raid" => legacy_raid_event_state(root, event_id),
        "carnival" => legacy_carnival_event_state(root, event_id),
        _ => None,
    }
}

fn legacy_rush_event_state(root: &Map<String, Value>, event_id: i64) -> Option<Map<String, Value>> {
    let source = root
        .get("rush_event_progress")?
        .as_object()?
        .get(&event_id.to_string())?
        .as_object()?;
    let mut event = default_event_state().as_object()?.clone();
    for key in [
        "endless_battle_next_round",
        "endless_battle_max_round",
        "active_folder_id",
    ] {
        if let Some(value) = source.get(key) {
            event.insert(key.to_owned(), value.clone());
        }
    }
    if let Some(value) = source.get("best_elapsed_time_ms") {
        event.insert("endless_battle_max_round_time".to_owned(), value.clone());
    }
    if let Some(value) = legacy_party_list(source.get("rush_battle_played_party_list")) {
        event.insert("folder_played_party_list".to_owned(), value);
    }
    if let Some(value) = legacy_party_list(source.get("endless_battle_played_party_list")) {
        event.insert("endless_played_party_list".to_owned(), value);
    }
    Some(event)
}

fn legacy_raid_event_state(root: &Map<String, Value>, event_id: i64) -> Option<Map<String, Value>> {
    let source = root
        .get("raid_event_played_party_list")?
        .as_object()?
        .get(&event_id.to_string())?;
    let mut event = default_event_state().as_object()?.clone();
    event.insert(
        "folder_played_party_list".to_owned(),
        legacy_party_list(Some(source))?,
    );
    Some(event)
}

fn legacy_carnival_event_state(
    root: &Map<String, Value>,
    event_id: i64,
) -> Option<Map<String, Value>> {
    let source = root.get("carnival_event_records")?.as_object()?;
    let prefix = format!("{event_id}:");
    let records = source
        .iter()
        .filter_map(|(key, record)| {
            let folder_id = key.strip_prefix(&prefix)?.parse::<i64>().ok()?;
            let score = record.get("score")?.as_i64()?;
            Some(json!({
                "folder_id": folder_id,
                "best_score": score,
                "previous_score": score,
                "previous_character_ids": record.get("character_ids").cloned().unwrap_or_else(|| json!([null, null, null])),
                "previous_unison_character_ids": [null, null, null],
            }))
        })
        .collect::<Vec<_>>();
    if records.is_empty() {
        return None;
    }
    let mut event = default_event_state().as_object()?.clone();
    event.insert("carnival_records".to_owned(), Value::Array(records));
    Some(event)
}

fn legacy_party_list(value: Option<&Value>) -> Option<Value> {
    let parties = value?.as_object()?;
    Some(Value::Object(
        parties
            .iter()
            .map(|(key, party)| (key.clone(), legacy_party(party)))
            .collect(),
    ))
}

fn legacy_party(party: &Value) -> Value {
    if party.get("character_id_1").is_some() {
        return party.clone();
    }
    let character_ids = party
        .get("character_ids")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let character_id = |index: usize| character_ids.get(index).cloned().unwrap_or(Value::Null);
    json!({
        "character_id_1": character_id(0),
        "character_id_2": character_id(1),
        "character_id_3": character_id(2),
        "unison_character_id_1": null,
        "unison_character_id_2": null,
        "unison_character_id_3": null,
        "equipment_id_1": null,
        "equipment_id_2": null,
        "equipment_id_3": null,
        "ability_soul_id_1": null,
        "ability_soul_id_2": null,
        "ability_soul_id_3": null,
        "evolution_img_level_1": null,
        "evolution_img_level_2": null,
        "evolution_img_level_3": null,
        "unison_evolution_img_level_1": null,
        "unison_evolution_img_level_2": null,
        "unison_evolution_img_level_3": null,
    })
}

fn merge_legacy_event_state(event: &mut Map<String, Value>, legacy: Map<String, Value>) {
    let Some(defaults) = default_event_state().as_object().cloned() else {
        return;
    };
    for (key, value) in legacy {
        let current_is_default = event
            .get(&key)
            .zip(defaults.get(&key))
            .is_some_and(|(current, default)| current == default);
        if current_is_default {
            event.insert(key, value);
        }
    }
}
// //// /读取既有玩家快照中的活动进度 ////

fn activity_state_mut(
    root: &mut Map<String, Value>,
) -> Result<&mut Map<String, Value>, PersonalServiceError> {
    root.entry(ACTIVITY_STATE_KEY.to_owned())
        .or_insert_with(|| {
            json!({
                "current_event_ids": {},
                "event_families": {},
            })
        })
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN activity state is invalid"))
}

fn default_event_state() -> Value {
    json!({
        "active_folder_id": null,
        "carnival_records": [],
        "cleared_folder_id_list": [],
        "endless_battle_max_round": null,
        "endless_battle_max_round_time": null,
        "endless_battle_next_round": 1,
        "endless_played_party_list": {},
        "folder_played_party_list": {},
        "last_battle": null,
        "party_group_list": null,
        "ranking_reward_requested": false,
    })
}

fn remove_rounds_at_or_after(
    event: &mut Map<String, Value>,
    key: &str,
    first_removed_round: i64,
) -> Result<(), PersonalServiceError> {
    let parties = required_object_mut(event, key)?;
    parties.retain(|round, _| {
        round
            .parse::<i64>()
            .map(|round| round < first_removed_round)
            .unwrap_or(false)
    });
    Ok(())
}

fn smallest_missing_round(parties: &Map<String, Value>) -> i64 {
    let mut round = 1;
    while parties.contains_key(&round.to_string()) {
        round += 1;
    }
    round
}

fn required_object_mut<'a>(
    object: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>, PersonalServiceError> {
    object
        .entry(key.to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new(format!("stored CN activity {key} is invalid")))
}
