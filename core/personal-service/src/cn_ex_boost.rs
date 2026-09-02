// audience: internal
// # personal-service-cn-ex-boost
//
// 该模块实现 CN 角色 EX 能力抽取、暂存和确认.

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, format_client_time, player_snapshot, require_object,
    require_root,
};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use getrandom::getrandom;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;

const EX_BOOST_ASSET: &str = include_str!("../../../assets/ex_boost.json");
const EX_STATUS_ASSET: &str = include_str!("../../../assets/ex_status.json");
const EX_ABILITY_ASSET: &str = include_str!("../../../assets/ex_ability.json");
const CHARACTER_ASSET: &str = include_str!("../../../assets/character.json");
static EX_BOOST_DATA: OnceLock<Result<Value, String>> = OnceLock::new();
static EX_STATUS_DATA: OnceLock<Result<Value, String>> = OnceLock::new();
static EX_ABILITY_DATA: OnceLock<Result<Value, String>> = OnceLock::new();
static CHARACTER_DATA: OnceLock<Result<Value, String>> = OnceLock::new();

const ABILITY_GROUP_A_PREFIXES: &[&str] = &[
    "atk_self_",
    "skilldamage_self_",
    "directdamage_self_",
    "abilitydamage_self_",
    "abilitydagame_self_",
    "atk_party_",
    "skilldamage_party_",
    "directdamage_party_",
    "abilitydamage_party_",
    "abilitydagame_party_",
    "powerflipdamage_",
    "hp_self_",
];
const ABILITY_GROUP_B_OVERRIDES: &[&str] = &["powerflipdamage_buffextend_"];

#[derive(Deserialize)]
struct DrawRequest {
    viewer_id: i64,
    character_id: i64,
    cost_item_id: i64,
}

#[derive(Deserialize)]
struct SelectRequest {
    viewer_id: i64,
    is_confirm: bool,
}

struct DrawResult {
    character_id: i64,
    status_id: i64,
    ability_ids: Vec<i64>,
}

// //// 分派 CN EX 能力请求 [@x380kkm 2026-08-22] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    let response = match request.path() {
        "/api/index.php/ex_boost/draw" => draw(request, database, false),
        "/api/index.php/ex_boost/first_draw" => draw(request, database, true),
        "/api/index.php/ex_boost/select" => select(request, database),
        _ => return None,
    };
    Some(response)
}
// //// /分派 CN EX 能力请求 ////

// //// 消耗素材并生成 EX 能力 [@x380kkm 2026-08-22] ////
fn draw(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    auto_accept: bool,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<DrawRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.character_id > 0 && body.cost_item_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let material = match asset_object(&EX_BOOST_DATA, EX_BOOST_ASSET, body.cost_item_id)? {
        Some(material) => material,
        None => return Ok(error_response("400 Bad Request", "invalid_cost_item")),
    };
    let character_asset = match asset_object(&CHARACTER_DATA, CHARACTER_ASSET, body.character_id)? {
        Some(character) => character,
        None => return Ok(error_response("400 Bad Request", "character_not_found")),
    };
    if material
        .get("element")
        .and_then(Value::as_i64)
        .is_some_and(|element| {
            character_asset.get("element").and_then(Value::as_i64) != Some(element)
        })
    {
        return Ok(error_response(
            "400 Bad Request",
            "invalid_cost_item_element",
        ));
    }
    let tier = material
        .get("tier")
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new("CN EX material tier is missing"))?;
    let required_count = material
        .get("count")
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new("CN EX material count is missing"))?;
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let character = root
        .get("user_character_list")
        .and_then(Value::as_object)
        .and_then(|characters| characters.get(&body.character_id.to_string()))
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| PersonalServiceError::new("CN EX character is not owned"))?;
    let rarity = character_asset
        .get("rarity")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let max_over_limit = match rarity {
        1 => 12,
        2 => 10,
        3 => 8,
        4 => 6,
        5 => 4,
        _ => {
            return Ok(error_response(
                "400 Bad Request",
                "invalid_character_rarity",
            ))
        }
    };
    if character
        .get("over_limit_step")
        .and_then(Value::as_i64)
        .unwrap_or_default()
        < max_over_limit
    {
        return Ok(error_response(
            "400 Bad Request",
            "character_not_max_over_limit",
        ));
    }
    let items = require_object(root, "item_list")?;
    let item_key = body.cost_item_id.to_string();
    let remaining_items = match items
        .get(&item_key)
        .and_then(Value::as_i64)
        .unwrap_or_default()
        .checked_sub(required_count)
    {
        Some(value) if value >= 0 => value,
        _ => return Ok(error_response("400 Bad Request", "not_enough_cost_item")),
    };
    items.insert(item_key.clone(), Value::from(remaining_items));
    let result = DrawResult {
        character_id: body.character_id,
        status_id: choose_asset_id(&EX_STATUS_DATA, EX_STATUS_ASSET, tier, 0)?,
        ability_ids: draw_ability_ids(tier, material.contains_key("element"))?,
    };
    let mut item_response = Map::new();
    item_response.insert(item_key, Value::from(remaining_items));
    let response_time = server_time(database)?;
    let data = if auto_accept {
        let character = require_object(root, "user_character_list")?
            .get_mut(&body.character_id.to_string())
            .and_then(Value::as_object_mut)
            .ok_or_else(|| PersonalServiceError::new("stored CN character data is invalid"))?;
        character.insert("ex_boost".to_owned(), draw_value(&result));
        json!({
            "character_list": [serialize_character(body.viewer_id, body.character_id, character, response_time)],
            "item_list": item_response,
            "mail_arrived": false,
        })
    } else {
        root.insert("pending_ex_boost".to_owned(), pending_draw_value(&result));
        json!({
            "character_id": body.character_id,
            "draw_result": draw_value(&result),
            "item_list": item_response,
            "mail_arrived": false,
        })
    };
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(body.viewer_id, false, response_time, data)
}
// //// /消耗素材并生成 EX 能力 ////

// //// 接受或丢弃暂存的 EX 能力 [@x380kkm 2026-08-22] ////
fn select(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<SelectRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let pending = match root.remove("pending_ex_boost") {
        Some(pending) => pending,
        None => return Ok(error_response("400 Bad Request", "ex_boost_draw_not_found")),
    };
    let result = decode_pending_draw(&pending)?;
    let response_time = server_time(database)?;
    let data = if body.is_confirm {
        let character = require_object(root, "user_character_list")?
            .get_mut(&result.character_id.to_string())
            .and_then(Value::as_object_mut)
            .ok_or_else(|| PersonalServiceError::new("stored CN character data is invalid"))?;
        character.insert("ex_boost".to_owned(), draw_value(&result));
        json!({
            "character_list": [serialize_character(body.viewer_id, result.character_id, character, response_time)],
            "mail_arrived": false,
        })
    } else {
        json!({"mail_arrived": false})
    };
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(body.viewer_id, false, response_time, data)
}
// //// /接受或丢弃暂存的 EX 能力 ////

fn serialize_character(
    viewer_id: i64,
    character_id: i64,
    character: &Map<String, Value>,
    response_time: i64,
) -> Value {
    let mut response = character.clone();
    response.insert("viewer_id".to_owned(), Value::from(viewer_id));
    response.insert("character_id".to_owned(), Value::from(character_id));
    let time = format_client_time(response_time);
    response.insert("create_time".to_owned(), Value::String(time.clone()));
    response.insert("update_time".to_owned(), Value::String(time.clone()));
    response.insert("join_time".to_owned(), Value::String(time));
    Value::Object(response)
}

fn choose_asset_id(
    cache: &'static OnceLock<Result<Value, String>>,
    asset: &'static str,
    tier: i64,
    offset: usize,
) -> Result<i64, PersonalServiceError> {
    let document = parsed_asset(cache, asset)?;
    let values = document
        .get(tier.to_string())
        .and_then(Value::as_array)
        .filter(|values| !values.is_empty())
        .ok_or_else(|| PersonalServiceError::new("CN EX draw pool is missing"))?;
    values[(random_index(values.len())? + offset) % values.len()]
        .as_i64()
        .ok_or_else(|| PersonalServiceError::new("CN EX draw pool id is invalid"))
}

fn draw_ability_ids(tier: i64, uses_element_stone: bool) -> Result<Vec<i64>, PersonalServiceError> {
    let weights = ability_rarity_weights(tier, uses_element_stone)
        .ok_or_else(|| PersonalServiceError::new("CN EX ability weights are missing"))?;
    [true, false]
        .into_iter()
        .zip(weights)
        .filter_map(|(is_group_a, rarity_weights)| {
            choose_ability_id(is_group_a, rarity_weights).transpose()
        })
        .collect()
}

fn choose_ability_id(
    is_group_a: bool,
    rarity_weights: [usize; 3],
) -> Result<Option<i64>, PersonalServiceError> {
    let Some(rarity) = choose_weighted_rarity(rarity_weights)? else {
        return Ok(None);
    };
    let document = parsed_asset(&EX_ABILITY_DATA, EX_ABILITY_ASSET)?;
    let abilities = document
        .as_object()
        .ok_or_else(|| PersonalServiceError::new("CN EX ability asset is invalid"))?;
    let pool = abilities
        .iter()
        .filter_map(|(id, row)| {
            let name = ability_name(row)?;
            (ability_is_group_a(name) == is_group_a && ability_rarity(name) == rarity)
                .then(|| id.parse::<i64>().ok())
                .flatten()
        })
        .collect::<Vec<_>>();
    if pool.is_empty() {
        return Err(PersonalServiceError::new("CN EX ability pool is missing"));
    }
    Ok(Some(pool[random_index(pool.len())?]))
}

fn choose_weighted_rarity(weights: [usize; 3]) -> Result<Option<usize>, PersonalServiceError> {
    let total = weights.iter().sum::<usize>();
    if total == 0 {
        return Ok(None);
    }
    let roll = random_index(total)?;
    let mut cumulative = 0;
    for rarity in (1..=3).rev() {
        cumulative += weights[rarity - 1];
        if roll < cumulative {
            return Ok(Some(rarity));
        }
    }
    Ok(None)
}

fn ability_rarity_weights(tier: i64, uses_element_stone: bool) -> Option<[[usize; 3]; 2]> {
    match (uses_element_stone, tier) {
        (false, 1) => Some([[188, 413, 150], [170, 375, 136]]),
        (false, 2) => Some([[166, 457, 208], [151, 415, 189]]),
        (false, 3) => Some([[0, 100, 900], [0, 91, 818]]),
        (true, 1) => Some([[200, 400, 200], [182, 364, 182]]),
        (true, 2) => Some([[178, 445, 267], [162, 405, 243]]),
        (true, 3) => Some([[0, 0, 1000], [0, 0, 909]]),
        _ => None,
    }
}

fn ability_name(row: &Value) -> Option<&str> {
    row.as_array()?.first()?.as_array()?.first()?.as_str()
}

fn ability_is_group_a(name: &str) -> bool {
    !ABILITY_GROUP_B_OVERRIDES
        .iter()
        .any(|prefix| name.starts_with(prefix))
        && ABILITY_GROUP_A_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
}

fn ability_rarity(name: &str) -> usize {
    if name.ends_with("_r5") {
        3
    } else if name.ends_with("_r4") {
        2
    } else {
        1
    }
}

fn random_index(length: usize) -> Result<usize, PersonalServiceError> {
    let mut bytes = [0_u8; 8];
    getrandom(&mut bytes).map_err(|error| {
        PersonalServiceError::new(format!("failed to draw CN EX boost: {error}"))
    })?;
    Ok(u64::from_le_bytes(bytes) as usize % length)
}

fn asset_object(
    cache: &'static OnceLock<Result<Value, String>>,
    asset: &'static str,
    id: i64,
) -> Result<Option<Map<String, Value>>, PersonalServiceError> {
    Ok(parsed_asset(cache, asset)?
        .get(id.to_string())
        .and_then(Value::as_object)
        .cloned())
}

fn parsed_asset(
    cache: &'static OnceLock<Result<Value, String>>,
    asset: &'static str,
) -> Result<&'static Value, PersonalServiceError> {
    cache
        .get_or_init(|| {
            serde_json::from_str(asset)
                .map_err(|error| format!("failed to decode CN EX asset: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

fn draw_value(result: &DrawResult) -> Value {
    json!({"status_id": result.status_id, "ability_id_list": result.ability_ids})
}

fn pending_draw_value(result: &DrawResult) -> Value {
    json!({
        "character_id": result.character_id,
        "status_id": result.status_id,
        "ability_id_list": result.ability_ids,
    })
}

fn decode_pending_draw(value: &Value) -> Result<DrawResult, PersonalServiceError> {
    Ok(DrawResult {
        character_id: value
            .get("character_id")
            .and_then(Value::as_i64)
            .ok_or_else(|| PersonalServiceError::new("stored CN EX character id is invalid"))?,
        status_id: value
            .get("status_id")
            .and_then(Value::as_i64)
            .ok_or_else(|| PersonalServiceError::new("stored CN EX status id is invalid"))?,
        ability_ids: value
            .get("ability_id_list")
            .and_then(Value::as_array)
            .ok_or_else(|| PersonalServiceError::new("stored CN EX ability list is invalid"))?
            .iter()
            .map(|value| {
                value
                    .as_i64()
                    .ok_or_else(|| PersonalServiceError::new("stored CN EX ability id is invalid"))
            })
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}
