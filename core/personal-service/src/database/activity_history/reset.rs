// audience: internal
// # personal-service-activity-reset-storage
// 重置在同一事务中校验活动战斗, 归档已有领取凭据, 清除指定任务进度和提交新轮次.
// 新轮次高于该存档的历史轮次, 存档恢复保留历史中的奖励投递身份.

use super::{decode_root, history_error, insert_event};
use crate::activity_history::{collect_snapshot_events, participation_round, reset_activity, ActivityHistoryEntry};
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use rusqlite::params;
use serde_json::{Map, Value};

impl ServiceDatabase {
    // //// 原子重置选定活动的存档状态 [@x380kkm 2026-09-21] ////
    pub(crate) fn reset_save_activity(
        &mut self, account_id: i64, activity_id: &str, expected_round: i64,
    ) -> Result<Result<Value, &'static str>, PersonalServiceError> {
        let captured_at = self.current_server_time_seconds()?;
        let period = crate::activity_projection::effective_regional_activity_period(self, activity_id, captured_at.saturating_mul(1_000))?
            .map(|period| period.start_at_ms);
        let fixture = crate::cn_battle_assets::load_battle_fixture()?;
        let transaction = self.connection.transaction().map_err(history_error)?;
        let data: String = transaction.query_row("SELECT data_json FROM player_snapshots WHERE account_id=?1",
            [account_id], |row| row.get(0)).map_err(history_error)?;
        let mut root = decode_root(&data)?;
        if let Some((family, id)) = activity_id.split_once(':') {
            if let Ok(id) = id.parse::<i64>() {
                crate::cn_activity::select_period(&mut root, family, id, period)?;
            }
        }
        if participation_round(&root, activity_id) != expected_round { return Ok(Err("activity_round_changed")); }
        let mut statement = transaction.prepare("SELECT category,quest_id FROM active_single_quests WHERE account_id=?1").map_err(history_error)?;
        let active = statement.query_map([account_id], |row| Ok((row.get::<_,i64>(0)?, row.get::<_,i64>(1)?)))
            .map_err(history_error)?.collect::<Result<Vec<_>,_>>().map_err(history_error)?;
        drop(statement);
        if active.iter().any(|(category,id)| fixture.quests.get(&format!("{category}:{id}"))
            .is_some_and(|quest| quest.activity_id.as_deref() == Some(activity_id))) {
            return Ok(Err("activity_battle_active"));
        }
        for event in collect_snapshot_events(&Map::new(), &root, captured_at)? {
            if event.activity_id == activity_id { insert_event(&transaction, account_id, &event)?; }
        }
        let latest_round: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(participation_round),0) FROM player_activity_history WHERE account_id=?1 AND activity_id=?2",
            params![account_id, activity_id], |row| row.get(0),
        ).map_err(history_error)?;
        let next_round = latest_round.max(expected_round).checked_add(1)
            .ok_or_else(|| PersonalServiceError::new("activity participation round overflow"))?;
        let reset = reset_activity(&mut root, activity_id, next_round, captured_at, period)?;
        for (category, mission_id) in reset.mission_ids {
            transaction.execute("DELETE FROM player_mission_progress WHERE account_id=?1 AND category=?2 AND mission_id=?3",
                params![account_id,category,mission_id]).map_err(history_error)?;
        }
        insert_event(&transaction, account_id, &ActivityHistoryEntry {
            activity_id: activity_id.to_owned(), participation_round: next_round,
            event_key: "reset".to_owned(), kind: "reset", details: reset.details.clone(),
            created_at: captured_at, mail_id: None,
        })?;
        transaction.execute("UPDATE player_snapshots SET data_json=?1 WHERE account_id=?2",
            params![Value::Object(root).to_string(),account_id]).map_err(history_error)?;
        transaction.execute("UPDATE local_save_slots SET updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE account_id=?1",
            [account_id]).map_err(history_error)?;
        transaction.commit().map_err(history_error)?;
        Ok(Ok(reset.details))
    }
}
