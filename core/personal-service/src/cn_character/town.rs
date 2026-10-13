// audience: internal
// # personal-service-cn-character-town
//
// 城镇角色领取资格来自客户端 StoryJoinCharacter 主表. 首次领取要求对应主线关卡存在 finished 记录.
// join_type 为 quest 且不是临时加入的角色在对应剧情关卡首次通关时直接入队; 城镇领取为没有入队的存档补领.

use super::{error_response, serialize_character};
use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_character_reward::{grant_character, record_character_encyclopedia_state};
use crate::cn_tutorial::{decode_player_data, encode_player_data, player_snapshot, require_root};
use crate::database::{ReceiveHistoryEntry, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;

const TOWN_CHARACTER_ASSET: &str = include_str!("../../assets/cn-character-town.json");

#[derive(Deserialize)]
struct TownCharacterDefinition {
    character_id: i64,
    quest_id: i64,
    quest_category: i64,
    join_type: String,
    join_temporary: bool,
}

#[derive(Default)]
pub(crate) struct StoryJoinResult {
    pub(crate) character_ids: Vec<i64>,
    pub(crate) characters: Vec<Value>,
    pub(crate) encyclopedia_info: Map<String, Value>,
    pub(crate) history: Vec<ReceiveHistoryEntry>,
}

#[derive(Deserialize)]
struct TownCharacterAsset {
    rows: Vec<TownCharacterDefinition>,
}

#[derive(Deserialize)]
struct AddCharacterFromTownRequest {
    viewer_id: i64,
    character_id: i64,
}

static TOWN_CHARACTER_DATA: OnceLock<Result<Vec<TownCharacterDefinition>, String>> =
    OnceLock::new();

// //// 读取城镇角色主表投影 [@x380kkm 2026-09-22] ////
fn town_character_definitions() -> Result<&'static [TownCharacterDefinition], PersonalServiceError>
{
    TOWN_CHARACTER_DATA
        .get_or_init(|| {
            let asset = serde_json::from_str::<TownCharacterAsset>(TOWN_CHARACTER_ASSET)
                .map_err(|error| format!("failed to decode CN town character asset: {error}"))?;
            Ok(asset.rows)
        })
        .as_ref()
        .map(Vec::as_slice)
        .map_err(|error| PersonalServiceError::new(error.clone()))
}
// //// /读取城镇角色主表投影 ////

// //// 查找城镇角色资格并要求主线关卡已完成 [@x380kkm 2026-09-22] ////
fn town_character_definition(
    character_id: i64,
) -> Result<Option<&'static TownCharacterDefinition>, PersonalServiceError> {
    Ok(town_character_definitions()?
        .iter()
        .find(|row| row.character_id == character_id))
}

fn has_finished_quest(
    root: &Map<String, Value>,
    category: i64,
    quest_id: i64,
) -> Result<bool, PersonalServiceError> {
    let Some(entries) = root
        .get("quest_progress")
        .and_then(Value::as_object)
        .and_then(|progress| progress.get(&category.to_string()))
    else {
        return Ok(false);
    };
    let entries = entries
        .as_array()
        .ok_or_else(|| PersonalServiceError::new("stored CN quest category progress is invalid"))?;
    Ok(entries.iter().any(|entry| {
        entry.get("quest_id").and_then(Value::as_i64) == Some(quest_id)
            && entry.get("finished").and_then(Value::as_bool) == Some(true)
    }))
}
// //// /查找城镇角色资格并要求主线关卡已完成 ////

// //// 剧情关卡首次通关时让主表按关卡加入的角色入队 [@x380kkm 2026-10-07] ////
pub(crate) fn join_story_characters(
    root: &mut Map<String, Value>,
    viewer_id: i64,
    quest_category: i64,
    quest_id: i64,
    server_time: i64,
) -> Result<StoryJoinResult, PersonalServiceError> {
    let mut result = StoryJoinResult::default();
    let definitions = town_character_definitions()?.iter().filter(|row| {
        row.join_type == "quest"
            && !row.join_temporary
            && row.quest_category == quest_category
            && row.quest_id == quest_id
    });
    for definition in definitions {
        if owns_character(root, definition.character_id) {
            continue;
        }
        let reward = grant_character(root, viewer_id, definition.character_id, server_time)?;
        result
            .encyclopedia_info
            .extend(record_character_encyclopedia_state(root, definition.character_id)?);
        result.character_ids.push(definition.character_id);
        result.characters.push(reward.character);
        result
            .history
            .push(ReceiveHistoryEntry::reward(5, Some(definition.character_id), 1));
    }
    Ok(result)
}

fn owns_character(root: &Map<String, Value>, character_id: i64) -> bool {
    root.get("user_character_list")
        .and_then(Value::as_object)
        .is_some_and(|characters| characters.contains_key(&character_id.to_string()))
}
// //// /剧情关卡首次通关时让主表按关卡加入的角色入队 ////

// //// 返回已拥有城镇角色的幂等响应 [@x380kkm 2026-09-22] ////
fn owned_character_response(
    root: &Map<String, Value>,
    viewer_id: i64,
    character_id: i64,
    response_time: i64,
) -> Option<Value> {
    let Some(characters) = root.get("user_character_list").and_then(Value::as_object) else {
        return None;
    };
    let Some(character) = characters
        .get(&character_id.to_string())
        .and_then(Value::as_object)
    else {
        return None;
    };
    Some(json!({
        "character_list": [serialize_character(viewer_id, character_id, character, response_time)],
        "encyclopedia_info": {},
        "active_mission_list": [],
        "mail_arrived": false,
    }))
}
// //// /返回已拥有城镇角色的幂等响应 ////

// //// 领取符合主表和主线条件的城镇角色 [@x380kkm 2026-09-22] ////
pub(super) fn add_character_from_town(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<AddCharacterFromTownRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.character_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let Some(definition) = town_character_definition(body.character_id)? else {
        return Ok(error_response(
            "400 Bad Request",
            "character_not_available_from_town",
        ));
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let response_time = server_time(database)?;
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    if let Some(response) =
        owned_character_response(root, body.viewer_id, body.character_id, response_time)
    {
        return msgpack_response_at(body.viewer_id, false, response_time, response);
    }
    if !has_finished_quest(root, definition.quest_category, definition.quest_id)? {
        return Ok(error_response("400 Bad Request", "town_quest_not_cleared"));
    }

    let reward = grant_character(root, body.viewer_id, body.character_id, response_time)?;
    let encyclopedia_info = record_character_encyclopedia_state(root, body.character_id)?;
    let active_mission_list = crate::cn_mission::refresh_active_missions(root, database)?;
    let response = json!({
        "character_list": [reward.character],
        "encyclopedia_info": encyclopedia_info,
        "active_mission_list": active_mission_list,
        "mail_arrived": false,
    });
    let history = [ReceiveHistoryEntry::reward(5, Some(body.character_id), 1)];
    database.save_player_snapshot_with_receive_history(
        snapshot.account_id,
        &encode_player_data(&player_data)?,
        &format!("town-character:{}", body.character_id),
        response_time,
        &history,
    )?;
    msgpack_response_at(body.viewer_id, false, response_time, response)
}
// //// /领取符合主表和主线条件的城镇角色 ////
