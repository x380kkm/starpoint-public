// audience: internal
// # personal-service-cn-reference-quest
//
// 该模块解锁参考 CN 任务并保存材料余额和任务进度.

use super::common::{error_response, json_document};
use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_battle_assets::load_battle_fixture;
use crate::cn_player::normalize_quest_progress_entry;
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, player_snapshot, require_object, require_root,
};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

const QUEST_UNLOCK_COSTS: &str = include_str!("../../assets/cn-quest-unlock-costs.json");
static QUEST_UNLOCK_DOCUMENT: OnceLock<Result<Value, String>> = OnceLock::new();

#[derive(Deserialize)]
struct QuestUnlockRequest {
    category: i64,
    quest_id: i64,
    viewer_id: i64,
}

// //// 解锁存在的 CN 任务并扣除材料 [@x380kkm 2026-08-22] ////
pub(super) fn unlock(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<QuestUnlockRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.category > 0 && body.quest_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    if !quest_exists(body.category, body.quest_id)? {
        return Ok(error_response("400 Bad Request", "quest_not_found"));
    }
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    if is_quest_unlocked(root, body.category, body.quest_id)? {
        return Ok(error_response("400 Bad Request", "quest_already_unlocked"));
    }
    let costs = quest_unlock_costs(body.quest_id)?;
    let items = require_object(root, "item_list")?;
    for (item_id, count) in &costs {
        let current = items
            .get(&item_id.to_string())
            .and_then(Value::as_i64)
            .unwrap_or_default();
        if current < *count {
            return Ok(error_response("400 Bad Request", "not_enough_items"));
        }
    }
    let mut item_updates = BTreeMap::new();
    for (item_id, count) in costs {
        let key = item_id.to_string();
        let updated = items.get(&key).and_then(Value::as_i64).unwrap_or_default() - count;
        items.insert(key.clone(), Value::from(updated));
        item_updates.insert(key, updated);
    }
    mark_quest_unlocked(root, body.category, body.quest_id)?;
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({"item_list": item_updates, "mail_arrived": false}),
    )
}
// //// /解锁存在的 CN 任务并扣除材料 ////

fn quest_exists(category: i64, quest_id: i64) -> Result<bool, PersonalServiceError> {
    let fixture = load_battle_fixture()?;
    Ok(fixture
        .quests
        .contains_key(&format!("{category}:{quest_id}")))
}

fn quest_unlock_costs(quest_id: i64) -> Result<Vec<(i64, i64)>, PersonalServiceError> {
    let document = json_document(
        &QUEST_UNLOCK_DOCUMENT,
        QUEST_UNLOCK_COSTS,
        "quest unlock costs",
    )?;
    let Some(cost) = document.get(quest_id.to_string()) else {
        return Ok(Vec::new());
    };
    let ids = cost
        .get("itemIds")
        .and_then(Value::as_array)
        .ok_or_else(|| PersonalServiceError::new("CN quest unlock item ids are invalid"))?;
    let counts = cost
        .get("itemCounts")
        .and_then(Value::as_array)
        .ok_or_else(|| PersonalServiceError::new("CN quest unlock item counts are invalid"))?;
    ids.iter()
        .enumerate()
        .map(|(index, id)| {
            let id = id
                .as_i64()
                .ok_or_else(|| PersonalServiceError::new("CN quest unlock item id is invalid"))?;
            let count = counts.get(index).and_then(Value::as_i64).unwrap_or(1);
            Ok((id, count))
        })
        .collect()
}

fn is_quest_unlocked(
    root: &Map<String, Value>,
    category: i64,
    quest_id: i64,
) -> Result<bool, PersonalServiceError> {
    let Some(entries) = root
        .get("quest_progress")
        .and_then(Value::as_object)
        .and_then(|progress| progress.get(&category.to_string()))
        .and_then(Value::as_array)
    else {
        return Ok(false);
    };
    Ok(entries.iter().any(|entry| {
        entry.get("quest_id").and_then(Value::as_i64) == Some(quest_id)
            && entry
                .get("unlocked")
                .and_then(Value::as_bool)
                .unwrap_or(false)
    }))
}

fn mark_quest_unlocked(
    root: &mut Map<String, Value>,
    category: i64,
    quest_id: i64,
) -> Result<(), PersonalServiceError> {
    let progress = root
        .entry("quest_progress".to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN quest progress is invalid"))?;
    let entries = progress
        .entry(category.to_string())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN quest category is invalid"))?;
    if let Some(existing) = entries
        .iter_mut()
        .find(|entry| entry.get("quest_id").and_then(Value::as_i64) == Some(quest_id))
    {
        normalize_quest_progress_entry(existing)?.insert("unlocked".to_owned(), Value::Bool(true));
    } else {
        entries.push(json!({
            "quest_id": quest_id,
            "finished": false,
            "unlocked": true,
            "high_score": 0,
            "clear_rank": null,
            "best_elapsed_time_ms": null,
        }));
    }
    Ok(())
}
