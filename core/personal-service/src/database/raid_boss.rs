// audience: internal
// # personal-service-raid-boss-storage
// 原始 CN 领主设置使用既有活动记录, 区域活动按开始时间分别保存血量和讨伐数.
// 本地成功战斗累计一次全局讨伐, 与玩家战斗结算共用事务并保留管理设置的血量.

use super::{database_error, RaidBossState, ServiceDatabase};
use crate::PersonalServiceError;
use rusqlite::{params, Connection, OptionalExtension};

// //// 创建区域期别的领主设置表 [@x380kkm 2026-09-27] ////
pub(super) fn migrate(connection: &Connection) -> Result<(), PersonalServiceError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS raid_boss_period_states (
            event_id INTEGER NOT NULL,
            period_start_ms INTEGER NOT NULL,
            hp_percentage INTEGER NOT NULL,
            total_kill_count INTEGER NOT NULL,
            PRIMARY KEY (event_id, period_start_ms)
        );",
    ).map_err(database_error)
}

impl ServiceDatabase {
    // //// 读取当前活动期别的领主设置 [@x380kkm 2026-09-27] ////
    pub(crate) fn raid_boss_state(&self, event_id: i64) -> Result<RaidBossState, PersonalServiceError> {
        let period = self.raid_boss_period(event_id)?;
        self.raid_boss_state_for_period(event_id, period)
    }

    pub(crate) fn raid_boss_state_for_period(
        &self,
        event_id: i64,
        period_start_ms: Option<i64>,
    ) -> Result<RaidBossState, PersonalServiceError> {
        let state = match period_start_ms {
            Some(start) => self.connection.query_row(
                "SELECT hp_percentage,total_kill_count FROM raid_boss_period_states WHERE event_id=?1 AND period_start_ms=?2",
                params![event_id, start], read_raid_boss_state,
            ),
            None => self.connection.query_row(
                "SELECT hp_percentage,total_kill_count FROM raid_boss_states WHERE event_id=?1",
                params![event_id], read_raid_boss_state,
            ),
        }.optional().map_err(database_error)?;
        Ok(state.unwrap_or(RaidBossState { hp_percentage: 100, total_kill_count: 0 }))
    }

    // //// 累计战斗所属期别的本地讨伐次数 [@x380kkm 2026-09-28] ////
    pub(crate) fn advance_raid_boss_for_period(
        &mut self,
        event_id: i64,
        period_start_ms: Option<i64>,
    ) -> Result<RaidBossState, PersonalServiceError> {
        let state = match period_start_ms {
            Some(start) => self.connection.query_row(
                "INSERT INTO raid_boss_period_states (event_id,period_start_ms,hp_percentage,total_kill_count)
                 VALUES (?1,?2,100,1)
                 ON CONFLICT(event_id,period_start_ms) DO UPDATE
                 SET total_kill_count=raid_boss_period_states.total_kill_count+1
                 WHERE raid_boss_period_states.total_kill_count<?3
                 RETURNING hp_percentage,total_kill_count",
                params![event_id, start, i64::MAX], read_raid_boss_state,
            ),
            None => self.connection.query_row(
                "INSERT INTO raid_boss_states (event_id,hp_percentage,total_kill_count)
                 VALUES (?1,100,1)
                 ON CONFLICT(event_id) DO UPDATE
                 SET total_kill_count=raid_boss_states.total_kill_count+1
                 WHERE raid_boss_states.total_kill_count<?2
                 RETURNING hp_percentage,total_kill_count",
                params![event_id, i64::MAX], read_raid_boss_state,
            ),
        }.optional().map_err(database_error)?;
        state.ok_or_else(|| PersonalServiceError::new("CN raid total kill count exceeds the supported range"))
    }

    // //// 保存当前活动期别的领主设置 [@x380kkm 2026-09-27] ////
    pub(crate) fn set_raid_boss_state(
        &mut self,
        event_id: i64,
        hp_percentage: i64,
        total_kill_count: i64,
    ) -> Result<(), PersonalServiceError> {
        self.set_raid_boss_state_for_period(event_id, self.raid_boss_period(event_id)?, hp_percentage, total_kill_count)
    }

    // //// 将管理统计写入已选择的活动期别 [@x380kkm 2026-09-28] ////
    pub(crate) fn set_raid_boss_state_for_period(
        &mut self,
        event_id:i64,
        period_start_ms:Option<i64>,
        hp_percentage:i64,
        total_kill_count:i64,
    ) -> Result<(),PersonalServiceError> {
        match period_start_ms {
            Some(start) => self.connection.execute(
                "INSERT OR REPLACE INTO raid_boss_period_states (event_id,period_start_ms,hp_percentage,total_kill_count) VALUES (?1,?2,?3,?4)",
                params![event_id, start, hp_percentage, total_kill_count],
            ),
            None => self.connection.execute(
                "INSERT OR REPLACE INTO raid_boss_states (event_id,hp_percentage,total_kill_count) VALUES (?1,?2,?3)",
                params![event_id, hp_percentage, total_kill_count],
            ),
        }.map_err(database_error)?;
        Ok(())
    }

    fn raid_boss_period(&self, event_id: i64) -> Result<Option<i64>, PersonalServiceError> {
        Ok(crate::activity_projection::effective_regional_activity_period(
            self, &format!("raid:{event_id}"), self.current_server_time_millis()?,
        )?.map(|period| period.start_at_ms))
    }
}

// //// 读取持久化的领主血量与讨伐数 [@x380kkm 2026-09-28] ////
fn read_raid_boss_state(row: &rusqlite::Row<'_>) -> rusqlite::Result<RaidBossState> {
    Ok(RaidBossState {
        hp_percentage: row.get(0)?,
        total_kill_count: row.get(1)?,
    })
}
