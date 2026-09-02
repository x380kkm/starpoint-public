// audience: internal
// # personal-service-server-profile-storage
//
// 该模块持久化设备级服务器配置. 内置本地配置始终存在, 当前配置只能指向现有记录.
// 修改远端连接地址时删除该配置的 viewer 映射.

use super::ServiceDatabase;
use crate::PersonalServiceError;
use rusqlite::{params, Connection, OptionalExtension, Row};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ServerProfileMode {
    Local,
    Remote,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct ServerProfile {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) mode: ServerProfileMode,
    pub(crate) scheme: Option<String>,
    pub(crate) host: Option<String>,
    pub(crate) port: Option<i64>,
    pub(crate) is_builtin: bool,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

pub(crate) struct ServerProfileState {
    pub(crate) active_profile_id: i64,
    pub(crate) profiles: Vec<ServerProfile>,
}

pub(crate) struct RemoteServerProfileInput {
    pub(crate) name: String,
    pub(crate) scheme: String,
    pub(crate) host: String,
    pub(crate) port: u16,
}

pub(crate) enum ServerProfileStoreError {
    NotFound,
    NameConflict,
    ActiveProfile,
    BuiltinProfile,
    Storage(PersonalServiceError),
}

// //// 创建服务器配置表和内置本地配置 [@x380kkm 2026-07-23] ////
pub(super) fn migrate(connection: &Connection) -> Result<(), PersonalServiceError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS server_profiles (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 name TEXT NOT NULL COLLATE NOCASE UNIQUE
                     CHECK (length(trim(name)) BETWEEN 1 AND 64),
                 mode TEXT NOT NULL CHECK (mode IN ('local', 'remote')),
                 scheme TEXT CHECK (scheme IN ('http', 'https')),
                 host TEXT CHECK (host IS NULL OR length(host) BETWEEN 1 AND 253),
                 port INTEGER CHECK (port BETWEEN 1 AND 65535),
                 is_builtin INTEGER NOT NULL CHECK (is_builtin IN (0, 1)),
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL,
                 CHECK (
                     (mode = 'local' AND scheme IS NULL AND host IS NULL AND port IS NULL)
                     OR
                     (mode = 'remote' AND scheme IS NOT NULL AND host IS NOT NULL AND port IS NOT NULL)
                 ),
                 CHECK (is_builtin = 0 OR mode = 'local')
             );
             CREATE UNIQUE INDEX IF NOT EXISTS server_profiles_builtin_local
                 ON server_profiles (mode)
                 WHERE is_builtin = 1;
             INSERT OR IGNORE INTO server_profiles (
                 id, name, mode, scheme, host, port, is_builtin, created_at, updated_at
             ) VALUES (
                 1,
                 'This device',
                 'local',
                 NULL,
                 NULL,
                 NULL,
                 1,
                 strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                 strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             );
             CREATE TABLE IF NOT EXISTS active_server_profile (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 profile_id INTEGER NOT NULL,
                 FOREIGN KEY (profile_id) REFERENCES server_profiles (id) ON DELETE RESTRICT
             );
             INSERT OR IGNORE INTO active_server_profile (id, profile_id)
                 SELECT 1, id FROM server_profiles WHERE is_builtin = 1;",
        )
        .map_err(profile_database_error)
}
// //// /创建服务器配置表和内置本地配置 ////

impl ServiceDatabase {
    // //// 读取当前服务器配置 [@x380kkm 2026-07-23] ////
    pub(crate) fn active_server_profile(&self) -> Result<ServerProfile, PersonalServiceError> {
        self.connection
            .query_row(
                "SELECT server_profiles.id,
                        server_profiles.name,
                        server_profiles.mode = 'local',
                        server_profiles.scheme,
                        server_profiles.host,
                        server_profiles.port,
                        server_profiles.is_builtin,
                        server_profiles.created_at,
                        server_profiles.updated_at
                 FROM active_server_profile
                 JOIN server_profiles ON server_profiles.id = active_server_profile.profile_id
                 WHERE active_server_profile.id = 1",
                [],
                read_server_profile,
            )
            .map_err(profile_database_error)
    }
    // //// /读取当前服务器配置 ////

    // //// 列出服务器配置和当前选择 [@x380kkm 2026-07-23] ////
    pub(crate) fn list_server_profiles(&self) -> Result<ServerProfileState, PersonalServiceError> {
        let active_profile_id = self
            .connection
            .query_row(
                "SELECT profile_id FROM active_server_profile WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .map_err(profile_database_error)?;
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, name, mode = 'local', scheme, host, port, is_builtin,
                        created_at, updated_at
                 FROM server_profiles
                 ORDER BY is_builtin DESC, name COLLATE NOCASE, id",
            )
            .map_err(profile_database_error)?;
        let profiles = statement
            .query_map([], read_server_profile)
            .map_err(profile_database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(profile_database_error)?;
        Ok(ServerProfileState {
            active_profile_id,
            profiles,
        })
    }

    pub(crate) fn get_server_profile(
        &self,
        profile_id: i64,
    ) -> Result<Option<ServerProfile>, PersonalServiceError> {
        find_server_profile(&self.connection, profile_id).map_err(|error| match error {
            ServerProfileStoreError::Storage(error) => error,
            _ => PersonalServiceError::new("failed to read server profile"),
        })
    }
    // //// /列出服务器配置和当前选择 ////

    // //// 创建远端服务器配置 [@x380kkm 2026-07-23] ////
    pub(crate) fn create_server_profile(
        &mut self,
        input: &RemoteServerProfileInput,
    ) -> Result<ServerProfile, ServerProfileStoreError> {
        if profile_name_exists(&self.connection, &input.name, None)? {
            return Err(ServerProfileStoreError::NameConflict);
        }
        self.connection
            .execute(
                "INSERT INTO server_profiles (
                     name, mode, scheme, host, port, is_builtin, created_at, updated_at
                 ) VALUES (
                     ?1, 'remote', ?2, ?3, ?4, 0,
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 )",
                params![input.name, input.scheme, input.host, i64::from(input.port)],
            )
            .map_err(profile_storage_error)?;
        find_server_profile(&self.connection, self.connection.last_insert_rowid())?
            .ok_or_else(|| profile_storage_message("created server profile is missing"))
    }
    // //// /创建远端服务器配置 ////

    // //// 更新远端服务器配置 [@x380kkm 2026-07-23] ////
    pub(crate) fn update_server_profile(
        &mut self,
        profile_id: i64,
        input: &RemoteServerProfileInput,
    ) -> Result<ServerProfile, ServerProfileStoreError> {
        let profile = find_server_profile(&self.connection, profile_id)?
            .ok_or(ServerProfileStoreError::NotFound)?;
        if profile.is_builtin {
            return Err(ServerProfileStoreError::BuiltinProfile);
        }
        if profile_name_exists(&self.connection, &input.name, Some(profile_id))? {
            return Err(ServerProfileStoreError::NameConflict);
        }
        let endpoint_changed = profile.scheme.as_deref() != Some(input.scheme.as_str())
            || profile.host.as_deref() != Some(input.host.as_str())
            || profile.port != Some(i64::from(input.port));
        let transaction = self
            .connection
            .transaction()
            .map_err(profile_storage_error)?;
        transaction
            .execute(
                "UPDATE server_profiles
                 SET name = ?1,
                     scheme = ?2,
                     host = ?3,
                     port = ?4,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?5",
                params![
                    input.name,
                    input.scheme,
                    input.host,
                    i64::from(input.port),
                    profile_id
                ],
            )
            .map_err(profile_storage_error)?;
        if endpoint_changed {
            transaction
                .execute(
                    "DELETE FROM server_profile_identities WHERE profile_id = ?1",
                    params![profile_id],
                )
                .map_err(profile_storage_error)?;
        }
        transaction.commit().map_err(profile_storage_error)?;
        find_server_profile(&self.connection, profile_id)?
            .ok_or_else(|| profile_storage_message("updated server profile is missing"))
    }
    // //// /更新远端服务器配置 ////

    // //// 删除未使用的远端服务器配置 [@x380kkm 2026-07-23] ////
    pub(crate) fn delete_server_profile(
        &mut self,
        profile_id: i64,
    ) -> Result<(), ServerProfileStoreError> {
        let flags = profile_flags(&self.connection, profile_id)?
            .ok_or(ServerProfileStoreError::NotFound)?;
        if flags.is_builtin {
            return Err(ServerProfileStoreError::BuiltinProfile);
        }
        if flags.is_active {
            return Err(ServerProfileStoreError::ActiveProfile);
        }
        self.connection
            .execute(
                "DELETE FROM server_profiles WHERE id = ?1",
                params![profile_id],
            )
            .map_err(profile_storage_error)?;
        Ok(())
    }
    // //// /删除未使用的远端服务器配置 ////

    // //// 切换当前服务器配置 [@x380kkm 2026-07-23] ////
    pub(crate) fn activate_server_profile(
        &mut self,
        profile_id: i64,
    ) -> Result<(), ServerProfileStoreError> {
        if find_server_profile(&self.connection, profile_id)?.is_none() {
            return Err(ServerProfileStoreError::NotFound);
        }
        self.connection
            .execute(
                "UPDATE active_server_profile SET profile_id = ?1 WHERE id = 1",
                params![profile_id],
            )
            .map_err(profile_storage_error)?;
        Ok(())
    }
    // //// /切换当前服务器配置 ////
}

// //// 查询服务器配置记录和保护状态 [@x380kkm 2026-07-23] ////
struct ProfileFlags {
    is_builtin: bool,
    is_active: bool,
}

fn read_server_profile(row: &Row<'_>) -> rusqlite::Result<ServerProfile> {
    Ok(ServerProfile {
        id: row.get(0)?,
        name: row.get(1)?,
        mode: if row.get(2)? {
            ServerProfileMode::Local
        } else {
            ServerProfileMode::Remote
        },
        scheme: row.get(3)?,
        host: row.get(4)?,
        port: row.get(5)?,
        is_builtin: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn find_server_profile(
    connection: &Connection,
    profile_id: i64,
) -> Result<Option<ServerProfile>, ServerProfileStoreError> {
    connection
        .query_row(
            "SELECT id, name, mode = 'local', scheme, host, port, is_builtin,
                    created_at, updated_at
             FROM server_profiles
             WHERE id = ?1",
            params![profile_id],
            read_server_profile,
        )
        .optional()
        .map_err(profile_storage_error)
}

fn profile_name_exists(
    connection: &Connection,
    name: &str,
    excluded_profile_id: Option<i64>,
) -> Result<bool, ServerProfileStoreError> {
    match excluded_profile_id {
        Some(profile_id) => connection
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM server_profiles WHERE name = ?1 AND id != ?2
                 )",
                params![name, profile_id],
                |row| row.get(0),
            )
            .map_err(profile_storage_error),
        None => connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM server_profiles WHERE name = ?1)",
                params![name],
                |row| row.get(0),
            )
            .map_err(profile_storage_error),
    }
}

fn profile_flags(
    connection: &Connection,
    profile_id: i64,
) -> Result<Option<ProfileFlags>, ServerProfileStoreError> {
    connection
        .query_row(
            "SELECT is_builtin,
                    EXISTS(
                        SELECT 1 FROM active_server_profile WHERE profile_id = server_profiles.id
                    )
             FROM server_profiles
             WHERE id = ?1",
            params![profile_id],
            |row| {
                Ok(ProfileFlags {
                    is_builtin: row.get(0)?,
                    is_active: row.get(1)?,
                })
            },
        )
        .optional()
        .map_err(profile_storage_error)
}
// //// /查询服务器配置记录和保护状态 ////

// //// 转换服务器配置存储错误 [@x380kkm 2026-07-23] ////
fn profile_storage_message(message: &str) -> ServerProfileStoreError {
    ServerProfileStoreError::Storage(PersonalServiceError::new(message))
}

fn profile_storage_error(error: rusqlite::Error) -> ServerProfileStoreError {
    ServerProfileStoreError::Storage(profile_database_error(error))
}

fn profile_database_error(error: rusqlite::Error) -> PersonalServiceError {
    PersonalServiceError::new(format!("failed to update server profile storage: {error}"))
}
// //// /转换服务器配置存储错误 ////
