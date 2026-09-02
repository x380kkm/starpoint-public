// audience: internal
// # personal-service-cn-mana-awake
//
// 该模块按角色稀有度, Mana node 槽位和底座规格计算二板觉醒消耗并保存节点等级.

use super::{
    character_mut, character_time, character_value, deduct_mana, error_response, mana_node,
    node_awake_levels, set_user_info_value, unlocked_nodes, user_info_value,
};
use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, player_snapshot, require_object, require_root,
};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

const CHARACTER_ASSET: &str = include_str!("../../../../assets/character.json");
const MANA_BOARD_ASSET: &str = include_str!("../../../../assets/mana_board.json");
const MANA_NODE_AWAKE_ASSET: &str = include_str!("../../../../assets/mana_node_awake.json");

static CHARACTER_DATA: OnceLock<Result<Value, String>> = OnceLock::new();
static MANA_BOARD_DATA: OnceLock<Result<Value, String>> = OnceLock::new();
static MANA_NODE_AWAKE_DATA: OnceLock<Result<Value, String>> = OnceLock::new();

#[derive(Deserialize)]
struct AwakeManaNodeRequest {
    viewer_id: i64,
    character_id: i64,
    mana_node_multiplied_id_list: Vec<i64>,
    awake_level: i64,
}

struct AwakeCost {
    mana_amount: i64,
    items: BTreeMap<i64, i64>,
}

struct ManaNodeBoardLocation {
    board_index: i64,
    pedestal_size: i64,
}

// //// 觉醒 CN Mana node 并保存完整节点状态 [@x380kkm 2026-08-23] ////
pub(super) fn awake_mana_node(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<AwakeManaNodeRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.character_id > 0 && body.awake_level > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    match character_value(root, body.character_id) {
        Ok(_) => {}
        Err(response) => return Ok(response),
    }
    let rarity = match character_rarity(body.character_id) {
        Ok(rarity) => rarity,
        Err(_) => return Ok(error_response("400 Bad Request", "character_not_found")),
    };
    let unlocked_set: BTreeSet<i64> = unlocked_nodes(root, body.character_id)
        .into_iter()
        .collect();
    let mut awake_levels = node_awake_levels(root, body.character_id);
    let mut mana_cost = 0_i64;
    let mut item_costs = BTreeMap::<i64, i64>::new();
    let mut response_nodes = Vec::new();
    let mut requested_node_ids = BTreeSet::new();
    let mut awake_board_levels = BTreeMap::<i64, i64>::new();

    for node_id in &body.mana_node_multiplied_id_list {
        if !requested_node_ids.insert(*node_id) {
            continue;
        }
        if !unlocked_set.contains(node_id) {
            return Ok(error_response("400 Bad Request", "mana_node_not_unlocked"));
        }
        let location = match mana_node_board_location(body.character_id, *node_id) {
            Ok(Some(location)) => location,
            Ok(None) | Err(_) => {
                return Ok(error_response(
                    "400 Bad Request",
                    "mana_node_awake_cost_not_found",
                ))
            }
        };
        awake_board_levels
            .entry(location.board_index)
            .and_modify(|stored| *stored = (*stored).max(body.awake_level))
            .or_insert(body.awake_level);
        let current_awake_level = awake_levels.get(node_id).copied().unwrap_or_default();
        if current_awake_level >= body.awake_level {
            response_nodes.push(json!({
                "mana_node_multiplied_id": node_id,
                "awake_level": current_awake_level,
            }));
            continue;
        }
        let cost = match awake_cost(body.character_id, *node_id, rarity, location.pedestal_size) {
            Ok(cost) => cost,
            Err(_) => {
                return Ok(error_response(
                    "400 Bad Request",
                    "mana_node_awake_cost_not_found",
                ))
            }
        };
        mana_cost = mana_cost
            .checked_add(cost.mana_amount)
            .ok_or_else(|| PersonalServiceError::new("CN Mana awake cost exceeds range"))?;
        for (item_id, amount) in cost.items {
            let stored = item_costs.entry(item_id).or_default();
            *stored = stored.checked_add(amount).ok_or_else(|| {
                PersonalServiceError::new("CN Mana awake item cost exceeds range")
            })?;
        }
        awake_levels.insert(*node_id, body.awake_level);
        response_nodes.push(json!({
            "mana_node_multiplied_id": node_id,
            "awake_level": body.awake_level,
        }));
    }

    let free_mana = user_info_value(root, "free_mana")?;
    let paid_mana = user_info_value(root, "paid_mana")?;
    let (new_free_mana, new_paid_mana) = match deduct_mana(free_mana, paid_mana, mana_cost) {
        Some(amounts) => amounts,
        None => return Ok(error_response("400 Bad Request", "not_enough_mana")),
    };
    let mut response_items = Map::new();
    let item_list = root
        .get("item_list")
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("stored CN item list is missing"))?;
    for (item_id, cost) in &item_costs {
        let current = item_list
            .get(&item_id.to_string())
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let amount = match current.checked_sub(*cost) {
            Some(amount) if amount >= 0 => amount,
            _ => return Ok(error_response("400 Bad Request", "not_enough_mana_item")),
        };
        response_items.insert(item_id.to_string(), Value::from(amount));
    }

    let item_list = require_object(root, "item_list")?;
    for (item_id, amount) in &response_items {
        item_list.insert(item_id.clone(), amount.clone());
    }
    set_user_info_value(root, "free_mana", new_free_mana)?;
    set_user_info_value(root, "paid_mana", new_paid_mana)?;
    let stored_nodes = awake_levels
        .iter()
        .map(|(node_id, awake_level)| {
            json!({"mana_node_multiplied_id": node_id, "awake_level": awake_level})
        })
        .collect();
    require_object(root, "user_character_mana_node_list")?
        .insert(body.character_id.to_string(), Value::Array(stored_nodes));

    let (evolution_level, join_time, update_time, bond_token_list, mut mana_board_awake) = {
        let character = character_mut(root, body.character_id)?;
        (
            character
                .get("evolution_level")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            character.get("join_time").cloned(),
            character.get("update_time").cloned(),
            character
                .get("bond_token_list")
                .and_then(Value::as_array)
                .cloned(),
            character
                .get("mana_board_awake")
                .and_then(Value::as_object)
                .cloned()
                .map(Value::Object)
                .unwrap_or_else(|| Value::Object(Map::new())),
        )
    };
    let awake_map = mana_board_awake
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN Mana board awake state is invalid"))?;
    for (board_index, awake_level) in awake_board_levels {
        let key = board_index.to_string();
        let stored_level = awake_map
            .get(&key)
            .map(|value| {
                value.as_i64().ok_or_else(|| {
                    PersonalServiceError::new("stored CN Mana board awake level is invalid")
                })
            })
            .transpose()?
            .unwrap_or_default();
        awake_map.insert(key, Value::from(stored_level.max(awake_level)));
    }
    let response_time = server_time(database)?;
    if !body.mana_node_multiplied_id_list.is_empty() {
        database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    }
    let mut response_character = json!({
        "character_id": body.character_id,
        "evolution_level": evolution_level,
        "evolution_img_level": evolution_level,
        "create_time": character_time(join_time.as_ref(), response_time),
        "update_time": character_time(update_time.as_ref(), response_time),
        "join_time": character_time(join_time.as_ref(), response_time),
        "mana_board_awake": mana_board_awake,
    });
    if let Some(bond_token_list) = bond_token_list {
        response_character
            .as_object_mut()
            .expect("CN Mana awake character response is an object")
            .insert("bond_token_list".to_owned(), Value::Array(bond_token_list));
    }
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        json!({
            "user_info": {
                "free_mana": new_free_mana,
                "paid_mana": new_paid_mana,
            },
            "character_list": [response_character],
            "user_character_mana_node_list": {
                body.character_id.to_string(): response_nodes,
            },
            "item_list": response_items,
            "evolution": [],
            "mail_arrived": false,
        }),
    )
}
// //// /觉醒 CN Mana node 并保存完整节点状态 ////

fn character_rarity(character_id: i64) -> Result<i64, PersonalServiceError> {
    asset(&CHARACTER_DATA, CHARACTER_ASSET, "character")?
        .get(&character_id.to_string())
        .and_then(|character| character.get("rarity"))
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new("CN character rarity is missing"))
}

fn awake_cost(
    character_id: i64,
    node_id: i64,
    rarity: i64,
    pedestal_size: i64,
) -> Result<AwakeCost, PersonalServiceError> {
    let node = mana_node(character_id, node_id)?;
    let slot = match node
        .get("field6")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "1" => 1,
        "2" => 2,
        "3" => 3,
        _ => 4,
    };
    let row = asset(
        &MANA_NODE_AWAKE_DATA,
        MANA_NODE_AWAKE_ASSET,
        "Mana node awake",
    )?
    .get(&rarity.to_string())
    .and_then(|rarity_data| rarity_data.get(slot.to_string()))
    .and_then(|slot_data| slot_data.get(pedestal_size.to_string()))
    .and_then(Value::as_array)
    .and_then(|rows| rows.first())
    .and_then(Value::as_array)
    .ok_or_else(|| PersonalServiceError::new("CN Mana node awake cost is missing"))?;
    let item_ids = row
        .first()
        .and_then(Value::as_str)
        .unwrap_or_default()
        .split(',');
    let item_amounts = row
        .get(1)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .split(',');
    let mut items = BTreeMap::new();
    for (item_id, amount) in item_ids.zip(item_amounts) {
        let item_id = item_id.parse::<i64>().unwrap_or_default();
        let amount = amount.parse::<i64>().unwrap_or_default();
        if item_id > 0 && amount > 0 {
            let stored = items.entry(item_id).or_default();
            *stored += amount;
        }
    }
    let mana_amount = integer_value(row.get(2))
        .ok_or_else(|| PersonalServiceError::new("CN Mana node awake mana cost is missing"))?;
    Ok(AwakeCost { mana_amount, items })
}

// //// 读取 CN Mana node 的 board 位置 [@x380kkm 2026-08-28] ////
fn mana_node_board_location(
    character_id: i64,
    node_id: i64,
) -> Result<Option<ManaNodeBoardLocation>, PersonalServiceError> {
    let Some(boards) = asset(&MANA_BOARD_DATA, MANA_BOARD_ASSET, "Mana board")?
        .get(character_id.to_string())
        .and_then(Value::as_object)
    else {
        return Ok(None);
    };
    for (board_index, board) in boards {
        let board_index = board_index
            .parse::<i64>()
            .map_err(|_| PersonalServiceError::new("CN Mana board index is invalid"))?;
        let board = board
            .as_object()
            .ok_or_else(|| PersonalServiceError::new("CN Mana board is invalid"))?;
        for rows in board.values().filter_map(Value::as_array) {
            let Some(row) = rows.first().and_then(Value::as_array) else {
                continue;
            };
            if integer_value(row.first()) == Some(node_id) {
                let pedestal_size = integer_value(row.get(4)).ok_or_else(|| {
                    PersonalServiceError::new("CN Mana node pedestal size is missing")
                })?;
                return Ok(Some(ManaNodeBoardLocation {
                    board_index,
                    pedestal_size,
                }));
            }
        }
    }
    Ok(None)
}
// //// /读取 CN Mana node 的 board 位置 ////

fn integer_value(value: Option<&Value>) -> Option<i64> {
    value
        .and_then(Value::as_i64)
        .or_else(|| value.and_then(Value::as_str)?.parse().ok())
}

fn asset(
    storage: &'static OnceLock<Result<Value, String>>,
    source: &'static str,
    name: &str,
) -> Result<&'static Value, PersonalServiceError> {
    storage
        .get_or_init(|| {
            serde_json::from_str(source)
                .map_err(|error| format!("failed to decode {name}: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}
