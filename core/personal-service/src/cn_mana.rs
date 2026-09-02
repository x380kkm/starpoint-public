// audience: internal
// # personal-service-cn-mana
//
// 该模块按 CN Mana node 资产执行节点解锁, 觉醒和载入投影, 扣除资源并保存玩家快照.

mod awake;

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, format_client_time, player_snapshot, require_object,
    require_root,
};
use crate::database::{parse_iso_timestamp, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

const MANA_NODE_ASSET: &str = include_str!("../../../assets/mana_node.json");
const MANA_BOARD2_OPEN_CONDITION_ASSET: &str =
    include_str!("../assets/cn-mana-board2-open-condition.json");
const JAPAN_STANDARD_OFFSET_MILLISECONDS: i64 = 9 * 60 * 60 * 1_000;
static MANA_NODE_DATA: OnceLock<Result<Value, String>> = OnceLock::new();
static MANA_BOARD2_OPEN_CONDITION_DATA: OnceLock<Result<Value, String>> = OnceLock::new();

#[derive(Deserialize)]
struct LearnManaNodeRequest {
    viewer_id: i64,
    character_id: i64,
    #[serde(default)]
    api_count: i64,
    mana_node_multiplied_id_list: Vec<i64>,
}

// //// 分派 CN Mana node 请求 [@x380kkm 2026-07-24] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    let response = match request.path() {
        "/api/index.php/character/learn_mana_node" => learn_mana_node(request, database),
        "/api/index.php/character/awake_mana_node" => awake::awake_mana_node(request, database),
        _ => return None,
    };
    Some(response)
}
// //// /分派 CN Mana node 请求 ////

// //// 学习 CN Mana node 并保存完整状态 [@x380kkm 2026-08-28] ////
fn learn_mana_node(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<LearnManaNodeRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.character_id > 0 && body.api_count >= 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let character = match character_value(root, body.character_id) {
        Ok(character) => character.clone(),
        Err(response) => return Ok(response),
    };
    let board_index = character
        .get("mana_board_index")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    let nodes = match mana_nodes(body.character_id, board_index) {
        Ok(nodes) => nodes,
        Err(_) => return Ok(error_response("400 Bad Request", "mana_nodes_not_found")),
    };
    let unlocked = unlocked_nodes(root, body.character_id);
    let unlocked_set: std::collections::BTreeSet<i64> = unlocked.iter().copied().collect();
    let mut resulting_unlocked = unlocked_set.clone();
    let mut mana_cost = 0_i64;
    let mut item_costs = BTreeMap::<i64, i64>::new();
    let mut response_nodes = Vec::new();
    let mut requested_node_ids = BTreeSet::new();
    for node_id in &body.mana_node_multiplied_id_list {
        if !requested_node_ids.insert(*node_id) {
            continue;
        }
        if unlocked_set.contains(node_id) {
            return Ok(error_response(
                "400 Bad Request",
                "mana_node_already_unlocked",
            ));
        }
        let node = match nodes.get(&node_id.to_string()).and_then(Value::as_object) {
            Some(node) => node,
            None => return Ok(error_response("400 Bad Request", "mana_node_not_found")),
        };
        let node_mana = node
            .get("manaCost")
            .and_then(Value::as_i64)
            .ok_or_else(|| PersonalServiceError::new("CN Mana node cost is missing"))?;
        mana_cost = mana_cost
            .checked_add(node_mana)
            .ok_or_else(|| PersonalServiceError::new("CN Mana cost exceeds supported range"))?;
        if let Some(items) = node.get("items").and_then(Value::as_object) {
            for (item_id, amount) in items {
                let item_id = item_id
                    .parse::<i64>()
                    .map_err(|_| PersonalServiceError::new("CN Mana item id is invalid"))?;
                let amount = amount
                    .as_i64()
                    .ok_or_else(|| PersonalServiceError::new("CN Mana item cost is invalid"))?;
                let entry = item_costs.entry(item_id).or_default();
                *entry = entry
                    .checked_add(amount)
                    .ok_or_else(|| PersonalServiceError::new("CN Mana item cost exceeds range"))?;
            }
        }
        response_nodes.push(json!({"mana_node_multiplied_id": node_id}));
        resulting_unlocked.insert(*node_id);
    }
    let free_mana = user_info_value(root, "free_mana")?;
    let paid_mana = user_info_value(root, "paid_mana")?;
    let (new_free_mana, new_paid_mana) = match deduct_mana(free_mana, paid_mana, mana_cost) {
        Some(amounts) => amounts,
        None => return Ok(error_response("400 Bad Request", "not_enough_mana")),
    };
    let item_list = require_object(root, "item_list")?;
    let mut response_items = Map::new();
    for (item_id, cost) in &item_costs {
        let current = item_list
            .get(&item_id.to_string())
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let new_amount = match current.checked_sub(*cost) {
            Some(value) if value >= 0 => value,
            _ => return Ok(error_response("400 Bad Request", "not_enough_mana_item")),
        };
        response_items.insert(item_id.to_string(), Value::from(new_amount));
    }
    for (item_id, amount) in &response_items {
        item_list.insert(item_id.clone(), amount.clone());
    }
    set_user_info_value(root, "free_mana", new_free_mana)?;
    set_user_info_value(root, "paid_mana", new_paid_mana)?;
    let board_complete = nodes.keys().all(|node_id| {
        node_id
            .parse::<i64>()
            .ok()
            .is_some_and(|node_id| resulting_unlocked.contains(&node_id))
    });
    let (new_evolution, join_time, update_time, bond_token_list, evolution) = {
        let character = character_mut(root, body.character_id)?;
        let current_evolution = character
            .get("evolution_level")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let mut new_evolution = current_evolution;
        let mut response_bond_tokens = character
            .get("bond_token_list")
            .and_then(Value::as_array)
            .cloned();
        let mut evolution = Value::Array(Vec::new());
        let token_index = usize::try_from(board_index - 1).ok();
        let grants_bond_token = if board_complete {
            token_index
                .and_then(|token_index| {
                    character
                        .get_mut("bond_token_list")
                        .and_then(Value::as_array_mut)
                        .and_then(|tokens| {
                            let token = tokens.get_mut(token_index)?;
                            if token.get("status").and_then(Value::as_i64) != Some(0) {
                                return None;
                            }
                            token["status"] = Value::from(1);
                            response_bond_tokens = Some(tokens.clone());
                            Some(())
                        })
                })
                .is_some()
        } else {
            false
        };
        if grants_bond_token && current_evolution == 0 {
            new_evolution = 1;
            character.insert("evolution_level".to_owned(), Value::from(new_evolution));
            evolution = json!({
                "character_id": body.character_id,
                "level": 1,
                "img_level": 1,
            });
        }
        (
            new_evolution,
            character.get("join_time").cloned(),
            character.get("update_time").cloned(),
            response_bond_tokens,
            evolution,
        )
    };
    if !response_nodes.is_empty() {
        let current_nodes = require_object(root, "user_character_mana_node_list")?;
        let stored_nodes = current_nodes
            .entry(body.character_id.to_string())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN Mana node list is invalid"))?;
        for node in &response_nodes {
            stored_nodes.push(node.clone());
        }
        let unlocked_node_count = total_unlocked_node_count(root)?;
        let learned_node_count = i64::try_from(response_nodes.len())
            .map_err(|_| PersonalServiceError::new("CN learned Mana node count exceeds range"))?;
        let mission_delta = crate::cn_mission::record_mana_node_action(
            root,
            database,
            snapshot.account_id,
            unlocked_node_count,
            learned_node_count,
        )?;
        database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
        let server_time = server_time(database)?;
        let mut response_character = json!({
            "character_id": body.character_id,
            "evolution_level": new_evolution,
            "evolution_img_level": new_evolution,
            "create_time": character_time(join_time.as_ref(), server_time),
            "update_time": character_time(update_time.as_ref(), server_time),
            "join_time": character_time(join_time.as_ref(), server_time),
        });
        if let Some(bond_token_list) = bond_token_list {
            response_character
                .as_object_mut()
                .expect("CN Mana character response is an object")
                .insert("bond_token_list".to_owned(), Value::Array(bond_token_list));
        }
        let mut response = json!({
            "user_info": {
                "free_mana": new_free_mana,
                "paid_mana": new_paid_mana,
            },
            "character_list": [response_character],
            "evolution": evolution,
            "item_list": response_items,
            "user_character_mana_node_list": {body.character_id.to_string(): response_nodes},
            "active_mission_list": mission_delta.active_mission_list,
            "mail_arrived": false,
        });
        if !mission_delta.mission_info.is_empty() {
            response
                .as_object_mut()
                .expect("CN Mana response is an object")
                .insert(
                    "mission_info".to_owned(),
                    Value::Array(mission_delta.mission_info),
                );
        }
        return msgpack_response_at(body.viewer_id, false, server_time, response);
    }
    let server_time = server_time(database)?;
    let mut response_character = json!({
        "character_id": body.character_id,
        "evolution_level": new_evolution,
        "evolution_img_level": new_evolution,
        "create_time": character_time(join_time.as_ref(), server_time),
        "update_time": character_time(update_time.as_ref(), server_time),
        "join_time": character_time(join_time.as_ref(), server_time),
    });
    if let Some(bond_token_list) = bond_token_list {
        response_character
            .as_object_mut()
            .expect("CN Mana character response is an object")
            .insert("bond_token_list".to_owned(), Value::Array(bond_token_list));
    }
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time,
        json!({
            "user_info": {
                "free_mana": new_free_mana,
                "paid_mana": new_paid_mana,
            },
            "character_list": [response_character],
            "evolution": evolution,
            "item_list": response_items,
            "user_character_mana_node_list": {body.character_id.to_string(): response_nodes},
            "active_mission_list": [],
            "mail_arrived": false,
        }),
    )
}
// //// /学习 CN Mana node 并保存完整状态 ////

fn mana_nodes(
    character_id: i64,
    board_index: i64,
) -> Result<&'static Map<String, Value>, PersonalServiceError> {
    mana_node_document()?
        .get(&character_id.to_string())
        .and_then(Value::as_object)
        .and_then(|boards| boards.get(&board_index.to_string()))
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("CN Mana node board is missing"))
}

pub(super) fn mana_node(
    character_id: i64,
    node_id: i64,
) -> Result<&'static Map<String, Value>, PersonalServiceError> {
    mana_node_document()?
        .get(&character_id.to_string())
        .and_then(Value::as_object)
        .and_then(|boards| {
            boards
                .values()
                .filter_map(Value::as_object)
                .find_map(|nodes| nodes.get(&node_id.to_string()))
        })
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("CN Mana node is missing"))
}

// //// 读取 CN Mana node 资产 [@x380kkm 2026-08-28] ////
fn mana_node_document() -> Result<&'static Value, PersonalServiceError> {
    MANA_NODE_DATA
        .get_or_init(|| {
            serde_json::from_str::<Value>(MANA_NODE_ASSET)
                .map_err(|error| format!("failed to decode Mana node asset: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}
// //// /读取 CN Mana node 资产 ////

pub(super) fn unlocked_nodes(root: &Map<String, Value>, character_id: i64) -> Vec<i64> {
    root.get("user_character_mana_node_list")
        .and_then(Value::as_object)
        .and_then(|characters| characters.get(&character_id.to_string()))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|node| {
            node.get("mana_node_multiplied_id")
                .and_then(Value::as_i64)
                .or_else(|| node.get("multiplied_id").and_then(Value::as_i64))
                .or_else(|| node.as_i64())
        })
        .collect()
}

// //// 统计玩家已解锁的 Mana node 总数 [@x380kkm 2026-08-28] ////
fn total_unlocked_node_count(root: &Map<String, Value>) -> Result<i64, PersonalServiceError> {
    let character_nodes = root
        .get("user_character_mana_node_list")
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("stored CN Mana node list is missing"))?;
    let mut unique_nodes = BTreeSet::new();
    for (character_id, nodes) in character_nodes {
        let nodes = nodes
            .as_array()
            .ok_or_else(|| PersonalServiceError::new("stored CN Mana node list is invalid"))?;
        for node in nodes {
            let node_id = mana_node_id(node)
                .ok_or_else(|| PersonalServiceError::new("stored CN Mana node id is invalid"))?;
            unique_nodes.insert((character_id.as_str(), node_id));
        }
    }
    i64::try_from(unique_nodes.len())
        .map_err(|_| PersonalServiceError::new("CN unlocked Mana node count exceeds range"))
}
// //// /统计玩家已解锁的 Mana node 总数 ////

// //// 按客户端时间投影 CN load 的 Mana board 状态 [@x380kkm 2026-08-28] ////
pub(crate) fn project_load_mana_board_state(
    player_data: &mut Value,
    response_time: i64,
) -> Result<(), PersonalServiceError> {
    let root = player_data
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("CN load player data is not an object"))?;
    let visible_board_indexes = {
        let characters = root
            .get("user_character_list")
            .and_then(Value::as_object)
            .ok_or_else(|| PersonalServiceError::new("stored CN character list is missing"))?;
        let mut visible_board_indexes = BTreeMap::new();
        for (character_id, character) in characters {
            let Some(character) = character.as_object() else {
                continue;
            };
            let Ok(character_id_number) = character_id.parse::<i64>() else {
                continue;
            };
            let stored_board_index = character
                .get("mana_board_index")
                .and_then(Value::as_i64)
                .filter(|index| *index > 0)
                .unwrap_or(1);
            visible_board_indexes.insert(
                character_id.clone(),
                visible_mana_board_index(character_id_number, stored_board_index, response_time)?,
            );
        }
        visible_board_indexes
    };

    if let Some(characters) = root
        .get_mut("user_character_list")
        .and_then(Value::as_object_mut)
    {
        for (character_id, visible_board_index) in &visible_board_indexes {
            if let Some(character) = characters
                .get_mut(character_id)
                .and_then(Value::as_object_mut)
            {
                character.insert(
                    "mana_board_index".to_owned(),
                    Value::from(*visible_board_index),
                );
            }
        }
    }

    let character_nodes = root
        .get("user_character_mana_node_list")
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("stored CN Mana node list is missing"))?;
    let mut projected_character_nodes = Map::new();
    for (character_id, nodes) in character_nodes {
        let Ok(character_id_number) = character_id.parse::<i64>() else {
            continue;
        };
        let Some(visible_board_index) = visible_board_indexes.get(character_id) else {
            continue;
        };
        let visible_node_ids = visible_mana_node_ids(character_id_number, *visible_board_index)?;
        let nodes = nodes
            .as_array()
            .ok_or_else(|| PersonalServiceError::new("stored CN Mana node list is invalid"))?;
        let awake_levels = node_awake_levels(root, character_id_number);
        let mut projected_nodes = Vec::new();
        let mut projected_node_ids = BTreeSet::new();
        for node in nodes {
            let node_id = mana_node_id(node)
                .ok_or_else(|| PersonalServiceError::new("stored CN Mana node id is invalid"))?;
            if visible_node_ids.contains(&node_id) && projected_node_ids.insert(node_id) {
                projected_nodes.push(json!({
                    "multiplied_id": node_id,
                    "awake_level": awake_levels.get(&node_id).copied().unwrap_or_default(),
                }));
            }
        }
        projected_character_nodes.insert(character_id.clone(), Value::Array(projected_nodes));
    }
    root.insert(
        "user_character_mana_node_list".to_owned(),
        Value::Object(projected_character_nodes),
    );
    Ok(())
}
// //// /按客户端时间投影 CN load 的 Mana board 状态 ////

// //// 计算角色当前可见的 Mana board 索引 [@x380kkm 2026-08-28] ////
fn visible_mana_board_index(
    character_id: i64,
    stored_board_index: i64,
    response_time: i64,
) -> Result<i64, PersonalServiceError> {
    let has_board_two = mana_node_document()?
        .get(&character_id.to_string())
        .and_then(Value::as_object)
        .is_some_and(|boards| boards.contains_key("2"));
    let highest_visible_board =
        if has_board_two && mana_board_two_is_open(character_id, response_time)? {
            2
        } else {
            1
        };
    Ok(stored_board_index.clamp(1, highest_visible_board))
}
// //// /计算角色当前可见的 Mana board 索引 ////

// //// 按 CN 客户端时区检查 Mana board 2 开放窗口 [@x380kkm 2026-08-28] ////
fn mana_board_two_is_open(
    character_id: i64,
    response_time: i64,
) -> Result<bool, PersonalServiceError> {
    let document = MANA_BOARD2_OPEN_CONDITION_DATA.get_or_init(|| {
        serde_json::from_str::<Value>(MANA_BOARD2_OPEN_CONDITION_ASSET)
            .map_err(|error| format!("failed to decode Mana board 2 open conditions: {error}"))
    });
    let document = document
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    let Some(window) = document
        .get(&character_id.to_string())
        .and_then(Value::as_array)
    else {
        return Ok(false);
    };
    let start_time = window
        .first()
        .and_then(Value::as_str)
        .map(parse_condition_timestamp)
        .transpose()?
        .flatten();
    let end_time = window
        .get(1)
        .and_then(Value::as_str)
        .map(parse_condition_timestamp)
        .transpose()?
        .flatten();
    let response_time = response_time.saturating_mul(1_000);
    Ok(
        start_time.map_or(true, |start_time| response_time >= start_time)
            && end_time.map_or(true, |end_time| response_time <= end_time),
    )
}
// //// /按 CN 客户端时区检查 Mana board 2 开放窗口 ////

// //// 解析 CN Mana board 条件的日本时区时间 [@x380kkm 2026-08-28] ////
fn parse_condition_timestamp(value: &str) -> Result<Option<i64>, PersonalServiceError> {
    if value == "(None)" {
        return Ok(None);
    }
    let normalized = format!("{}.000Z", value.replace(' ', "T"));
    let timestamp = parse_iso_timestamp(&normalized)
        .ok_or_else(|| PersonalServiceError::new("CN Mana board condition time is invalid"))?;
    timestamp
        .checked_sub(JAPAN_STANDARD_OFFSET_MILLISECONDS)
        .map(Some)
        .ok_or_else(|| PersonalServiceError::new("CN Mana board condition time is out of range"))
}
// //// /解析 CN Mana board 条件的日本时区时间 ////

// //// 读取角色已开放板的 Mana node ID [@x380kkm 2026-08-28] ////
fn visible_mana_node_ids(
    character_id: i64,
    mana_board_index: i64,
) -> Result<BTreeSet<i64>, PersonalServiceError> {
    let Some(boards) = mana_node_document()?
        .get(&character_id.to_string())
        .and_then(Value::as_object)
    else {
        return Ok(BTreeSet::new());
    };
    let mut visible_node_ids = BTreeSet::new();
    for (board_index, nodes) in boards {
        let board_index = board_index
            .parse::<i64>()
            .map_err(|_| PersonalServiceError::new("CN Mana board index is invalid"))?;
        if board_index > mana_board_index {
            continue;
        }
        let nodes = nodes
            .as_object()
            .ok_or_else(|| PersonalServiceError::new("CN Mana node board is invalid"))?;
        for node_id in nodes.keys() {
            visible_node_ids.insert(
                node_id
                    .parse::<i64>()
                    .map_err(|_| PersonalServiceError::new("CN Mana node id is invalid"))?,
            );
        }
    }
    Ok(visible_node_ids)
}
// //// /读取角色已开放板的 Mana node ID ////

// //// 读取兼容格式中的 Mana node ID [@x380kkm 2026-08-28] ////
fn mana_node_id(node: &Value) -> Option<i64> {
    node.get("mana_node_multiplied_id")
        .and_then(Value::as_i64)
        .or_else(|| node.get("multiplied_id").and_then(Value::as_i64))
        .or_else(|| node.as_i64())
}
// //// /读取兼容格式中的 Mana node ID ////

pub(super) fn node_awake_levels(
    root: &Map<String, Value>,
    character_id: i64,
) -> BTreeMap<i64, i64> {
    let mut levels = BTreeMap::<i64, i64>::new();
    for node in root
        .get("user_character_mana_node_list")
        .and_then(Value::as_object)
        .and_then(|characters| characters.get(&character_id.to_string()))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(node_id) = mana_node_id(node) else {
            continue;
        };
        let awake_level = node
            .get("awake_level")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        levels
            .entry(node_id)
            .and_modify(|stored| *stored = (*stored).max(awake_level))
            .or_insert(awake_level);
    }
    levels
}

fn deduct_mana(free_mana: i64, paid_mana: i64, cost: i64) -> Option<(i64, i64)> {
    if cost <= free_mana {
        return Some((free_mana - cost, paid_mana));
    }
    let paid_cost = cost.checked_sub(free_mana)?;
    let remaining_paid = paid_mana.checked_sub(paid_cost)?;
    (remaining_paid >= 0).then_some((0, remaining_paid))
}

pub(super) fn user_info_value(
    root: &Map<String, Value>,
    key: &str,
) -> Result<i64, PersonalServiceError> {
    root.get("user_info")
        .and_then(Value::as_object)
        .and_then(|info| info.get(key))
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new(format!("stored CN {key} value is missing")))
}

pub(super) fn set_user_info_value(
    root: &mut Map<String, Value>,
    key: &str,
    value: i64,
) -> Result<(), PersonalServiceError> {
    require_object(root, "user_info")?.insert(key.to_owned(), Value::from(value));
    Ok(())
}

pub(super) fn character_value<'a>(
    root: &'a Map<String, Value>,
    character_id: i64,
) -> Result<&'a Map<String, Value>, HttpResponse> {
    root.get("user_character_list")
        .and_then(Value::as_object)
        .and_then(|characters| characters.get(&character_id.to_string()))
        .and_then(Value::as_object)
        .ok_or_else(|| error_response("400 Bad Request", "character_not_owned"))
}

pub(super) fn character_mut<'a>(
    root: &'a mut Map<String, Value>,
    character_id: i64,
) -> Result<&'a mut Map<String, Value>, PersonalServiceError> {
    root.get_mut("user_character_list")
        .and_then(Value::as_object_mut)
        .and_then(|characters| characters.get_mut(&character_id.to_string()))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("CN character is not owned"))
}

pub(super) fn character_time(value: Option<&Value>, server_time: i64) -> String {
    value
        .and_then(Value::as_i64)
        .map(format_client_time)
        .or_else(|| value.and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_else(|| format_client_time(server_time))
}

pub(super) fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}

#[cfg(test)]
mod tests {
    use super::visible_mana_board_index;
    use crate::database::parse_iso_timestamp;

    // //// 按角色开放时刻投影 Mana board 2 [@x380kkm 2026-08-28] ////
    #[test]
    fn projects_character_specific_mana_board_time_window() {
        let before_open = parse_iso_timestamp("2025-02-13T02:59:59.000Z")
            .expect("Mana board time is valid")
            / 1_000;
        let at_open = parse_iso_timestamp("2025-02-13T03:00:00.000Z")
            .expect("Mana board time is valid")
            / 1_000;

        assert_eq!(
            visible_mana_board_index(111_147, 2, before_open)
                .expect("Mana board index is projected"),
            1
        );
        assert_eq!(
            visible_mana_board_index(111_147, 2, at_open).expect("Mana board index is projected"),
            2
        );
    }
    // //// /按角色开放时刻投影 Mana board 2 ////
}
