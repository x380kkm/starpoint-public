// audience: internal
// # personal-service-cn-shop-selection-catalog
// 自选商品按商店类型和活动编号归属, 兑换期沿用客户端主表与管理台日历.

use crate::database::{evaluate_activity_schedule, ActivityWindowStatus, ServiceDatabase};
use crate::PersonalServiceError;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

const SOURCE: &str = include_str!("../../../assets/cn-shop-selection.json");
static CATALOG: OnceLock<Result<Catalog, String>> = OnceLock::new();

#[derive(Deserialize)]
pub(super) struct Catalog {
    pub(super) campaigns: BTreeMap<String, Campaign>,
    pub(super) products: BTreeMap<String, Product>,
}

#[derive(Deserialize)]
pub(super) struct Campaign {
    pub(super) activity_id: String,
    pub(super) event_type: i64,
    pub(super) event_id: i64,
    start_time: i64,
    end_time: i64,
    exchange_end_time: i64,
    pub(super) lineups: Vec<i64>,
}

#[derive(Deserialize)]
pub(super) struct Product {
    pub(super) campaign: String,
    pub(super) lineup_id: Option<i64>,
}

pub(super) fn catalog() -> Result<&'static Catalog, PersonalServiceError> {
    CATALOG
        .get_or_init(|| serde_json::from_str(SOURCE).map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

impl Campaign {
    pub(super) fn event_activity_id(&self) -> Result<String, PersonalServiceError> {
        super::super::catalog::event_activity_id(self.event_type, self.event_id)
            .ok_or_else(|| PersonalServiceError::new("CN selection shop event is invalid"))
    }

    // //// 按独立自选活动的日历计算兑换期 [@x380kkm 2026-09-23] ////
    pub(super) fn is_available(
        &self,
        database: &ServiceDatabase,
        now: i64,
    ) -> Result<bool, PersonalServiceError> {
        let event = super::super::activity::event_shop_activity_state(
            database,
            &self.event_activity_id()?,
        )?;
        if event.status != ActivityWindowStatus::Open {
            return Ok(false);
        }
        let reward_period = self
            .exchange_end_time
            .saturating_sub(self.end_time)
            .max(0)
            .saturating_mul(1000);
        if database
            .activity_temporary_reward_window(&self.activity_id, reward_period)?
            .is_some()
        {
            return Ok(true);
        }
        if let Some(mut schedule) = database.get_activity_schedule(&self.activity_id)? {
            schedule.end_at_ms = schedule.end_at_ms.saturating_add(reward_period);
            return Ok(
                evaluate_activity_schedule(&schedule, now.saturating_mul(1000)).status
                    == ActivityWindowStatus::Open,
            );
        }
        Ok(now >= self.start_time && now <= self.exchange_end_time)
    }
}
