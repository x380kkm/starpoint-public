// audience: internal
// # personal-service-cn-payment-orders
// 订单随账号快照保存, 请求计数在 viewer 会话内定位一次点击. 待确认订单支持进程重启恢复.
// 管理台按持久化订单顺序展示已发放商品, 完成界面确认时间与发放时间分别保留.
// 可移植订单的会话身份由当前账号绑定, 跨会话请求计数独立, 扩展字段随订单保留.

use crate::PersonalServiceError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
pub(super) struct PaymentState {
    sequence: i64,
    orders: BTreeMap<String, PaymentOrder>,
    #[serde(flatten)]
    extra_fields: BTreeMap<String, Value>,
}

#[derive(Deserialize, Serialize)]
pub(super) struct PaymentOrder {
    pub(super) id: String,
    pub(super) product_id: String,
    pub(super) response: Value,
    pub(super) finished_at: Option<i64>,
    sequence: i64,
    viewer_id: Option<i64>,
    api_count: Option<i64>,
    pub(super) created_at: i64,
    #[serde(flatten)]
    extra_fields: BTreeMap<String, Value>,
}

#[derive(Serialize)]
pub(crate) struct ProductHistoryEntry {
    pub(crate) id: i64,
    pub(crate) product_id: String,
    pub(crate) created_at: i64,
    pub(crate) confirmed_at: Option<i64>,
    pub(crate) entitlement_kinds: Vec<&'static str>,
}

pub(crate) struct ProductHistoryPage {
    pub(crate) entries: Vec<ProductHistoryEntry>,
    pub(crate) next_before_id: Option<i64>,
}

// //// 按订单顺序投影已发放商品和界面确认状态 [@x380kkm 2026-09-23] ////
pub(crate) fn product_history_page(
    root: &Map<String, Value>,
    before_id: Option<i64>,
    limit: usize,
) -> Result<ProductHistoryPage, PersonalServiceError> {
    let catalog = super::catalog::catalog()?;
    let mut orders = PaymentState::load(root)?.orders.into_values()
        .filter(|order| before_id.map_or(true, |cursor| order.sequence < cursor))
        .collect::<Vec<_>>();
    orders.sort_unstable_by_key(|order| std::cmp::Reverse(order.sequence));
    let has_more = orders.len() > limit;
    let entries = orders.into_iter().take(limit).map(|order| {
        let entitlement_kinds = catalog.products.get(&order.product_id).into_iter()
            .flat_map(|product| &product.entitlements)
            .map(|entitlement| match entitlement {
                super::catalog::PaymentEntitlement::GrowthFund { .. } => "growth_fund",
                super::catalog::PaymentEntitlement::PassCard => "pass_card",
                super::catalog::PaymentEntitlement::Subscription { .. } => "subscription",
            }).collect();
        ProductHistoryEntry {
            id: order.sequence,
            product_id: order.product_id,
            created_at: order.created_at,
            confirmed_at: order.finished_at,
            entitlement_kinds,
        }
    }).collect::<Vec<_>>();
    let next_before_id = entries.last().filter(|_| has_more).map(|entry| entry.id);
    Ok(ProductHistoryPage { entries, next_before_id })
}
// //// /按订单顺序投影已发放商品和界面确认状态 ////

impl PaymentState {
    pub(super) fn load(root: &Map<String, Value>) -> Result<Self, PersonalServiceError> {
        let mut state: Self = root.get("payment_state").filter(|value| !value.is_null()).cloned().map(serde_json::from_value).transpose()
            .map(|state| state.unwrap_or_default())
            .map_err(|error| PersonalServiceError::new(format!("invalid CN payment state: {error}")))?;
        state.sequence = state.sequence.max(state.orders.values().map(|order| order.sequence).max().unwrap_or_default());
        Ok(state)
    }

    // //// 将存档订单绑定到当前会话并隔离历史请求计数 [@x380kkm 2026-09-28] ////
    pub(super) fn load_for_viewer(root: &mut Map<String, Value>, viewer_id: i64) -> Result<Self, PersonalServiceError> {
        let mut state = Self::load(root)?;
        for order in state.orders.values_mut() {
            if order.viewer_id != Some(viewer_id) {
                order.viewer_id = Some(viewer_id);
                order.api_count = None;
                bind_response_viewer(&mut order.response, viewer_id);
            }
        }
        if root.get("payment_state").is_some_and(|value| !value.is_null()) {
            state.save(root)?;
        }
        Ok(state)
    }

    pub(super) fn save(&self, root: &mut Map<String, Value>) -> Result<(), PersonalServiceError> {
        root.insert("payment_state".to_owned(), serde_json::to_value(self)
            .map_err(|error| PersonalServiceError::new(error.to_string()))?);
        Ok(())
    }

    pub(super) fn request_order(&self, viewer_id: i64, api_count: Option<i64>, product_id: &str) -> Option<&PaymentOrder> {
        self.orders.values().filter(|order| order.viewer_id == Some(viewer_id) && match api_count {
            Some(count) => order.api_count == Some(count),
            None => order.api_count.is_none() && order.finished_at.is_none() && order.product_id == product_id,
        }).max_by_key(|order| order.sequence)
    }

    pub(super) fn pending_order(&self) -> Option<&PaymentOrder> {
        self.orders.values().filter(|order| order.finished_at.is_none()).min_by_key(|order| order.sequence)
    }

    pub(super) fn resolve_order_mut(&mut self, order_id: Option<&str>, product_id: Option<&str>) -> Option<&mut PaymentOrder> {
        if let Some(id) = order_id { return self.orders.get_mut(id); }
        let product_id = product_id?;
        self.orders.values_mut().filter(|order| order.product_id == product_id).max_by_key(|order| order.sequence)
    }

    // //// 保存一次点击的顺序和响应 [@x380kkm 2026-09-22] ////
    pub(super) fn create_order(&mut self, viewer_id: i64, api_count: Option<i64>, product_id: &str, now: i64, response: Value) -> Result<&PaymentOrder, PersonalServiceError> {
        let mut bytes = [0_u8; 16];
        getrandom::getrandom(&mut bytes).map_err(|error| PersonalServiceError::new(format!("local payment id generation failed: {error}")))?;
        let id = bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
        self.sequence = self.sequence.checked_add(1).ok_or_else(|| PersonalServiceError::new("CN payment sequence exceeds range"))?;
        self.orders.insert(id.clone(), PaymentOrder {
            id: id.clone(), product_id: product_id.to_owned(), response, finished_at: None,
            sequence: self.sequence, viewer_id: Some(viewer_id), api_count, created_at: now,
            extra_fields: BTreeMap::new(),
        });
        Ok(&self.orders[&id])
    }
}

// //// 在载入时保留订单归属和已发奖励状态 [@x380kkm 2026-09-28] ////
pub(crate) fn bind_payment_orders_to_viewer(root: &mut Map<String, Value>, viewer_id: i64) -> Result<(), PersonalServiceError> {
    if root.get("payment_state").is_some_and(|value| !value.is_null()) {
        PaymentState::load_for_viewer(root, viewer_id)?;
    }
    Ok(())
}

// //// 为缓存领奖响应投影当前账号身份 [@x380kkm 2026-09-28] ////
fn bind_response_viewer(value: &mut Value, viewer_id: i64) {
    match value {
        Value::Array(entries) => {
            for entry in entries { bind_response_viewer(entry, viewer_id); }
        }
        Value::Object(fields) => {
            if fields.contains_key("viewer_id") {
                fields.insert("viewer_id".to_owned(), json!(viewer_id));
            }
            for field in ["character_list", "degree_list"] {
                if let Some(entries) = fields.get_mut(field).and_then(Value::as_array_mut) {
                    for entry in entries {
                        if let Some(entry) = entry.as_object_mut() {
                            entry.insert("viewer_id".to_owned(), json!(viewer_id));
                        }
                    }
                }
            }
            for entry in fields.values_mut() { bind_response_viewer(entry, viewer_id); }
        }
        _ => {}
    }
}
// //// /为缓存领奖响应投影当前账号身份 ////

impl PaymentOrder {
    pub(super) fn start_response(&self) -> Value {
        json!({"order_id":self.id,"orderId":self.id,"product_id":self.product_id,"exchangeRate":1,
            "zoneId":"","notifyUrl":"","extInfo":""})
    }
}

pub(super) fn purchase_count(root: &Map<String, Value>, product_id: &str) -> i64 {
    ["purchased_times_list", "payment_purchase_counts"].into_iter().filter_map(|field| {
        root.get(field)?.get(product_id)?.as_i64()
    }).max().unwrap_or_default().max(0)
}

pub(super) fn has_purchase(root: &Map<String, Value>) -> bool {
    ["purchased_times_list", "payment_purchase_counts"].into_iter().any(|field| {
        root.get(field).and_then(Value::as_object).is_some_and(|counts| counts.values().any(|value| value.as_i64().is_some_and(|count| count > 0)))
    })
}

pub(super) fn record_purchase(root: &mut Map<String, Value>, product_id: &str) -> Result<(), PersonalServiceError> {
    let count = purchase_count(root, product_id).checked_add(1).ok_or_else(|| PersonalServiceError::new("CN payment count exceeds range"))?;
    if root.get("purchased_times_list").is_some_and(Value::is_null) {
        root.insert("purchased_times_list".to_owned(), json!({}));
    }
    root.entry("purchased_times_list".to_owned()).or_insert_with(|| json!({})).as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("invalid CN purchase counts"))?.insert(product_id.to_owned(), json!(count));
    Ok(())
}

// //// 将当前资产投影到严格的客户端支付回包 [@x380kkm 2026-09-22] ////
pub(super) fn refresh_finish_balances(response: &mut Value, root: &Map<String, Value>) {
    let user = root.get("user_info");
    response["after_vmoney"] = json!(user.and_then(|user| user.get("vmoney")).and_then(Value::as_i64).unwrap_or_default());
    response["after_free_vmoney"] = json!(user.and_then(|user| user.get("free_vmoney")).and_then(Value::as_i64).unwrap_or_default());
    response["monthly_payment_total"] = json!(user.and_then(|user| user.get("monthly_payment_total")).and_then(Value::as_i64).unwrap_or_default());
    for field in ["purchased_times_list", "monthly_charge_bonus_info", "premium_bonus_list"] {
        response[field] = root.get(field).cloned().unwrap_or(Value::Null);
    }
    for field in ["item_list", "user_daily_challenge_point_list"] {
        if let Some(value) = root.get(field) { response[field] = value.clone(); }
    }
    let mut user_delta = response.get("user_info").and_then(Value::as_object).cloned().unwrap_or_default();
    user_delta.remove("vmoney");
    user_delta.remove("free_vmoney");
    for field in [
        "free_mana", "paid_mana", "exp_pool", "star_crumb", "boost_point", "boss_boost_point",
        "is_bought_fund_laite", "is_bought_fund_laite2", "is_bought_fund_laite3",
        "is_bought_fund_main_quest", "is_bought_fund_ex_quest", "month_card_remain_days",
        "weekly_bonus_remain_days", "enable_auto_3x",
    ] {
        if let Some(value) = user.and_then(|user| user.get(field)) { user_delta.insert(field.to_owned(), value.clone()); }
    }
    user_delta.insert("renewal_gift_remain_days".to_owned(), user.and_then(|user| user.get("renewal_gift_remain_days"))
        .filter(|value| value.is_object()).cloned().unwrap_or_else(|| json!({})));
    response["user_info"] = Value::Object(user_delta);
}
// //// /将当前资产投影到严格的客户端支付回包 ////
