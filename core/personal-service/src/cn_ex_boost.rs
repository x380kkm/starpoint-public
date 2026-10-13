// audience: internal
// # personal-service-cn-ex-boost
// EX 材料按主表消耗, 抽取结果和请求凭据随玩家快照保存. 暂存结果通过载入响应恢复确认.

mod catalog;
mod timestamps;

pub(crate) use timestamps::normalize_saved_character_times;

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_character::serialize_character;
use crate::cn_request_receipt::RequestReceipt;
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, player_snapshot, require_object, require_root,
};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;

pub(crate) const RECEIPTS_FIELD: &str = "ex_boost_request_receipts";
const CHARACTER_ASSET: &str = include_str!("../../../assets/character.json");
static CHARACTER_DATA: OnceLock<Result<Value, String>> = OnceLock::new();

#[derive(Deserialize)]
struct DrawRequest {
    viewer_id: i64,
    character_id: i64,
    cost_item_id: i64,
    api_count: Option<i64>,
}

#[derive(Deserialize)]
struct SelectRequest {
    viewer_id: i64,
    is_confirm: bool,
    api_count: Option<i64>,
}

#[derive(Deserialize)]
struct DrawResult {
    character_id: i64,
    status_id: i64,
    #[serde(rename = "ability_id_list")]
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

// //// 消耗素材并保存 EX 抽取结果和请求凭据 [@x380kkm 2026-09-23] ////
fn draw(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    auto_accept: bool,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<DrawRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.character_id > 0
                && body.cost_item_id > 0
                && body.api_count.map_or(true, |count| count >= 0) =>
        {
            body
        }
        _ => return Ok(error_response("invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let action = if auto_accept { "first_draw" } else { "draw" };
    let receipt = body.api_count.map(|count| {
        RequestReceipt::new(
            "ex_boost",
            action,
            body.viewer_id,
            count,
            json!({"character_id":body.character_id,"cost_item_id":body.cost_item_id}),
        )
    });
    if let Some(response) = receipt
        .as_ref()
        .map(|value| value.replay(root))
        .transpose()?
        .flatten()
    {
        return Ok(response);
    }
    if root
        .get("pending_ex_boost")
        .is_some_and(|value| !value.is_null())
    {
        return Ok(error_response("ex_boost_draw_pending"));
    }
    let Some(material) = catalog::material(body.cost_item_id)? else {
        return Ok(error_response("invalid_cost_item"));
    };
    let Some(character_asset) = character_asset(body.character_id)? else {
        return Ok(error_response("character_not_found"));
    };
    if !character_asset
        .get("element")
        .and_then(Value::as_i64)
        .is_some_and(|element| material.elements.contains(&element))
    {
        return Ok(error_response("invalid_cost_item_element"));
    }
    let Some(character) = root
        .get("user_character_list")
        .and_then(Value::as_object)
        .and_then(|characters| characters.get(&body.character_id.to_string()))
        .and_then(Value::as_object)
    else {
        return Ok(error_response("character_not_owned"));
    };
    let has_ex_boost = character.get("ex_boost").is_some_and(Value::is_object);
    if auto_accept == has_ex_boost {
        return Ok(error_response(if auto_accept {
            "ex_boost_already_initialized"
        } else {
            "ex_boost_not_initialized"
        }));
    }
    let max_over_limit = match character_asset.get("rarity").and_then(Value::as_i64) {
        Some(1) => 12,
        Some(2) => 10,
        Some(3) => 8,
        Some(4) => 6,
        Some(5) => 4,
        _ => return Ok(error_response("invalid_character_rarity")),
    };
    if character
        .get("over_limit_step")
        .and_then(Value::as_i64)
        .unwrap_or_default()
        < max_over_limit
    {
        return Ok(error_response("character_not_max_over_limit"));
    }
    let items = require_object(root, "item_list")?;
    let item_key = body.cost_item_id.to_string();
    let remaining_items = match items
        .get(&item_key)
        .and_then(Value::as_i64)
        .unwrap_or_default()
        .checked_sub(material.cost_item_count)
    {
        Some(value) if value >= 0 => value,
        _ => return Ok(error_response("not_enough_cost_item")),
    };
    let result = catalog::draw(body.character_id, material)?;
    items.insert(item_key.clone(), Value::from(remaining_items));
    let response_time = server_time(database)?;
    let mail_arrived = database.has_unreceived_mail(snapshot.account_id, response_time)?;
    let data = if auto_accept {
        let character = apply_result(root, &result, response_time)?;
        json!({
            "character_list": [serialize_character(body.viewer_id, body.character_id, character, response_time)],
            "item_list": {item_key:remaining_items},
            "mail_arrived": mail_arrived,
        })
    } else {
        root.insert("pending_ex_boost".to_owned(), pending_draw_value(&result));
        json!({
            "character_id": body.character_id,
            "draw_result": draw_value(&result),
            "item_list": {item_key:remaining_items},
            "mail_arrived": mail_arrived,
        })
    };
    if let Some(receipt) = receipt {
        receipt.record(root, &data, response_time)?;
    }
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(body.viewer_id, false, response_time, data)
}
// //// /消耗素材并保存 EX 抽取结果和请求凭据 ////

// //// 接受或丢弃暂存结果并重放已提交的选择 [@x380kkm 2026-09-23] ////
fn select(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<SelectRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.api_count.map_or(true, |count| count >= 0) => body,
        _ => return Ok(error_response("invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let receipt = body.api_count.map(|count| {
        RequestReceipt::new(
            "ex_boost",
            "select",
            body.viewer_id,
            count,
            json!({"is_confirm":body.is_confirm}),
        )
    });
    if let Some(response) = receipt
        .as_ref()
        .map(|value| value.replay(root))
        .transpose()?
        .flatten()
    {
        return Ok(response);
    }
    let pending = match root.remove("pending_ex_boost") {
        Some(pending) => pending,
        None => return Ok(error_response("ex_boost_draw_not_found")),
    };
    let result = decode_pending_draw(&pending)?;
    let response_time = server_time(database)?;
    let mail_arrived = database.has_unreceived_mail(snapshot.account_id, response_time)?;
    let data = if body.is_confirm {
        let character = apply_result(root, &result, response_time)?;
        json!({
            "character_list": [serialize_character(body.viewer_id, result.character_id, character, response_time)],
            "mail_arrived": mail_arrived,
        })
    } else {
        json!({"mail_arrived":mail_arrived})
    };
    if let Some(receipt) = receipt {
        receipt.record(root, &data, response_time)?;
    }
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(body.viewer_id, false, response_time, data)
}
// //// /接受或丢弃暂存结果并重放已提交的选择 ////

// //// 将暂存结果投影为客户端启动时的 EX 确认数据 [@x380kkm 2026-09-23] ////
pub(crate) fn project_pending_draw(
    root: &mut Map<String, Value>,
) -> Result<(), PersonalServiceError> {
    let pending = root
        .get("pending_ex_boost")
        .filter(|value| !value.is_null())
        .map(decode_pending_draw)
        .transpose()?;
    root.insert(
        "ex_boost_draw_result".to_owned(),
        pending.map_or(Value::Null, |result| {
            json!({
                "character_id":result.character_id,"draw_result":draw_value(&result),
            })
        }),
    );
    Ok(())
}
// //// /将暂存结果投影为客户端启动时的 EX 确认数据 ////

// //// 写入角色已接受的 EX 能力及更新时间 [@x380kkm 2026-09-23] ////
fn apply_result<'a>(
    root: &'a mut Map<String, Value>,
    result: &DrawResult,
    response_time: i64,
) -> Result<&'a Map<String, Value>, PersonalServiceError> {
    let character = require_object(root, "user_character_list")?
        .get_mut(&result.character_id.to_string())
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN EX character data is invalid"))?;
    character.insert("ex_boost".to_owned(), draw_value(result));
    character.insert("update_time".to_owned(), Value::from(response_time));
    Ok(character)
}
// //// /写入角色已接受的 EX 能力及更新时间 ////

fn character_asset(id: i64) -> Result<Option<&'static Map<String, Value>>, PersonalServiceError> {
    Ok(CHARACTER_DATA
        .get_or_init(|| {
            serde_json::from_str(CHARACTER_ASSET)
                .map_err(|error| format!("failed to decode CN character asset: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?
        .get(id.to_string())
        .and_then(Value::as_object))
}

fn draw_value(result: &DrawResult) -> Value {
    json!({"status_id": result.status_id, "ability_id_list": result.ability_ids})
}

fn pending_draw_value(result: &DrawResult) -> Value {
    json!({"character_id":result.character_id,"status_id":result.status_id,"ability_id_list":result.ability_ids})
}

fn decode_pending_draw(value: &Value) -> Result<DrawResult, PersonalServiceError> {
    serde_json::from_value(value.clone()).map_err(|error| {
        PersonalServiceError::new(format!("stored CN EX draw is invalid: {error}"))
    })
}

fn error_response(code: &str) -> HttpResponse {
    HttpResponse::json("400 Bad Request", json!({"error":code}).to_string())
}
