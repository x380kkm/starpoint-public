// audience: internal
// # personal-service-cn-event-reward-period
// 手动开放允许即时领奖, 定时活动沿用客户端投影的统计和领奖期限.

use super::rewards::EventPeriod;
use crate::activity_projection::{AGGREGATION_DELAY_MS, REWARD_DELAY_MS};
use crate::database::{
    evaluate_activity_schedule, ActivityMode, ActivityWindowStatus, ServiceDatabase,
};
use crate::PersonalServiceError;

// //// 计算活动查询和领取使用的时间范围 [@x380kkm 2026-09-26] ////
fn window(
    database: &ServiceDatabase,
    activity_id: &str,
    period: &EventPeriod,
) -> Result<Result<Option<(i64, i64, Option<i64>)>, &'static str>, PersonalServiceError> {
    let now = database.current_server_time_millis()?;
    if let Some((opened_at_ms, expires_at_ms)) =
        database.activity_temporary_reward_window(activity_id, REWARD_DELAY_MS)?
    {
        let (start, end) = database.project_wall_time_window(opened_at_ms, expires_at_ms)?;
        return Ok(Ok(Some((
            start,
            end.saturating_add(AGGREGATION_DELAY_MS),
            Some(end.saturating_add(REWARD_DELAY_MS)),
        ))));
    }
    if let Some(mut schedule) = database.get_activity_schedule(activity_id)? {
        if !schedule.enabled {
            return Ok(Err("activity_disabled"));
        }
        if matches!(schedule.mode, ActivityMode::Manual | ActivityMode::Always) {
            return Ok(Ok(None));
        }
        schedule.end_at_ms = schedule.end_at_ms.saturating_add(REWARD_DELAY_MS);
        let evaluated = evaluate_activity_schedule(&schedule, now);
        return Ok(match (evaluated.active_start_ms, evaluated.active_end_ms) {
            (Some(start), Some(end)) => Ok(Some((
                start,
                end.saturating_sub(REWARD_DELAY_MS)
                    .saturating_add(AGGREGATION_DELAY_MS),
                Some(end),
            ))),
            _ if evaluated.status == ActivityWindowStatus::NotStarted => {
                Err("activity_not_started")
            }
            _ => Err("activity_reward_expired"),
        });
    }
    if let Some(regional) = crate::activity_projection::regional_activity_period(activity_id, now)? {
        return Ok(Ok(Some((regional.start_at_ms, regional.aggregation_end_at_ms, Some(regional.reward_end_at_ms)))));
    }
    Ok(Ok(Some((
        period.start_at_ms,
        period
            .aggregation_end_at_ms
            .or(period.play_end_at_ms)
            .unwrap_or(i64::MAX),
        period.reward_end_at_ms,
    ))))
}

// //// 允许活动总奖励在领奖期限内查询 [@x380kkm 2026-09-21] ////
pub(in crate::cn_activity) fn summary_error(
    database: &ServiceDatabase,
    activity_id: &str,
    period: &EventPeriod,
) -> Result<Option<&'static str>, PersonalServiceError> {
    let window = match window(database, activity_id, period)? {
        Ok(window) => window,
        Err(error) => return Ok(Some(error)),
    };
    let Some((start, _, end)) = window else {
        return Ok(None);
    };
    let now = database.current_server_time_millis()?;
    Ok(if now < start {
        Some("activity_not_started")
    } else if end.is_some_and(|end| now > end) {
        Some("activity_reward_expired")
    } else {
        None
    })
}

// //// 在统计完成后发放排名奖励 [@x380kkm 2026-09-21] ////
pub(in crate::cn_activity) fn ranking_error(
    database: &ServiceDatabase,
    activity_id: &str,
    period: &EventPeriod,
) -> Result<Option<&'static str>, PersonalServiceError> {
    if let Some(error) = summary_error(database, activity_id, period)? {
        return Ok(Some(error));
    }
    let Ok(Some((_, ready, _))) = window(database, activity_id, period)? else {
        return Ok(None);
    };
    Ok((database.current_server_time_millis()? <= ready).then_some("activity_reward_not_ready"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::parse_iso_timestamp;
    use tempfile::TempDir;

    // //// 在区域团队战结束后保留原始领奖期限 [@x380kkm 2026-09-27] ////
    #[test]
    fn regional_raid_keeps_reward_tail() {
        let root = TempDir::new().unwrap();
        let mut database = ServiceDatabase::open(root.path()).unwrap();
        let now = parse_iso_timestamp("2021-12-17T04:00:00Z").unwrap();
        let event = &super::super::rewards::catalog().unwrap().raid[&1];
        database.set_virtual_time(true, now, 1.0).unwrap();
        assert_eq!(summary_error(&database, "raid:1", &event.period).unwrap(), None);
        database.upsert_manual_activity("raid:1", false).unwrap();
        assert_eq!(summary_error(&database, "raid:1", &event.period).unwrap(), Some("activity_disabled"));
    }
    // //// /在区域团队战结束后保留原始领奖期限 ////
}
