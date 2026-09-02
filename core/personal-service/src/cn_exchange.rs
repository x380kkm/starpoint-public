// audience: internal
// # personal-service-cn-exchange
//
// 该模块实现 CN 星屑兑换并把费用与奖励写入玩家快照.

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{
    create_character_response, create_stored_character, decode_player_data, encode_player_data,
    player_snapshot, require_object, require_root, set_user_info_value, user_info_value,
};
use crate::database::{parse_iso_timestamp, ReceiveHistoryEntry, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;

const EXCHANGE_ASSET: &str = include_str!("../../../assets/star_crumb_exchange.json");
const EXCHANGE_COST_ASSET: &str = include_str!("../../../assets/star_crumb_exchange_cost.json");
const BOND_TOKEN_EXCHANGE_ASSET: &str =
    include_str!("../../../assets/bond_token_exchange.json");
static EXCHANGE_DATA: OnceLock<Result<Value, String>> = OnceLock::new();
static EXCHANGE_COSTS: OnceLock<Result<Value, String>> = OnceLock::new();
static BOND_TOKEN_EXCHANGE_DATA: OnceLock<Result<Value, String>> = OnceLock::new();

const BOND_TOKEN_EXCHANGE_COUNT_PREFIX: &str = "bond_token_exchange:";
const JAPAN_STANDARD_OFFSET_SECONDS: i64 = 8 * 60 * 60;

#[derive(Deserialize)]
struct ExchangeRequest {
    viewer_id: i64,
    exchange_id: i64,
}

#[derive(Deserialize)]
struct BondTokenExchangeRequest {
    viewer_id: i64,
    equipment_id: i64,
}

#[derive(Deserialize)]
struct ViewerRequest {
    viewer_id: i64,
}

struct ExchangeEntry {
    kind: i64,
    target_id: i64,
    cost: i64,
}

#[derive(Clone, Copy)]
struct BondTokenExchangeEntry {
    equipment_id: i64,
    cost: i64,
    stock: i64,
    start_time: i64,
    end_time: i64,
}

// //// 分派 CN 星屑兑换请求 [@x380kkm 2026-08-22] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    let response = match request.path() {
        "/api/index.php/exchange/star_crumb" => exchange_star_crumb(request, database),
        "/api/index.php/exchange/get_bond_token_exchange_list" => {
            bond_token_exchange_list(request, database)
        }
        "/api/index.php/exchange/bond_token" => exchange_bond_token(request, database),
        _ => return None,
    };
    Some(response)
}
// //// /分派 CN 星屑兑换请求 ////

// //// 返回羁绊证兑换目录 [@x380kkm 2026-08-29] ////
fn bond_token_exchange_list(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ViewerRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let list = bond_token_exchange_entries()?
        .into_iter()
        .map(|entry| {
            let exchange_count = bond_token_exchange_count(root, entry.equipment_id);
            json!({
                "equipment_id": entry.equipment_id,
                "exchange_count": exchange_count,
            })
        })
        .collect::<Vec<_>>();
    msgpack_response_at(body.viewer_id, false, server_time(database)?, list)
}
// //// /返回羁绊证兑换目录 ////

// //// 扣除羁绊证并发放装备 [@x380kkm 2026-08-29] ////
fn exchange_bond_token(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<BondTokenExchangeRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.equipment_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let response_time = server_time(database)?;
    let entry = match bond_token_exchange_entry(body.equipment_id)? {
        Some(entry) => entry,
        None => return Ok(error_response("400 Bad Request", "exchange_not_found")),
    };
    if response_time < entry.start_time || response_time > entry.end_time {
        return Ok(error_response("400 Bad Request", "shop_period_error"));
    }
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let exchange_count = bond_token_exchange_count(root, entry.equipment_id);
    if exchange_count >= entry.stock {
        return Ok(error_response("400 Bad Request", "shop_out_of_stock"));
    }
    let current_bond_token = user_info_value(root, "bond_token")?;
    let remaining_bond_token = match current_bond_token.checked_sub(entry.cost) {
        Some(value) if value >= 0 => value,
        _ => return Ok(error_response("400 Bad Request", "not_enough_bond_token")),
    };
    set_user_info_value(root, "bond_token", remaining_bond_token)?;
    let equipment = add_bond_token_equipment(root, entry.equipment_id)?;
    let new_exchange_count = exchange_count
        .checked_add(1)
        .ok_or_else(|| PersonalServiceError::new("CN bond token exchange count exceeds range"))?;
    set_bond_token_exchange_count(root, entry.equipment_id, new_exchange_count)?;
    database.save_player_snapshot_with_receive_history(
        snapshot.account_id,
        &encode_player_data(&player_data)?,
        &format!(
            "bond-token-exchange:{}:{}:{}",
            entry.equipment_id, exchange_count, body.viewer_id
        ),
        response_time,
        &[ReceiveHistoryEntry::reward(6, Some(entry.equipment_id), 1)],
    )?;
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        json!({
            "user_info": {"bond_token": remaining_bond_token},
            "character_list": null,
            "item_list": null,
            "equipment_list": [equipment],
            "active_mission_list": null,
            "mission_info": null,
            "over_max": null,
            "mail_arrived": false,
            "config": null,
            "user_daily_challenge_point_list": null,
            "encyclopedia_info": null,
            "fund_receive_list": null,
            "monthly_charge_bonus_info": null,
            "crazy_gacha_result_list": null,
        }),
    )
}
// //// /扣除羁绊证并发放装备 ////

fn bond_token_exchange_entries() -> Result<Vec<BondTokenExchangeEntry>, PersonalServiceError> {
    let document = parsed_asset(
        BOND_TOKEN_EXCHANGE_ASSET,
        &BOND_TOKEN_EXCHANGE_DATA,
        "failed to decode CN bond token exchanges",
    )?;
    let document = document
        .as_object()
        .ok_or_else(|| PersonalServiceError::new("CN bond token exchange asset is invalid"))?;
    document
        .iter()
        .map(|(equipment_id, row)| {
            let equipment_id = equipment_id.parse::<i64>().map_err(|error| {
                PersonalServiceError::new(format!(
                    "CN bond token equipment id is invalid: {error}"
                ))
            })?;
            parse_bond_token_exchange_entry(equipment_id, row)
        })
        .collect()
}

fn bond_token_exchange_entry(
    equipment_id: i64,
) -> Result<Option<BondTokenExchangeEntry>, PersonalServiceError> {
    let document = parsed_asset(
        BOND_TOKEN_EXCHANGE_ASSET,
        &BOND_TOKEN_EXCHANGE_DATA,
        "failed to decode CN bond token exchanges",
    )?;
    document
        .get(equipment_id.to_string())
        .map(|row| parse_bond_token_exchange_entry(equipment_id, row))
        .transpose()
}

fn parse_bond_token_exchange_entry(
    equipment_id: i64,
    row: &Value,
) -> Result<BondTokenExchangeEntry, PersonalServiceError> {
    let row = row
        .as_array()
        .ok_or_else(|| PersonalServiceError::new("CN bond token exchange row is invalid"))?;
    let field = |index: usize| {
        row.get(index)
            .and_then(Value::as_str)
            .ok_or_else(|| PersonalServiceError::new("CN bond token exchange field is invalid"))
    };
    let cost = field(0)?
        .parse::<i64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| PersonalServiceError::new("CN bond token exchange cost is invalid"))?;
    let stock = field(1)?
        .parse::<i64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| PersonalServiceError::new("CN bond token exchange stock is invalid"))?;
    Ok(BondTokenExchangeEntry {
        equipment_id,
        cost,
        stock,
        start_time: parse_bond_token_exchange_time(field(2)?)?,
        end_time: parse_bond_token_exchange_time(field(3)?)?,
    })
}

fn parse_bond_token_exchange_time(value: &str) -> Result<i64, PersonalServiceError> {
    let normalized = format!("{}.000Z", value.replace(' ', "T"));
    parse_iso_timestamp(&normalized)
        .and_then(|millis| millis.checked_div(1_000))
        .and_then(|seconds| seconds.checked_sub(JAPAN_STANDARD_OFFSET_SECONDS))
        .ok_or_else(|| PersonalServiceError::new("CN bond token exchange time is invalid"))
}

fn bond_token_exchange_count(root: &Map<String, Value>, equipment_id: i64) -> i64 {
    root.get("shop_purchase_counts")
        .and_then(Value::as_object)
        .and_then(|counts| counts.get(&bond_token_exchange_count_key(equipment_id)))
        .and_then(Value::as_i64)
        .unwrap_or_default()
        .max(0)
}

fn set_bond_token_exchange_count(
    root: &mut Map<String, Value>,
    equipment_id: i64,
    count: i64,
) -> Result<(), PersonalServiceError> {
    let counts = root
        .entry("shop_purchase_counts".to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN shop counts are invalid"))?;
    counts.insert(bond_token_exchange_count_key(equipment_id), Value::from(count));
    Ok(())
}

fn bond_token_exchange_count_key(equipment_id: i64) -> String {
    format!("{BOND_TOKEN_EXCHANGE_COUNT_PREFIX}{equipment_id}")
}

fn add_bond_token_equipment(
    root: &mut Map<String, Value>,
    equipment_id: i64,
) -> Result<Value, PersonalServiceError> {
    let equipment_list = require_object(root, "user_equipment_list")?;
    let key = equipment_id.to_string();
    let was_owned = equipment_list.contains_key(&key);
    let equipment = equipment_list
        .entry(key)
        .or_insert_with(|| {
            json!({
                "enhancement_level": 0,
                "level": 1,
                "protection": false,
                "stack": 0,
            })
        })
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN equipment data is invalid"))?;
    let stack = if was_owned {
        equipment
            .get("stack")
            .and_then(Value::as_i64)
            .unwrap_or_default()
            .checked_add(1)
            .ok_or_else(|| PersonalServiceError::new("CN equipment stack exceeds range"))?
    } else {
        equipment
            .get("stack")
            .and_then(Value::as_i64)
            .unwrap_or_default()
    };
    equipment.insert("stack".to_owned(), Value::from(stack));
    Ok(json!({
        "equipment_id": equipment_id,
        "protection": equipment.get("protection").and_then(Value::as_bool).unwrap_or(false),
        "level": equipment.get("level").and_then(Value::as_i64).unwrap_or(1),
        "enhancement_level": equipment.get("enhancement_level").and_then(Value::as_i64).unwrap_or_default(),
        "stack": stack,
    }))
}

// //// 扣除星屑并发放兑换奖励 [@x380kkm 2026-08-22] ////
fn exchange_star_crumb(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ExchangeRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.exchange_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let entry = match exchange_entry(body.exchange_id)? {
        Some(entry) => entry,
        None => return Ok(error_response("400 Bad Request", "exchange_not_found")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let response_time = server_time(database)?;
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let user_info = require_object(root, "user_info")?;
    let current_star_crumb = user_info
        .get("star_crumb")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let remaining_star_crumb = match current_star_crumb.checked_sub(entry.cost) {
        Some(value) if value >= 0 => value,
        _ => return Ok(error_response("400 Bad Request", "not_enough_star_crumb")),
    };
    if exchange_reward_is_owned(root, &entry) {
        return Ok(error_response("400 Bad Request", "exchange_reward_owned"));
    }
    require_object(root, "user_info")?
        .insert("star_crumb".to_owned(), Value::from(remaining_star_crumb));

    let mut character_list = Vec::new();
    let mut equipment_list = Vec::new();
    let mut item_list = Map::new();
    match entry.kind {
        0 => {
            let stored = create_stored_character(entry.target_id, response_time)?;
            let response =
                create_character_response(body.viewer_id, entry.target_id, &stored, response_time);
            require_object(root, "user_character_list")?
                .insert(entry.target_id.to_string(), stored);
            character_list.push(response);
        }
        1 => {
            let items = require_object(root, "item_list")?;
            let key = entry.target_id.to_string();
            let total = items
                .get(&key)
                .and_then(Value::as_i64)
                .unwrap_or_default()
                .checked_add(1)
                .ok_or_else(|| PersonalServiceError::new("CN exchange item count exceeds range"))?;
            items.insert(key.clone(), Value::from(total));
            item_list.insert(key, Value::from(total));
        }
        2 => {
            let equipment = json!({
                "enhancement_level": 0,
                "level": 1,
                "protection": false,
                "stack": 0,
            });
            require_object(root, "user_equipment_list")?
                .insert(entry.target_id.to_string(), equipment.clone());
            equipment_list.push(json!({
                "equipment_id": entry.target_id,
                "protection": false,
                "level": 1,
                "enhancement_level": 0,
                "stack": 0,
            }));
        }
        _ => return Err(PersonalServiceError::new("CN exchange kind is invalid")),
    }
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        json!({
            "user_info": {"star_crumb": remaining_star_crumb},
            "character_list": character_list,
            "item_list": item_list,
            "equipment_list": equipment_list,
            "active_mission_list": null,
            "mission_info": null,
            "over_max": null,
            "mail_arrived": false,
            "config": null,
            "user_daily_challenge_point_list": null,
            "encyclopedia_info": null,
            "fund_receive_list": null,
            "monthly_charge_bonus_info": null,
            "crazy_gacha_result_list": null,
        }),
    )
}
// //// /扣除星屑并发放兑换奖励 ////

fn exchange_reward_is_owned(root: &Map<String, Value>, entry: &ExchangeEntry) -> bool {
    let key = entry.target_id.to_string();
    let owned = match entry.kind {
        0 => root
            .get("user_character_list")
            .and_then(Value::as_object)
            .is_some_and(|characters| characters.contains_key(&key)),
        2 => root
            .get("user_equipment_list")
            .and_then(Value::as_object)
            .is_some_and(|equipment| equipment.contains_key(&key)),
        _ => false,
    };
    owned
}

fn exchange_entry(exchange_id: i64) -> Result<Option<ExchangeEntry>, PersonalServiceError> {
    let exchanges = parsed_asset(
        EXCHANGE_ASSET,
        &EXCHANGE_DATA,
        "failed to decode CN star crumb exchanges",
    )?;
    let Some(entry) = exchanges
        .get(&exchange_id.to_string())
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .and_then(Value::as_array)
    else {
        return Ok(None);
    };
    let kind = parse_entry_i64(entry, 0)?;
    let target_id = parse_entry_i64(entry, 1)?;
    let rarity = parse_entry_i64(entry, 8)?;
    let costs = parsed_asset(
        EXCHANGE_COST_ASSET,
        &EXCHANGE_COSTS,
        "failed to decode CN star crumb exchange costs",
    )?;
    let cost_index = usize::from(rarity == 5);
    let cost = costs
        .get(&kind.to_string())
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .and_then(Value::as_array)
        .and_then(|row| row.get(cost_index))
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| PersonalServiceError::new("CN exchange cost is invalid"))?;
    Ok(Some(ExchangeEntry {
        kind,
        target_id,
        cost,
    }))
}

fn parse_entry_i64(entry: &[Value], index: usize) -> Result<i64, PersonalServiceError> {
    entry
        .get(index)
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<i64>().ok())
        .ok_or_else(|| PersonalServiceError::new("CN exchange entry is invalid"))
}

fn parsed_asset(
    asset: &'static str,
    cache: &'static OnceLock<Result<Value, String>>,
    context: &str,
) -> Result<&'static Value, PersonalServiceError> {
    cache
        .get_or_init(|| serde_json::from_str(asset).map_err(|error| format!("{context}: {error}")))
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}
