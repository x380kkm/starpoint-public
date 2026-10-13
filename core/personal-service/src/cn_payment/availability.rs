// audience: internal
// # personal-service-cn-payment-availability
// 商品目录与下单共用虚拟时窗, 临时开放沿用活动日历的真实时间租约.
// 管理日历的右开毫秒边界转换为客户端包含终点的 Unix 秒.

use super::catalog::{catalog, PaymentEntitlement, PaymentProduct, PaymentReward};
use crate::database::{evaluate_activity_schedule, ActivityWindowStatus, ServiceDatabase};
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};

const ALWAYS_END_SECONDS: i64 = 253_402_300_799;

impl PaymentProduct {
    // //// 按当前时窗和关联权益选取商品有效期 [@x380kkm 2026-09-27] ////
    pub(crate) fn available_period(
        &self,
        root: &Map<String, Value>,
        database: &ServiceDatabase,
        now: i64,
    ) -> Result<Option<(i64, i64)>, PersonalServiceError> {
        let Some(period) = self.open_period(database, now)? else { return Ok(None); };
        if self.entitlements.iter().any(|value| matches!(value, PaymentEntitlement::PassCard))
            && crate::cn_pass_card::purchase_event_id(root, database, now)?.is_none()
        {
            return Ok(None);
        }
        if self.rewards.iter().any(|value| matches!(value, PaymentReward::PassCardPoint { .. }))
            && crate::cn_pass_card::point_purchase_target(root, database, now)?.is_none()
        {
            return Ok(None);
        }
        Ok(Some(period))
    }

    // //// 将商品日历规则映射为客户端购买窗口 [@x380kkm 2026-09-27] ////
    fn open_period(&self, database: &ServiceDatabase, now: i64) -> Result<Option<(i64, i64)>, PersonalServiceError> {
        let id = format!("payment:{}", self.store_product_id);
        if let Some((opened_at, expires_at)) = database.activity_temporary_open_window(&id)? {
            let (start, end) = database.project_wall_time_window(opened_at, expires_at)?;
            return Ok(Some(client_period(start, end)));
        }
        if let Some(schedule) = database.get_activity_schedule(&id)? {
            let active = evaluate_activity_schedule(&schedule, now.saturating_mul(1000));
            return Ok((active.status == ActivityWindowStatus::Open).then(|| {
                active.active_start_ms.zip(active.active_end_ms)
                    .map(|(start, end)| client_period(start, end))
                    .unwrap_or((0, ALWAYS_END_SECONDS))
            }));
        }
        Ok((self.start_time <= now && now <= self.end_time).then_some((self.start_time, self.end_time)))
    }
}

// //// 将右开毫秒窗口转换为包含终点的秒窗口 [@x380kkm 2026-09-27] ////
fn client_period(start: i64, end: i64) -> (i64, i64) {
    (start / 1000, end.saturating_sub(1) / 1000)
}

// //// 在内购管理中提供全部商品与原始开放时间 [@x380kkm 2026-09-27] ////
pub(crate) fn activity_definitions() -> Result<Vec<Value>, PersonalServiceError> {
    Ok(catalog()?.products.values().map(|product| json!({
        "activity_id":format!("payment:{}", product.store_product_id),
        "kind":"payment", "name":product.name,
        "description":product.store_product_id, "tags":["内购商品"],
        "default_start_at_ms":product.start_time.saturating_mul(1000),
        "default_end_at_ms":product.end_time.saturating_add(1).saturating_mul(1000),
    })).collect())
}

// //// 返回商品主表的管理日历默认窗口 [@x380kkm 2026-09-27] ////
pub(crate) fn default_activity_window(activity_id: &str) -> Result<Option<(i64, i64)>, PersonalServiceError> {
    let Some(id) = activity_id.strip_prefix("payment:") else { return Ok(None); };
    Ok(catalog()?.products.get(id).map(|product| (
        product.start_time.saturating_mul(1000),
        product.end_time.saturating_add(1).saturating_mul(1000),
    )))
}
