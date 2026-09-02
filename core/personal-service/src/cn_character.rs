// audience: internal
// # personal-service-cn-character
//
// 该模块实现 CN 角色外观、突破和 Mana board 开启协议. 状态写入玩家快照.

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_character_reward::grant_character;
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, format_client_time, player_snapshot, require_object,
    require_root,
};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;

const CHARACTER_ASSET: &str = include_str!("../../../assets/character.json");
const MANA_NODE_ASSET: &str = include_str!("../../../assets/mana_node.json");
static CHARACTER_DATA: OnceLock<Result<Value, String>> = OnceLock::new();
static MANA_NODE_DATA: OnceLock<Result<Value, String>> = OnceLock::new();

#[derive(Clone, Copy)]
pub(crate) struct CharacterAssetData {
    pub(crate) rarity: i64,
    pub(crate) element: i64,
}

#[derive(Deserialize)]
struct SetIllustrationSettingsRequest {
    viewer_id: i64,
    character_id: i64,
    illustration_settings: Vec<i64>,
}

#[derive(Deserialize)]
struct SetProtectionRequest {
    viewer_id: i64,
    protect_character_ids: Vec<i64>,
    unprotect_character_ids: Vec<i64>,
}

#[derive(Deserialize)]
struct CharacterBoardRequest {
    viewer_id: i64,
    character_id: i64,
    mana_board_index: i64,
}

#[derive(Deserialize)]
struct OverLimitRequest {
    viewer_id: i64,
    character_id: i64,
    over_limit_count: i64,
    use_stack: bool,
    item_id: Option<i64>,
}

#[derive(Deserialize)]
struct BulkOverLimitRequest {
    viewer_id: i64,
}

#[derive(Deserialize)]
struct AddCharacterFromTownRequest {
    viewer_id: i64,
    character_id: i64,
}

// //// 分派 CN 角色养成请求 [@x380kkm 2026-07-24] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    let response = match request.path() {
        "/api/index.php/character/set_illustration_settings" => {
            set_illustration_settings(request, database)
        }
        "/api/index.php/character/set_protection" => set_protection(request, database),
        "/api/index.php/character/receive_bond_token" => receive_bond_token(request, database),
        "/api/index.php/character/open_mana_board" => open_mana_board(request, database),
        "/api/index.php/character/over_limit" => over_limit(request, database),
        "/api/index.php/character/bulk_over_limit" => bulk_over_limit(request, database),
        "/api/index.php/character/add_character_from_town" => {
            add_character_from_town(request, database)
        }
        _ => return None,
    };
    Some(response)
}

// //// 消耗所有可用角色副本完成批量突破 [@x380kkm 2026-08-22] ////
fn bulk_over_limit(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<BulkOverLimitRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let candidates = root
        .get("user_character_list")
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("stored CN character list is missing"))?
        .iter()
        .filter_map(|(character_id, character)| {
            Some((
                character_id.parse::<i64>().ok()?,
                character.get("over_limit_step")?.as_i64()?,
                character.get("stack")?.as_i64()?,
            ))
        })
        .collect::<Vec<_>>();
    let response_time = server_time(database)?;
    let mut character_list = Vec::new();
    let mut total_over_limit_count = 0_i64;

    for (character_id, current_over_limit, current_stack) in candidates {
        let Ok((max_over_limit, _)) = character_limits(character_id) else {
            continue;
        };
        let count = current_stack.min(max_over_limit.saturating_sub(current_over_limit));
        if count <= 0 {
            continue;
        }
        total_over_limit_count = total_over_limit_count.checked_add(count).ok_or_else(|| {
            PersonalServiceError::new("CN bulk over-limit count exceeds supported range")
        })?;
        let character = character_mut(root, character_id).map_err(|_| {
            PersonalServiceError::new("stored CN character disappeared during bulk over limit")
        })?;
        character.insert(
            "over_limit_step".to_owned(),
            Value::from(current_over_limit + count),
        );
        character.insert("stack".to_owned(), Value::from(current_stack - count));
        character_list.push(serialize_character(
            body.viewer_id,
            character_id,
            character,
            response_time,
        ));
    }

    let mission_delta = crate::cn_mission::record_character_over_limit_action(
        root,
        database,
        snapshot.account_id,
        total_over_limit_count,
    )?;
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    let mut response = json!({
        "character_list": character_list,
        "active_mission_list": mission_delta.active_mission_list,
        "mail_arrived": false,
    });
    if !mission_delta.mission_info.is_empty() {
        response
            .as_object_mut()
            .expect("CN bulk over-limit response is an object")
            .insert(
                "mission_info".to_owned(),
                Value::Array(mission_delta.mission_info),
            );
    }
    msgpack_response_at(body.viewer_id, false, response_time, response)
}
// //// /消耗所有可用角色副本完成批量突破 ////

// //// 将城镇加入角色写入玩家快照 [@x380kkm 2026-08-22] ////
fn add_character_from_town(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<AddCharacterFromTownRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.character_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    if character_limits(body.character_id).is_err() {
        return Ok(error_response("400 Bad Request", "character_not_found"));
    }
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let response_time = server_time(database)?;
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    grant_character(root, body.viewer_id, body.character_id, response_time)?;
    let response_character = town_character_response(root, body.character_id, response_time)?;
    let mut encyclopedia_info = Map::new();
    encyclopedia_info.insert(format!("1{}01", body.character_id), json!({"read": false}));
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        json!({
            "character_list": [response_character],
            "encyclopedia_info": encyclopedia_info,
            "mail_arrived": false,
        }),
    )
}
// //// /将城镇加入角色写入玩家快照 ////

// //// 构造城镇加入角色响应 [@x380kkm 2026-08-23] ////
fn town_character_response(
    root: &Map<String, Value>,
    character_id: i64,
    response_time: i64,
) -> Result<Value, PersonalServiceError> {
    let character = root
        .get("user_character_list")
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("stored CN character list is missing"))?
        .get(&character_id.to_string())
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("stored CN character data is invalid"))?;
    let entry_count = character
        .get("entry_count")
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new("stored CN character entry count is missing"))?;
    let evolution_level = character
        .get("evolution_level")
        .and_then(Value::as_i64)
        .ok_or_else(|| {
            PersonalServiceError::new("stored CN character evolution level is missing")
        })?;
    let bond_token_list = character
        .get("bond_token_list")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| {
            PersonalServiceError::new("stored CN character bond token list is missing")
        })?;
    let join_time = character
        .get("join_time")
        .and_then(Value::as_i64)
        .unwrap_or(response_time);
    let update_time = character
        .get("update_time")
        .and_then(Value::as_i64)
        .unwrap_or(response_time);

    Ok(json!({
        "character_id": character_id,
        "entry_count": entry_count,
        "evolution_level": evolution_level,
        "bond_token_list": bond_token_list,
        "create_time": format_client_time(join_time),
        "update_time": format_client_time(update_time),
        "join_time": format_client_time(join_time),
    }))
}
// //// /构造城镇加入角色响应 ////
// //// /分派 CN 角色养成请求 ////

// //// 领取 CN 角色羁绊代币 [@x380kkm 2026-08-23] ////
fn receive_bond_token(
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
    let mut bond_tokens = match character_value(root, body.character_id) {
        Ok(character) => character,
        Err(response) => return Ok(response),
    }
    .get("bond_token_list")
    .and_then(Value::as_array)
    .cloned()
    .ok_or_else(|| PersonalServiceError::new("stored CN bond token list is missing"))?;
    let token_index = usize::try_from(body.mana_board_index - 1)
        .map_err(|_| PersonalServiceError::new("CN bond token index is out of range"))?;
    let token = match bond_tokens.get_mut(token_index) {
        Some(token) => token,
        None => {
            return Ok(error_response(
                "400 Bad Request",
                "invalid_mana_board_index",
            ))
        }
    };
    let bond_token = crate::cn_tutorial::user_info_value(root, "bond_token")?;
    let (new_bond_token, claimed) = match token.get("status").and_then(Value::as_i64) {
        Some(1) => {
            token["status"] = Value::from(2);
            let updated = bond_token.checked_add(1).ok_or_else(|| {
                PersonalServiceError::new("CN bond token exceeds supported range")
            })?;
            (updated, true)
        }
        Some(2) => (bond_token, false),
        _ => return Ok(error_response("400 Bad Request", "bond_token_not_ready")),
    };
    if claimed {
        crate::cn_tutorial::set_user_info_value(root, "bond_token", new_bond_token)?;
        let character = match character_mut(root, body.character_id) {
            Ok(character) => character,
            Err(response) => return Ok(response),
        };
        character.insert(
            "bond_token_list".to_owned(),
            Value::Array(bond_tokens.clone()),
        );
    }
    let server_time = server_time(database)?;
    let character = character_value(root, body.character_id)
        .map_err(|_| PersonalServiceError::new("stored CN character disappeared"))?;
    let response_character =
        bond_token_response_character(body.character_id, character, bond_tokens, server_time);
    let mission_delta =
        crate::cn_mission::record_bond_token_action(root, database, snapshot.account_id, claimed)?;
    if claimed {
        database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    }
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time,
        json!({
            "user_info": {"bond_token": new_bond_token},
            "character_list": [response_character],
            "user_character_mana_node_list": {},
            "item_list": {},
            "evolution": [],
            "mission_info": mission_delta.mission_info,
            "active_mission_list": mission_delta.active_mission_list,
            "mail_arrived": false,
        }),
    )
}
// //// /领取 CN 角色羁绊代币 ////

fn set_illustration_settings(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<SetIllustrationSettingsRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.character_id > 0
                && body.illustration_settings.len() <= 6 =>
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
    let character = match character_mut(root, body.character_id) {
        Ok(character) => character,
        Err(response) => return Ok(response),
    };
    character.insert(
        "illustration_settings".to_owned(),
        Value::Array(
            body.illustration_settings
                .into_iter()
                .map(Value::from)
                .collect(),
        ),
    );
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        Value::Object(Map::new()),
    )
}

// //// 更新 CN 角色保护状态 [@x380kkm 2026-08-24] ////
fn set_protection(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<SetProtectionRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.protect_character_ids.len() <= 100
                && body.unprotect_character_ids.len() <= 100 =>
        {
            body
        }
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    if body
        .protect_character_ids
        .iter()
        .chain(&body.unprotect_character_ids)
        .any(|character_id| *character_id <= 0)
    {
        return Ok(error_response("400 Bad Request", "invalid_character_id"));
    }
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let characters = root
        .get_mut("user_character_list")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN user_character_list is missing"))?;
    for (character_ids, protection) in [
        (&body.protect_character_ids, true),
        (&body.unprotect_character_ids, false),
    ] {
        for character_id in character_ids {
            let Some(character) = characters.get_mut(&character_id.to_string()) else {
                continue;
            };
            let character = character
                .as_object_mut()
                .ok_or_else(|| PersonalServiceError::new("stored CN character data is invalid"))?;
            character.insert("protection".to_owned(), Value::from(protection));
        }
    }
    let response_time = server_time(database)?;
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(body.viewer_id, false, response_time, json!({}))
}
// //// /更新 CN 角色保护状态 ////

// //// 按角色主表开启 CN Mana board [@x380kkm 2026-08-23] ////
fn open_mana_board(
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
    let character = match character_mut(root, body.character_id) {
        Ok(character) => character,
        Err(response) => return Ok(response),
    };
    character.insert(
        "mana_board_index".to_owned(),
        Value::from(body.mana_board_index),
    );
    let server_time = server_time(database)?;
    let response_character = json!({
        "viewer_id": body.viewer_id,
        "character_id": body.character_id,
        "mana_board_index": body.mana_board_index,
        "create_time": character_time(character.get("join_time"), server_time),
        "update_time": character_time(character.get("update_time"), server_time),
        "join_time": character_time(character.get("join_time"), server_time),
    });
    let mission_info = if previous_board_index < 2 && body.mana_board_index >= 2 {
        crate::cn_mission::record_mana_board_open_action(root, database, snapshot.account_id)?
    } else {
        Vec::new()
    };
    let free_vmoney = crate::cn_tutorial::user_info_value(root, "free_vmoney")?;
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    let mut response = json!({
        "user_info": {"free_vmoney": free_vmoney},
        "character_list": [response_character],
        "mail_arrived": false,
    });
    if !mission_info.is_empty() {
        response
            .as_object_mut()
            .expect("CN Mana board response is an object")
            .insert("mission_info".to_owned(), Value::Array(mission_info));
    }
    msgpack_response_at(body.viewer_id, false, server_time, response)
}
// //// /按角色主表开启 CN Mana board ////

fn over_limit(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<OverLimitRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.character_id > 0 && body.over_limit_count > 0 => {
            body
        }
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    if !body.use_stack && body.item_id.is_none() {
        return Ok(error_response("400 Bad Request", "invalid_request_body"));
    }
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let stored = match character_value(root, body.character_id) {
        Ok(character) => character,
        Err(response) => return Ok(response),
    };
    let current_over_limit = stored
        .get("over_limit_step")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let new_over_limit = current_over_limit
        .checked_add(body.over_limit_count)
        .ok_or_else(|| PersonalServiceError::new("CN character over limit exceeds range"))?;
    let (max_over_limit, _) = match character_limits(body.character_id) {
        Ok(limits) => limits,
        Err(_) => return Ok(error_response("400 Bad Request", "character_not_found")),
    };
    if new_over_limit > max_over_limit {
        return Ok(error_response("400 Bad Request", "over_limit_exceeded"));
    }
    let current_stack = stored
        .get("stack")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let mut response_items = Map::new();
    let after_stack = if body.use_stack {
        let after_stack = match current_stack.checked_sub(body.over_limit_count) {
            Some(after_stack) => after_stack,
            None => return Ok(error_response("400 Bad Request", "not_enough_stack")),
        };
        if after_stack < 0 {
            return Ok(error_response("400 Bad Request", "not_enough_stack"));
        }
        after_stack
    } else {
        let item_id = body.item_id.unwrap_or_default();
        if !matches!(item_id, 10_001..=10_003) {
            return Ok(error_response("400 Bad Request", "invalid_uncapping_item"));
        }
        let item_list = require_object(root, "item_list")?;
        let current = item_list
            .get(&item_id.to_string())
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let after = match current.checked_sub(body.over_limit_count) {
            Some(after) => after,
            None => {
                return Ok(error_response(
                    "400 Bad Request",
                    "not_enough_uncapping_item",
                ))
            }
        };
        if after < 0 {
            return Ok(error_response(
                "400 Bad Request",
                "not_enough_uncapping_item",
            ));
        }
        item_list.insert(item_id.to_string(), Value::from(after));
        response_items.insert(item_id.to_string(), Value::from(after));
        current_stack
    };
    let character = match character_mut(root, body.character_id) {
        Ok(character) => character,
        Err(response) => return Ok(response),
    };
    character.insert("over_limit_step".to_owned(), Value::from(new_over_limit));
    character.insert("stack".to_owned(), Value::from(after_stack));
    let server_time = server_time(database)?;
    let response_character = json!({
        "character_id": body.character_id,
        "over_limit_step": new_over_limit,
        "stack": after_stack,
        "create_time": character_time(character.get("join_time"), server_time),
        "update_time": character_time(character.get("update_time"), server_time),
        "join_time": character_time(character.get("join_time"), server_time),
    });
    let mission_delta = crate::cn_mission::record_character_over_limit_action(
        root,
        database,
        snapshot.account_id,
        body.over_limit_count,
    )?;
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    let mut response = json!({
        "character_list": [response_character],
        "item_list": response_items,
        "active_mission_list": mission_delta.active_mission_list,
        "mail_arrived": false,
    });
    if !mission_delta.mission_info.is_empty() {
        response
            .as_object_mut()
            .expect("CN character over-limit response is an object")
            .insert(
                "mission_info".to_owned(),
                Value::Array(mission_delta.mission_info),
            );
    }
    msgpack_response_at(body.viewer_id, false, server_time, response)
}

fn character_value<'a>(
    root: &'a Map<String, Value>,
    character_id: i64,
) -> Result<&'a Map<String, Value>, HttpResponse> {
    root.get("user_character_list")
        .and_then(Value::as_object)
        .and_then(|characters| characters.get(&character_id.to_string()))
        .and_then(Value::as_object)
        .ok_or_else(|| error_response("400 Bad Request", "character_not_owned"))
}

fn character_mut<'a>(
    root: &'a mut Map<String, Value>,
    character_id: i64,
) -> Result<&'a mut Map<String, Value>, HttpResponse> {
    root.get_mut("user_character_list")
        .and_then(Value::as_object_mut)
        .and_then(|characters| characters.get_mut(&character_id.to_string()))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| error_response("400 Bad Request", "character_not_owned"))
}

// //// 构造 CN 羁绊代币响应角色 [@x380kkm 2026-08-23] ////
fn bond_token_response_character(
    character_id: i64,
    character: &Map<String, Value>,
    bond_tokens: Vec<Value>,
    server_time: i64,
) -> Value {
    let evolution_level = character
        .get("evolution_level")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    json!({
        "character_id": character_id,
        "evolution_level": evolution_level,
        "evolution_img_level": evolution_level,
        "create_time": character_time(character.get("join_time"), server_time),
        "update_time": character_time(character.get("update_time"), server_time),
        "join_time": character_time(character.get("join_time"), server_time),
        "bond_token_list": bond_tokens,
    })
}
// //// /构造 CN 羁绊代币响应角色 ////

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

fn serialize_character(
    viewer_id: i64,
    character_id: i64,
    character: &Map<String, Value>,
    server_time: i64,
) -> Value {
    let mut response = character.clone();
    response.insert("viewer_id".to_owned(), Value::from(viewer_id));
    response.insert("character_id".to_owned(), Value::from(character_id));
    response.insert(
        "create_time".to_owned(),
        Value::String(character_time(character.get("join_time"), server_time)),
    );
    response.insert(
        "update_time".to_owned(),
        Value::String(character_time(character.get("update_time"), server_time)),
    );
    response.insert(
        "join_time".to_owned(),
        Value::String(character_time(character.get("join_time"), server_time)),
    );
    Value::Object(response)
}

fn character_time(value: Option<&Value>, server_time: i64) -> String {
    value
        .and_then(Value::as_i64)
        .map(format_client_time)
        .or_else(|| value.and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_else(|| format_client_time(server_time))
}

// //// 读取 CN 角色主表属性 [@x380kkm 2026-08-23] ////
pub(crate) fn character_asset_data(
    character_id: i64,
) -> Result<CharacterAssetData, PersonalServiceError> {
    let document = CHARACTER_DATA.get_or_init(|| {
        serde_json::from_str::<Value>(CHARACTER_ASSET)
            .map_err(|error| format!("failed to decode character asset: {error}"))
    });
    let character = document
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?
        .get(&character_id.to_string())
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("CN character asset is missing"))?;
    let rarity = character
        .get("rarity")
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new("CN character rarity is missing"))?;
    let element = character
        .get("element")
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new("CN character element is missing"))?;
    Ok(CharacterAssetData { rarity, element })
}

pub(crate) fn character_mana_board_count(character_id: i64) -> Result<usize, PersonalServiceError> {
    let document = MANA_NODE_DATA.get_or_init(|| {
        serde_json::from_str::<Value>(MANA_NODE_ASSET)
            .map_err(|error| format!("failed to decode Mana node asset: {error}"))
    });
    let document = document
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    let Some(boards) = document
        .get(&character_id.to_string())
        .and_then(Value::as_object)
    else {
        return Ok(0);
    };
    Ok(boards.len())
}

fn required_open_mana_board_exp(rarity: i64) -> Option<i64> {
    match rarity {
        3 => Some(37_241),
        4 => Some(76_272),
        5 => Some(153_988),
        _ => None,
    }
}

fn required_open_mana_board_over_limit(rarity: i64) -> Result<i64, PersonalServiceError> {
    match rarity {
        1 => Ok(10),
        2 => Ok(8),
        3 => Ok(6),
        4 => Ok(4),
        5 => Ok(2),
        _ => Err(PersonalServiceError::new("CN character rarity is invalid")),
    }
}

fn character_limits(character_id: i64) -> Result<(i64, i64), PersonalServiceError> {
    let rarity = character_asset_data(character_id)?.rarity;
    let max_over_limit = match rarity {
        1 => 12,
        2 => 10,
        3 => 8,
        4 => 6,
        5 => 4,
        _ => return Err(PersonalServiceError::new("CN character rarity is invalid")),
    };
    let required_over_limit = required_open_mana_board_over_limit(rarity)?;
    Ok((max_over_limit, required_over_limit))
}
// //// /读取 CN 角色主表属性 ////

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}
