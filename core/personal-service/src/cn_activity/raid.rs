// audience: internal
// # personal-service-cn-raid-event
//
// 该模块实现 CN raid 活动入口, 队伍, 排行, folder 和战斗状态.

use super::{closed_activity_response, error_response, party, state};
use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::format_client_time;
use crate::database::{ActiveSingleQuest, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

const FAMILY: &str = "raid";
const QUEST_CATEGORY: i64 = 23;

#[derive(Deserialize)]
struct EventRequest {
    event_id: i64,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct OptionalEventRequest {
    event_id: Option<i64>,
    quest_id: Option<i64>,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct SelectFolderRequest {
    event_id: i64,
    folder_id: i64,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct BattleStartRequest {
    event_id: Option<i64>,
    is_auto_start_mode: bool,
    party_group_id: i64,
    play_id: String,
    quest_id: i64,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct ResetRequest {
    event_id: i64,
    is_reset_after_target_round: Option<bool>,
    quest_type: i64,
    reset_target_id: Option<i64>,
    viewer_id: i64,
}

pub(super) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let response = match request.path() {
        "/api/index.php/event/raid/summary" => summary(request, database, asset_root),
        "/api/index.php/event/raid/ranking_reward" => ranking_reward(request, database, asset_root),
        "/api/index.php/event/raid/party" => event_party(request, database, asset_root),
        "/api/index.php/event/raid/ranking" => ranking(request, database, asset_root),
        "/api/index.php/event/raid/ranking/party" => ranking_party(request, database, asset_root),
        "/api/index.php/event/raid/battle/start" => battle_start(request, database, asset_root),
        "/api/index.php/event/raid/select_folder" => select_folder(request, database, asset_root),
        "/api/index.php/event/raid/reset" => reset(request, database, asset_root),
        _ => return None,
    };
    Some(response)
}

// //// 返回 raid 活动摘要 [@x380kkm 2026-08-22] ////
fn summary(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<EventRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.event_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let mut player = match load_open_player(database, asset_root, body.viewer_id, body.event_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    let response_time = server_time(database)?;
    let root = player.root_mut()?;
    state::set_current_event(root, FAMILY, body.event_id)?;
    let event = state::event_state_mut(root, FAMILY, body.event_id)?;
    let next_round = event
        .get("endless_battle_next_round")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    let active_folder = event
        .get("active_folder_id")
        .cloned()
        .unwrap_or(Value::Null);
    let cleared_folders = event
        .get("cleared_folder_id_list")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let endless_parties = event
        .get("endless_played_party_list")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let folder_parties = event
        .get("folder_played_party_list")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let my_ranking = super::rush::player_ranking(root, FAMILY, body.event_id, 0);
    let boss = database.raid_boss_state(body.event_id)?;
    player.save(database)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        json!({
            "aggregated_time": format_client_time(response_time),
            "auto_start_point": 0,
            "kill_count_reward_data": { "received_up_to": 0, "reward_list": [] },
            "quest_list": {},
            "raid_boss": {
                "hp_percentage": boss.hp_percentage,
                "total_kill_count": boss.total_kill_count,
            },
            "endless_battle_next_round": next_round,
            "active_rush_battle_folder_id": active_folder,
            "endless_battle_played_max_round": next_round,
            "cleared_folder_id_list": cleared_folders,
            "endless_battle_played_party_list": endless_parties,
            "rush_battle_played_party_list": folder_parties,
            "endless_battle_my_ranking": my_ranking,
        }),
    )
}
// //// /返回 raid 活动摘要 ////

fn ranking_reward(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    _asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<OptionalEventRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let _player = match state::load_player(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({ "reward_list": [], "status": 0 }),
    )
}

fn event_party(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<OptionalEventRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let mut player = match state::load_player(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    let managed_event = state::managed_event_id(database, FAMILY)?;
    let event_id =
        resolve_event_id(&player, body.event_id, body.quest_id, managed_event).unwrap_or_default();
    if event_id > 0 {
        if let Some(response) =
            closed_activity_response(database, asset_root, &format!("{FAMILY}:{event_id}"))?
        {
            return Ok(response);
        }
    }
    let root = player.root_mut()?;
    if event_id > 0 {
        state::set_current_event(root, FAMILY, event_id)?;
    }
    let mut party_groups = party::raid_party_groups(root, FAMILY, event_id)?;
    complete_raid_party_groups(root, &mut party_groups)?;
    state::event_state_mut(root, FAMILY, event_id)?
        .insert("party_group_list".to_owned(), party_groups.clone());
    player.save(database)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({ "user_party_group_list": party_groups }),
    )
}

fn ranking(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let (body, mut player, event_id) = match load_optional_event(request, database, asset_root)? {
        Ok(context) => context,
        Err(response) => return Ok(response),
    };
    if event_id > 0 {
        state::set_current_event(player.root_mut()?, FAMILY, event_id)?;
        player.save(database)?;
    }
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({ "aggregated_time": "", "quest_list": {} }),
    )
}

fn ranking_party(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let (body, mut player, event_id) = match load_optional_event(request, database, asset_root)? {
        Ok(context) => context,
        Err(response) => return Ok(response),
    };
    if event_id > 0 {
        state::set_current_event(player.root_mut()?, FAMILY, event_id)?;
        player.save(database)?;
    }
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({ "raid_ranking_party": [] }),
    )
}

// //// 持久化 raid 活动战斗 [@x380kkm 2026-08-22] ////
fn battle_start(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<BattleStartRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.quest_id > 0
                && body.party_group_id > 0
                && !body.play_id.is_empty() =>
        {
            body
        }
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let mut player = match state::load_player(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    let managed_event = state::managed_event_id(database, FAMILY)?;
    let event_id =
        match resolve_event_id(&player, body.event_id, Some(body.quest_id), managed_event) {
            Some(event_id) => event_id,
            None => return Ok(error_response("400 Bad Request", "invalid_event_id")),
        };
    if let Some(response) =
        closed_activity_response(database, asset_root, &format!("{FAMILY}:{event_id}"))?
    {
        return Ok(response);
    }
    let response_time = server_time(database)?;
    let root = player.root_mut()?;
    state::set_current_event(root, FAMILY, event_id)?;
    party::raid_party_groups(root, FAMILY, event_id)?;
    state::record_battle(
        state::event_state_mut(root, FAMILY, event_id)?,
        body.quest_id,
        body.party_group_id,
        &body.play_id,
        response_time,
    );
    player.start_battle(
        database,
        &ActiveSingleQuest {
            play_id: body.play_id,
            quest_id: body.quest_id,
            category: QUEST_CATEGORY,
            use_boss_boost_point: false,
            use_boost_point: false,
            is_auto_start_mode: body.is_auto_start_mode,
        },
    )?;
    msgpack_response_at(body.viewer_id, false, response_time, json!({}))
}
// //// /持久化 raid 活动战斗 ////

fn select_folder(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<SelectFolderRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.event_id > 0 && body.folder_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let mut player = match load_open_player(database, asset_root, body.viewer_id, body.event_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    let root = player.root_mut()?;
    state::set_current_event(root, FAMILY, body.event_id)?;
    state::event_state_mut(root, FAMILY, body.event_id)?
        .insert("active_folder_id".to_owned(), Value::from(body.folder_id));
    player.save(database)?;
    msgpack_response_at(body.viewer_id, false, server_time(database)?, json!({}))
}

fn reset(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ResetRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.event_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let mut player = match load_open_player(database, asset_root, body.viewer_id, body.event_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    let root = player.root_mut()?;
    state::set_current_event(root, FAMILY, body.event_id)?;
    let reset_kind = if body.quest_type == 1 {
        1
    } else if body.reset_target_id.is_some() {
        2
    } else {
        0
    };
    state::reset_progress(
        state::event_state_mut(root, FAMILY, body.event_id)?,
        reset_kind,
        body.reset_target_id,
        body.is_reset_after_target_round.unwrap_or(false),
    )?;
    player.save(database)?;
    msgpack_response_at(body.viewer_id, false, server_time(database)?, json!({}))
}

fn complete_raid_party_groups(
    root: &serde_json::Map<String, Value>,
    party_groups: &mut Value,
) -> Result<(), PersonalServiceError> {
    let groups = party_groups
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("CN raid party groups are invalid"))?;
    if groups.is_empty() {
        groups.push(json!({
            "party_group_color_id": 15,
            "party_group_id": 1,
            "party_list": [],
        }));
    }
    groups.truncate(1);
    let group = groups[0]
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("CN raid party group is invalid"))?;
    group.insert("party_group_color_id".to_owned(), Value::from(15));
    group.insert("party_group_id".to_owned(), Value::from(1));
    let parties = group
        .entry("party_list".to_owned())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("CN raid party list is invalid"))?;
    parties.truncate(3);
    let mut used_characters = parties
        .iter()
        .filter_map(|party| party.get("character_ids"))
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(Value::as_i64)
        .collect::<BTreeSet<_>>();
    let character_ids = root
        .get("user_character_list")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|characters| characters.keys())
        .filter_map(|character_id| character_id.parse::<i64>().ok())
        .filter(|character_id| *character_id > 0)
        .collect::<BTreeSet<_>>();
    while parties.len() < 3 {
        let party_id = parties.len() as i64 + 1;
        let leader_id = character_ids
            .iter()
            .find(|character_id| !used_characters.contains(character_id))
            .copied();
        if let Some(leader_id) = leader_id {
            used_characters.insert(leader_id);
        }
        parties.push(json!({
            "ability_soul_ids": [null, null, null],
            "character_ids": [leader_id, null, null],
            "equipment_ids": [null, null, null],
            "unison_character_ids": [null, null, null],
            "options": { "allow_other_players_to_heal_me": true },
            "party_edited": false,
            "party_id": party_id,
            "party_name": format!("Party {party_id}"),
        }));
    }
    Ok(())
}

fn load_open_player(
    database: &ServiceDatabase,
    asset_root: &Path,
    viewer_id: i64,
    event_id: i64,
) -> Result<Result<state::ActivityPlayer, HttpResponse>, PersonalServiceError> {
    let player = match state::load_player(database, viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(Err(response)),
    };
    if let Some(response) =
        closed_activity_response(database, asset_root, &format!("{FAMILY}:{event_id}"))?
    {
        return Ok(Err(response));
    }
    Ok(Ok(player))
}

fn load_optional_event(
    request: &HttpRequest,
    database: &ServiceDatabase,
    asset_root: &Path,
) -> Result<
    Result<(OptionalEventRequest, state::ActivityPlayer, i64), HttpResponse>,
    PersonalServiceError,
> {
    let body = match decode_request::<OptionalEventRequest>(request) {
        Ok(body) => body,
        Err(_) => {
            return Ok(Err(error_response(
                "400 Bad Request",
                "invalid_request_body",
            )))
        }
    };
    if body.viewer_id <= 0 {
        return Ok(Err(error_response(
            "400 Bad Request",
            "invalid_request_body",
        )));
    }
    let player = match state::load_player(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(Err(response)),
    };
    let managed_event = state::managed_event_id(database, FAMILY)?;
    let event_id =
        resolve_event_id(&player, body.event_id, body.quest_id, managed_event).unwrap_or_default();
    if event_id > 0 {
        if let Some(response) =
            closed_activity_response(database, asset_root, &format!("{FAMILY}:{event_id}"))?
        {
            return Ok(Err(response));
        }
    }
    Ok(Ok((body, player, event_id)))
}

fn resolve_event_id(
    player: &state::ActivityPlayer,
    event_id: Option<i64>,
    quest_id: Option<i64>,
    managed_event_id: Option<i64>,
) -> Option<i64> {
    event_id
        .filter(|event_id| *event_id > 0)
        .or_else(|| state::current_event_id(player.root().ok()?, FAMILY))
        .or(managed_event_id)
        .or_else(|| {
            quest_id
                .filter(|quest_id| *quest_id > 0)
                .map(|quest_id| quest_id / 1000)
        })
        .filter(|event_id| *event_id > 0)
}
