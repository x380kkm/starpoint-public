// audience: internal
// # personal-service-cn-shop-activity
// 活动商店的兑换期限与客户端活动投影共用结束偏移, 库存窗口覆盖比赛和兑换期限.

use super::catalog::event_shop_activity_alias;
use crate::activity_projection::{managed_item_periods, ItemPeriods, REWARD_DELAY_MS};
use crate::database::{
    evaluate_activity_schedule, ActivityMode, ActivityWindowStatus, ServiceDatabase,
};
use crate::PersonalServiceError;
use serde_json::{Map, Value};

pub(super) struct EventShopActivityState {
    pub(super) activity_id: String,
    pub(super) status: ActivityWindowStatus,
    item_periods: Option<ItemPeriods>,
    pub(super) key: Option<String>,
    configured: bool,
}

impl EventShopActivityState {
    // //// 按客户端商品窗口判断兑换时间 [@x380kkm 2026-09-26] ////
    pub(super) fn item_is_active(
        &self,
        shop_item_id: i64,
        item: &Map<String, Value>,
        response_time: i64,
    ) -> bool {
        self.item_periods.as_ref().map_or_else(
            || super::catalog::shop_item_is_active(item, response_time),
            |periods| {
                periods.get(&shop_item_id).is_some_and(|(start, end)| {
                    let now = response_time.saturating_mul(1_000);
                    now >= *start && now <= *end
                })
            },
        )
    }
}

// //// 解析活动商店的兑换窗口和共享目录别名 [@x380kkm 2026-09-21] ////
pub(super) fn event_shop_activity_state(
    database: &ServiceDatabase,
    activity_id: &str,
) -> Result<EventShopActivityState, PersonalServiceError> {
    let state = activity_state(database, activity_id)?;
    if !state.configured {
        if let Some(alias) = event_shop_activity_alias(activity_id)? {
            let alias_state = activity_state(database, &alias)?;
            if alias_state.configured {
                return Ok(alias_state);
            }
        }
    }
    Ok(state)
}
// //// /解析活动商店的兑换窗口和共享目录别名 ////

// //// 计算单个活动的兑换状态和库存窗口标识 [@x380kkm 2026-09-21] ////
fn activity_state(
    database: &ServiceDatabase,
    activity_id: &str,
) -> Result<EventShopActivityState, PersonalServiceError> {
    if let Some((opened_at_ms, _)) =
        database.activity_temporary_reward_window(activity_id, REWARD_DELAY_MS)?
    {
        return Ok(EventShopActivityState {
            activity_id: activity_id.to_owned(),
            status: ActivityWindowStatus::Open,
            item_periods: managed_item_periods(database, activity_id)?,
            key: Some(format!("temporary:{activity_id}:{opened_at_ms}")),
            configured: true,
        });
    }
    let Some(mut schedule) = database.get_activity_schedule(activity_id)? else {
        return Ok(EventShopActivityState {
            activity_id: activity_id.to_owned(),
            status: ActivityWindowStatus::Open,
            item_periods: managed_item_periods(database, activity_id)?,
            key: Some(format!("static:{activity_id}:persistent")),
            configured: false,
        });
    };
    schedule.end_at_ms = schedule.end_at_ms.saturating_add(REWARD_DELAY_MS);
    let evaluation = evaluate_activity_schedule(&schedule, database.current_server_time_millis()?);
    let key = (evaluation.status == ActivityWindowStatus::Open).then(|| match schedule.mode {
        ActivityMode::Manual | ActivityMode::Always => format!("schedule:{activity_id}:persistent"),
        _ => format!(
            "schedule:{activity_id}:{}",
            evaluation.active_start_ms.unwrap_or(schedule.start_at_ms)
        ),
    });
    Ok(EventShopActivityState {
        activity_id: activity_id.to_owned(),
        status: evaluation.status,
        item_periods: if evaluation.status == ActivityWindowStatus::Open {
            managed_item_periods(database, activity_id)?
        } else {
            None
        },
        key,
        configured: true,
    })
}
// //// /计算单个活动的兑换状态和库存窗口标识 ////
