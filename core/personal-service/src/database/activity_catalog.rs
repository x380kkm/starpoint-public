// audience: internal
// # personal-service-activity-catalog-storage
//
// 该模块保存当前个人服务实例的活动收藏. 活动元数据来自 CN 资产根中的可重建 manifest,
// SQLite 只保存用户选择, 不复制客户端资源.

use super::ServiceDatabase;
use crate::PersonalServiceError;
use rusqlite::{params, Connection};
use std::collections::HashSet;

// //// 创建活动收藏表 [@x380kkm 2026-08-19] ////
pub(super) fn migrate(connection: &Connection) -> Result<(), PersonalServiceError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS activity_favorites (
                 activity_id TEXT PRIMARY KEY
                     CHECK (length(trim(activity_id)) BETWEEN 1 AND 128),
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL
             );",
        )
        .map_err(activity_catalog_database_error)
}
// //// /创建活动收藏表 ////

impl ServiceDatabase {
    // //// 列出当前实例收藏的活动 [@x380kkm 2026-08-19] ////
    pub(crate) fn list_activity_favorites(&self) -> Result<HashSet<String>, PersonalServiceError> {
        let mut statement = self
            .connection
            .prepare("SELECT activity_id FROM activity_favorites ORDER BY activity_id")
            .map_err(activity_catalog_database_error)?;
        let favorites = statement
            .query_map([], |row| row.get(0))
            .map_err(activity_catalog_database_error)?
            .collect::<Result<HashSet<_>, _>>()
            .map_err(activity_catalog_database_error)?;
        Ok(favorites)
    }
    // //// /列出当前实例收藏的活动 ////

    // //// 设置当前实例的活动收藏状态 [@x380kkm 2026-08-19] ////
    pub(crate) fn set_activity_favorite(
        &mut self,
        activity_id: &str,
        favorite: bool,
    ) -> Result<bool, PersonalServiceError> {
        if !is_valid_activity_id(activity_id) {
            return Ok(false);
        }
        if favorite {
            self.connection
                .execute(
                    "INSERT INTO activity_favorites (activity_id, created_at, updated_at)
                     VALUES (?1,
                         strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                         strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                     ON CONFLICT(activity_id) DO UPDATE SET
                         updated_at = excluded.updated_at",
                    params![activity_id],
                )
                .map_err(activity_catalog_database_error)?;
        } else {
            self.connection
                .execute(
                    "DELETE FROM activity_favorites WHERE activity_id = ?1",
                    params![activity_id],
                )
                .map_err(activity_catalog_database_error)?;
        }
        Ok(true)
    }
    // //// /设置当前实例的活动收藏状态 ////
}

pub(crate) fn is_valid_activity_id(activity_id: &str) -> bool {
    !activity_id.is_empty()
        && activity_id.len() <= 128
        && activity_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'.' | b'_' | b'-'))
}

fn activity_catalog_database_error(error: rusqlite::Error) -> PersonalServiceError {
    PersonalServiceError::new(format!(
        "failed to access activity catalog storage: {error}"
    ))
}
