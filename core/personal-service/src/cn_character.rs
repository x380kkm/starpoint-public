// audience: internal
// # personal-service-cn-character
//
// 该模块实现 CN 角色养成和城镇领取协议. 资产和任务在同一事务提交, 突破请求凭据随玩家快照保存.

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, player_snapshot, require_root,
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
pub(crate) use over_limit::RECEIPTS_FIELD;
pub(crate) use response::{character_time, complete_character_response_times, project_load_character_times};
pub(crate) use town::join_story_characters;
type CharacterAction =
    fn(&HttpRequest, &mut ServiceDatabase) -> Result<HttpResponse, PersonalServiceError>;

mod board;
mod bond_token;
mod over_limit;
mod response;
mod town;

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

// //// 分派 CN 角色养成请求 [@x380kkm 2026-07-24] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    let action: CharacterAction = match request.path() {
        "/api/index.php/character/set_illustration_settings" => set_illustration_settings,
        "/api/index.php/character/set_protection" => set_protection,
        "/api/index.php/character/receive_bond_token" => bond_token::receive,
        "/api/index.php/character/open_mana_board" => board::open,
        "/api/index.php/character/over_limit" => over_limit::apply,
        "/api/index.php/character/bulk_over_limit" => over_limit::apply_bulk,
        "/api/index.php/character/add_character_from_town" => town::add_character_from_town,
        _ => return None,
    };
    Some(
        database
            .with_player_settlement(|database| action(request, database).map(Some))
            .and_then(|response| {
                response.ok_or_else(|| {
                    PersonalServiceError::new("CN character settlement returned no response")
                })
            }),
    )
}

// //// /分派 CN 角色养成请求 ////

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

pub(crate) fn serialize_character(
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
