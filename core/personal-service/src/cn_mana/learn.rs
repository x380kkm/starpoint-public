// audience: internal
// # personal-service-cn-mana-learn
//
// 节点学习在同一结算事务中保存资产, 任务, 角色进化和羁绊状态.

use super::progression::illustration_evolution_level;
use super::{
    character_mut, character_time, character_value, deduct_mana, error_response,
    mana_node_with_board, node_awake_levels, set_user_info_value, total_unlocked_node_count,
    user_info_value, visible_mana_board_index,
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

#[derive(Deserialize)]
struct LearnManaNodeRequest {
    viewer_id: i64,
    character_id: i64,
    #[serde(default)]
    api_count: i64,
    mana_node_multiplied_id_list: Vec<i64>,
}

// //// 学习 CN Mana node 并原子保存任务和资产 [@x380kkm 2026-09-23] ////
pub(super) fn learn_mana_node(
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
    let stored_board_index = character
        .get("mana_board_index")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    let highest_board_index = visible_mana_board_index(
        body.character_id,
        stored_board_index,
        server_time(database)?,
    )?;
    let awake_levels = node_awake_levels(root, body.character_id)?;
    let unlocked_set: BTreeSet<i64> = awake_levels.keys().copied().collect();
    let mut resulting_unlocked = unlocked_set.clone();
    let mut mana_cost = 0_i64;
    let mut item_costs = BTreeMap::<i64, i64>::new();
    let mut response_nodes = Vec::new();
    let mut new_nodes = Vec::new();
    let mut requested_node_ids = BTreeSet::new();
    let mut requested_board_indices = BTreeSet::new();
    let mut ability_node_count = 0_i64;
    for node_id in &body.mana_node_multiplied_id_list {
        if !requested_node_ids.insert(*node_id) {
            continue;
        }
        let (board_index, node) = match mana_node_with_board(body.character_id, *node_id) {
            Ok(node) => node,
            Err(_) => return Ok(error_response("400 Bad Request", "mana_node_not_found")),
        };
        if board_index > highest_board_index {
            return Ok(error_response("400 Bad Request", "mana_board_not_open"));
        }
        requested_board_indices.insert(board_index);
        if unlocked_set.contains(node_id) {
            response_nodes.push(json!({
                "mana_node_multiplied_id": node_id,
                "awake_level": awake_levels.get(node_id).copied().unwrap_or_default(),
            }));
            continue;
        }
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
        let response_node = json!({"mana_node_multiplied_id": node_id});
        response_nodes.push(response_node.clone());
        new_nodes.push(response_node);
        if node.get("field5").and_then(Value::as_str) == Some("0") {
            ability_node_count += 1;
        }
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
    let completed_boards = super::progression::completed_boards(
        body.character_id,
        &requested_board_indices,
        &resulting_unlocked,
    )?;
    if !new_nodes.is_empty() {
        let stored_nodes = super::state::stored_nodes_mut(root, body.character_id)?;
        stored_nodes.extend(new_nodes.iter().cloned());
    }
    let reconciled_evolution =
        super::progression::reconcile_character_evolution(root, body.character_id)?;
    let (new_evolution, join_time, update_time, bond_token_list, evolution) = {
        let character = character_mut(root, body.character_id)?;
        let current_evolution = character
            .get("evolution_level")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let mut response_bond_tokens = character
            .get("bond_token_list")
            .and_then(Value::as_array)
            .cloned();
        let evolution = reconciled_evolution
            .map(|level| {
                json!({
                    "character_id": body.character_id,
                    "level": level,
                "img_level": illustration_evolution_level(level),
                })
            })
            .unwrap_or_else(|| Value::Array(Vec::new()));
        if !new_nodes.is_empty() {
            if let Some(tokens) = character
                .get_mut("bond_token_list")
                .and_then(Value::as_array_mut)
            {
                for board_index in completed_boards {
                    if let Some(token) = tokens.iter_mut().find(|token| {
                        token.get("mana_board_index").and_then(Value::as_i64) == Some(board_index)
                    }) {
                        if token.get("status").and_then(Value::as_i64) == Some(0) {
                            token["status"] = Value::from(1);
                        }
                    }
                }
                response_bond_tokens = Some(tokens.clone());
            }
        }
        (
            current_evolution,
            character.get("join_time").cloned(),
            character.get("update_time").cloned(),
            response_bond_tokens,
            evolution,
        )
    };
    if !new_nodes.is_empty() {
        let unlocked_node_count = total_unlocked_node_count(root)?;
        let learned_node_count = i64::try_from(new_nodes.len())
            .map_err(|_| PersonalServiceError::new("CN learned Mana node count exceeds range"))?;
        return database.with_player_settlement(|database| {
            let root = require_root(&mut player_data)?;
            let mut mission_delta = crate::cn_mission::record_mana_node_action(
                root,
                database,
                snapshot.account_id,
                unlocked_node_count,
                learned_node_count,
            )?;
            mission_delta.active_mission_list.extend(crate::cn_mission::record_active_counters(
                root, database, &[("total_used_mana_count", mana_cost), ("total_released_ability_node_count", ability_node_count)],
            )?);
            let server_time = server_time(database)?;
            let mana_acquired = crate::cn_mission::total_mana(root)?.saturating_sub(new_free_mana.saturating_add(new_paid_mana));
            mission_delta.mission_info.extend(crate::cn_mission::record_economic_actions(
                root, &crate::cn_mission::plan_economic_missions(database, snapshot.account_id)?,
                crate::cn_mission::EconomicActions { mana_acquired, mana_spent: mana_cost, ..crate::cn_mission::EconomicActions::default() }, server_time,
            )?);
            let mut response_character = json!({
                "character_id": body.character_id,
                "evolution_level": new_evolution,
                "evolution_img_level": illustration_evolution_level(new_evolution),
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
            crate::cn_mission::apply_mission_reward_response(
                &mut response,
                root,
                &mission_delta.mission_info,
                body.viewer_id,
                server_time,
            )?;
            if !mission_delta.mission_info.is_empty() {
                response
                    .as_object_mut()
                    .expect("CN Mana response is an object")
                    .insert(
                        "mission_info".to_owned(),
                        Value::Array(mission_delta.mission_info),
                    );
            }
            database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
            let response = msgpack_response_at(body.viewer_id, false, server_time, response)?;
            Ok(Some(response))
        })?.ok_or_else(|| PersonalServiceError::new("CN Mana settlement did not produce a response"));
    }
    if reconciled_evolution.is_some() {
        database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    }
    let server_time = server_time(database)?;
    let mut response_character = json!({
        "character_id": body.character_id,
        "evolution_level": new_evolution,
        "evolution_img_level": illustration_evolution_level(new_evolution),
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
// //// /学习 CN Mana node 并原子保存任务和资产 ////
