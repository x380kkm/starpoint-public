// audience: internal
// # personal-service-server-profile-identities
//
// 该模块持久化远端服务器配置使用的设备和 viewer 映射. 每个服务器配置保留多个设备身份,
// 并单独选择最近完成 signup 的身份. 本地账号和玩家快照不进入这些表.

use super::ServiceDatabase;
use crate::PersonalServiceError;
use rusqlite::{params, Connection, OptionalExtension};

pub(crate) struct ServerProfileIdentity {
    pub(crate) profile_id: i64,
    pub(crate) device_id: i64,
    pub(crate) viewer_id: i64,
}

// //// 创建服务器身份映射表 [@x380kkm 2026-07-23] ////
pub(super) fn migrate(connection: &Connection) -> Result<(), PersonalServiceError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS server_profile_identities (
                 profile_id INTEGER NOT NULL,
                 device_id INTEGER NOT NULL CHECK (device_id > 0),
                 viewer_id INTEGER NOT NULL CHECK (viewer_id > 0),
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL,
                 PRIMARY KEY (profile_id, device_id),
                 UNIQUE (profile_id, viewer_id),
                 FOREIGN KEY (profile_id) REFERENCES server_profiles (id) ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS active_server_profile_identity (
                 profile_id INTEGER PRIMARY KEY,
                 device_id INTEGER NOT NULL,
                 FOREIGN KEY (profile_id, device_id)
                     REFERENCES server_profile_identities (profile_id, device_id)
                     ON DELETE CASCADE
             );",
        )
        .map_err(identity_database_error)
}
// //// /创建服务器身份映射表 ////

impl ServiceDatabase {
    // //// 读取服务器配置当前使用的 viewer [@x380kkm 2026-07-23] ////
    pub(crate) fn active_server_profile_viewer_id(
        &self,
        profile_id: i64,
    ) -> Result<Option<i64>, PersonalServiceError> {
        self.connection
            .query_row(
                "SELECT identities.viewer_id
                 FROM active_server_profile_identity AS active
                 JOIN server_profile_identities AS identities
                   ON identities.profile_id = active.profile_id
                  AND identities.device_id = active.device_id
                 WHERE active.profile_id = ?1",
                params![profile_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(identity_database_error)
    }
    // //// /读取服务器配置当前使用的 viewer ////

    // //// 保存并选择服务器配置的设备身份 [@x380kkm 2026-07-23] ////
    pub(crate) fn save_and_activate_server_profile_identity(
        &mut self,
        identity: &ServerProfileIdentity,
    ) -> Result<(), PersonalServiceError> {
        if identity.device_id <= 0 || identity.viewer_id <= 0 {
            return Err(PersonalServiceError::new(
                "server profile identity values must be positive",
            ));
        }
        let transaction = self
            .connection
            .transaction()
            .map_err(identity_database_error)?;
        transaction
            .execute(
                "DELETE FROM server_profile_identities
                 WHERE profile_id = ?1 AND viewer_id = ?2 AND device_id != ?3",
                params![identity.profile_id, identity.viewer_id, identity.device_id],
            )
            .map_err(identity_database_error)?;
        transaction
            .execute(
                "INSERT INTO server_profile_identities (
                     profile_id, device_id, viewer_id, created_at, updated_at
                 )
                 SELECT ?1, ?2, ?3,
                        strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                        strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 FROM server_profiles
                 WHERE id = ?1 AND mode = 'remote'
                 ON CONFLICT(profile_id, device_id) DO UPDATE SET
                     viewer_id = excluded.viewer_id,
                     updated_at = excluded.updated_at",
                params![identity.profile_id, identity.device_id, identity.viewer_id],
            )
            .map_err(identity_database_error)?;
        if transaction.changes() != 1 {
            return Err(PersonalServiceError::new(
                "remote server profile is missing for identity update",
            ));
        }
        transaction
            .execute(
                "INSERT INTO active_server_profile_identity (profile_id, device_id)
                 VALUES (?1, ?2)
                 ON CONFLICT(profile_id) DO UPDATE SET device_id = excluded.device_id",
                params![identity.profile_id, identity.device_id],
            )
            .map_err(identity_database_error)?;
        transaction.commit().map_err(identity_database_error)
    }
    // //// /保存并选择服务器配置的设备身份 ////
}

fn identity_database_error(error: rusqlite::Error) -> PersonalServiceError {
    PersonalServiceError::new(format!(
        "failed to access server profile identity storage: {error}"
    ))
}
