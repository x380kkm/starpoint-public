// audience: internal
// # personal-service-database-gameplay-settings
//
// 该模块保存个人服务的战斗资源掉落倍率配置.

use super::database_error;
use crate::PersonalServiceError;
use rusqlite::{params, Connection};

const MIN_DROP_MULTIPLIER: i64 = 1;
const MAX_DROP_MULTIPLIER: i64 = 100;

pub(crate) fn migrate(connection: &Connection) -> Result<(), PersonalServiceError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS gameplay_settings (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 drop_multiplier INTEGER NOT NULL CHECK (drop_multiplier BETWEEN 1 AND 100)
             );
             INSERT OR IGNORE INTO gameplay_settings (id, drop_multiplier)
             VALUES (1, 1);",
        )
        .map_err(database_error)
}

pub(crate) fn get(connection: &Connection) -> Result<i64, PersonalServiceError> {
    connection
        .query_row(
            "SELECT drop_multiplier FROM gameplay_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(database_error)
}

pub(crate) fn set(
    connection: &Connection,
    drop_multiplier: i64,
) -> Result<i64, PersonalServiceError> {
    if !(MIN_DROP_MULTIPLIER..=MAX_DROP_MULTIPLIER).contains(&drop_multiplier) {
        return Err(PersonalServiceError::new(
            "drop multiplier must be between 1 and 100",
        ));
    }
    connection
        .execute(
            "UPDATE gameplay_settings SET drop_multiplier = ?1 WHERE id = 1",
            params![drop_multiplier],
        )
        .map_err(database_error)?;
    get(connection)
}
