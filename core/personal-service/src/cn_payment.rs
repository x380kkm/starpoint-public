// audience: internal
// # personal-service-cn-payment
// 商品点击在本地存档发放奖励. 订单确认, 中断恢复和领取历史共享已提交的订单结果.

pub(crate) mod catalog;
mod availability;
mod grant;
mod orders;
mod subscriptions;
pub(crate) use subscriptions::{capture_daily_resources, prepare_load as prepare_subscription_rewards, SubscriptionDelivery};
pub(crate) use orders::{bind_payment_orders_to_viewer, product_history_page, ProductHistoryPage};
pub(crate) use availability::{activity_definitions, default_activity_window};

use crate::cn::{decode_request, msgpack_response_at, msgpack_result_code_response_at, server_time};
use crate::cn_tutorial::{decode_player_data, encode_player_data, player_snapshot, require_root};
use crate::database::{ReceiveHistoryEntry, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Deserialize)]
struct PaymentRequest {
    viewer_id: i64,
    api_count: Option<i64>,
    product_id: Option<String>,
    order_id: Option<String>,
    #[serde(default)]
    payment: PaymentReference,
}

#[derive(Default, Deserialize)]
struct PaymentReference {
    product_id: Option<String>,
    order_id: Option<String>,
    transaction_id: Option<String>,
    original_receipt: Option<String>,
}

impl PaymentRequest {
    fn product_id(&self) -> Option<&str> {
        self.payment.product_id.as_deref().or(self.product_id.as_deref())
    }

    fn order_id(&self) -> Option<&str> {
        self.payment.order_id.as_deref().filter(|id| !id.is_empty())
            .or(self.order_id.as_deref().filter(|id| !id.is_empty()))
            .or(self.payment.transaction_id.as_deref().filter(|id| !id.is_empty()))
            .or_else(|| self.payment.original_receipt.as_deref()?.rsplit_once(':').map(|(_, id)| id))
    }
}

struct PaymentOutcome {
    data: Value,
    history: Vec<ReceiveHistoryEntry>,
    event_key: String,
    result_code: i64,
    mails: Vec<subscriptions::SubscriptionMail>,
}

impl PaymentOutcome {
    fn response(data: Value) -> Self {
        Self { data, history: Vec::new(), event_key: String::new(), result_code: 1, mails: Vec::new() }
    }

    fn unavailable() -> Self {
        Self { result_code: 20001, ..Self::response(json!({})) }
    }
}

type Handler = fn(&PaymentRequest, &mut Map<String, Value>, &ServiceDatabase, i64) -> Result<PaymentOutcome, PersonalServiceError>;

// //// 将商品领取和订单恢复绑定到当前存档 [@x380kkm 2026-09-22] ////
pub(crate) fn route(request: &HttpRequest, database: &mut ServiceDatabase) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" { return None; }
    let handler: Handler = match request.path() {
        "/api/index.php/payment/item_list" => item_list,
        "/api/index.php/payment/start" => start,
        "/api/index.php/payment/finish" => finish,
        "/api/index.php/channels/channel_leiting_pay/query_unfinish_order" => unfinished_order,
        "/api/index.php/channels/channel_leiting_pay/query_purcharge" => order_status,
        "/api/index.php/channels/channel_leiting_pay/set_unfinish_order_status" => acknowledge_order,
        "/api/index.php/payment/report_purchase_result"
        | "/api/index.php/channels/channel_leiting_pay/set_unfinish_order_error" => report,
        _ => return None,
    };
    Some(handle(request, database, handler))
}

fn handle(request: &HttpRequest, database: &mut ServiceDatabase, handler: Handler) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<PaymentRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.api_count.map_or(true, |count| count >= 0) => body,
        Ok(_) | Err(_) => return Ok(HttpResponse::json("400 Bad Request", json!({"error":"invalid_request_body"}).to_string())),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let now = server_time(database)?;
    let mut player_data = decode_player_data(&snapshot.data)?;
    let before = player_data.clone();
    let outcome = handler(&body, require_root(&mut player_data)?, database, now)?;
    if outcome.result_code != 1 {
        return msgpack_result_code_response_at(body.viewer_id, now, outcome.result_code);
    }
    let response = msgpack_response_at(body.viewer_id, false, now, outcome.data)?;
    if player_data != before {
        let data = encode_player_data(&player_data)?;
        database.with_player_settlement(|database| {
            database.save_player_snapshot_with_receive_history(
                snapshot.account_id, &data, &outcome.event_key, now, &outcome.history,
            )?;
            deliver_subscription_mails(database, snapshot.account_id, &data, &outcome.mails)?;
            Ok(Some(()))
        })?;
    }
    Ok(response)
}
// //// /将商品领取和订单恢复绑定到当前存档 ////

// //// 按游戏时间和关联修行活动返回商品 [@x380kkm 2026-09-26] ////
fn item_list(_: &PaymentRequest, root: &mut Map<String, Value>, database: &ServiceDatabase, now: i64) -> Result<PaymentOutcome, PersonalServiceError> {
    let mut items = Vec::new();
    for product in catalog::catalog()?.products.values() {
        if let Some(period) = product.available_period(root, database, now)? {
            items.push(product.item_list_entry(Some(orders::purchase_count(root, &product.store_product_id)), period));
        }
    }
    Ok(PaymentOutcome::response(json!({"payment_item_list":items,"refund_penalty_status":null})))
}

// //// 点击商品时一次提交配置奖励和本地订单 [@x380kkm 2026-09-22] ////
fn start(body: &PaymentRequest, root: &mut Map<String, Value>, database: &ServiceDatabase, now: i64) -> Result<PaymentOutcome, PersonalServiceError> {
    let Some(product_id) = body.product_id() else { return Ok(PaymentOutcome::unavailable()); };
    let mut state = orders::PaymentState::load_for_viewer(root, body.viewer_id)?;
    if let Some(order) = state.request_order(body.viewer_id, body.api_count, product_id) {
        return Ok(if order.product_id == product_id { PaymentOutcome::response(order.start_response()) } else { PaymentOutcome::unavailable() });
    }
    let Some(product) = catalog::catalog()?.products.get(product_id) else { return Ok(PaymentOutcome::unavailable()); };
    if product.available_period(root, database, now)?.is_none() { return Ok(PaymentOutcome::unavailable()); }
    let account_id = database.player_account_id_for_viewer(body.viewer_id)?
        .ok_or_else(|| PersonalServiceError::new("CN payment account is missing"))?;
    let previous_items = crate::cn_mission::capture_collect_item_counts(root)?;
    let previous_mana = crate::cn_mission::total_mana(root)?;
    let first_payment = !orders::has_purchase(root);
    let first_time = orders::purchase_count(root, product_id) == 0;
    let delivery = grant::grant_product(root, product, database, now)?;
    orders::record_purchase(root, product_id)?;
    let mut mission_info = crate::cn_mission::record_collected_items(root, database, account_id, &previous_items)?;
    let acquired_mana = crate::cn_mission::total_mana(root)?.saturating_sub(previous_mana);
    mission_info.extend(crate::cn_mission::record_acquired_mana(root, database, account_id, acquired_mana, now)?);
    let mut response = json!({"first_payment":first_payment,"first_time":first_time});
    response["mission_info"] = json!(mission_info);
    crate::cn_mission::apply_mission_reward_response(&mut response, root, &mission_info, body.viewer_id, now)?;
    let order = state.create_order(body.viewer_id, body.api_count, product_id, now, response)?;
    let outcome = PaymentOutcome { data: order.start_response(), event_key: format!("payment:{}", order.id), history: delivery.entries, mails: delivery.mails, result_code: 1 };
    state.save(root)?;
    Ok(outcome)
}
// //// /点击商品时一次提交配置奖励和本地订单 ////

// //// 确认已到账订单并返回客户端资产字段 [@x380kkm 2026-09-22] ////
fn finish(body: &PaymentRequest, root: &mut Map<String, Value>, _: &ServiceDatabase, now: i64) -> Result<PaymentOutcome, PersonalServiceError> {
    let mut state = orders::PaymentState::load_for_viewer(root, body.viewer_id)?;
    let Some(order) = state.resolve_order_mut(body.order_id(), body.product_id()) else { return Ok(PaymentOutcome::unavailable()); };
    if body.product_id().is_some_and(|id| id != order.product_id) { return Ok(PaymentOutcome::unavailable()); }
    order.finished_at.get_or_insert(now);
    let mut response = order.response.clone();
    crate::cn_character::complete_character_response_times(&mut response, root, order.created_at);
    orders::refresh_finish_balances(&mut response, root);
    state.save(root)?;
    Ok(PaymentOutcome::response(response))
}

fn unfinished_order(body: &PaymentRequest, root: &mut Map<String, Value>, _: &ServiceDatabase, _: i64) -> Result<PaymentOutcome, PersonalServiceError> {
    let state = orders::PaymentState::load_for_viewer(root, body.viewer_id)?;
    let data = match state.pending_order() {
        Some(order) => json!({"order_id":order.id,"product_key":order.product_id,"exchangeRate":1,
            "payment_money":catalog::catalog()?.products.get(&order.product_id).map_or(0, |product| product.price)}),
        None => json!({"order_id":""}),
    };
    Ok(PaymentOutcome::response(data))
}

fn order_status(body: &PaymentRequest, root: &mut Map<String, Value>, _: &ServiceDatabase, _: i64) -> Result<PaymentOutcome, PersonalServiceError> {
    let mut state = orders::PaymentState::load_for_viewer(root, body.viewer_id)?;
    Ok(match state.resolve_order_mut(body.order_id(), body.product_id()) {
        Some(order) => PaymentOutcome::response(json!({"status":3,"order_id":order.id,"product_id":order.product_id})),
        None => PaymentOutcome::unavailable(),
    })
}

fn acknowledge_order(body: &PaymentRequest, root: &mut Map<String, Value>, _: &ServiceDatabase, now: i64) -> Result<PaymentOutcome, PersonalServiceError> {
    let mut state = orders::PaymentState::load_for_viewer(root, body.viewer_id)?;
    if let Some(order) = state.resolve_order_mut(body.order_id(), body.product_id()) {
        order.finished_at.get_or_insert(now);
        state.save(root)?;
    }
    Ok(PaymentOutcome::response(json!({})))
}

fn report(_: &PaymentRequest, _: &mut Map<String, Value>, _: &ServiceDatabase, _: i64) -> Result<PaymentOutcome, PersonalServiceError> {
    Ok(PaymentOutcome::response(json!({})))
}
// //// /确认已到账订单并返回客户端资产字段 ////

// //// 在玩家保存点内投递订阅到期奖励 [@x380kkm 2026-09-22] ////
pub(crate) fn deliver_subscription_mails(database: &mut ServiceDatabase, account_id: i64, snapshot: &str, mails: &[subscriptions::SubscriptionMail]) -> Result<(), PersonalServiceError> {
    for mail in mails {
        database.deliver_reward_mail_with_snapshot_once(&crate::database::CreateMailInput {
            account_id, title: mail.title.clone(), body: "订阅到期奖励.".to_owned(), sender: "系统".to_owned(),
            rewards: mail.rewards.clone(), created_at: mail.created_at, expires_at: None,
        }, snapshot, &mail.event_key, None)?;
    }
    Ok(())
}
