// audience: internal
// # personal-service-database-rescue-settings
// 救援配置独立于玩家存档保存, 重启和存档切换沿用同一组管理选择.

use super::{database_error, ServiceDatabase};
use crate::rescue_settings::RescuePolicy;
use crate::PersonalServiceError;
use rusqlite::{params, Connection, OptionalExtension};

pub(crate) fn migrate(connection: &Connection) -> Result<(), PersonalServiceError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS rescue_settings (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 settings_json TEXT NOT NULL
             );",
        )
        .map_err(database_error)
}

impl ServiceDatabase {
    // //// 读取实例救援配置 [@x380kkm 2026-09-23] ////
    pub(crate) fn rescue_settings(&self) -> Result<RescuePolicy, PersonalServiceError> {
        let serialized = self
            .connection
            .query_row(
                "SELECT settings_json FROM rescue_settings WHERE id = 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(database_error)?;
        serialized.map_or_else(
            || Ok(RescuePolicy::default()),
            |serialized| {
                serde_json::from_str(&serialized).map_err(|error| {
                    PersonalServiceError::new(format!(
                        "stored rescue settings are invalid: {error}"
                    ))
                })
            },
        )
    }
    // //// /读取实例救援配置 ////

    // //// 原子替换实例救援配置 [@x380kkm 2026-09-23] ////
    pub(crate) fn set_rescue_settings(
        &mut self,
        settings: &RescuePolicy,
    ) -> Result<(), PersonalServiceError> {
        let serialized = serde_json::to_string(settings).map_err(|error| {
            PersonalServiceError::new(format!("failed to encode rescue settings: {error}"))
        })?;
        self.connection
            .execute(
                "INSERT INTO rescue_settings (id, settings_json) VALUES (1, ?1)
             ON CONFLICT (id) DO UPDATE SET settings_json = excluded.settings_json",
                params![serialized],
            )
            .map_err(database_error)?;
        Ok(())
    }
    // //// /原子替换实例救援配置 ////
}
