// audience: internal
// # personal-service-cn-shop-period
//
// 该模块根据客户端商店 master 的购买限制计算当前日、月和活动窗口库存.

use super::catalog::{
    activity_shop_item_ids, event_activity_id, event_shop_identity, shop_purchase_key,
    shop_purchase_limits, ShopPurchaseLimits, CN_TIMEZONE_OFFSET_SECONDS, SHOP_TYPE_EVENT,
};
use crate::cn_tutorial::format_client_time;
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use serde_json::{Map, Value};
use std::path::Path;

const PURCHASE_COUNTS_FIELD: &str = "shop_purchase_counts";
const PURCHASE_BASELINES_FIELD: &str = "shop_purchase_count_baselines";
const PURCHASE_WINDOWS_FIELD: &str = "shop_purchase_windows";
pub(super) const CLIENT_DAILY_RESET_SECONDS: i64 = 5 * 60 * 60;

struct ClientPeriodKeys {
    day: String,
    month: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PurchaseWindow {
    day_key: String,
    day_baseline: i64,
    month_key: String,
    month_baseline: i64,
    activity_key: Option<String>,
    activity_baseline: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ShopPurchaseState {
    pub(super) total_purchase_num: i64,
    pub(super) today_purchase_num: i64,
    pub(super) this_month_purchase_num: i64,
    pub(super) stock_quantity: i64,
    pub(super) changed: bool,
}

// //// 读取当前商品的累计购买次数 ////
fn cumulative_purchase_count(root: &Map<String, Value>, purchase_key: &str) -> i64 {
    root.get(PURCHASE_COUNTS_FIELD)
        .and_then(Value::as_object)
        .and_then(|counts| counts.get(purchase_key))
        .and_then(Value::as_i64)
        .unwrap_or_default()
        .max(0)
}
// //// /读取当前商品的累计购买次数 ////

// //// 读取历史兼容基线 ////
fn legacy_purchase_baseline(root: &Map<String, Value>, purchase_key: &str) -> Option<i64> {
    root.get(PURCHASE_BASELINES_FIELD)
        .and_then(Value::as_object)
        .and_then(|baselines| baselines.get(purchase_key))
        .and_then(Value::as_i64)
        .map(|value| value.max(0))
}
// //// /读取历史兼容基线 ////

// //// 读取已保存的周期窗口 ////
fn read_purchase_window(root: &Map<String, Value>, purchase_key: &str) -> Option<PurchaseWindow> {
    let entry = root
        .get(PURCHASE_WINDOWS_FIELD)
        .and_then(Value::as_object)
        .and_then(|windows| windows.get(purchase_key))
        .and_then(Value::as_object)?;
    Some(PurchaseWindow {
        day_key: entry.get("day_key")?.as_str()?.to_owned(),
        day_baseline: entry.get("day_baseline")?.as_i64()?.max(0),
        month_key: entry.get("month_key")?.as_str()?.to_owned(),
        month_baseline: entry.get("month_baseline")?.as_i64()?.max(0),
        activity_key: entry
            .get("activity_key")
            .and_then(Value::as_str)
            .map(str::to_owned),
        activity_baseline: entry
            .get("activity_baseline")
            .and_then(Value::as_i64)
            .unwrap_or_default()
            .max(0),
    })
}
// //// /读取已保存的周期窗口 ////

// //// 保存周期窗口 ////
fn write_purchase_window(
    root: &mut Map<String, Value>,
    purchase_key: &str,
    window: &PurchaseWindow,
) -> Result<(), PersonalServiceError> {
    let windows = root
        .entry(PURCHASE_WINDOWS_FIELD.to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN shop windows are invalid"))?;
    windows.insert(
        purchase_key.to_owned(),
        serde_json::json!({
            "day_key": window.day_key,
            "day_baseline": window.day_baseline,
            "month_key": window.month_key,
            "month_baseline": window.month_baseline,
            "activity_key": window.activity_key,
            "activity_baseline": window.activity_baseline,
        }),
    );
    Ok(())
}
// //// /保存周期窗口 ////

// //// 返回客户端商店使用的日和月窗口 [@x380kkm 2026-08-29] ////
fn client_period_keys(response_time: i64) -> Result<ClientPeriodKeys, PersonalServiceError> {
    let shifted_time = response_time
        .saturating_add(CN_TIMEZONE_OFFSET_SECONDS)
        .saturating_sub(CLIENT_DAILY_RESET_SECONDS);
    let client_time = format_client_time(shifted_time);
    let day = client_time
        .get(..10)
        .ok_or_else(|| PersonalServiceError::new("CN shop client time is invalid"))?
        .to_owned();
    let month = client_time
        .get(..7)
        .ok_or_else(|| PersonalServiceError::new("CN shop client month is invalid"))?
        .to_owned();
    Ok(ClientPeriodKeys { day, month })
}
// //// /返回客户端商店使用的日和月窗口 ////

// //// 返回活动商店当前窗口标识 ////
fn active_activity_key(
    database: &ServiceDatabase,
    _asset_root: &Path,
    shop_type: i64,
    shop_item_id: i64,
    _now_ms: i64,
) -> Result<Option<String>, PersonalServiceError> {
    if shop_type != SHOP_TYPE_EVENT {
        return Ok(None);
    }
    if let Some(activity_id) = super::selection::product_activity_id(shop_type, shop_item_id)? {
        return Ok(super::activity::event_shop_activity_state(database, activity_id)?.key);
    }
    let Some((event_type, event_id)) = event_shop_identity(shop_item_id)? else {
        return Ok(None);
    };
    let Some(activity_id) = event_activity_id(event_type, event_id) else {
        return Ok(None);
    };
    Ok(super::activity::event_shop_activity_state(database, &activity_id)?.key)
}
// //// /返回活动商店当前窗口标识 ////

// //// 判断静态活动窗口是否沿用当前购买基线 [@x380kkm 2026-08-30] ////
fn static_activity_reuses_baseline(previous_key: Option<&str>, next_key: Option<&str>) -> bool {
    next_key.is_some_and(|key| key.starts_with("static:"))
        && previous_key.map_or(true, |key| key.starts_with("default:"))
}
// //// /判断静态活动窗口是否沿用当前购买基线 ////

// //// 计算限制后的剩余库存 ////
fn remaining_stock(
    limits: ShopPurchaseLimits,
    today_purchase_num: i64,
    this_month_purchase_num: i64,
    window_purchase_num: i64,
) -> i64 {
    let mut remaining = Vec::new();
    if let Some(limit) = limits.daily_stock {
        remaining.push(limit.saturating_sub(today_purchase_num).max(0));
    }
    if let Some(limit) = limits.monthly_stock {
        remaining.push(limit.saturating_sub(this_month_purchase_num).max(0));
    }
    if let Some(limit) = limits.max_frequency {
        remaining.push(limit.saturating_sub(window_purchase_num).max(0));
    }
    remaining.into_iter().min().unwrap_or(-1)
}
// //// /计算限制后的剩余库存 ////

// //// 计算并同步商品周期购买状态 ////
pub(super) fn synchronize_purchase_state(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    asset_root: &Path,
    shop_type: i64,
    shop_item_id: i64,
    response_time: i64,
) -> Result<ShopPurchaseState, PersonalServiceError> {
    calculate_purchase_state(
        root,
        database,
        asset_root,
        shop_type,
        shop_item_id,
        response_time,
        true,
    )
}
// //// /计算并同步商品周期购买状态 ////

// //// 计算商品周期购买状态 ////
pub(super) fn purchase_state(
    root: &Map<String, Value>,
    database: &ServiceDatabase,
    asset_root: &Path,
    shop_type: i64,
    shop_item_id: i64,
    response_time: i64,
) -> Result<ShopPurchaseState, PersonalServiceError> {
    let mut copy = root.clone();
    calculate_purchase_state(
        &mut copy,
        database,
        asset_root,
        shop_type,
        shop_item_id,
        response_time,
        false,
    )
}
// //// /计算商品周期购买状态 ////

fn calculate_purchase_state(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    asset_root: &Path,
    shop_type: i64,
    shop_item_id: i64,
    response_time: i64,
    persist_window: bool,
) -> Result<ShopPurchaseState, PersonalServiceError> {
    let purchase_key = shop_purchase_key(shop_type, shop_item_id);
    let cumulative = cumulative_purchase_count(root, &purchase_key);
    let legacy_baseline = legacy_purchase_baseline(root, &purchase_key);
    let period_keys = client_period_keys(response_time)?;
    let day_key = period_keys.day;
    let month_key = period_keys.month;
    let activity_key = active_activity_key(
        database,
        asset_root,
        shop_type,
        shop_item_id,
        response_time.saturating_mul(1_000),
    )?;
    let previous = read_purchase_window(root, &purchase_key);
    let manual_baseline = legacy_baseline.unwrap_or_default().min(cumulative);
    let initial_activity_baseline = activity_key
        .as_deref()
        .is_some_and(|key| key.starts_with("schedule:") || key.starts_with("temporary:"))
        .then_some(cumulative)
        .unwrap_or(manual_baseline);
    let mut next = previous.clone().unwrap_or_else(|| PurchaseWindow {
        day_key: day_key.clone(),
        day_baseline: cumulative,
        month_key: month_key.clone(),
        month_baseline: cumulative,
        activity_key: activity_key.clone(),
        activity_baseline: initial_activity_baseline,
    });
    if next.day_key != day_key {
        next.day_key = day_key;
        next.day_baseline = cumulative;
    }
    if next.month_key != month_key {
        next.month_key = month_key;
        next.month_baseline = cumulative;
    }
    if next.activity_key != activity_key {
        let reuses_baseline =
            static_activity_reuses_baseline(next.activity_key.as_deref(), activity_key.as_deref());
        next.activity_key = activity_key.clone();
        if !reuses_baseline {
            next.activity_baseline = cumulative;
        }
    }
    let changed = previous.as_ref() != Some(&next);
    if persist_window && changed {
        write_purchase_window(root, &purchase_key, &next)?;
    }
    let day_baseline = next.day_baseline.max(manual_baseline).min(cumulative);
    let month_baseline = next.month_baseline.max(manual_baseline).min(cumulative);
    let total_baseline = next
        .activity_key
        .is_some()
        .then_some(next.activity_baseline)
        .unwrap_or_default()
        .max(manual_baseline)
        .min(cumulative);
    let limits = shop_purchase_limits(shop_type, shop_item_id)?;
    let today_purchase_num = cumulative.saturating_sub(day_baseline).max(0);
    let this_month_purchase_num = cumulative.saturating_sub(month_baseline).max(0);
    let displayed_total_purchase_num = cumulative.saturating_sub(total_baseline).max(0);
    let stock_quantity = remaining_stock(
        limits,
        today_purchase_num,
        this_month_purchase_num,
        displayed_total_purchase_num,
    );
    Ok(ShopPurchaseState {
        total_purchase_num: displayed_total_purchase_num,
        today_purchase_num,
        this_month_purchase_num,
        stock_quantity,
        changed,
    })
}
// //// /计算商品周期购买状态 ////

// //// 手动建立新的商品库存基线 ////
pub(super) fn reset_purchase_state(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    asset_root: &Path,
    shop_type: i64,
    shop_item_id: i64,
    response_time: i64,
) -> Result<ShopPurchaseState, PersonalServiceError> {
    synchronize_purchase_state(
        root,
        database,
        asset_root,
        shop_type,
        shop_item_id,
        response_time,
    )?;
    let purchase_key = shop_purchase_key(shop_type, shop_item_id);
    let cumulative = cumulative_purchase_count(root, &purchase_key);
    let period_keys = client_period_keys(response_time)?;
    let activity_key = active_activity_key(
        database,
        asset_root,
        shop_type,
        shop_item_id,
        response_time.saturating_mul(1_000),
    )?;
    let window = PurchaseWindow {
        day_key: period_keys.day,
        day_baseline: cumulative,
        month_key: period_keys.month,
        month_baseline: cumulative,
        activity_key,
        activity_baseline: cumulative,
    };
    write_purchase_window(root, &purchase_key, &window)?;
    let baselines = root
        .entry(PURCHASE_BASELINES_FIELD.to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN shop baselines are invalid"))?;
    baselines.insert(purchase_key, Value::from(cumulative));
    calculate_purchase_state(
        root,
        database,
        asset_root,
        shop_type,
        shop_item_id,
        response_time,
        false,
    )
}
// //// /手动建立新的商品库存基线 ////

// //// 将活动商店的累计次数设为新库存基线 [@x380kkm 2026-09-21] ////
pub(super) fn reset_activity_shop_purchases(
    root: &mut Map<String, Value>,
    activity_id: &str,
) -> Result<usize, PersonalServiceError> {
    let mut reset_count = 0;
    for item_id in activity_shop_item_ids(activity_id)? {
        let key = shop_purchase_key(SHOP_TYPE_EVENT, item_id);
        let cumulative = cumulative_purchase_count(root, &key);
        let has_window = root
            .get(PURCHASE_WINDOWS_FIELD)
            .and_then(Value::as_object)
            .is_some_and(|windows| windows.contains_key(&key));
        if cumulative == 0 && !has_window {
            continue;
        }
        let baselines = root
            .entry(PURCHASE_BASELINES_FIELD.to_owned())
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN shop baselines are invalid"))?;
        baselines.insert(key.clone(), Value::from(cumulative));
        if let Some(windows) = root
            .get_mut(PURCHASE_WINDOWS_FIELD)
            .and_then(Value::as_object_mut)
        {
            windows.remove(&key);
        }
        reset_count += 1;
    }
    Ok(reset_count)
}
// //// /将活动商店的累计次数设为新库存基线 ////

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // //// 活动重置仅刷新映射商品并保留资产和累计购买次数 [@x380kkm 2026-09-21] ////
    #[test]
    fn resets_only_the_selected_activity_shop() {
        let mut data = json!({
            "item_list": {"90030": 10},
            "shop_purchase_counts": {"4:1": 2, "4:310126": 9, "7:1": 3},
            "shop_purchase_count_baselines": {"4:310126": 1},
            "shop_purchase_windows": {"4:1": {"day_baseline": 0}, "4:310126": {"day_baseline": 1}},
        });
        let count =
            reset_activity_shop_purchases(data.as_object_mut().unwrap(), "carnival:1").unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            data["shop_purchase_counts"],
            json!({"4:1": 2, "4:310126": 9, "7:1": 3})
        );
        assert_eq!(
            data["shop_purchase_count_baselines"],
            json!({"4:1": 2, "4:310126": 1})
        );
        assert_eq!(
            data["shop_purchase_windows"],
            json!({"4:310126": {"day_baseline": 1}})
        );
        assert_eq!(data["item_list"], json!({"90030": 10}));
        assert!(super::super::has_activity_shop("rush:700011").unwrap());
        assert!(!super::super::has_activity_shop("carnival:999999").unwrap());
    }
    // //// /活动重置仅刷新映射商品并保留资产和累计购买次数 ////
}
