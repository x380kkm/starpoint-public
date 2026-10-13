// audience: internal
// # personal-service-cn-ranking-period
// 定时竞速的统计与领奖期限和客户端活动投影使用同一时长. 手动开放规则允许即时领奖.

use super::Event;
use crate::activity_projection::{AGGREGATION_DELAY_MS, REWARD_DELAY_MS};
use crate::database::{evaluate_activity_schedule, ActivityMode, ActivityWindowStatus, ServiceDatabase};
use crate::PersonalServiceError;

// //// 保留结束活动的成绩查询 [@x380kkm 2026-09-21] ////
pub(super) fn summary_error(
    database: &ServiceDatabase,
    event_id: i64,
) -> Result<Option<&'static str>, PersonalServiceError> {
    if database.activity_temporary_reward_window(&format!("ranking:{event_id}"), REWARD_DELAY_MS)?.is_some() {
        return Ok(None);
    }
    let now = database.current_server_time_millis()?;
    Ok(match database.activity_window_status(&format!("ranking:{event_id}"), now)? {
        ActivityWindowStatus::Disabled => Some("activity_disabled"),
        ActivityWindowStatus::NotStarted => Some("activity_not_started"),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::parse_iso_timestamp;
    use tempfile::TempDir;

    // //// 按原始竞速统计和领取日期开放区域奖励 [@x380kkm 2026-09-27] ////
    #[test]
    fn regional_reward_period_uses_aggregation_and_receive_dates() {
        let root = TempDir::new().unwrap();
        let mut database = ServiceDatabase::open(root.path()).unwrap();
        let ready = parse_iso_timestamp("2020-11-12T04:00:00Z").unwrap();
        let period = crate::activity_projection::regional_activity_period("ranking:2", ready).unwrap().unwrap();
        assert_eq!(period.reward_end_at_ms, parse_iso_timestamp("2020-11-19T03:59:59Z").unwrap());
        let event = &super::super::catalog().unwrap()[&2];
        database.set_virtual_time(true, period.aggregation_end_at_ms - 1000, 1.0).unwrap();
        assert_eq!(receive_error(&database, 2, event).unwrap(), Some("ranking_reward_not_ready"));
        database.set_virtual_time(true, ready, 1.0).unwrap();
        assert_eq!(receive_error(&database, 2, event).unwrap(), None);
        database.upsert_manual_activity("ranking:2", false).unwrap();
        assert_eq!(receive_error(&database, 2, event).unwrap(), Some("activity_disabled"));
    }
    // //// /按原始竞速统计和领取日期开放区域奖励 ////
}

// //// 按统计结束和领奖结束判断领取窗口 [@x380kkm 2026-09-21] ////
pub(super) fn receive_error(
    database: &ServiceDatabase,
    event_id: i64,
    event: &Event,
) -> Result<Option<&'static str>, PersonalServiceError> {
    let activity_id = format!("ranking:{event_id}");
    if let Some((_, expires_at_ms)) = database.activity_temporary_reward_window(&activity_id, REWARD_DELAY_MS)? {
        let wall_now = database.current_wall_time_millis()?;
        let elapsed = wall_now.saturating_sub(expires_at_ms) as f64 * database.current_server_time_rate()?;
        return Ok((wall_now < expires_at_ms || elapsed <= AGGREGATION_DELAY_MS as f64)
            .then_some("ranking_reward_not_ready"));
    }
    let now = database.current_server_time_millis()?;
    let (start, playable_end, aggregation_end, reward_end) = if let Some(mut schedule) = database.get_activity_schedule(&activity_id)? {
        if !schedule.enabled { return Ok(Some("activity_disabled")); }
        if matches!(schedule.mode, ActivityMode::Manual | ActivityMode::Always) { return Ok(None); }
        schedule.end_at_ms = schedule.end_at_ms.saturating_add(REWARD_DELAY_MS);
        let window = evaluate_activity_schedule(&schedule, now);
        match (window.active_start_ms, window.active_end_ms) {
            (Some(start), Some(reward_end)) => {
                let playable_end = reward_end.saturating_sub(REWARD_DELAY_MS);
                (start, playable_end, playable_end.saturating_add(AGGREGATION_DELAY_MS), reward_end)
            }
            _ => return Ok(Some(if window.status == ActivityWindowStatus::NotStarted {
                "activity_not_started"
            } else { "ranking_reward_expired" })),
        }
    } else if let Some(period) = crate::activity_projection::regional_activity_period(&activity_id, now)? {
        (period.start_at_ms, period.end_at_ms, period.aggregation_end_at_ms, period.reward_end_at_ms)
    } else {
        (event.start_at_ms, event.play_end_at_ms, event.aggregation_end_at_ms, event.reward_end_at_ms)
    };
    Ok(if now < start { Some("activity_not_started") }
        else if now <= playable_end { Some("ranking_reward_not_ready") }
        else if now <= aggregation_end { Some("ranking_reward_not_ready") }
        else if now > reward_end { Some("ranking_reward_expired") }
        else { None })
}
