// audience: internal
// # personal-service-cn-reference-active-mission
//
// 该模块从载入响应中移除 CN 主表没有收录的主动任务, 并领取主动任务阶段奖励和保存领取记录.

use super::common::{error_response, json_document, required_i64};
use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_mission::{awake_mission_stage_rewards, apply_reward_response, settle_rewards, MissionReward};
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, player_snapshot,
    require_object, require_root,
};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;

const ACTIVE_MISSION_REWARDS: &str = include_str!("../../assets/cn-active-mission-rewards.json");
static ACTIVE_MISSION_DOCUMENT: OnceLock<Result<Value, String>> = OnceLock::new();

#[derive(Deserialize)]
struct ActiveMissionReceiveRequest {
    viewer_id: i64,
    #[serde(default)]
    active_mission_list: Vec<ActiveMissionReceiveEntry>,
}

#[derive(Deserialize)]
struct ActiveMissionReceiveEntry {
    mission_id: i64,
    #[serde(default)]
    stages: Vec<i64>,
}

// //// 移除载入响应中的未知主动任务 [@x380kkm 2026-08-28] ////
pub(crate) fn remove_unknown_active_missions_from_load(
    player_data: &mut Value,
) -> Result<(), PersonalServiceError> {
    let document = json_document(
        &ACTIVE_MISSION_DOCUMENT,
        ACTIVE_MISSION_REWARDS,
        "active mission rewards",
    )?;
    let missions = player_data
        .get_mut("all_active_mission_list")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN active mission list is missing"))?;
    missions.retain(|mission_id, _| {
        mission_id
            .parse::<i64>()
            .ok()
            .is_some_and(|mission_id| document.get(mission_id.to_string()).is_some())
    });
    Ok(())
}
// //// /移除载入响应中的未知主动任务 ////

// //// 领取主动任务阶段奖励 [@x380kkm 2026-08-22] ////
pub(super) fn receive(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ActiveMissionReceiveRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.active_mission_list.iter().all(|entry| {
                    entry.mission_id > 0 && entry.stages.iter().all(|stage| *stage > 0)
                }) =>
        {
            body
        }
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let mana_before = crate::cn_mission::total_mana(root)?;
    let items_before = crate::cn_mission::capture_collect_item_counts(root)?;
    let mut result_list = Vec::with_capacity(body.active_mission_list.len());
    let mut claimed_rewards = Vec::new();
    let response_time = server_time(database)?;

    for entry in body.active_mission_list {
        let mut response_stages = Vec::with_capacity(entry.stages.len());
        let mut response_progress = active_mission_progress(root, entry.mission_id);
        for stage in entry.stages {
            let receipt_key = format!("{}:{stage}", entry.mission_id);
            let explicit_receipt = root.get("active_mission_receipts").and_then(|receipts| receipts.get(&receipt_key)).is_some();
            let awake = crate::cn_mission::awake_mission_progress(root, entry.mission_id, stage)?;
            let already_received = explicit_receipt
                || awake.is_some() && crate::cn_mission::awake_stage_received(root, entry.mission_id, stage)
                || active_mission_stage_received(root, entry.mission_id, stage);
            let rewards = match awake {
                Some(_) => awake_mission_stage_rewards(entry.mission_id, stage)?.to_vec(),
                None => active_mission_rewards(entry.mission_id, stage)?,
            };
            let (progress, target) = match awake {
                Some(state) => state,
                None => (active_mission_progress(root, entry.mission_id), active_mission_target(entry.mission_id, stage)?.unwrap_or(i64::MAX)),
            };
            response_progress = response_progress.max(progress);
            if !already_received && (progress < target || rewards.is_empty()) {
                return Ok(error_response("400 Bad Request", "mission_not_completed"));
            }
            if explicit_receipt || !already_received {
                let recorded = rewards.iter().filter(|reward| reward.kind != 0 && reward.kind != 6).cloned().collect::<Vec<_>>();
                if settle_rewards(root, "active_mission_receipts", &receipt_key, &rewards, &recorded, response_time)?.is_some() {
                    claimed_rewards.extend(rewards);
                }
            }
            mark_active_mission_stage_received(root, entry.mission_id, stage)?;
            response_stages.push(json!({"stage": stage, "received": true}));
        }
        result_list.push(json!({
            "mission_id": entry.mission_id,
            "progress_value": response_progress,
            "stages": response_stages,
        }));
    }

    let mut missions = crate::cn_mission::record_collected_items(root, database, snapshot.account_id, &items_before)?;
    let acquired_mana = crate::cn_mission::total_mana(root)?.saturating_sub(mana_before);
    missions.extend(crate::cn_mission::record_acquired_mana(root, database, snapshot.account_id, acquired_mana, response_time)?);
    let user_info = require_object(root, "user_info")?;
    let free_mana = required_i64(user_info, "free_mana")?;
    let exp_pool = required_i64(user_info, "exp_pool")?;
    let exp_pooled_time = required_i64(user_info, "exp_pooled_time")?;
    let mut response = json!({
            "active_mission_list": result_list,
            "mission_info": missions,
            "user_info": {
                "free_mana": free_mana,
                "exp_pool": exp_pool,
                "exp_pooled_time": exp_pooled_time,
            },
            "item_list": {},
            "mail_arrived": false,
        });
    apply_reward_response(&mut response, root, &claimed_rewards, body.viewer_id, response_time)?;
    crate::cn_mission::apply_mission_reward_response(&mut response, root, &missions, body.viewer_id, response_time)?;
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(body.viewer_id, false, response_time, response)
}
// //// /领取主动任务阶段奖励 ////



fn active_mission_rewards(
    mission_id: i64,
    stage: i64,
) -> Result<Vec<MissionReward>, PersonalServiceError> {
    let document = json_document(
        &ACTIVE_MISSION_DOCUMENT,
        ACTIVE_MISSION_REWARDS,
        "active mission rewards",
    )?;
    let Some(row) = document
        .get(mission_id.to_string())
        .and_then(|mission| mission.get(stage.to_string()))
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .and_then(Value::as_array)
    else {
        return Ok(Vec::new());
    };
    let mut rewards = Vec::new();
    for slot in 0..4 {
        let base = 7 + slot * 6;
        let Some(kind) = csv_i64(row.get(base)) else { continue; };
        let amount = if kind == 6 { 1 } else { csv_i64(row.get(base + 1)).unwrap_or_default() };
        if kind < 0 || amount <= 0 {
            continue;
        }
        rewards.push(MissionReward {
            kind,
            amount,
            item_id: csv_i64(row.get(base + 2)).filter(|value| *value > 0),
            character_id: csv_i64(row.get(base + 3)).filter(|value| *value > 0),
            equipment_id: csv_i64(row.get(base + 4)).filter(|value| *value > 0),
            degree_id: csv_i64(row.get(base + 5)).filter(|value| *value > 0),
            pass_card_event_id: None,
        });
    }
    Ok(rewards)
}

fn csv_i64(value: Option<&Value>) -> Option<i64> {
    value.and_then(|value| match value {
        Value::Number(number) => number.as_i64(),
        Value::String(text) if !text.is_empty() && text != "(None)" => text.parse().ok(),
        _ => None,
    })
}

fn active_mission_target(mission_id: i64, stage: i64) -> Result<Option<i64>, PersonalServiceError> {
    let document = json_document(&ACTIVE_MISSION_DOCUMENT, ACTIVE_MISSION_REWARDS, "active mission rewards")?;
    Ok(document.get(mission_id.to_string()).and_then(|mission| mission.get(stage.to_string()))
        .and_then(Value::as_array).and_then(|rows| rows.first()).and_then(Value::as_array)
        .and_then(|row| csv_i64(row.get(3))))
}

// //// 依据明确的主动任务凭据补齐货币奖励 [@x380kkm 2026-09-21] ////
pub(crate) fn recover_recorded_active_mission_rewards(root: &mut Map<String, Value>, response_time: i64) -> Result<(), PersonalServiceError> {
    let receipts = root.get("active_mission_receipts").and_then(Value::as_object).cloned().unwrap_or_default();
    for (key, value) in receipts {
        if value != Value::Bool(true) { continue; }
        let Some((mission_id, stage)) = key.split_once(':').and_then(|(mission, stage)| Some((mission.parse::<i64>().ok()?, stage.parse::<i64>().ok()?))) else { continue; };
        let awake = awake_mission_stage_rewards(mission_id, stage)?;
        let rewards = if awake.is_empty() { active_mission_rewards(mission_id, stage)? } else { awake.to_vec() };
        let recorded = rewards.iter().filter(|reward| reward.kind != 0 && reward.kind != 6).cloned().collect::<Vec<_>>();
        settle_rewards(root, "active_mission_receipts", &key, &rewards, &recorded, response_time)?;
    }
    Ok(())
}
// //// /依据明确的主动任务凭据补齐货币奖励 ////

fn active_mission_progress(root: &Map<String, Value>, mission_id: i64) -> i64 {
    root.get("all_active_mission_list")
        .and_then(Value::as_object)
        .and_then(|missions| missions.get(&mission_id.to_string()))
        .and_then(Value::as_object)
        .and_then(|mission| mission.get("progress"))
        .and_then(Value::as_i64)
        .or_else(|| {
            root.get("active_mission_list")
                .and_then(Value::as_array)
                .and_then(|missions| {
                    missions.iter().find(|mission| {
                        mission.get("mission_id").and_then(Value::as_i64) == Some(mission_id)
                    })
                })
                .and_then(|mission| {
                    mission
                        .get("progress_value")
                        .or_else(|| mission.get("progress"))
                })
                .and_then(Value::as_i64)
        })
        .unwrap_or_default()
}

pub(super) fn active_mission_stage_received(root: &Map<String, Value>, mission_id: i64, stage: i64) -> bool {
    let Some(stages) = root
        .get("all_active_mission_list")
        .and_then(Value::as_object)
        .and_then(|missions| missions.get(&mission_id.to_string()))
        .and_then(Value::as_object)
        .and_then(|mission| mission.get("stages"))
    else {
        return false;
    };
    match stages {
        Value::Object(stages) => stages
            .get(&stage.to_string())
            .and_then(Value::as_bool)
            .unwrap_or(false),
        Value::Array(stages) => stages.iter().any(|entry| {
            entry.get("stage").and_then(Value::as_i64) == Some(stage)
                && entry.get("received").and_then(Value::as_bool) == Some(true)
        }),
        _ => false,
    }
}

pub(super) fn qualifying_incentive_stage(mission_id: i64) -> Result<Option<i64>, PersonalServiceError> {
    let document = json_document(&ACTIVE_MISSION_DOCUMENT, ACTIVE_MISSION_REWARDS, "active mission rewards")?;
    Ok(document.get(mission_id.to_string()).and_then(Value::as_object).and_then(|stages| {
        stages.iter().find_map(|(stage, rows)| {
            let row = rows.as_array()?.first()?.as_array()?;
            (row.get(2)?.as_str()? == "true").then(|| stage.parse().ok()).flatten()
        })
    }))
}

// //// 保存主动任务阶段的领取状态 [@x380kkm 2026-08-28] ////
fn mark_active_mission_stage_received(
    root: &mut Map<String, Value>,
    mission_id: i64,
    stage: i64,
) -> Result<(), PersonalServiceError> {
    let Some(mission) = root
        .get_mut("all_active_mission_list")
        .and_then(Value::as_object_mut)
        .and_then(|missions| missions.get_mut(&mission_id.to_string()))
        .and_then(Value::as_object_mut)
    else {
        return Ok(());
    };
    let stages = mission
        .entry("stages".to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    match stages {
        Value::Object(stages) => {
            stages.insert(stage.to_string(), Value::Bool(true));
        }
        Value::Array(stages) => {
            let mut found = false;
            for entry in stages.iter_mut() {
                let Some(entry_object) = entry.as_object_mut() else {
                    continue;
                };
                if entry_object.get("stage").and_then(Value::as_i64) == Some(stage) {
                    entry_object.insert("received".to_owned(), Value::Bool(true));
                    found = true;
                    break;
                }
            }
            if !found {
                stages.push(json!({"stage": stage, "received": true}));
            }
        }
        _ => {
            return Err(PersonalServiceError::new(
                "stored CN active mission stages are invalid",
            ));
        }
    }
    Ok(())
}

// //// 验证主动任务领奖读取持久化进度 [@x380kkm 2026-08-28] ////
#[cfg(test)]
mod tests {
    use super::{
        active_mission_progress, active_mission_stage_received, mark_active_mission_stage_received,
    };
    use serde_json::json;
    use serde_json::Map;

    #[test]
    fn reads_progress_from_persisted_active_mission_map() {
        let root = Map::from_iter([
            (
                "all_active_mission_list".to_owned(),
                json!({"12010": {"progress": 4, "stages": {}}}),
            ),
            (
                "active_mission_list".to_owned(),
                json!([{"mission_id": 12010, "progress_value": 0, "stages": []}]),
            ),
        ]);

        assert_eq!(active_mission_progress(&root, 12010), 4);
    }

    #[test]
    fn persists_received_stage_in_active_mission_map() {
        let mut root = Map::from_iter([(
            "all_active_mission_list".to_owned(),
            json!({"12010": {"progress": 4, "stages": {"1": false}}}),
        )]);

        mark_active_mission_stage_received(&mut root, 12010, 1).unwrap();

        assert_eq!(
            root["all_active_mission_list"]["12010"]["stages"]["1"],
            true
        );
        assert!(active_mission_stage_received(&root, 12010, 1));
    }
}
