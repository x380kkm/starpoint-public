// audience: internal
// # personal-service-cn-character-board
// 玛纳板索引保存角色已开放的最高板块, 第一块板始终可用, 后续板按等级, 突破和前板完成状态开放.

use super::{
    character_asset_data, character_mana_board_count, character_mut, character_time,
    character_value, error_response, required_open_mana_board_exp,
    required_open_mana_board_over_limit, CharacterBoardRequest,
};
use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{decode_player_data, encode_player_data, player_snapshot, require_root};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};

// //// 按角色主表开启 CN Mana board [@x380kkm 2026-08-23] ////
pub(super) fn open(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<CharacterBoardRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.character_id > 0 && body.mana_board_index > 0 => {
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
    if let Err(response) = character_value(root, body.character_id) {
        return Ok(response);
    }
    let asset = match character_asset_data(body.character_id) {
        Ok(asset) => asset,
        Err(_) => return Ok(error_response("400 Bad Request", "character_not_found")),
    };
    let board_count = character_mana_board_count(body.character_id)?;
    let (bond_tokens, bond_tokens_changed) =
        reconcile_bond_token_list(root, body.character_id, board_count)?;
    let stored = match character_value(root, body.character_id) {
        Ok(character) => character.clone(),
        Err(response) => return Ok(response),
    };
    let previous_board_index = stored
        .get("mana_board_index")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    let board_index = usize::try_from(body.mana_board_index - 1)
        .map_err(|_| PersonalServiceError::new("CN Mana board index is out of range"))?;
    let validation_error = if board_index >= board_count {
        Some(error_response(
            "400 Bad Request",
            "invalid_mana_board_index",
        ))
    } else if body.mana_board_index <= previous_board_index || body.mana_board_index == 1 {
        None
    } else if required_open_mana_board_exp(asset.rarity).is_some_and(|required_exp| {
        stored
            .get("exp")
            .and_then(Value::as_i64)
            .unwrap_or_default()
            < required_exp
    }) {
        Some(error_response("400 Bad Request", "character_level_too_low"))
    } else if stored
        .get("over_limit_step")
        .and_then(Value::as_i64)
        .unwrap_or_default()
        < required_open_mana_board_over_limit(asset.rarity)?
    {
        Some(error_response("400 Bad Request", "character_not_uncapped"))
    } else if board_index > 0
        && bond_tokens[board_index - 1]
            .get("status")
            .and_then(Value::as_i64)
            .unwrap_or_default()
            < 1
    {
        Some(error_response(
            "400 Bad Request",
            "previous_mana_board_locked",
        ))
    } else {
        None
    };
    if let Some(response) = validation_error {
        if bond_tokens_changed {
            database
                .save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
        }
        return Ok(response);
    }
    let opened_board_index = previous_board_index.max(body.mana_board_index);
    let character = match character_mut(root, body.character_id) {
        Ok(character) => character,
        Err(response) => return Ok(response),
    };
    character.insert(
        "mana_board_index".to_owned(),
        Value::from(opened_board_index),
    );
    let server_time = server_time(database)?;
    let response_character = json!({
        "viewer_id": body.viewer_id,
        "character_id": body.character_id,
        "mana_board_index": opened_board_index,
        "create_time": character_time(character.get("join_time"), server_time),
        "update_time": character_time(character.get("update_time"), server_time),
        "join_time": character_time(character.get("join_time"), server_time),
    });
    let mission_info = if previous_board_index < 2 && opened_board_index >= 2 {
        crate::cn_mission::record_mana_board_open_action(root, database, snapshot.account_id)?
    } else {
        Vec::new()
    };
    let free_vmoney = crate::cn_tutorial::user_info_value(root, "free_vmoney")?;
    let mut response = json!({
        "user_info": {"free_vmoney": free_vmoney},
        "character_list": [response_character],
        "mail_arrived": false,
    });
    crate::cn_mission::apply_mission_reward_response(
        &mut response,
        root,
        &mission_info,
        body.viewer_id,
        server_time,
    )?;
    if !mission_info.is_empty() {
        response
            .as_object_mut()
            .expect("CN Mana board response is an object")
            .insert("mission_info".to_owned(), Value::Array(mission_info));
    }
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(body.viewer_id, false, server_time, response)
}
// //// /按角色主表开启 CN Mana board ////

// //// 按角色主表归一化 CN 羁绊代币列表 [@x380kkm 2026-08-23] ////
fn reconcile_bond_token_list(
    root: &mut Map<String, Value>,
    character_id: i64,
    board_count: usize,
) -> Result<(Vec<Value>, bool), PersonalServiceError> {
    let character = root
        .get_mut("user_character_list")
        .and_then(Value::as_object_mut)
        .and_then(|characters| characters.get_mut(&character_id.to_string()))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN character is missing"))?;
    let active_board_index = character
        .get("mana_board_index")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    let awake_levels = character
        .get("mana_board_awake")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let bond_tokens = character
        .entry("bond_token_list".to_owned())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN bond token list is invalid"))?;
    let previous = bond_tokens.clone();

    for index in 0..board_count {
        let board_index = i64::try_from(index + 1)
            .map_err(|_| PersonalServiceError::new("CN Mana board index exceeds range"))?;
        if let Some(token) = bond_tokens.get_mut(index) {
            if let Some(token) = token.as_object_mut() {
                token.insert("mana_board_index".to_owned(), Value::from(board_index));
                if token.get("status").and_then(Value::as_i64).is_none() {
                    token.insert("status".to_owned(), Value::from(0));
                }
            } else {
                *token = json!({"mana_board_index": board_index, "status": 0});
            }
        } else {
            bond_tokens.push(json!({"mana_board_index": board_index, "status": 0}));
        }
    }

    while bond_tokens.len() > board_count {
        let extra_board_index = bond_tokens.len();
        let extra_board_key = extra_board_index.to_string();
        let token_is_empty = bond_tokens
            .last()
            .is_some_and(|token| token.get("status").and_then(Value::as_i64) == Some(0));
        let awake_is_empty = awake_levels
            .get(&extra_board_key)
            .and_then(Value::as_i64)
            .unwrap_or_default()
            == 0;
        if !token_is_empty
            || active_board_index >= i64::try_from(extra_board_index).unwrap_or(i64::MAX)
            || !awake_is_empty
        {
            break;
        }
        bond_tokens.pop();
    }

    let changed = previous != *bond_tokens;
    Ok((bond_tokens.clone(), changed))
}
// //// /按角色主表归一化 CN 羁绊代币列表 ////
