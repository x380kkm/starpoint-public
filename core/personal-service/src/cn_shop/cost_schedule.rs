// audience: internal
// # personal-service-cn-shop-cost-schedule
// 商店月度成本以 CN 每日重置时刻所属月份选择, 单次和批量兑换共用同一费用表.

use super::catalog::{shop_item_is_active, shop_purchase_key, CN_TIMEZONE_OFFSET_SECONDS};
use crate::cn_tutorial::format_client_time;
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Deserialize)]
struct CostSchedules {
    items: BTreeMap<String, String>,
    schedules: BTreeMap<String, Vec<Map<String, Value>>>,
}

static COST_SCHEDULES: OnceLock<Result<CostSchedules, String>> = OnceLock::new();

// //// 将有效月份的兑换成本应用到商品 [@x380kkm 2026-09-21] ////
pub(super) fn apply_cost_schedule(
    item: &mut Map<String, Value>,
    shop_type: i64,
    shop_item_id: i64,
    response_time: i64,
) -> Result<bool, PersonalServiceError> {
    let schedules = COST_SCHEDULES
        .get_or_init(|| {
            serde_json::from_str(include_str!(
                "../../../../assets/cn-shop-cost-schedules.json"
            ))
            .map_err(|error| format!("CN shop cost schedules are invalid: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    let Some(group) = schedules
        .items
        .get(&shop_purchase_key(shop_type, shop_item_id))
    else {
        return Ok(true);
    };
    let period_time = format_client_time(
        response_time
            .saturating_add(CN_TIMEZONE_OFFSET_SECONDS)
            .saturating_sub(super::period::CLIENT_DAILY_RESET_SECONDS),
    );
    let month = period_time
        .get(5..7)
        .and_then(|value| value.parse::<i64>().ok())
        .ok_or_else(|| PersonalServiceError::new("CN shop cost month is invalid"))?;
    let Some(schedule) = schedules
        .schedules
        .get(group)
        .into_iter()
        .flatten()
        .find(|row| {
            row.get("month").and_then(Value::as_i64) == Some(month)
                && shop_item_is_active(row, response_time)
        })
    else {
        return Ok(false);
    };
    item.insert(
        "costs".to_owned(),
        schedule
            .get("costs")
            .cloned()
            .ok_or_else(|| PersonalServiceError::new("CN shop schedule costs are missing"))?,
    );
    Ok(true)
}
// //// /将有效月份的兑换成本应用到商品 ////
