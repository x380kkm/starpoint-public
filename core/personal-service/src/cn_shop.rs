// audience: internal
// # personal-service-cn-shop
//
// 该模块处理 CN 商店目录和体力恢复请求. 请求要求有效 viewer 会话和完整玩家快照.
// 成功恢复时, 体力和星导石余额由一次 SQLite 更新共同提交.
// 无效会话, 体力已满或余额不足时不写入玩家快照.

mod catalog;
mod period;
mod purchase;
mod sales;

use crate::cn::{
    decode_request, msgpack_response_at, msgpack_result_code_response_at, server_time,
};
use crate::cn_stamina::current_stamina;
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, format_client_time, player_snapshot, require_root,
    set_user_info_value, user_info_value,
};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::management;
use crate::PersonalServiceError;
use catalog::{
    event_activity_id, event_shop_identity, shop_item, shop_item_is_active, shop_purchase_key,
    shop_purchase_limits, SHOP_TYPE_EVENT,
};
use purchase::{apply_shop_costs, apply_shop_rewards, shop_costs};
use sales::{event_shop_activity_state, get_sales_list};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

const STAMINA_RECOVERY_COST: i64 = 50;
const STAMINA_RECOVERY_AMOUNT: i64 = 100;
const STAMINA_OVERFLOW_LIMIT: i64 = 999;
const STAMINA_LIMIT_RESULT_CODE: i64 = 2102;
const SHOP_STOCK_REFRESH_PATH: &str = "/v1/shop-stock/refresh";

#[derive(Deserialize)]
struct RecoverStaminaRequest {
    viewer_id: i64,
}

#[derive(Deserialize)]
struct BuyRequest {
    viewer_id: i64,
    shop_type: i64,
    shop_item_id: i64,
    number: i64,
}

#[derive(Deserialize)]
struct BulkBuyRequest {
    viewer_id: i64,
    shop_type: Option<i64>,
    #[serde(default, alias = "shop_item_list")]
    buy_item_list: BulkBuyItems,
}

#[derive(Default, Deserialize)]
#[serde(untagged)]
enum BulkBuyItems {
    Items(BTreeMap<String, i64>),
    EmptyList(Vec<Value>),
    #[default]
    Missing,
}

#[derive(Deserialize)]
struct CampaignLineupRequest {
    viewer_id: i64,
    lineup_id: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RefreshShopStockRequest {
    viewer_id: i64,
    shop_type: i64,
    shop_item_id: i64,
}

// //// 分派 CN 商店请求 [@x380kkm 2026-08-04] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    match request.path() {
        SHOP_STOCK_REFRESH_PATH => Some(refresh_shop_stock(request, database, asset_root)),
        "/api/index.php/shop/get_sales_list" => Some(get_sales_list(request, database, asset_root)),
        "/api/index.php/shop/buy" => Some(buy(request, database, asset_root)),
        "/api/index.php/shop/bulk_buy" => Some(bulk_buy(request, database, asset_root)),
        "/api/index.php/shop/get_campaign_lineup_id" => {
            Some(get_campaign_lineup_id(request, database))
        }
        "/api/index.php/shop/set_campaign_lineup_id" => {
            Some(set_campaign_lineup_id(request, database))
        }
        "/api/index.php/shop/recover_stamina" => Some(recover_stamina(request, database)),
        _ => None,
    }
}
// //// /分派 CN 商店请求 ////

// //// 返回出售指定奖励的 CN 商店商品 [@x380kkm 2026-08-24] ////
pub(crate) fn how_to_get_sales(
    root: &Map<String, Value>,
    item_id: Option<i64>,
    equipment_id: Option<i64>,
    now: &str,
    database: &ServiceDatabase,
    asset_root: &Path,
) -> Result<Vec<Value>, PersonalServiceError> {
    sales::how_to_get_sales(root, item_id, equipment_id, now, database, asset_root)
}
// //// /返回出售指定奖励的 CN 商店商品 ////

// //// 购买 CN 商店商品并保存费用、奖励和库存 [@x380kkm 2026-08-22] ////
fn buy(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<BuyRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.shop_item_id > 0
                && body.number > 0
                && body.number <= 100 =>
        {
            body
        }
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let response_time = server_time(database)?;
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let rewards = match purchase_shop_item(
        root,
        database,
        asset_root,
        body.viewer_id,
        body.shop_type,
        body.shop_item_id,
        body.number,
        response_time,
    )? {
        Ok(rewards) => rewards,
        Err(code) => return Ok(error_response("400 Bad Request", code)),
    };
    let purchase_count = root
        .get("shop_purchase_counts")
        .and_then(Value::as_object)
        .and_then(|counts| counts.get(&shop_purchase_key(body.shop_type, body.shop_item_id)))
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let active_mission_list =
        crate::cn_mission::record_shop_purchase_active_missions(root, body.number)?;
    database.save_player_snapshot_with_receive_history(
        snapshot.account_id,
        &encode_player_data(&player_data)?,
        &format!(
            "shop:buy:{}:{}:{purchase_count}",
            body.shop_type, body.shop_item_id
        ),
        response_time,
        &rewards.history_entries,
    )?;
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        json!({
            "user_info": rewards.user_info,
            "joined_character_id_list": rewards.joined_character_id_list,
            "character_list": rewards.character_list,
            "equipment_list": rewards.equipment_list,
            "item_list": rewards.item_list,
            "active_mission_list": active_mission_list,
            "mail_arrived": false,
        }),
    )
}
// //// /购买 CN 商店商品并保存费用、奖励和库存 ////

// //// 批量购买 CN 商店商品并一次保存完整结果 [@x380kkm 2026-08-24] ////
fn bulk_buy(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<BulkBuyRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let items = match body.buy_item_list {
        BulkBuyItems::Items(items) => {
            let mut parsed = BTreeMap::new();
            for (shop_item_id, count) in items {
                let Ok(shop_item_id) = shop_item_id.parse::<i64>() else {
                    return Ok(error_response("400 Bad Request", "invalid_request_body"));
                };
                parsed.insert(shop_item_id, count);
            }
            parsed
        }
        BulkBuyItems::EmptyList(items) if items.is_empty() => BTreeMap::new(),
        BulkBuyItems::EmptyList(_) => {
            return Ok(error_response("400 Bad Request", "invalid_request_body"))
        }
        BulkBuyItems::Missing => BTreeMap::new(),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let response_time = server_time(database)?;
    if items.is_empty() {
        return msgpack_response_at(body.viewer_id, false, response_time, json!({}));
    }
    let Some(shop_type) = body.shop_type.filter(|shop_type| *shop_type > 0) else {
        return Ok(error_response("400 Bad Request", "invalid_request_body"));
    };
    if items.len() > 100
        || items
            .iter()
            .any(|(shop_item_id, count)| *shop_item_id <= 0 || *count <= 0 || *count > 100)
    {
        return Ok(error_response("400 Bad Request", "invalid_request_body"));
    }

    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let mut combined_rewards: Option<purchase::ShopRewards> = None;
    for (shop_item_id, count) in &items {
        let rewards = match purchase_shop_item(
            root,
            database,
            asset_root,
            body.viewer_id,
            shop_type,
            *shop_item_id,
            *count,
            response_time,
        )? {
            Ok(rewards) => rewards,
            Err(code) => return Ok(error_response("400 Bad Request", code)),
        };
        if let Some(combined) = &mut combined_rewards {
            combined.merge(rewards);
        } else {
            combined_rewards = Some(rewards);
        }
    }
    let rewards = combined_rewards
        .ok_or_else(|| PersonalServiceError::new("CN bulk shop purchase is empty"))?;
    let purchased_count = items.values().try_fold(0_i64, |total, count| {
        total
            .checked_add(*count)
            .ok_or_else(|| PersonalServiceError::new("CN bulk shop purchase count exceeds range"))
    })?;
    let active_mission_list =
        crate::cn_mission::record_shop_purchase_active_missions(root, purchased_count)?;
    let purchase_counts = root.get("shop_purchase_counts").and_then(Value::as_object);
    let operation_counts = items
        .keys()
        .map(|shop_item_id| {
            let count = purchase_counts
                .and_then(|counts| counts.get(&shop_purchase_key(shop_type, *shop_item_id)))
                .and_then(Value::as_i64)
                .unwrap_or_default();
            (*shop_item_id, count)
        })
        .collect::<BTreeMap<_, _>>();
    database.save_player_snapshot_with_receive_history(
        snapshot.account_id,
        &encode_player_data(&player_data)?,
        &format!("shop:bulk_buy:{shop_type}:{operation_counts:?}"),
        response_time,
        &rewards.history_entries,
    )?;
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        json!({
            "user_info": rewards.user_info,
            "joined_character_id_list": rewards.joined_character_id_list,
            "character_list": rewards.character_list,
            "equipment_list": rewards.equipment_list,
            "item_list": rewards.item_list,
            "active_mission_list": active_mission_list,
            "mail_arrived": false,
        }),
    )
}
// //// /批量购买 CN 商店商品并一次保存完整结果 ////

// //// 应用一个 CN 商店商品的费用、奖励和库存 [@x380kkm 2026-08-24] ////
fn purchase_shop_item(
    root: &mut Map<String, Value>,
    database: &mut ServiceDatabase,
    asset_root: &Path,
    viewer_id: i64,
    shop_type: i64,
    shop_item_id: i64,
    purchase_count: i64,
    response_time: i64,
) -> Result<Result<purchase::ShopRewards, &'static str>, PersonalServiceError> {
    let event_shop_state = if shop_type == SHOP_TYPE_EVENT {
        let Some((event_type, event_id)) = event_shop_identity(shop_item_id)? else {
            return Ok(Err("shop_item_not_found"));
        };
        let activity_id = event_activity_id(event_type, event_id)
            .ok_or_else(|| PersonalServiceError::new("CN event shop type is invalid"))?;
        let state = event_shop_activity_state(database, asset_root, &activity_id)?;
        if state.status != crate::database::ActivityWindowStatus::Open {
            let code = match state.status {
                crate::database::ActivityWindowStatus::Disabled => "activity_disabled",
                crate::database::ActivityWindowStatus::NotStarted => "activity_not_started",
                crate::database::ActivityWindowStatus::Ended => "activity_ended",
                crate::database::ActivityWindowStatus::Unscheduled => "activity_not_found",
                crate::database::ActivityWindowStatus::Open => unreachable!(),
            };
            return Ok(Err(code));
        }
        Some(state)
    } else {
        None
    };
    let Some(item) = shop_item(shop_type, shop_item_id)? else {
        return Ok(Err("shop_item_not_found"));
    };
    if !event_shop_state.is_some_and(|state| state.overrides_item_period)
        && !shop_item_is_active(&item, &format_client_time(response_time))
    {
        return Ok(Err("shop_item_unavailable"));
    }
    let limits = shop_purchase_limits(shop_type, shop_item_id)?;
    if limits
        .buy_max_count
        .is_some_and(|buy_max_count| purchase_count > buy_max_count)
    {
        return Ok(Err("shop_stock_exceeded"));
    }
    let purchase_state = period::synchronize_purchase_state(
        root,
        database,
        asset_root,
        shop_type,
        shop_item_id,
        &item,
        response_time,
    )?;
    let purchase_key = shop_purchase_key(shop_type, shop_item_id);
    let purchase_counts = root.get("shop_purchase_counts").and_then(Value::as_object);
    let purchased = purchase_counts
        .and_then(|counts| counts.get(&purchase_key))
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let new_purchase_count = purchased
        .checked_add(purchase_count)
        .ok_or_else(|| PersonalServiceError::new("CN shop purchase count exceeds range"))?;
    if purchase_state.stock_quantity >= 0 && purchase_count > purchase_state.stock_quantity {
        return Ok(Err("shop_stock_exceeded"));
    }

    let costs = shop_costs(root, &item, purchase_count)?;
    apply_shop_costs(root, &costs)?;
    let rewards = apply_shop_rewards(
        root,
        viewer_id,
        shop_type,
        &item,
        purchase_count,
        response_time,
        &costs,
    )?;
    let purchase_counts = root
        .entry("shop_purchase_counts".to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN shop counts are invalid"))?;
    purchase_counts.insert(purchase_key, Value::from(new_purchase_count));
    Ok(Ok(rewards))
}
// //// /应用一个 CN 商店商品的费用、奖励和库存 ////

// //// 为指定玩家和商品记录 CN 商店库存刷新基线 [@x380kkm 2026-08-24] ////
fn refresh_shop_stock(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    if !management::is_authorized(request, database) {
        return Ok(management::unauthorized_response());
    }
    if !request
        .header("content-type")
        .is_some_and(|content_type| content_type.starts_with("application/json"))
    {
        return Ok(error_response("400 Bad Request", "invalid_request_body"));
    }
    let body = match serde_json::from_slice::<RefreshShopStockRequest>(request.body()) {
        Ok(body) if body.viewer_id > 0 && body.shop_type > 0 && body.shop_item_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let Some(item) = shop_item(body.shop_type, body.shop_item_id)? else {
        return Ok(error_response("404 Not Found", "shop_item_not_found"));
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(_) => return Ok(error_response("404 Not Found", "viewer_not_found")),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let purchase_key = shop_purchase_key(body.shop_type, body.shop_item_id);
    let historical_purchase_count = root
        .get("shop_purchase_counts")
        .and_then(Value::as_object)
        .and_then(|counts| counts.get(&purchase_key))
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let response_time = server_time(database)?;
    let state = period::reset_purchase_state(
        root,
        database,
        asset_root,
        body.shop_type,
        body.shop_item_id,
        &item,
        response_time,
    )?;
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    Ok(HttpResponse::json(
        "200 OK",
        serde_json::to_string(&json!({
            "viewer_id": body.viewer_id,
            "shop_type": body.shop_type,
            "shop_item_id": body.shop_item_id,
            "historical_purchase_num": historical_purchase_count,
            "stock_purchase_num": state.total_purchase_num,
        }))
        .map_err(|error| {
            PersonalServiceError::new(format!(
                "failed to encode CN shop refresh response: {error}"
            ))
        })?,
    ))
}
// //// /为指定玩家和商品记录 CN 商店库存刷新基线 ////

// //// 返回当前商店活动阵容编号 [@x380kkm 2026-08-22] ////
fn get_campaign_lineup_id(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<CampaignLineupRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let player_data = decode_player_data(&snapshot.data)?;
    let lineup_id = player_data
        .get("shop_campaign_lineup_id")
        .cloned()
        .unwrap_or(Value::Null);
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({"lineup_id": lineup_id}),
    )
}
// //// /返回当前商店活动阵容编号 ////

// //// 保存当前商店活动阵容编号 [@x380kkm 2026-08-22] ////
fn set_campaign_lineup_id(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<CampaignLineupRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    require_root(&mut player_data)?.insert(
        "shop_campaign_lineup_id".to_owned(),
        body.lineup_id.map(Value::from).unwrap_or(Value::Null),
    );
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(body.viewer_id, false, server_time(database)?, json!({}))
}
// //// /保存当前商店活动阵容编号 ////

// //// 恢复体力并扣除星导石 [@x380kkm 2026-08-04] ////
fn recover_stamina(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<RecoverStaminaRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let response_time = server_time(database)?;
    let current_stamina = current_stamina(root, response_time)?;
    let recovered_stamina = current_stamina
        .saturating_add(STAMINA_RECOVERY_AMOUNT)
        .min(STAMINA_OVERFLOW_LIMIT);
    if recovered_stamina == current_stamina {
        return msgpack_result_code_response_at(
            body.viewer_id,
            response_time,
            STAMINA_LIMIT_RESULT_CODE,
        );
    }

    let free_vmoney = user_info_value(root, "free_vmoney")?;
    if free_vmoney < STAMINA_RECOVERY_COST {
        return msgpack_result_code_response_at(body.viewer_id, response_time, 0);
    }
    let remaining_vmoney = free_vmoney - STAMINA_RECOVERY_COST;
    set_user_info_value(root, "stamina", recovered_stamina)?;
    set_user_info_value(root, "stamina_heal_time", response_time)?;
    set_user_info_value(root, "free_vmoney", remaining_vmoney)?;
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;

    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        json!({
            "user_info": {
                "stamina": recovered_stamina,
                "stamina_heal_time": response_time,
                "free_vmoney": remaining_vmoney,
            },
        }),
    )
}
// //// /恢复体力并扣除星导石 ////

// //// 编码 CN 商店错误响应 [@x380kkm 2026-08-04] ////
fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}
// //// /编码 CN 商店错误响应 ////
