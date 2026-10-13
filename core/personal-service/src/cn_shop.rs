// audience: internal
// # personal-service-cn-shop
//
// 该模块处理 CN 商店目录和体力恢复请求. 请求要求有效 viewer 会话和完整玩家快照.
// 成功恢复时, 体力, 星导石余额和请求回执共同提交.
// 无效会话, 体力已满或余额不足时不写入玩家快照.

mod activity;
mod capacity;
mod catalog;
mod cost_schedule;
mod enhancement;
mod period;
mod purchase;
mod receipt;
mod sales;
mod selection;
mod transactions;

use crate::cn::{
    decode_request, msgpack_response_at, msgpack_result_code_response_at, server_time,
};
use crate::cn_request_receipt::RequestReceipt;
use crate::cn_stamina::current_stamina;
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, player_snapshot, require_root, set_user_info_value,
};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::management;
use crate::PersonalServiceError;
use activity::event_shop_activity_state;
use catalog::{
    event_shop_identity, shop_item, shop_item_is_active, shop_purchase_key,
    shop_purchase_limits, SHOP_TYPE_EVENT,
};
pub(crate) use catalog::{event_activity_id, event_shop_activity_alias};
use purchase::{apply_shop_costs, apply_shop_rewards, shop_costs};
use sales::get_sales_list;
pub(crate) use sales::{prepare_sales_list, SalesQuery};
pub(crate) use selection::product_activity_id as selection_activity_id;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::path::Path;
use transactions::{bulk_buy, buy};

const STAMINA_RECOVERY_COST: i64 = 50;
const STAMINA_RECOVERY_AMOUNT: i64 = 100;
const STAMINA_OVERFLOW_LIMIT: i64 = 999;
const STAMINA_LIMIT_RESULT_CODE: i64 = 2102;
const SHOP_STOCK_REFRESH_PATH: &str = "/v1/shop-stock/refresh";

#[derive(Deserialize)]
struct RecoverStaminaRequest {
    viewer_id: i64,
    api_count: Option<i64>,
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
        "/api/index.php/shop/get_campaign_lineup_id" => Some(selection::get(request, database)),
        "/api/index.php/shop/set_campaign_lineup_id" => Some(selection::set(request, database)),
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
) -> Result<sales::ShopSources, PersonalServiceError> {
    sales::how_to_get_sales(root, item_id, equipment_id, now, database, asset_root)
}
// //// /返回出售指定奖励的 CN 商店商品 ////

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
    let availability =
        selection::availability(root, database, shop_type, shop_item_id, response_time)?;
    if matches!(
        availability,
        selection::Availability::Unavailable | selection::Availability::Unselected
    ) {
        return Ok(Err(selection::UNAVAILABLE_ERROR));
    }
    let event_shop_state = if shop_type == SHOP_TYPE_EVENT {
        let Some((event_type, event_id)) = event_shop_identity(shop_item_id)? else {
            return Ok(Err("shop_item_not_found"));
        };
        let activity_id = event_activity_id(event_type, event_id)
            .ok_or_else(|| PersonalServiceError::new("CN event shop type is invalid"))?;
        let state = event_shop_activity_state(database, &activity_id)?;
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
    let Some(mut item) = shop_item(shop_type, shop_item_id)? else {
        return Ok(Err("shop_item_not_found"));
    };
    if availability != selection::Availability::Available
        && !event_shop_state.as_ref().map_or_else(
            || shop_item_is_active(&item, response_time),
            |state| state.item_is_active(shop_item_id, &item, response_time),
        )
    {
        return Ok(Err("shop_item_unavailable"));
    }
    if !cost_schedule::apply_cost_schedule(&mut item, shop_type, shop_item_id, response_time)? {
        return Ok(Err("shop_item_unavailable"));
    }
    if shop_type == catalog::SHOP_TYPE_EQUIPMENT_ENHANCEMENT {
        if let Some(error) = enhancement::purchase_error(root, &item, purchase_count)? {
            return Ok(Err(error));
        }
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

    let costs = match shop_costs(root, &item, purchase_count)? {
        Ok(costs) => costs,
        Err(code) => return Ok(Err(code)),
    };
    crate::cn_exp_pool::settle(root, response_time)?;
    apply_shop_costs(root, &costs)?;
    if !capacity::can_receive(root, &item, purchase_count)? {
        return Ok(Err("shop_reward_capacity_exceeded"));
    }
    let rewards = apply_shop_rewards(
        root,
        viewer_id,
        shop_type,
        &item,
        purchase_count,
        response_time,
        &costs,
    )?;
    if rewards.pass_card_points > 0 {
        let Some(target) = crate::cn_pass_card::point_purchase_target(root, database, response_time)? else {
            return Ok(Err("shop_item_unavailable"));
        };
        if rewards.pass_card_points > target.capacity {
            return Ok(Err("shop_reward_capacity_exceeded"));
        }
        crate::cn_pass_card::add_points_for_event(root, target.event_id, rewards.pass_card_points)?;
    }
    let purchase_counts = root
        .entry("shop_purchase_counts".to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN shop counts are invalid"))?;
    purchase_counts.insert(purchase_key, Value::from(new_purchase_count));
    Ok(Ok(rewards))
}
// //// /应用一个 CN 商店商品的费用、奖励和库存 ////

// //// 判断活动是否具有可兑换的商店目录 [@x380kkm 2026-09-21] ////
pub(crate) fn has_activity_shop(activity_id: &str) -> Result<bool, PersonalServiceError> {
    Ok(!catalog::activity_shop_item_ids(activity_id)?.is_empty())
}
// //// /判断活动是否具有可兑换的商店目录 ////

// //// 重置所选活动的商店库存基线并保留累计购买记录 [@x380kkm 2026-09-21] ////
pub(crate) fn reset_activity_shop_purchases(
    root: &mut Map<String, Value>,
    activity_id: &str,
) -> Result<usize, PersonalServiceError> {
    selection::reset(root, activity_id)?;
    period::reset_activity_shop_purchases(root, activity_id)
}
// //// /重置所选活动的商店库存基线并保留累计购买记录 ////

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
    if shop_item(body.shop_type, body.shop_item_id)?.is_none() {
        return Ok(error_response("404 Not Found", "shop_item_not_found"));
    }
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
    let receipt = body.api_count.map(|count| {
        RequestReceipt::new("shop", "recover_stamina", body.viewer_id, count, json!({}))
    });
    if let Some(receipt) = &receipt {
        if let Some(response) = receipt.replay(root)? {
            return Ok(response);
        }
    }
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

    let Some(remaining) = crate::cn_vmoney::spend(root, STAMINA_RECOVERY_COST)? else {
        return msgpack_result_code_response_at(body.viewer_id, response_time, 0);
    };
    set_user_info_value(root, "stamina", recovered_stamina)?;
    set_user_info_value(root, "stamina_heal_time", response_time)?;
    let response = json!({
        "user_info": {
            "stamina": recovered_stamina,
            "stamina_heal_time": response_time,
            "free_vmoney": remaining.free,
            "vmoney": remaining.paid,
        },
    });
    if let Some(receipt) = receipt {
        receipt.record(root, &response, response_time)?;
    }
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(body.viewer_id, false, response_time, response)
}
// //// /恢复体力并扣除星导石 ////

// //// 编码 CN 商店错误响应 [@x380kkm 2026-08-04] ////
fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}
// //// /编码 CN 商店错误响应 ////
