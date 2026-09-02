// audience: internal
// # personal-service-cn-payment
//
// 该模块实现 CN 支付目录、订单确认和 SDK 回执兼容. 商品时段使用个人服务虚拟时间.

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{decode_player_data, encode_player_data, player_snapshot, require_root};
use crate::database::{PlayerSnapshot, ServiceDatabase, ViewerSessionPlayer};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;

const PAYMENT_PRODUCTS_ASSET: &str = include_str!("../../../assets/payment_products.json");
const MAX_VMONEY: i64 = 99_999_999;
static PAYMENT_PRODUCTS: OnceLock<Result<Value, String>> = OnceLock::new();

#[derive(Deserialize)]
struct PaymentRequest {
    viewer_id: Option<i64>,
    #[serde(rename = "api_count")]
    _api_count: Option<i64>,
}

#[derive(Deserialize)]
struct PaymentStartRequest {
    viewer_id: Option<i64>,
    product_id: Option<String>,
    payment: Option<PaymentProductReference>,
}

#[derive(Deserialize)]
struct PaymentProductReference {
    product_id: Option<String>,
}

#[derive(Deserialize)]
struct PaymentReportRequest {
    viewer_id: i64,
}

// //// 分派 CN 支付兼容请求 [@x380kkm 2026-07-24] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    match request.path() {
        "/api/index.php/payment/item_list" => Some(item_list(request, database)),
        "/api/index.php/payment/start" => Some(start(request, database)),
        "/api/index.php/payment/finish" => Some(finish(request, database)),
        "/api/index.php/payment/report_purchase_result" => {
            Some(report_purchase_result(request, database))
        }
        _ => None,
    }
}
// //// /分派 CN 支付兼容请求 ////

fn item_list(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<PaymentRequest>(request) {
        Ok(body) if body.viewer_id.is_some_and(|viewer_id| viewer_id > 0) => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let viewer_id = body.viewer_id.unwrap_or_default();
    match player_snapshot(database, viewer_id)? {
        Ok(_) => {}
        Err(response) => return Ok(response),
    }
    msgpack_response_at(
        viewer_id,
        false,
        server_time(database)?,
        json!({"payment_item_list": [], "refund_penalty_status": null}),
    )
}

// //// 接受 CN 支付开始请求 [@x380kkm 2026-08-22] ////
fn start(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<PaymentStartRequest>(request) {
        Ok(body) => body,
        Err(_) => return empty_payment_response(database, None),
    };
    let Some(viewer_id) = body.viewer_id.filter(|viewer_id| *viewer_id > 0) else {
        return empty_payment_response(database, body.viewer_id);
    };
    empty_payment_response(database, Some(viewer_id))
}
// //// /接受 CN 支付开始请求 ////

// //// 完成 CN 支付并保存货币与购买次数 [@x380kkm 2026-08-22] ////
fn finish(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<PaymentStartRequest>(request) {
        Ok(body) => body,
        Err(_) => return empty_payment_response(database, None),
    };
    let Some(viewer_id) = body.viewer_id.filter(|viewer_id| *viewer_id > 0) else {
        return empty_payment_response(database, body.viewer_id);
    };
    let snapshot = match payment_snapshot(database, viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let response_time = server_time(database)?;
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let Some(product_id) = requested_product_id(&body) else {
        return msgpack_response_at(viewer_id, false, response_time, json!({}));
    };
    let Some(product) = payment_product(&product_id, response_time)? else {
        return msgpack_response_at(viewer_id, false, response_time, json!({}));
    };
    let paid = product
        .get("charge_vmoney_num")
        .and_then(Value::as_i64)
        .unwrap_or_default()
        .max(0);
    let free = product
        .get("free_vmoney_num")
        .and_then(Value::as_i64)
        .unwrap_or_default()
        .max(0);
    let user_info = crate::cn_tutorial::require_object(root, "user_info")?;
    let after_paid = user_info
        .get("vmoney")
        .and_then(Value::as_i64)
        .unwrap_or_default()
        .saturating_add(paid)
        .min(MAX_VMONEY);
    let after_free = user_info
        .get("free_vmoney")
        .and_then(Value::as_i64)
        .unwrap_or_default()
        .saturating_add(free)
        .min(MAX_VMONEY);
    user_info.insert("vmoney".to_owned(), Value::from(after_paid));
    user_info.insert("free_vmoney".to_owned(), Value::from(after_free));
    let purchase_counts = root
        .entry("payment_purchase_counts".to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN payment counts are invalid"))?;
    let times = purchase_counts
        .get(&product_id)
        .and_then(Value::as_i64)
        .unwrap_or_default()
        .checked_add(1)
        .ok_or_else(|| PersonalServiceError::new("CN payment count exceeds range"))?;
    purchase_counts.insert(product_id.clone(), Value::from(times));
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    let mut purchased_times = Map::new();
    purchased_times.insert(product_id, Value::from(times));
    msgpack_response_at(
        viewer_id,
        false,
        response_time,
        json!({
            "after_vmoney": after_paid,
            "after_free_vmoney": after_free,
            "first_payment": times == 1,
            "first_time": times == 1,
            "purchased_times_list": purchased_times,
            "monthly_payment_total": 0,
            "monthly_charge_bonus_info": null,
            "premium_bonus_list": null,
        }),
    )
}
// //// /完成 CN 支付并保存货币与购买次数 ////

// //// 接受 CN 支付 SDK 回执状态 [@x380kkm 2026-08-22] ////
fn report_purchase_result(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<PaymentReportRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    match player_snapshot(database, body.viewer_id)? {
        Ok(_) => {}
        Err(response) => return Ok(response),
    }
    msgpack_response_at(body.viewer_id, false, server_time(database)?, json!({}))
}
// //// /接受 CN 支付 SDK 回执状态 ////

fn requested_product_id(request: &PaymentStartRequest) -> Option<String> {
    request.product_id.clone().or_else(|| {
        request
            .payment
            .as_ref()
            .and_then(|payment| payment.product_id.clone())
    })
}

fn payment_snapshot(
    database: &ServiceDatabase,
    viewer_id: i64,
) -> Result<Result<PlayerSnapshot, HttpResponse>, PersonalServiceError> {
    match database.lookup_viewer_session_player(viewer_id)? {
        ViewerSessionPlayer::Present(snapshot) => Ok(Ok(snapshot)),
        ViewerSessionPlayer::InvalidSession => Ok(Err(msgpack_response_at(
            viewer_id,
            false,
            server_time(database)?,
            json!({}),
        )?)),
        ViewerSessionPlayer::MissingPlayer => Ok(Err(error_response(
            "500 Internal Server Error",
            "no_player_bound",
        ))),
        ViewerSessionPlayer::MissingPlayerData(_) => Ok(Err(error_response(
            "500 Internal Server Error",
            "player_not_found",
        ))),
    }
}

fn empty_payment_response(
    database: &ServiceDatabase,
    viewer_id: Option<i64>,
) -> Result<HttpResponse, PersonalServiceError> {
    msgpack_response_at(
        viewer_id.unwrap_or_default(),
        false,
        server_time(database)?,
        json!({}),
    )
}

fn payment_product(
    product_id: &str,
    response_time: i64,
) -> Result<Option<Map<String, Value>>, PersonalServiceError> {
    let document = PAYMENT_PRODUCTS.get_or_init(|| {
        serde_json::from_str::<Value>(PAYMENT_PRODUCTS_ASSET)
            .map_err(|error| format!("failed to decode CN payment products: {error}"))
    });
    let document = document
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    let Some(product) = document.get(product_id).and_then(Value::as_object) else {
        return Ok(None);
    };
    let starts_at = product
        .get("start_time")
        .and_then(Value::as_i64)
        .unwrap_or(i64::MIN);
    let ends_at = product
        .get("end_time")
        .and_then(Value::as_i64)
        .unwrap_or(i64::MAX);
    Ok((starts_at <= response_time && response_time <= ends_at).then(|| product.clone()))
}

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}
