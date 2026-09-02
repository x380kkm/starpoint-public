// audience: internal
// # personal-service-activity-calendar
//
// 该模块保存活动时间窗口. 窗口判断使用个人服务的虚拟游戏时间,
// 临时开放租约使用真实墙钟并与窗口状态叠加.

use super::ServiceDatabase;
use crate::PersonalServiceError;
use rusqlite::{params, Connection, OptionalExtension};

const MAX_INTERVAL_DAYS: i64 = 3650;
const MILLIS_PER_SECOND: i64 = 1_000;
const MILLIS_PER_MINUTE: i64 = 60 * MILLIS_PER_SECOND;
const MILLIS_PER_HOUR: i64 = 60 * MILLIS_PER_MINUTE;
const MILLIS_PER_DAY: i64 = 24 * MILLIS_PER_HOUR;
const TEMPORARY_OPEN_DURATION_MS: i64 = MILLIS_PER_DAY;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivityMode {
    Manual,
    Always,
    Window,
    Periodic,
}

impl ActivityMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Always => "always",
            Self::Window => "window",
            Self::Periodic => "periodic",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "manual" => Some(Self::Manual),
            "always" => Some(Self::Always),
            "window" => Some(Self::Window),
            "periodic" => Some(Self::Periodic),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivityPeriod {
    Once,
    Daily,
    Weekly,
    Monthly,
    IntervalDays,
}

impl ActivityPeriod {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
            Self::IntervalDays => "interval_days",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "once" => Some(Self::Once),
            "daily" => Some(Self::Daily),
            "weekly" => Some(Self::Weekly),
            "monthly" => Some(Self::Monthly),
            "interval_days" => Some(Self::IntervalDays),
            _ => None,
        }
    }

    fn is_periodic(self) -> bool {
        !matches!(self, Self::Once)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActivitySchedule {
    pub(crate) activity_id: String,
    pub(crate) enabled: bool,
    pub(crate) mode: ActivityMode,
    pub(crate) period: ActivityPeriod,
    pub(crate) interval_days: Option<i64>,
    pub(crate) start_at_ms: i64,
    pub(crate) end_at_ms: i64,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivityWindowStatus {
    Unscheduled,
    Disabled,
    NotStarted,
    Open,
    Ended,
}

#[derive(Debug)]
pub(crate) enum ActivityScheduleStoreError {
    Invalid,
    NotFound,
    Storage(PersonalServiceError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActivityScheduleEvaluation {
    pub(crate) status: ActivityWindowStatus,
    pub(crate) active_start_ms: Option<i64>,
    pub(crate) active_end_ms: Option<i64>,
    pub(crate) next_start_ms: Option<i64>,
    pub(crate) next_end_ms: Option<i64>,
}

// //// 创建或升级活动规则表 [@x380kkm 2026-08-19] ////
pub(super) fn migrate(connection: &Connection) -> Result<(), PersonalServiceError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS activity_schedules (
                 activity_id TEXT PRIMARY KEY
                     CHECK (length(trim(activity_id)) BETWEEN 1 AND 128),
                 enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
                 start_at_ms INTEGER NOT NULL CHECK (start_at_ms >= 0),
                 end_at_ms INTEGER NOT NULL CHECK (end_at_ms > start_at_ms),
                 mode TEXT NOT NULL DEFAULT 'window',
                 period TEXT NOT NULL DEFAULT 'once',
                 interval_days INTEGER,
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS activity_temporary_open_leases (
                 activity_id TEXT PRIMARY KEY
                     CHECK (length(trim(activity_id)) BETWEEN 1 AND 128),
                 opened_at_ms INTEGER NOT NULL CHECK (opened_at_ms >= 0),
                 expires_at_ms INTEGER NOT NULL CHECK (expires_at_ms > opened_at_ms)
             );
             CREATE INDEX IF NOT EXISTS activity_temporary_open_leases_expiry
                 ON activity_temporary_open_leases (expires_at_ms);",
        )
        .map_err(activity_calendar_database_error)?;
    add_column_if_missing(connection, "mode", "TEXT NOT NULL DEFAULT 'window'")?;
    add_column_if_missing(connection, "period", "TEXT NOT NULL DEFAULT 'once'")?;
    add_column_if_missing(connection, "interval_days", "INTEGER")?;
    connection
        .execute(
            "UPDATE activity_schedules
             SET mode = 'window', period = 'once', interval_days = NULL
             WHERE mode NOT IN ('manual', 'always', 'window', 'periodic')
                OR period NOT IN ('once', 'daily', 'weekly', 'monthly', 'interval_days')
                OR (period = 'interval_days'
                    AND (interval_days IS NULL OR interval_days < 1 OR interval_days > ?1))
                OR (mode = 'periodic' AND period = 'once')",
            params![MAX_INTERVAL_DAYS],
        )
        .map_err(activity_calendar_database_error)?;
    Ok(())
}
// //// /创建或升级活动规则表 ////

impl ServiceDatabase {
    // //// 列出活动时间窗口 [@x380kkm 2026-08-19] ////
    pub(crate) fn list_activity_schedules(
        &self,
    ) -> Result<Vec<ActivitySchedule>, PersonalServiceError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT activity_id, enabled, mode, period, interval_days,
                        start_at_ms, end_at_ms, created_at, updated_at
                 FROM activity_schedules
                 ORDER BY activity_id",
            )
            .map_err(activity_calendar_database_error)?;
        let schedules = statement
            .query_map([], read_activity_schedule)
            .map_err(activity_calendar_database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(activity_calendar_database_error)?;
        Ok(schedules)
    }
    // //// /列出活动时间窗口 ////

    // //// 读取单个活动时间窗口 [@x380kkm 2026-08-19] ////
    pub(crate) fn get_activity_schedule(
        &self,
        activity_id: &str,
    ) -> Result<Option<ActivitySchedule>, PersonalServiceError> {
        self.connection
            .query_row(
                "SELECT activity_id, enabled, mode, period, interval_days,
                        start_at_ms, end_at_ms, created_at, updated_at
                 FROM activity_schedules WHERE activity_id = ?1",
                params![activity_id],
                read_activity_schedule,
            )
            .optional()
            .map_err(activity_calendar_database_error)
    }
    // //// /读取单个活动时间窗口 ////

    // //// 写入活动时间窗口 [@x380kkm 2026-08-19] ////
    pub(crate) fn upsert_activity_schedule(
        &mut self,
        activity_id: &str,
        enabled: bool,
        start_at_ms: i64,
        end_at_ms: i64,
    ) -> Result<ActivitySchedule, ActivityScheduleStoreError> {
        self.upsert_activity_rule(
            activity_id,
            enabled,
            ActivityMode::Window,
            ActivityPeriod::Once,
            None,
            start_at_ms,
            end_at_ms,
        )
    }
    // //// /写入活动时间窗口 ////

    // //// 写入手动活动规则 [@x380kkm 2026-08-19] ////
    pub(crate) fn upsert_manual_activity(
        &mut self,
        activity_id: &str,
        enabled: bool,
    ) -> Result<ActivitySchedule, ActivityScheduleStoreError> {
        self.upsert_activity_rule(
            activity_id,
            enabled,
            ActivityMode::Manual,
            ActivityPeriod::Once,
            None,
            0,
            i64::MAX,
        )
    }
    // //// /写入手动活动规则 ////

    // //// 写入始终开启规则 [@x380kkm 2026-08-19] ////
    pub(crate) fn upsert_always_activity(
        &mut self,
        activity_id: &str,
    ) -> Result<ActivitySchedule, ActivityScheduleStoreError> {
        self.upsert_activity_rule(
            activity_id,
            true,
            ActivityMode::Always,
            ActivityPeriod::Once,
            None,
            0,
            i64::MAX,
        )
    }
    // //// /写入始终开启规则 ////

    // //// 写入活动规则 [@x380kkm 2026-08-19] ////
    pub(crate) fn upsert_activity_rule(
        &mut self,
        activity_id: &str,
        enabled: bool,
        mode: ActivityMode,
        period: ActivityPeriod,
        interval_days: Option<i64>,
        start_at_ms: i64,
        end_at_ms: i64,
    ) -> Result<ActivitySchedule, ActivityScheduleStoreError> {
        if !is_valid_activity_id(activity_id)
            || !valid_rule(mode, period, interval_days, start_at_ms, end_at_ms)
        {
            return Err(ActivityScheduleStoreError::Invalid);
        }
        self.connection
            .execute(
                "INSERT INTO activity_schedules (
                     activity_id, enabled, mode, period, interval_days,
                     start_at_ms, end_at_ms, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7,
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                 ON CONFLICT(activity_id) DO UPDATE SET
                     enabled = excluded.enabled,
                     mode = excluded.mode,
                     period = excluded.period,
                     interval_days = excluded.interval_days,
                     start_at_ms = excluded.start_at_ms,
                     end_at_ms = excluded.end_at_ms,
                     updated_at = excluded.updated_at",
                params![
                    activity_id,
                    enabled,
                    mode.as_str(),
                    period.as_str(),
                    interval_days,
                    start_at_ms,
                    end_at_ms
                ],
            )
            .map_err(|error| {
                ActivityScheduleStoreError::Storage(activity_calendar_database_error(error))
            })?;
        self.get_activity_schedule(activity_id)
            .map_err(ActivityScheduleStoreError::Storage)?
            .ok_or(ActivityScheduleStoreError::NotFound)
    }
    // //// /写入活动规则 ////

    // //// 删除活动时间窗口 [@x380kkm 2026-08-19] ////
    pub(crate) fn delete_activity_schedule(
        &mut self,
        activity_id: &str,
    ) -> Result<bool, PersonalServiceError> {
        let deleted = self
            .connection
            .execute(
                "DELETE FROM activity_schedules WHERE activity_id = ?1",
                params![activity_id],
            )
            .map_err(activity_calendar_database_error)?;
        Ok(deleted != 0)
    }
    // //// /删除活动时间窗口 ////

    // //// 清除活动覆盖规则 [@x380kkm 2026-08-24] ////
    pub(crate) fn reset_activity_overrides(
        &mut self,
    ) -> Result<(usize, usize), PersonalServiceError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(activity_calendar_database_error)?;
        let schedule_count = transaction
            .execute("DELETE FROM activity_schedules", params![])
            .map_err(activity_calendar_database_error)?;
        let lease_count = transaction
            .execute("DELETE FROM activity_temporary_open_leases", params![])
            .map_err(activity_calendar_database_error)?;
        transaction
            .commit()
            .map_err(activity_calendar_database_error)?;
        Ok((schedule_count, lease_count))
    }
    // //// /清除活动覆盖规则 ////

    // //// 创建真实墙钟 24 小时临时开放租约 [@x380kkm 2026-08-24] ////
    pub(crate) fn create_activity_temporary_open_lease(
        &mut self,
        activity_id: &str,
    ) -> Result<i64, ActivityScheduleStoreError> {
        if !is_valid_activity_id(activity_id) {
            return Err(ActivityScheduleStoreError::Invalid);
        }
        let opened_at_ms = self
            .current_wall_time_millis()
            .map_err(ActivityScheduleStoreError::Storage)?;
        let expires_at_ms = opened_at_ms
            .checked_add(TEMPORARY_OPEN_DURATION_MS)
            .ok_or(ActivityScheduleStoreError::Invalid)?;
        self.delete_expired_activity_temporary_open_leases(opened_at_ms)
            .map_err(ActivityScheduleStoreError::Storage)?;
        self.connection
            .execute(
                "INSERT INTO activity_temporary_open_leases (
                     activity_id, opened_at_ms, expires_at_ms
                 ) VALUES (?1, ?2, ?3)
                 ON CONFLICT(activity_id) DO UPDATE SET
                     opened_at_ms = excluded.opened_at_ms,
                     expires_at_ms = excluded.expires_at_ms",
                params![activity_id, opened_at_ms, expires_at_ms],
            )
            .map_err(|error| {
                ActivityScheduleStoreError::Storage(activity_calendar_database_error(error))
            })?;
        Ok(expires_at_ms)
    }
    // //// /创建真实墙钟 24 小时临时开放租约 ////

    // //// 读取有效临时开放租约 [@x380kkm 2026-08-24] ////
    pub(crate) fn activity_temporary_open_until(
        &self,
        activity_id: &str,
    ) -> Result<Option<i64>, PersonalServiceError> {
        Ok(self
            .activity_temporary_open_window(activity_id)?
            .map(|(_, expires_at_ms)| expires_at_ms))
    }

    pub(crate) fn activity_temporary_open_window(
        &self,
        activity_id: &str,
    ) -> Result<Option<(i64, i64)>, PersonalServiceError> {
        let wall_time_ms = self.current_wall_time_millis()?;
        self.connection
            .query_row(
                "SELECT opened_at_ms, expires_at_ms
                 FROM activity_temporary_open_leases
                 WHERE activity_id = ?1 AND expires_at_ms > ?2",
                params![activity_id, wall_time_ms],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(activity_calendar_database_error)
    }

    pub(crate) fn list_active_activity_temporary_open_leases(
        &self,
    ) -> Result<Vec<(String, i64)>, PersonalServiceError> {
        let wall_time_ms = self.current_wall_time_millis()?;
        self.delete_expired_activity_temporary_open_leases(wall_time_ms)?;
        let mut statement = self
            .connection
            .prepare(
                "SELECT activity_id, expires_at_ms
                 FROM activity_temporary_open_leases
                 WHERE expires_at_ms > ?1
                 ORDER BY activity_id",
            )
            .map_err(activity_calendar_database_error)?;
        let leases = statement
            .query_map(params![wall_time_ms], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(activity_calendar_database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(activity_calendar_database_error)?;
        Ok(leases)
    }
    // //// /读取有效临时开放租约 ////

    // //// 结束临时开放租约 [@x380kkm 2026-08-24] ////
    pub(crate) fn delete_activity_temporary_open_lease(
        &mut self,
        activity_id: &str,
    ) -> Result<bool, PersonalServiceError> {
        let deleted = self
            .connection
            .execute(
                "DELETE FROM activity_temporary_open_leases WHERE activity_id = ?1",
                params![activity_id],
            )
            .map_err(activity_calendar_database_error)?;
        Ok(deleted != 0)
    }

    fn delete_expired_activity_temporary_open_leases(
        &self,
        wall_time_ms: i64,
    ) -> Result<(), PersonalServiceError> {
        self.connection
            .execute(
                "DELETE FROM activity_temporary_open_leases WHERE expires_at_ms <= ?1",
                params![wall_time_ms],
            )
            .map_err(activity_calendar_database_error)?;
        Ok(())
    }
    // //// /结束临时开放租约 ////

    // //// 按虚拟时间判断活动窗口 [@x380kkm 2026-08-19] ////
    pub(crate) fn activity_window_status(
        &self,
        activity_id: &str,
        now_ms: i64,
    ) -> Result<ActivityWindowStatus, PersonalServiceError> {
        if self.activity_temporary_open_until(activity_id)?.is_some() {
            return Ok(ActivityWindowStatus::Open);
        }
        Ok(self
            .get_activity_schedule(activity_id)?
            .map(|schedule| evaluate_activity_schedule(&schedule, now_ms).status)
            .unwrap_or(ActivityWindowStatus::Unscheduled))
    }
    // //// /按虚拟时间判断活动窗口 ////
}

pub(crate) fn evaluate_activity_schedule(
    schedule: &ActivitySchedule,
    now_ms: i64,
) -> ActivityScheduleEvaluation {
    if !schedule.enabled {
        return ActivityScheduleEvaluation {
            status: ActivityWindowStatus::Disabled,
            active_start_ms: None,
            active_end_ms: None,
            next_start_ms: None,
            next_end_ms: None,
        };
    }
    match schedule.mode {
        ActivityMode::Manual | ActivityMode::Always => ActivityScheduleEvaluation {
            status: ActivityWindowStatus::Open,
            active_start_ms: None,
            active_end_ms: None,
            next_start_ms: None,
            next_end_ms: None,
        },
        ActivityMode::Window => {
            evaluate_single_window(schedule.start_at_ms, schedule.end_at_ms, now_ms)
        }
        ActivityMode::Periodic => evaluate_periodic_window(schedule, now_ms),
    }
}

pub(crate) fn activity_schedule_overlaps_range(
    schedule: &ActivitySchedule,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
) -> bool {
    if matches!(schedule.mode, ActivityMode::Manual | ActivityMode::Always) {
        return false;
    }
    let range_start = from_ms.unwrap_or(0);
    let range_end = to_ms.unwrap_or(i64::MAX);
    if range_end < range_start || !schedule.enabled || range_end < schedule.start_at_ms {
        return false;
    }
    if schedule.mode == ActivityMode::Window || schedule.period == ActivityPeriod::Once {
        return schedule.start_at_ms <= range_end && schedule.end_at_ms > range_start;
    }
    let duration_ms = schedule.end_at_ms - schedule.start_at_ms;
    let effective_start = range_start.max(schedule.start_at_ms);
    match schedule.period {
        ActivityPeriod::Daily => periodic_window_overlaps_range(
            schedule,
            effective_start,
            range_end,
            duration_ms,
            MILLIS_PER_DAY,
        ),
        ActivityPeriod::Weekly => periodic_window_overlaps_range(
            schedule,
            effective_start,
            range_end,
            duration_ms,
            7 * MILLIS_PER_DAY,
        ),
        ActivityPeriod::IntervalDays => periodic_window_overlaps_range(
            schedule,
            effective_start,
            range_end,
            duration_ms,
            schedule
                .interval_days
                .unwrap_or(1)
                .saturating_mul(MILLIS_PER_DAY),
        ),
        ActivityPeriod::Monthly => {
            monthly_window_overlaps_range(schedule, effective_start, range_end, duration_ms)
        }
        ActivityPeriod::Once => false,
    }
}

fn periodic_window_overlaps_range(
    schedule: &ActivitySchedule,
    range_start: i64,
    range_end: i64,
    duration_ms: i64,
    period_ms: i64,
) -> bool {
    if range_end.saturating_sub(range_start) >= period_ms {
        return true;
    }
    let first_index = range_start
        .saturating_sub(schedule.start_at_ms)
        .saturating_sub(duration_ms)
        .div_euclid(period_ms)
        .max(0);
    let last_index = range_end
        .saturating_sub(schedule.start_at_ms)
        .div_euclid(period_ms)
        .max(0);
    (first_index..=last_index).any(|index| {
        let window_start = periodic_start(schedule, index);
        let window_end = window_start.saturating_add(duration_ms);
        window_start <= range_end && window_end > range_start
    })
}

fn monthly_window_overlaps_range(
    schedule: &ActivitySchedule,
    range_start: i64,
    range_end: i64,
    duration_ms: i64,
) -> bool {
    let current_index = monthly_index(schedule.start_at_ms, range_start);
    let first_index = current_index.saturating_sub(1).max(0);
    let last_index = current_index.saturating_add(1);
    (first_index..=last_index).any(|index| {
        let window_start = periodic_start(schedule, index);
        let window_end = window_start.saturating_add(duration_ms);
        window_start <= range_end && window_end > range_start
    })
}

fn evaluate_single_window(
    start_at_ms: i64,
    end_at_ms: i64,
    now_ms: i64,
) -> ActivityScheduleEvaluation {
    if now_ms < start_at_ms {
        ActivityScheduleEvaluation {
            status: ActivityWindowStatus::NotStarted,
            active_start_ms: None,
            active_end_ms: None,
            next_start_ms: Some(start_at_ms),
            next_end_ms: Some(end_at_ms),
        }
    } else if now_ms >= end_at_ms {
        ActivityScheduleEvaluation {
            status: ActivityWindowStatus::Ended,
            active_start_ms: None,
            active_end_ms: None,
            next_start_ms: None,
            next_end_ms: None,
        }
    } else {
        ActivityScheduleEvaluation {
            status: ActivityWindowStatus::Open,
            active_start_ms: Some(start_at_ms),
            active_end_ms: Some(end_at_ms),
            next_start_ms: None,
            next_end_ms: None,
        }
    }
}

fn evaluate_periodic_window(
    schedule: &ActivitySchedule,
    now_ms: i64,
) -> ActivityScheduleEvaluation {
    let duration_ms = schedule.end_at_ms - schedule.start_at_ms;
    if now_ms < schedule.start_at_ms {
        return ActivityScheduleEvaluation {
            status: ActivityWindowStatus::NotStarted,
            active_start_ms: None,
            active_end_ms: None,
            next_start_ms: Some(schedule.start_at_ms),
            next_end_ms: Some(schedule.end_at_ms),
        };
    }
    let index = match schedule.period {
        ActivityPeriod::Daily => now_ms
            .saturating_sub(schedule.start_at_ms)
            .div_euclid(MILLIS_PER_DAY),
        ActivityPeriod::Weekly => now_ms
            .saturating_sub(schedule.start_at_ms)
            .div_euclid(7 * MILLIS_PER_DAY),
        ActivityPeriod::IntervalDays => now_ms
            .saturating_sub(schedule.start_at_ms)
            .div_euclid(schedule.interval_days.unwrap_or(1) * MILLIS_PER_DAY),
        ActivityPeriod::Monthly => monthly_index(schedule.start_at_ms, now_ms),
        ActivityPeriod::Once => 0,
    }
    .max(0);
    let candidate_start = periodic_start(schedule, index);
    let candidate_end = candidate_start.saturating_add(duration_ms);
    if candidate_start <= now_ms && now_ms < candidate_end {
        let next_start = periodic_start(schedule, index.saturating_add(1));
        return ActivityScheduleEvaluation {
            status: ActivityWindowStatus::Open,
            active_start_ms: Some(candidate_start),
            active_end_ms: Some(candidate_end),
            next_start_ms: Some(next_start),
            next_end_ms: Some(next_start.saturating_add(duration_ms)),
        };
    }
    if candidate_start > now_ms && index > 0 {
        let previous_start = periodic_start(schedule, index - 1);
        let previous_end = previous_start.saturating_add(duration_ms);
        if previous_start <= now_ms && now_ms < previous_end {
            return ActivityScheduleEvaluation {
                status: ActivityWindowStatus::Open,
                active_start_ms: Some(previous_start),
                active_end_ms: Some(previous_end),
                next_start_ms: Some(candidate_start),
                next_end_ms: Some(candidate_end),
            };
        }
    }
    let next_index = if candidate_start > now_ms {
        index
    } else {
        index.saturating_add(1)
    };
    let next_start = periodic_start(schedule, next_index);
    ActivityScheduleEvaluation {
        status: ActivityWindowStatus::Ended,
        active_start_ms: None,
        active_end_ms: None,
        next_start_ms: Some(next_start),
        next_end_ms: Some(next_start.saturating_add(duration_ms)),
    }
}

fn periodic_start(schedule: &ActivitySchedule, index: i64) -> i64 {
    match schedule.period {
        ActivityPeriod::Daily => schedule
            .start_at_ms
            .saturating_add(index.saturating_mul(MILLIS_PER_DAY)),
        ActivityPeriod::Weekly => schedule
            .start_at_ms
            .saturating_add(index.saturating_mul(7 * MILLIS_PER_DAY)),
        ActivityPeriod::IntervalDays => schedule.start_at_ms.saturating_add(
            index.saturating_mul(
                schedule
                    .interval_days
                    .unwrap_or(1)
                    .saturating_mul(MILLIS_PER_DAY),
            ),
        ),
        ActivityPeriod::Monthly => add_months(schedule.start_at_ms, index),
        ActivityPeriod::Once => schedule.start_at_ms,
    }
}

fn monthly_index(start_at_ms: i64, now_ms: i64) -> i64 {
    let (start_year, start_month, _, _) = civil_at(start_at_ms);
    let (now_year, now_month, _, _) = civil_at(now_ms);
    (now_year - start_year)
        .saturating_mul(12)
        .saturating_add(i64::from(now_month) - i64::from(start_month))
        .max(0)
}

fn add_months(timestamp_ms: i64, months: i64) -> i64 {
    let (year, month, day, day_time_ms) = civil_at(timestamp_ms);
    let ordinal = year.saturating_mul(12).saturating_add(i64::from(month) - 1);
    let target = ordinal.saturating_add(months);
    let target_year = target.div_euclid(12);
    let target_month = target.rem_euclid(12) as u32 + 1;
    let target_day = day.min(days_in_month(target_year, target_month));
    days_from_civil(target_year, target_month, target_day)
        .saturating_mul(MILLIS_PER_DAY)
        .saturating_add(day_time_ms)
}

fn civil_at(timestamp_ms: i64) -> (i64, u32, u32, i64) {
    let days = timestamp_ms.div_euclid(MILLIS_PER_DAY);
    let day_time_ms = timestamp_ms.rem_euclid(MILLIS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    (year, month, day, day_time_ms)
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        2 if is_leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn is_leap_year(year: i64) -> bool {
    year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0)
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let adjusted_year = year - i64::from(month <= 2);
    let era = (if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    })
    .div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let month_prime = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days.saturating_add(719_468);
    let era = (if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    })
    .div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524
        - day_of_era / 146_096)
        .div_euclid(365);
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2).div_euclid(153);
    let day = day_of_year - (153 * month_prime + 2).div_euclid(5) + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    (year + i64::from(month <= 2), month as u32, day as u32)
}

fn valid_rule(
    mode: ActivityMode,
    period: ActivityPeriod,
    interval_days: Option<i64>,
    start_at_ms: i64,
    end_at_ms: i64,
) -> bool {
    if start_at_ms < 0 || end_at_ms <= start_at_ms {
        return false;
    }
    match mode {
        ActivityMode::Manual | ActivityMode::Always | ActivityMode::Window => {
            period == ActivityPeriod::Once && interval_days.is_none()
        }
        ActivityMode::Periodic => {
            period.is_periodic()
                && match period {
                    ActivityPeriod::IntervalDays => {
                        interval_days.is_some_and(|days| (1..=MAX_INTERVAL_DAYS).contains(&days))
                    }
                    _ => interval_days.is_none(),
                }
        }
    }
}

fn add_column_if_missing(
    connection: &Connection,
    name: &str,
    definition: &str,
) -> Result<(), PersonalServiceError> {
    let exists = connection
        .prepare("SELECT 1 FROM pragma_table_info('activity_schedules') WHERE name = ?1")
        .and_then(|mut statement| statement.query_row(params![name], |_| Ok(())))
        .optional()
        .map_err(activity_calendar_database_error)?
        .is_some();
    if !exists {
        connection
            .execute(
                &format!("ALTER TABLE activity_schedules ADD COLUMN {name} {definition}"),
                [],
            )
            .map_err(activity_calendar_database_error)?;
    }
    Ok(())
}

fn is_valid_activity_id(activity_id: &str) -> bool {
    !activity_id.is_empty()
        && activity_id.len() <= 128
        && activity_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'.' | b'_' | b'-'))
}

fn read_activity_schedule(row: &rusqlite::Row<'_>) -> rusqlite::Result<ActivitySchedule> {
    let mode = row
        .get::<_, String>(2)
        .ok()
        .and_then(|value| ActivityMode::parse(&value))
        .unwrap_or(ActivityMode::Window);
    let period = row
        .get::<_, String>(3)
        .ok()
        .and_then(|value| ActivityPeriod::parse(&value))
        .unwrap_or(ActivityPeriod::Once);
    Ok(ActivitySchedule {
        activity_id: row.get(0)?,
        enabled: row.get(1)?,
        mode,
        period,
        interval_days: row.get(4)?,
        start_at_ms: row.get(5)?,
        end_at_ms: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn activity_calendar_database_error(error: rusqlite::Error) -> PersonalServiceError {
    PersonalServiceError::new(format!(
        "failed to access activity calendar storage: {error}"
    ))
}
