// audience: internal
// # personal-service-cn-multi-special-exchange-catalog
// 活动和候选角色沿用客户端主表, 兑换截止时间保留主表的领取宽限期.

use crate::database::{
    evaluate_activity_schedule, ActivityMode, ActivityWindowStatus, ServiceDatabase,
};
use crate::PersonalServiceError;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

const FIXTURE: &str = include_str!("../../assets/cn-multi-special-exchange.json");
static CATALOG: OnceLock<Result<Catalog, String>> = OnceLock::new();
pub(super) const FAMILY: &str = "multi-special-exchange";

#[derive(Deserialize)]
pub(super) struct Catalog {
    pub(super) campaigns: BTreeMap<i64, Campaign>,
    pub(super) tickets: BTreeMap<i64, Vec<i64>>,
}

#[derive(Deserialize)]
pub(super) struct Campaign {
    pub(super) start_at_ms: i64,
    pub(super) end_at_ms: i64,
    pub(super) exchange_end_at_ms: i64,
    pub(super) ticket_ids: Vec<i64>,
}

pub(super) fn definitions() -> Result<&'static Catalog, PersonalServiceError> {
    CATALOG
        .get_or_init(|| serde_json::from_str(FIXTURE).map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| {
            PersonalServiceError::new(format!(
                "invalid CN multi special exchange definitions: {error}"
            ))
        })
}

// //// 按管理活动窗口和主表期限判断领取与兑换资格 [@x380kkm 2026-09-22] ////
impl Campaign {
    pub(super) fn can_draw(
        &self,
        database: &ServiceDatabase,
        campaign_id: i64,
    ) -> Result<bool, PersonalServiceError> {
        self.is_open(database, campaign_id, self.end_at_ms)
    }

    pub(super) fn can_exchange(
        &self,
        database: &ServiceDatabase,
        campaign_id: i64,
    ) -> Result<bool, PersonalServiceError> {
        self.is_open(database, campaign_id, self.exchange_end_at_ms)
    }

    fn is_open(
        &self,
        database: &ServiceDatabase,
        campaign_id: i64,
        end_at_ms: i64,
    ) -> Result<bool, PersonalServiceError> {
        let activity_id = format!("{FAMILY}:{campaign_id}");
        let reward_delay_ms = end_at_ms.saturating_sub(self.end_at_ms).max(0);
        if database
            .activity_temporary_reward_window(&activity_id, reward_delay_ms)?
            .is_some()
        {
            return Ok(true);
        }
        let now = database.current_server_time_millis()?;
        if let Some(mut schedule) = database.get_activity_schedule(&activity_id)? {
            if !schedule.enabled {
                return Ok(false);
            }
            if matches!(schedule.mode, ActivityMode::Manual | ActivityMode::Always) {
                return Ok(true);
            }
            schedule.end_at_ms = schedule
                .end_at_ms
                .saturating_add(reward_delay_ms);
            return Ok(
                evaluate_activity_schedule(&schedule, now).status == ActivityWindowStatus::Open
            );
        }
        Ok(self.start_at_ms <= now && now <= end_at_ms)
    }
}
// //// /按管理活动窗口和主表期限判断领取与兑换资格 ////
