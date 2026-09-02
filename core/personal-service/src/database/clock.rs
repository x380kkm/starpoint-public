// audience: internal
// # personal-service-virtual-time
//
// 该模块持久化个人服务的虚拟时间锚点. 未启用时使用设备 UTC 时间, 启用后按真实时间和倍率推进.
// CN 每日扭蛋标记按账号, 卡池和 UTC 日期保存在内部表中, 不写入玩家快照.

use super::receive_history::{insert_receive_history_in_transaction, ReceiveHistoryEntry};
use super::ServiceDatabase;
use crate::PersonalServiceError;
use rusqlite::{params, Connection};
use std::time::{SystemTime, UNIX_EPOCH};

const MILLISECONDS_PER_UTC_DAY: i64 = 86_400_000;

pub(crate) struct VirtualTimeState {
    pub(crate) enabled: bool,
    pub(crate) unix_time_ms: i64,
    pub(crate) iso: String,
    pub(crate) rate: f64,
}

// //// 创建虚拟时间锚点表 [@x380kkm 2026-07-24] ////
pub(super) fn migrate(connection: &Connection) -> Result<(), PersonalServiceError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS virtual_time (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
                 virtual_anchor_ms INTEGER NOT NULL CHECK (virtual_anchor_ms >= 0),
                 real_anchor_ms INTEGER NOT NULL CHECK (real_anchor_ms >= 0),
                 rate REAL NOT NULL CHECK (rate > 0.0 AND rate <= 1000.0)
             );
             INSERT OR IGNORE INTO virtual_time (
                  id, enabled, virtual_anchor_ms, real_anchor_ms, rate
             ) VALUES (1, 0, 0, 0, 1.0);
             CREATE TABLE IF NOT EXISTS cn_gacha_daily_draws (
                 account_id INTEGER NOT NULL,
                 gacha_id INTEGER NOT NULL CHECK (gacha_id > 0),
                 utc_day INTEGER NOT NULL CHECK (utc_day >= 0),
                 consumed_at_ms INTEGER NOT NULL CHECK (consumed_at_ms >= 0),
                 PRIMARY KEY (account_id, gacha_id, utc_day),
                 FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
             );",
        )
        .map_err(clock_database_error)
}
// //// /创建虚拟时间锚点表 ////

impl ServiceDatabase {
    // //// 首次启动时设置活动时间锚点 [@x380kkm 2026-08-20] ////
    pub(crate) fn initialize_virtual_time_if_pristine(
        &mut self,
        unix_time_ms: i64,
    ) -> Result<bool, PersonalServiceError> {
        if unix_time_ms <= 0 {
            return Err(PersonalServiceError::new(
                "initial virtual time must be positive",
            ));
        }
        let real_anchor_ms = system_time_millis()?;
        let changed = self
            .connection
            .execute(
                "UPDATE virtual_time
                 SET enabled = 1,
                     virtual_anchor_ms = ?1,
                     real_anchor_ms = ?2,
                     rate = 1.0
                 WHERE id = 1
                   AND enabled = 0
                   AND virtual_anchor_ms = 0
                   AND real_anchor_ms = 0
                   AND rate = 1.0",
                params![unix_time_ms, real_anchor_ms],
            )
            .map_err(clock_database_error)?;
        Ok(changed == 1)
    }
    // //// /首次启动时设置活动时间锚点 ////

    // //// 读取当前虚拟时间 [@x380kkm 2026-07-24] ////
    pub(crate) fn virtual_time_state(&self) -> Result<VirtualTimeState, PersonalServiceError> {
        let (enabled, virtual_anchor_ms, real_anchor_ms, rate) = self.read_virtual_time()?;
        let unix_time_ms = if enabled {
            project_time(
                virtual_anchor_ms,
                real_anchor_ms,
                rate,
                system_time_millis()?,
            )
        } else {
            system_time_millis()?
        };
        let iso = self.format_timestamp(unix_time_ms, "%Y-%m-%dT%H:%M:%fZ")?;
        Ok(VirtualTimeState {
            enabled,
            unix_time_ms,
            iso,
            rate,
        })
    }
    // //// /读取当前虚拟时间 ////

    // //// 返回 CN 服务使用的 Unix 毫秒时间 [@x380kkm 2026-07-24] ////
    pub(crate) fn current_server_time_millis(&self) -> Result<i64, PersonalServiceError> {
        let (enabled, virtual_anchor_ms, real_anchor_ms, rate) = self.read_virtual_time()?;
        let now = system_time_millis()?;
        if enabled {
            Ok(project_time(virtual_anchor_ms, real_anchor_ms, rate, now))
        } else {
            Ok(now)
        }
    }

    pub(crate) fn current_server_time_seconds(&self) -> Result<i64, PersonalServiceError> {
        Ok(self.current_server_time_millis()? / 1_000)
    }

    pub(crate) fn current_server_utc_day(&self) -> Result<i64, PersonalServiceError> {
        Ok(utc_day_from_millis(self.current_server_time_millis()?))
    }

    pub(crate) fn current_client_time(&self) -> Result<String, PersonalServiceError> {
        self.format_timestamp(self.current_server_time_millis()?, "%Y-%m-%d %H:%M:%S")
    }
    // //// /返回 CN 服务使用的 Unix 毫秒时间 ////

    // //// 返回真实墙钟毫秒时间 [@x380kkm 2026-08-23] ////
    pub(crate) fn current_wall_time_millis(&self) -> Result<i64, PersonalServiceError> {
        system_time_millis()
    }
    // //// /返回真实墙钟毫秒时间 ////

    // //// 保存虚拟时间锚点 [@x380kkm 2026-07-24] ////
    pub(crate) fn set_virtual_time(
        &mut self,
        enabled: bool,
        unix_time_ms: i64,
        rate: f64,
    ) -> Result<(), PersonalServiceError> {
        if unix_time_ms < 0 || !rate.is_finite() || !(0.0 < rate && rate <= 1000.0) {
            return Err(PersonalServiceError::new("virtual time values are invalid"));
        }
        let real_anchor_ms = system_time_millis()?;
        self.connection
            .execute(
                "UPDATE virtual_time
                 SET enabled = ?1,
                     virtual_anchor_ms = ?2,
                     real_anchor_ms = ?3,
                     rate = ?4
                 WHERE id = 1",
                params![enabled, unix_time_ms, real_anchor_ms, rate],
            )
            .map_err(clock_database_error)?;
        Ok(())
    }
    // //// /保存虚拟时间锚点 ////

    fn read_virtual_time(&self) -> Result<(bool, i64, i64, f64), PersonalServiceError> {
        self.connection
            .query_row(
                "SELECT enabled, virtual_anchor_ms, real_anchor_ms, rate
                 FROM virtual_time WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(clock_database_error)
    }

    // //// 读取 CN 每日扭蛋在当前 UTC 日期的可用状态 [@x380kkm 2026-08-18] ////
    pub(crate) fn is_cn_gacha_daily_available(
        &self,
        account_id: i64,
        gacha_id: i64,
    ) -> Result<bool, PersonalServiceError> {
        let utc_day = self.current_server_utc_day()?;
        self.connection
            .query_row(
                "SELECT NOT EXISTS(
                     SELECT 1 FROM cn_gacha_daily_draws
                     WHERE account_id = ?1 AND gacha_id = ?2 AND utc_day = ?3
                 )",
                params![account_id, gacha_id, utc_day],
                |row| row.get(0),
            )
            .map_err(daily_gacha_database_error)
    }
    // //// /读取 CN 每日扭蛋在当前 UTC 日期的可用状态 ////

    // //// 原子保存玩家快照并消费当前 UTC 日期的 CN 每日扭蛋 [@x380kkm 2026-08-18] ////
    pub(crate) fn save_player_snapshot_with_cn_daily_draw(
        &mut self,
        account_id: i64,
        gacha_id: i64,
        data: &str,
        history_event_key: &str,
        history_created_at: i64,
        history_entries: &[ReceiveHistoryEntry],
    ) -> Result<bool, PersonalServiceError> {
        let consumed_at_ms = self.current_server_time_millis()?;
        let utc_day = utc_day_from_millis(consumed_at_ms);
        let transaction = self
            .connection
            .transaction()
            .map_err(daily_gacha_database_error)?;
        let inserted = transaction
            .execute(
                "INSERT OR IGNORE INTO cn_gacha_daily_draws (
                     account_id, gacha_id, utc_day, consumed_at_ms
                 ) VALUES (?1, ?2, ?3, ?4)",
                params![account_id, gacha_id, utc_day, consumed_at_ms],
            )
            .map_err(daily_gacha_database_error)?;
        if inserted == 0 {
            return Ok(false);
        }
        transaction
            .execute(
                "INSERT INTO player_snapshots (account_id, data_json) VALUES (?1, ?2)
                 ON CONFLICT(account_id) DO UPDATE SET data_json = excluded.data_json",
                params![account_id, data],
            )
            .map_err(daily_gacha_database_error)?;
        insert_receive_history_in_transaction(
            &transaction,
            account_id,
            history_event_key,
            history_created_at,
            history_entries,
        )?;
        transaction.commit().map_err(daily_gacha_database_error)?;
        Ok(true)
    }
    // //// /原子保存玩家快照并消费当前 UTC 日期的 CN 每日扭蛋 ////

    fn format_timestamp(
        &self,
        unix_time_ms: i64,
        format: &str,
    ) -> Result<String, PersonalServiceError> {
        self.connection
            .query_row(
                "SELECT strftime(?1, ?2 / 1000.0, 'unixepoch')",
                params![format, unix_time_ms],
                |row| row.get(0),
            )
            .map_err(clock_database_error)
    }
}

fn system_time_millis() -> Result<i64, PersonalServiceError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| {
            PersonalServiceError::new(format!("system clock is before Unix epoch: {error}"))
        })?;
    i64::try_from(duration.as_millis())
        .map_err(|_| PersonalServiceError::new("system clock exceeds supported range"))
}

fn project_time(virtual_anchor_ms: i64, real_anchor_ms: i64, rate: f64, now_ms: i64) -> i64 {
    let elapsed_ms = now_ms.saturating_sub(real_anchor_ms) as f64;
    let projected = virtual_anchor_ms as f64 + elapsed_ms * rate;
    if !projected.is_finite() || projected >= i64::MAX as f64 {
        i64::MAX
    } else if projected <= 0.0 {
        0
    } else {
        projected.round() as i64
    }
}

// //// 解析管理接口使用的 UTC 时间 [@x380kkm 2026-07-24] ////
pub(crate) fn parse_iso_timestamp(value: &str) -> Option<i64> {
    let value = value.strip_suffix('Z')?;
    let (date, time) = value.split_once('T')?;
    let mut date_parts = date.split('-');
    let year = parse_fixed_i32(date_parts.next()?, 4)?;
    let month = parse_fixed_i32(date_parts.next()?, 2)?;
    let day = parse_fixed_i32(date_parts.next()?, 2)?;
    if date_parts.next().is_some() || !(1..=12).contains(&month) {
        return None;
    }

    let (clock, fraction) = time.split_once('.').map_or((time, ""), |parts| parts);
    let mut clock_parts = clock.split(':');
    let hour = parse_fixed_i32(clock_parts.next()?, 2)?;
    let minute = parse_fixed_i32(clock_parts.next()?, 2)?;
    let second = parse_fixed_i32(clock_parts.next()?, 2)?;
    if clock_parts.next().is_some() || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let fraction_ms = parse_fraction_millis(fraction)?;
    let days = days_from_civil(year, month, day)?;
    days.checked_mul(86_400_000)?
        .checked_add(i64::from(hour) * 3_600_000)
        .and_then(|value| value.checked_add(i64::from(minute) * 60_000))
        .and_then(|value| value.checked_add(i64::from(second) * 1_000))
        .and_then(|value| value.checked_add(fraction_ms))
}

fn parse_fixed_i32(value: &str, length: usize) -> Option<i32> {
    (value.len() == length && value.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| value.parse().ok())?
}

fn parse_fraction_millis(value: &str) -> Option<i64> {
    if value.is_empty() || value.len() > 9 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let mut milliseconds = value.parse::<i64>().ok()?;
    for _ in value.len()..3 {
        milliseconds *= 10;
    }
    if value.len() > 3 {
        milliseconds /= 10_i64.pow((value.len() - 3) as u32);
    }
    Some(milliseconds)
}

fn days_from_civil(year: i32, month: i32, day: i32) -> Option<i64> {
    let max_day = match month {
        2 => {
            if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day < 1 || day > max_day {
        return None;
    }
    let year = i64::from(year) - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_offset = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_offset + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146_097 + day_of_era - 719_468)
}

fn clock_database_error(error: rusqlite::Error) -> PersonalServiceError {
    PersonalServiceError::new(format!("failed to access virtual time storage: {error}"))
}

fn daily_gacha_database_error(error: rusqlite::Error) -> PersonalServiceError {
    PersonalServiceError::new(format!("failed to access CN daily gacha storage: {error}"))
}

// //// 将 Unix 毫秒转换为 UTC 日序号 [@x380kkm 2026-08-18] ////
fn utc_day_from_millis(unix_time_ms: i64) -> i64 {
    unix_time_ms.div_euclid(MILLISECONDS_PER_UTC_DAY)
}
// //// /将 Unix 毫秒转换为 UTC 日序号 ////

#[cfg(test)]
mod tests {
    use super::{parse_iso_timestamp, utc_day_from_millis, MILLISECONDS_PER_UTC_DAY};

    #[test]
    fn parses_rfc3339_utc_milliseconds() {
        assert_eq!(
            parse_iso_timestamp("2030-01-01T00:00:00.000Z"),
            Some(1_893_456_000_000)
        );
        assert_eq!(
            parse_iso_timestamp("1970-01-01T00:00:00.123456Z"),
            Some(123)
        );
    }

    #[test]
    fn rejects_invalid_utc_timestamps() {
        for value in [
            "2030-02-30T00:00:00Z",
            "2030-01-01T24:00:00Z",
            "2030-01-01T00:00:00+08:00",
            "not-a-time",
        ] {
            assert_eq!(parse_iso_timestamp(value), None, "{value}");
        }
    }

    // //// 验证 UTC 日序号在日期边界稳定 [@x380kkm 2026-08-18] ////
    #[test]
    fn uses_utc_calendar_day_boundaries() {
        assert_eq!(utc_day_from_millis(0), 0);
        assert_eq!(utc_day_from_millis(MILLISECONDS_PER_UTC_DAY - 1), 0);
        assert_eq!(utc_day_from_millis(MILLISECONDS_PER_UTC_DAY), 1);
        assert_eq!(utc_day_from_millis(1_893_456_000_000), 21_915);
    }
    // //// /验证 UTC 日序号在日期边界稳定 ////
}
