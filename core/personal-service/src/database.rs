// audience: internal
// # personal-service-database
//
// 该模块封装 rusqlite. 其他模块只使用个人服务数据接口.
// 所有连接使用 WAL 和 FULL synchronous. checkpoint 只截断已经提交的 WAL.

use crate::PersonalServiceError;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

const HTTP_OBSERVATION_LIMIT: i64 = 256;

mod activity_calendar;
mod activity_catalog;
mod ai_teams;
mod clock;
mod gameplay_settings;
mod local_save_keys;
mod local_saves;
mod mails;
mod mission_counters;
mod multiplayer;
mod receive_history;
mod save_automation;
mod save_sync;
mod server_profile_identities;
mod server_profiles;
mod transfer_bindings;

pub(crate) use activity_calendar::{
    activity_schedule_overlaps_range, evaluate_activity_schedule, ActivityMode, ActivityPeriod,
    ActivitySchedule, ActivityScheduleStoreError, ActivityWindowStatus,
};
pub(crate) use activity_catalog::is_valid_activity_id;
pub(crate) use ai_teams::{AiTeamSnapshot, AiTeamSnapshotInput, AiTeamStoreError};
pub(crate) use clock::{parse_iso_timestamp, VirtualTimeState};
pub(crate) use mails::{
    CreateMailInput, MailClaimReward, MailReward, StoredMail, MAX_MAIL_PAGE, MAX_MAIL_PAGE_SIZE,
    MAX_MAIL_REWARD_ENTRIES,
};
pub(crate) use multiplayer::{
    MultiplayerAiMate, MultiplayerAiMateInput, MultiplayerBattleAbort, MultiplayerBattleContinue,
    MultiplayerBattleFinish, MultiplayerBattleIdentity, MultiplayerBattleReceipt,
    MultiplayerBattleStart, MultiplayerMember, MultiplayerRoom, MultiplayerRoomEvent,
    MultiplayerRoomEventKind,
};
pub(crate) use receive_history::{ReceiveHistoryEntry, StoredReceiveHistory};
pub(crate) use server_profile_identities::ServerProfileIdentity;
pub(crate) use server_profiles::{
    RemoteServerProfileInput, ServerProfile, ServerProfileMode, ServerProfileState,
    ServerProfileStoreError,
};

pub(crate) struct SignupAccount {
    pub(crate) account_id: i64,
    pub(crate) created_at: String,
    pub(crate) is_new: bool,
    pub(crate) viewer_id: i64,
}

pub(crate) enum SignupDeviceError {
    BindingConflict,
    Storage(PersonalServiceError),
}

pub(crate) struct PlayerSnapshot {
    pub(crate) account_id: i64,
    pub(crate) data: String,
}

pub(crate) struct ActiveSingleQuest {
    pub(crate) play_id: String,
    pub(crate) quest_id: i64,
    pub(crate) category: i64,
    pub(crate) use_boss_boost_point: bool,
    pub(crate) use_boost_point: bool,
    pub(crate) is_auto_start_mode: bool,
}

pub(crate) struct UnfinishedQuest {
    pub(crate) play_id: String,
    pub(crate) continue_count: i64,
    pub(crate) is_multi: bool,
}

pub(crate) struct RaidBossState {
    pub(crate) hp_percentage: i64,
    pub(crate) total_kill_count: i64,
}

pub(crate) enum ViewerSessionPlayer {
    InvalidSession,
    MissingPlayer,
    MissingPlayerData(i64),
    Present(PlayerSnapshot),
}

#[derive(Serialize)]
pub(crate) struct HttpObservation {
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) status: i64,
    pub(crate) count: i64,
    pub(crate) first_seen: String,
    pub(crate) last_seen: String,
}

pub(crate) struct ServiceDatabase {
    connection: Connection,
    management_token: String,
    multiplayer_session_port: u16,
}

impl ServiceDatabase {
    // //// 创建并迁移本地 SQLite 数据库 [@x380kkm 2026-07-22] ////
    pub(crate) fn open(root_path: &Path) -> Result<Self, PersonalServiceError> {
        fs::create_dir_all(root_path).map_err(|error| {
            PersonalServiceError::new(format!("failed to create service directory: {error}"))
        })?;
        let database_path = root_path.join("personal-service.sqlite3");
        let mut connection = Connection::open(database_path).map_err(|error| {
            PersonalServiceError::new(format!("failed to open database: {error}"))
        })?;
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = FULL;
                 PRAGMA foreign_keys = ON;
                 CREATE TABLE IF NOT EXISTS service_state (
                     id INTEGER PRIMARY KEY CHECK (id = 1),
                     generation INTEGER NOT NULL
                 );
                 INSERT OR IGNORE INTO service_state (id, generation) VALUES (1, 0);
                 CREATE TABLE IF NOT EXISTS http_observations (
                     method TEXT NOT NULL,
                     path TEXT NOT NULL,
                     status INTEGER NOT NULL,
                     count INTEGER NOT NULL CHECK (count > 0),
                     first_seen TEXT NOT NULL,
                     last_seen TEXT NOT NULL,
                     PRIMARY KEY (method, path, status)
                 );
                 CREATE INDEX IF NOT EXISTS http_observations_last_seen
                     ON http_observations (last_seen DESC);
                 DELETE FROM http_observations
                     WHERE path = '/manage'
                        OR path LIKE '/manage/%'
                        OR path = '/v1'
                        OR path LIKE '/v1/%';
                 CREATE TABLE IF NOT EXISTS raid_boss_states (
                     event_id INTEGER PRIMARY KEY,
                     hp_percentage INTEGER NOT NULL CHECK (hp_percentage BETWEEN 0 AND 100),
                     total_kill_count INTEGER NOT NULL CHECK (total_kill_count >= 0)
                 );
                  CREATE TABLE IF NOT EXISTS management_state (
                      id INTEGER PRIMARY KEY CHECK (id = 1),
                      token_hash TEXT NOT NULL CHECK (length(token_hash) = 64)
                  );
                  CREATE TABLE IF NOT EXISTS player_access_tokens (
                      token_hash TEXT PRIMARY KEY NOT NULL CHECK (length(token_hash) = 64),
                      account_id INTEGER NOT NULL UNIQUE,
                     created_at TEXT NOT NULL,
                     FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
                 );
                 CREATE TABLE IF NOT EXISTS accounts (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     app_id TEXT NOT NULL,
                     first_login_time TEXT NOT NULL,
                     idp_alias TEXT NOT NULL,
                     idp_code TEXT NOT NULL,
                     idp_id TEXT NOT NULL UNIQUE,
                     reg_time TEXT NOT NULL,
                     last_login_time TEXT NOT NULL,
                     status TEXT NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS players (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     account_id INTEGER NOT NULL UNIQUE,
                     FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
                 );
                 CREATE TABLE IF NOT EXISTS player_snapshots (
                     account_id INTEGER PRIMARY KEY,
                     data_json TEXT NOT NULL,
                     FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
                 );
                  CREATE TABLE IF NOT EXISTS active_single_quests (
                      account_id INTEGER PRIMARY KEY,
                      play_id TEXT NOT NULL DEFAULT '',
                      quest_id INTEGER NOT NULL,
                     category INTEGER NOT NULL,
                      use_boss_boost_point INTEGER NOT NULL,
                      use_boost_point INTEGER NOT NULL,
                      is_auto_start_mode INTEGER NOT NULL,
                      continue_count INTEGER NOT NULL DEFAULT 0 CHECK (continue_count >= 0),
                      FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
                  );
                  CREATE TABLE IF NOT EXISTS single_battle_finish_receipts (
                      account_id INTEGER PRIMARY KEY,
                      play_id TEXT NOT NULL,
                      category INTEGER NOT NULL CHECK (category > 0),
                      quest_id INTEGER NOT NULL CHECK (quest_id > 0),
                      response_json TEXT NOT NULL,
                      FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
                  );
                  CREATE TABLE IF NOT EXISTS single_battle_action_receipts (
                      account_id INTEGER NOT NULL,
                      action TEXT NOT NULL CHECK (action IN ('start', 'continue', 'abort')),
                      play_id TEXT NOT NULL,
                      api_count INTEGER NOT NULL DEFAULT -1,
                      response_json TEXT NOT NULL,
                      PRIMARY KEY (account_id, action, play_id, api_count),
                      FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
                  );
                  CREATE TABLE IF NOT EXISTS sessions (
                     token TEXT PRIMARY KEY NOT NULL,
                     account_id INTEGER NOT NULL,
                     expires TEXT NOT NULL,
                     type INTEGER NOT NULL,
                     FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE,
                     UNIQUE (account_id, type)
                 );",
            )
            .map_err(|error| {
                PersonalServiceError::new(format!("failed to migrate database: {error}"))
            })?;
        migrate_credential_digest_schema(&mut connection)?;
        migrate_active_single_quest_schema(&connection)?;
        migrate_single_battle_finish_receipt_schema(&connection)?;
        local_saves::migrate(&connection)?;
        ai_teams::migrate(&connection)?;
        activity_calendar::migrate(&connection)?;
        activity_catalog::migrate(&connection)?;
        local_save_keys::migrate(&connection)?;
        save_sync::migrate(&connection)?;
        save_automation::migrate(&connection)?;
        mails::migrate(&connection)?;
        receive_history::migrate(&connection)?;
        mission_counters::migrate(&connection)?;
        multiplayer::migrate(&connection)?;
        server_profiles::migrate(&connection)?;
        server_profile_identities::migrate(&connection)?;
        transfer_bindings::migrate(&connection)?;
        clock::migrate(&connection)?;
        gameplay_settings::migrate(&connection)?;
        let management_token = rotate_management_token(&connection)?;
        Ok(Self {
            connection,
            management_token,
            multiplayer_session_port: 17_172,
        })
    }
    // //// /创建并迁移本地 SQLite 数据库 ////

    pub(crate) fn multiplayer_session_port(&self) -> u16 {
        self.multiplayer_session_port
    }

    pub(crate) fn set_multiplayer_session_port(&mut self, port: u16) {
        self.multiplayer_session_port = port;
    }

    // //// 截断已经提交的 SQLite WAL [@x380kkm 2026-07-22] ////
    pub(crate) fn checkpoint(&self) -> Result<(), PersonalServiceError> {
        self.connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|error| {
                PersonalServiceError::new(format!("failed to checkpoint database: {error}"))
            })
    }
    // //// /截断已经提交的 SQLite WAL ////

    pub(crate) fn generation(&self) -> Result<i64, PersonalServiceError> {
        self.connection
            .query_row(
                "SELECT generation FROM service_state WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|error| {
                PersonalServiceError::new(format!("failed to read service state: {error}"))
            })
    }

    pub(crate) fn management_token(&self) -> &str {
        &self.management_token
    }

    pub(crate) fn drop_multiplier(&self) -> Result<i64, PersonalServiceError> {
        gameplay_settings::get(&self.connection)
    }

    pub(crate) fn set_drop_multiplier(
        &mut self,
        drop_multiplier: i64,
    ) -> Result<i64, PersonalServiceError> {
        gameplay_settings::set(&self.connection, drop_multiplier)
    }

    // //// 记录并读取最近的 HTTP 请求 [@x380kkm 2026-08-21] ////
    pub(crate) fn record_http_observation(
        &mut self,
        method: &str,
        path: &str,
        status: impl ToString,
    ) -> Result<(), PersonalServiceError> {
        if path == "/manage"
            || path.starts_with("/manage/")
            || path == "/v1"
            || path.starts_with("/v1/")
        {
            return Ok(());
        }
        let status = status
            .to_string()
            .split_ascii_whitespace()
            .next()
            .and_then(|value| value.parse::<i64>().ok())
            .ok_or_else(|| PersonalServiceError::new("invalid HTTP response status"))?;
        let transaction = self.connection.transaction().map_err(database_error)?;
        transaction
            .execute(
                "INSERT INTO http_observations (
                     method, path, status, count, first_seen, last_seen
                 ) VALUES (
                     ?1, ?2, ?3, 1,
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 )
                 ON CONFLICT(method, path, status) DO UPDATE SET
                     count = count + 1,
                     last_seen = excluded.last_seen",
                params![method, path, status],
            )
            .map_err(database_error)?;
        transaction
            .execute(
                "DELETE FROM http_observations
                 WHERE rowid IN (
                     SELECT rowid
                     FROM http_observations
                     ORDER BY last_seen DESC, rowid DESC
                     LIMIT -1 OFFSET ?1
                 )",
                params![HTTP_OBSERVATION_LIMIT],
            )
            .map_err(database_error)?;
        transaction.commit().map_err(database_error)
    }

    pub(crate) fn http_observations(&self) -> Result<Vec<HttpObservation>, PersonalServiceError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT method, path, status, count, first_seen, last_seen
                 FROM http_observations
                 ORDER BY last_seen DESC, method, path, status
                 LIMIT ?1",
            )
            .map_err(database_error)?;
        let observations = statement
            .query_map(params![HTTP_OBSERVATION_LIMIT], |row| {
                Ok(HttpObservation {
                    method: row.get(0)?,
                    path: row.get(1)?,
                    status: row.get(2)?,
                    count: row.get(3)?,
                    first_seen: row.get(4)?,
                    last_seen: row.get(5)?,
                })
            })
            .map_err(database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database_error)?;
        Ok(observations)
    }
    // //// /记录并读取最近的 HTTP 请求 ////

    // //// 签发和撤销账号存档访问 token [@x380kkm 2026-07-24] ////
    pub(crate) fn issue_player_access_token(
        &mut self,
        viewer_id: i64,
    ) -> Result<Option<String>, PersonalServiceError> {
        let transaction = self.connection.transaction().map_err(database_error)?;
        let account_id = transaction
            .query_row(
                "SELECT account_id FROM sessions WHERE token = ?1 AND type = 2",
                params![viewer_id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(database_error)?;
        let Some(account_id) = account_id else {
            return Ok(None);
        };
        let token = generate_url_token()?;
        let created_at = sqlite_utc_timestamp(&transaction).map_err(database_error)?;
        transaction
            .execute(
                "DELETE FROM player_access_tokens WHERE account_id = ?1",
                params![account_id],
            )
            .map_err(database_error)?;
        transaction
            .execute(
                "INSERT INTO player_access_tokens (token_hash, account_id, created_at) VALUES (?1, ?2, ?3)",
                params![player_access_token_hash(&token), account_id, created_at],
            )
            .map_err(database_error)?;
        transaction.commit().map_err(database_error)?;
        Ok(Some(token))
    }

    pub(crate) fn revoke_player_access_token(
        &mut self,
        viewer_id: i64,
    ) -> Result<bool, PersonalServiceError> {
        let account_id = self
            .connection
            .query_row(
                "SELECT account_id FROM sessions WHERE token = ?1 AND type = 2",
                params![viewer_id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(database_error)?;
        let Some(account_id) = account_id else {
            return Ok(false);
        };
        let deleted = self
            .connection
            .execute(
                "DELETE FROM player_access_tokens WHERE account_id = ?1",
                params![account_id],
            )
            .map_err(database_error)?;
        Ok(deleted > 0)
    }

    pub(crate) fn player_access_account_id(
        &self,
        token: &str,
    ) -> Result<Option<i64>, PersonalServiceError> {
        self.connection
            .query_row(
                "SELECT account_id FROM player_access_tokens WHERE token_hash = ?1",
                params![player_access_token_hash(token)],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(database_error)
    }

    pub(crate) fn player_account_id_for_viewer(
        &self,
        viewer_id: i64,
    ) -> Result<Option<i64>, PersonalServiceError> {
        self.connection
            .query_row(
                "SELECT account_id FROM sessions WHERE token = ?1 AND type = 2",
                params![viewer_id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(database_error)
    }
    // //// /签发和撤销账号存档访问 token ////

    // //// 读取活动 Boss 状态 [@x380kkm 2026-07-24] ////
    pub(crate) fn raid_boss_state(
        &self,
        event_id: i64,
    ) -> Result<RaidBossState, PersonalServiceError> {
        self.connection
            .query_row(
                "SELECT hp_percentage, total_kill_count FROM raid_boss_states WHERE event_id = ?1",
                params![event_id],
                |row| {
                    Ok(RaidBossState {
                        hp_percentage: row.get(0)?,
                        total_kill_count: row.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(database_error)
            .map(|state| {
                state.unwrap_or(RaidBossState {
                    hp_percentage: 100,
                    total_kill_count: 0,
                })
            })
    }
    // //// /读取活动 Boss 状态 ////

    // //// 写入活动 Boss 状态 [@x380kkm 2026-07-24] ////
    pub(crate) fn set_raid_boss_state(
        &mut self,
        event_id: i64,
        hp_percentage: i64,
        total_kill_count: i64,
    ) -> Result<(), PersonalServiceError> {
        self.connection
            .execute(
                "INSERT OR REPLACE INTO raid_boss_states (event_id, hp_percentage, total_kill_count) VALUES (?1, ?2, ?3)",
                params![event_id, hp_percentage, total_kill_count],
            )
            .map_err(database_error)?;
        Ok(())
    }
    // //// /写入活动 Boss 状态 ////

    // //// 原子增加持久化验证计数 [@x380kkm 2026-07-22] ////
    pub(crate) fn increment_generation(&self) -> Result<i64, PersonalServiceError> {
        self.connection
            .execute(
                "UPDATE service_state SET generation = generation + 1 WHERE id = ?1",
                params![1],
            )
            .map_err(|error| {
                PersonalServiceError::new(format!("failed to update service state: {error}"))
            })?;
        self.generation()
    }
    // //// /原子增加持久化验证计数 ////

    // //// 读取客户端格式的当前服务时间 [@x380kkm 2026-07-24] ////
    pub(crate) fn get_current_client_time(&self) -> Result<String, PersonalServiceError> {
        self.current_client_time()
    }
    // //// /读取客户端格式的当前服务时间 ////

    // //// 创建账号和默认玩家数据并轮换唯一的 viewer 会话 [@x380kkm 2026-07-22] ////
    pub(crate) fn get_or_create_account_and_rotate_viewer_session(
        &mut self,
        device_id: i64,
        default_player_data: &str,
    ) -> Result<SignupAccount, SignupDeviceError> {
        let transaction = self.connection.transaction().map_err(storage_error)?;
        let idp_id = format!("cn:{device_id}");
        let idp_alias = format!("wf_cn:{device_id}:android");
        let active_account_id = local_saves::active_local_save_account_id(&transaction, device_id)
            .map_err(storage_error)?;
        let read_account = |row: &rusqlite::Row<'_>| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        };
        let existing_account = match active_account_id {
            Some(account_id) => transaction
                .query_row(
                    "SELECT id, app_id, idp_alias, reg_time FROM accounts WHERE id = ?1",
                    params![account_id],
                    read_account,
                )
                .optional()
                .map_err(storage_error)?,
            None => transaction
                .query_row(
                    "SELECT id, app_id, idp_alias, reg_time FROM accounts WHERE idp_id = ?1",
                    params![idp_id],
                    read_account,
                )
                .optional()
                .map_err(storage_error)?,
        };
        let now = sqlite_utc_timestamp(&transaction).map_err(storage_error)?;
        let (account_id, created_at, is_new) = match existing_account {
            Some((account_id, app_id, stored_alias, created_at)) => {
                if app_id != "wf_cn" || (active_account_id.is_none() && stored_alias != idp_alias) {
                    return Err(SignupDeviceError::BindingConflict);
                }
                transaction
                    .execute(
                        "UPDATE accounts SET last_login_time = ?1 WHERE id = ?2",
                        params![now, account_id],
                    )
                    .map_err(storage_error)?;
                (account_id, created_at, false)
            }
            None => {
                transaction
                    .execute(
                        "INSERT INTO accounts (
                            app_id, first_login_time, idp_alias, idp_code, idp_id,
                            reg_time, last_login_time, status
                         ) VALUES ('wf_cn', ?3, ?1, 'leiting', ?2, ?3, ?3, 'normal')",
                        params![idp_alias, idp_id, now],
                    )
                    .map_err(storage_error)?;
                let account_id = transaction.last_insert_rowid();
                (account_id, now.clone(), true)
            }
        };
        transaction
            .execute(
                "INSERT OR IGNORE INTO players (account_id) VALUES (?1)",
                params![account_id],
            )
            .map_err(storage_error)?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO player_snapshots (account_id, data_json) VALUES (?1, ?2)",
                params![account_id, default_player_data],
            )
            .map_err(storage_error)?;
        local_saves::ensure_local_save_slot(&transaction, account_id, device_id)
            .map_err(storage_error)?;
        transaction
            .execute(
                "DELETE FROM sessions WHERE account_id = ?1 AND type = 2",
                params![account_id],
            )
            .map_err(storage_error)?;
        let viewer_id = insert_unique_viewer_session(&transaction, account_id, &now)
            .map_err(SignupDeviceError::Storage)?;
        transaction.commit().map_err(storage_error)?;
        Ok(SignupAccount {
            account_id,
            created_at,
            is_new,
            viewer_id,
        })
    }
    // //// /创建账号和默认玩家数据并轮换唯一的 viewer 会话 ////

    // //// 校验 viewer 会话并定位玩家主记录 [@x380kkm 2026-07-22] ////
    pub(crate) fn lookup_viewer_session_player(
        &self,
        viewer_id: i64,
    ) -> Result<ViewerSessionPlayer, PersonalServiceError> {
        let session = self
            .connection
            .query_row(
                "SELECT sessions.type, sessions.account_id, players.id, player_snapshots.data_json
                 FROM sessions
                 LEFT JOIN players ON players.account_id = sessions.account_id
                 LEFT JOIN player_snapshots ON player_snapshots.account_id = sessions.account_id
                 WHERE sessions.token = ?1",
                params![viewer_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(database_error)?;
        match session {
            Some((2, account_id, Some(_), Some(data))) => {
                Ok(ViewerSessionPlayer::Present(PlayerSnapshot {
                    account_id,
                    data,
                }))
            }
            Some((2, account_id, Some(_), None)) => {
                Ok(ViewerSessionPlayer::MissingPlayerData(account_id))
            }
            Some((2, _, None, _)) => Ok(ViewerSessionPlayer::MissingPlayer),
            _ => Ok(ViewerSessionPlayer::InvalidSession),
        }
    }
    // //// /校验 viewer 会话并定位玩家主记录 ////

    // //// 原子保存账号玩家快照 [@x380kkm 2026-07-22] ////
    pub(crate) fn save_player_snapshot(
        &mut self,
        account_id: i64,
        data: &str,
    ) -> Result<(), PersonalServiceError> {
        self.connection
            .execute(
                "INSERT INTO player_snapshots (account_id, data_json) VALUES (?1, ?2)
                 ON CONFLICT(account_id) DO UPDATE SET data_json = excluded.data_json",
                params![account_id, data],
            )
            .map_err(database_error)?;
        Ok(())
    }
    // //// /原子保存账号玩家快照 ////

    // //// 原子保存玩家快照和当前单机战斗 [@x380kkm 2026-07-22] ////
    pub(crate) fn start_active_single_quest(
        &mut self,
        account_id: i64,
        data: &str,
        quest: &ActiveSingleQuest,
    ) -> Result<(), PersonalServiceError> {
        self.start_active_single_quest_transaction(account_id, data, quest, None)
    }

    pub(crate) fn start_active_single_quest_with_receipt(
        &mut self,
        account_id: i64,
        data: &str,
        quest: &ActiveSingleQuest,
        response_json: &str,
    ) -> Result<(), PersonalServiceError> {
        self.start_active_single_quest_transaction(account_id, data, quest, Some(response_json))
    }

    fn start_active_single_quest_transaction(
        &mut self,
        account_id: i64,
        data: &str,
        quest: &ActiveSingleQuest,
        response_json: Option<&str>,
    ) -> Result<(), PersonalServiceError> {
        let transaction = self.connection.transaction().map_err(database_error)?;
        let updated = transaction
            .execute(
                "UPDATE player_snapshots SET data_json = ?1 WHERE account_id = ?2",
                params![data, account_id],
            )
            .map_err(database_error)?;
        if updated != 1 {
            return Err(PersonalServiceError::new(
                "failed to update the CN player snapshot during battle start",
            ));
        }
        transaction
            .execute(
                "DELETE FROM single_battle_finish_receipts WHERE account_id = ?1",
                params![account_id],
            )
            .map_err(database_error)?;
        transaction
            .execute(
                "DELETE FROM single_battle_action_receipts WHERE account_id = ?1",
                params![account_id],
            )
            .map_err(database_error)?;
        transaction
            .execute(
                "INSERT INTO active_single_quests (
                     account_id, play_id, quest_id, category, use_boss_boost_point,
                     use_boost_point, is_auto_start_mode, continue_count
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)
                 ON CONFLICT(account_id) DO UPDATE SET
                     play_id = excluded.play_id,
                     quest_id = excluded.quest_id,
                     category = excluded.category,
                      use_boss_boost_point = excluded.use_boss_boost_point,
                      use_boost_point = excluded.use_boost_point,
                      is_auto_start_mode = excluded.is_auto_start_mode,
                      continue_count = 0",
                params![
                    account_id,
                    quest.play_id.as_str(),
                    quest.quest_id,
                    quest.category,
                    quest.use_boss_boost_point,
                    quest.use_boost_point,
                    quest.is_auto_start_mode,
                ],
            )
            .map_err(database_error)?;
        if let Some(response_json) = response_json {
            transaction
                .execute(
                    "INSERT INTO single_battle_action_receipts (
                         account_id, action, play_id, api_count, response_json
                     ) VALUES (?1, 'start', ?2, -1, ?3)",
                    params![account_id, quest.play_id.as_str(), response_json],
                )
                .map_err(database_error)?;
        }
        transaction.commit().map_err(database_error)?;
        Ok(())
    }
    // //// /原子保存玩家快照和当前单机战斗 ////

    // //// 读取账号当前的单机战斗 [@x380kkm 2026-07-22] ////
    pub(crate) fn get_active_single_quest(
        &self,
        account_id: i64,
    ) -> Result<Option<ActiveSingleQuest>, PersonalServiceError> {
        self.connection
            .query_row(
                "SELECT play_id, quest_id, category, use_boss_boost_point, use_boost_point,
                        is_auto_start_mode
                 FROM active_single_quests WHERE account_id = ?1",
                params![account_id],
                |row| {
                    Ok(ActiveSingleQuest {
                        play_id: row.get(0)?,
                        quest_id: row.get(1)?,
                        category: row.get(2)?,
                        use_boss_boost_point: row.get(3)?,
                        use_boost_point: row.get(4)?,
                        is_auto_start_mode: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(database_error)
    }
    // //// /读取账号当前的单机战斗 ////

    // //// 读取账号载入响应的未完成战斗 [@x380kkm 2026-08-23] ////
    pub(crate) fn get_unfinished_quest(
        &self,
        account_id: i64,
    ) -> Result<Option<UnfinishedQuest>, PersonalServiceError> {
        self.connection
            .query_row(
                "SELECT quests.play_id,
                        COALESCE((
                            SELECT members.continue_count
                            FROM multiplayer_battle_players AS battle_players
                            JOIN multiplayer_room_members AS members
                              ON members.room_number = battle_players.room_number
                             AND members.account_id = battle_players.account_id
                            WHERE battle_players.account_id = quests.account_id
                              AND battle_players.play_id = quests.play_id
                            LIMIT 1
                        ), quests.continue_count),
                        EXISTS (
                            SELECT 1
                            FROM multiplayer_battle_players AS battle_players
                            WHERE battle_players.account_id = quests.account_id
                              AND battle_players.play_id = quests.play_id
                        )
                 FROM active_single_quests AS quests
                 WHERE quests.account_id = ?1",
                params![account_id],
                |row| {
                    Ok(UnfinishedQuest {
                        play_id: row.get(0)?,
                        continue_count: row.get(1)?,
                        is_multi: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(database_error)
    }
    // //// /读取账号载入响应的未完成战斗 ////

    // //// 原子保存继续费用并累计当前战斗继续次数 [@x380kkm 2026-08-22] ////
    pub(crate) fn continue_active_single_quest_with_receipt(
        &mut self,
        account_id: i64,
        play_id: &str,
        api_count: Option<i64>,
        data: &str,
        response_json: &str,
    ) -> Result<Option<i64>, PersonalServiceError> {
        let transaction = self.connection.transaction().map_err(database_error)?;
        let continue_count = transaction
            .query_row(
                "UPDATE active_single_quests
                 SET continue_count = continue_count + 1
                 WHERE account_id = ?1 AND play_id = ?2
                 RETURNING continue_count",
                params![account_id, play_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(database_error)?;
        let Some(continue_count) = continue_count else {
            return Ok(None);
        };
        let updated = transaction
            .execute(
                "UPDATE player_snapshots SET data_json = ?1 WHERE account_id = ?2",
                params![data, account_id],
            )
            .map_err(database_error)?;
        if updated != 1 {
            return Err(PersonalServiceError::new(
                "failed to update the CN player snapshot during battle continue",
            ));
        }
        transaction
            .execute(
                "INSERT INTO single_battle_action_receipts (
                     account_id, action, play_id, api_count, response_json
                 ) VALUES (?1, 'continue', ?2, ?3, ?4)
                 ON CONFLICT(account_id, action, play_id, api_count) DO UPDATE SET
                     response_json = excluded.response_json",
                params![account_id, play_id, api_count.unwrap_or(-1), response_json],
            )
            .map_err(database_error)?;
        transaction.commit().map_err(database_error)?;
        Ok(Some(continue_count))
    }
    // //// /原子保存继续费用并累计当前战斗继续次数 ////

    pub(crate) fn finish_active_single_quest_with_receipt(
        &mut self,
        account_id: i64,
        play_id: &str,
        category: i64,
        quest_id: i64,
        data: &str,
        response_json: &str,
    ) -> Result<(), PersonalServiceError> {
        let transaction = self.connection.transaction().map_err(database_error)?;
        let updated = transaction
            .execute(
                "UPDATE player_snapshots SET data_json = ?1 WHERE account_id = ?2",
                params![data, account_id],
            )
            .map_err(database_error)?;
        if updated != 1 {
            return Err(PersonalServiceError::new(
                "failed to update the CN player snapshot during battle finish",
            ));
        }
        let deleted = transaction
            .execute(
                "DELETE FROM active_single_quests
                 WHERE account_id = ?1 AND play_id = ?2 AND category = ?3 AND quest_id = ?4",
                params![account_id, play_id, category, quest_id],
            )
            .map_err(database_error)?;
        if deleted != 1 {
            return Err(PersonalServiceError::new(
                "failed to delete the finished CN single battle",
            ));
        }
        transaction
            .execute(
                "INSERT INTO single_battle_finish_receipts (
                     account_id, play_id, category, quest_id, response_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(account_id) DO UPDATE SET
                     play_id = excluded.play_id,
                     category = excluded.category,
                     quest_id = excluded.quest_id,
                     response_json = excluded.response_json",
                params![account_id, play_id, category, quest_id, response_json],
            )
            .map_err(database_error)?;
        transaction.commit().map_err(database_error)
    }

    pub(crate) fn single_battle_finish_receipt(
        &self,
        account_id: i64,
        play_id: &str,
        category: i64,
        quest_id: i64,
    ) -> Result<Option<String>, PersonalServiceError> {
        self.connection
            .query_row(
                "SELECT response_json FROM single_battle_finish_receipts
                 WHERE account_id = ?1 AND play_id = ?2 AND category = ?3 AND quest_id = ?4",
                params![account_id, play_id, category, quest_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(database_error)
    }

    // //// 保存和读取单机战斗动作收据 [@x380kkm 2026-08-23] ////
    pub(crate) fn single_battle_start_receipt(
        &self,
        account_id: i64,
        play_id: &str,
    ) -> Result<Option<String>, PersonalServiceError> {
        self.single_battle_action_receipt(account_id, "start", play_id)
    }

    pub(crate) fn single_battle_continue_receipt(
        &self,
        account_id: i64,
        play_id: &str,
        api_count: Option<i64>,
    ) -> Result<Option<String>, PersonalServiceError> {
        let Some(api_count) = api_count else {
            return self
                .connection
                .query_row(
                    "SELECT response_json FROM single_battle_action_receipts
                     WHERE account_id = ?1 AND action = 'continue' AND play_id = ?2
                     ORDER BY api_count DESC LIMIT 1",
                    params![account_id, play_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(database_error);
        };
        self.connection
            .query_row(
                "SELECT response_json FROM single_battle_action_receipts
                 WHERE account_id = ?1 AND action = 'continue'
                   AND play_id = ?2 AND api_count = ?3",
                params![account_id, play_id, api_count],
                |row| row.get(0),
            )
            .optional()
            .map_err(database_error)
    }

    pub(crate) fn single_battle_abort_receipt(
        &self,
        account_id: i64,
        play_id: &str,
    ) -> Result<Option<String>, PersonalServiceError> {
        self.single_battle_action_receipt(account_id, "abort", play_id)
    }

    fn single_battle_action_receipt(
        &self,
        account_id: i64,
        action: &str,
        play_id: &str,
    ) -> Result<Option<String>, PersonalServiceError> {
        self.connection
            .query_row(
                "SELECT response_json FROM single_battle_action_receipts
                 WHERE account_id = ?1 AND action = ?2 AND play_id = ?3 AND api_count = -1",
                params![account_id, action, play_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(database_error)
    }

    pub(crate) fn abort_active_single_quest_with_receipt(
        &mut self,
        account_id: i64,
        play_id: &str,
        response_json: &str,
    ) -> Result<bool, PersonalServiceError> {
        let transaction = self.connection.transaction().map_err(database_error)?;
        let deleted = transaction
            .execute(
                "DELETE FROM active_single_quests
                 WHERE account_id = ?1",
                params![account_id],
            )
            .map_err(database_error)?;
        if deleted != 1 {
            return Ok(false);
        }
        transaction
            .execute(
                "INSERT INTO single_battle_action_receipts (
                     account_id, action, play_id, api_count, response_json
                 ) VALUES (?1, 'abort', ?2, -1, ?3)
                 ON CONFLICT(account_id, action, play_id, api_count) DO UPDATE SET
                     response_json = excluded.response_json",
                params![account_id, play_id, response_json],
            )
            .map_err(database_error)?;
        transaction.commit().map_err(database_error)?;
        Ok(true)
    }
    // //// /保存和读取单机战斗动作收据 ////
}

// //// 每次启动轮换内存中的管理 token [@x380kkm 2026-07-27] ////
fn rotate_management_token(connection: &Connection) -> Result<String, PersonalServiceError> {
    let token = generate_url_token()?;
    connection
        .execute(
            "INSERT OR REPLACE INTO management_state (id, token_hash) VALUES (1, ?1)",
            params![management_token_hash(&token)],
        )
        .map_err(database_error)?;
    Ok(token)
}

// //// /每次启动轮换内存中的管理 token ////

// //// 迁移管理和玩家 token 的摘要存储 [@x380kkm 2026-07-27] ////
fn migrate_credential_digest_schema(
    connection: &mut Connection,
) -> Result<(), PersonalServiceError> {
    let management_uses_plaintext = table_has_column(connection, "management_state", "token")?;
    let player_access_uses_plaintext =
        table_has_column(connection, "player_access_tokens", "token")?;
    if !management_uses_plaintext && !player_access_uses_plaintext {
        return Ok(());
    }

    let legacy_player_access_tokens = if player_access_uses_plaintext {
        read_legacy_player_access_tokens(connection)?
    } else {
        Vec::new()
    };
    let transaction = connection.transaction().map_err(database_error)?;
    if management_uses_plaintext {
        transaction
            .execute_batch(
                "DROP TABLE management_state;
                 CREATE TABLE management_state (
                     id INTEGER PRIMARY KEY CHECK (id = 1),
                     token_hash TEXT NOT NULL CHECK (length(token_hash) = 64)
                 );",
            )
            .map_err(database_error)?;
    }
    if player_access_uses_plaintext {
        transaction
            .execute_batch(
                "CREATE TABLE player_access_tokens_replacement (
                     token_hash TEXT PRIMARY KEY NOT NULL CHECK (length(token_hash) = 64),
                     account_id INTEGER NOT NULL UNIQUE,
                     created_at TEXT NOT NULL,
                     FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
                 );",
            )
            .map_err(database_error)?;
        for (token, account_id, created_at) in legacy_player_access_tokens {
            if token.len() != 43 {
                return Err(PersonalServiceError::new(
                    "stored player access token has an invalid length",
                ));
            }
            transaction
                .execute(
                    "INSERT INTO player_access_tokens_replacement (
                         token_hash, account_id, created_at
                     ) VALUES (?1, ?2, ?3)",
                    params![player_access_token_hash(&token), account_id, created_at],
                )
                .map_err(database_error)?;
        }
        transaction
            .execute_batch(
                "DROP TABLE player_access_tokens;
                 ALTER TABLE player_access_tokens_replacement RENAME TO player_access_tokens;",
            )
            .map_err(database_error)?;
    }
    transaction.commit().map_err(database_error)
}

fn table_has_column(
    connection: &Connection,
    table: &str,
    column: &str,
) -> Result<bool, PersonalServiceError> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(database_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(database_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_error)?;
    Ok(columns.iter().any(|stored| stored == column))
}

// //// 建立单机战斗恢复字段 [@x380kkm 2026-08-18] ////
fn migrate_active_single_quest_schema(connection: &Connection) -> Result<(), PersonalServiceError> {
    if !table_has_column(connection, "active_single_quests", "play_id")? {
        connection
            .execute(
                "ALTER TABLE active_single_quests
                 ADD COLUMN play_id TEXT NOT NULL DEFAULT ''",
                [],
            )
            .map_err(database_error)?;
        connection
            .execute("DELETE FROM active_single_quests WHERE play_id = ''", [])
            .map_err(database_error)?;
    }
    if !table_has_column(connection, "active_single_quests", "continue_count")? {
        connection
            .execute(
                "ALTER TABLE active_single_quests
                 ADD COLUMN continue_count INTEGER NOT NULL DEFAULT 0",
                [],
            )
            .map_err(database_error)?;
    }
    Ok(())
}
// //// /建立单机战斗恢复字段 ////

// //// 建立单机结算收据的战斗身份 [@x380kkm 2026-08-23] ////
fn migrate_single_battle_finish_receipt_schema(
    connection: &Connection,
) -> Result<(), PersonalServiceError> {
    if !table_has_column(connection, "single_battle_finish_receipts", "play_id")? {
        connection
            .execute(
                "ALTER TABLE single_battle_finish_receipts
                 ADD COLUMN play_id TEXT NOT NULL DEFAULT ''",
                [],
            )
            .map_err(database_error)?;
    }
    Ok(())
}
// //// /建立单机结算收据的战斗身份 ////

fn read_legacy_player_access_tokens(
    connection: &Connection,
) -> Result<Vec<(String, i64, String)>, PersonalServiceError> {
    let mut statement = connection
        .prepare("SELECT token, account_id, created_at FROM player_access_tokens")
        .map_err(database_error)?;
    let tokens = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .map_err(database_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_error)?;
    Ok(tokens)
}
// //// /迁移管理和玩家 token 的摘要存储 ////

fn generate_url_token() -> Result<String, PersonalServiceError> {
    let mut bytes = [0_u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|error| {
        PersonalServiceError::new(format!("failed to generate access token: {error}"))
    })?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn management_token_hash(token: &str) -> String {
    credential_token_hash("management", token)
}

fn player_access_token_hash(token: &str) -> String {
    credential_token_hash("player-access", token)
}

fn credential_token_hash(kind: &str, token: &str) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!("starpoint-personal-service-{kind}:{token}").as_bytes())
    )
}

fn storage_error(error: rusqlite::Error) -> SignupDeviceError {
    SignupDeviceError::Storage(database_error(error))
}

fn sqlite_utc_timestamp(transaction: &Transaction<'_>) -> Result<String, rusqlite::Error> {
    transaction.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |row| {
        row.get(0)
    })
}

// //// 写入未占用的九位 viewer 会话 [@x380kkm 2026-07-22] ////
fn insert_unique_viewer_session(
    transaction: &Transaction<'_>,
    account_id: i64,
    created_at: &str,
) -> Result<i64, PersonalServiceError> {
    loop {
        let viewer_id = generate_viewer_id().map_err(|error| {
            PersonalServiceError::new(format!("failed to generate viewer session: {error}"))
        })?;
        let exists = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sessions WHERE token = ?1)",
                params![viewer_id.to_string()],
                |row| row.get::<_, bool>(0),
            )
            .map_err(database_error)?;
        if exists {
            continue;
        }
        transaction
            .execute(
                "INSERT INTO sessions (token, account_id, expires, type) VALUES (?1, ?2, ?3, 2)",
                params![viewer_id.to_string(), account_id, created_at],
            )
            .map_err(database_error)?;
        return Ok(viewer_id);
    }
}
// //// /写入未占用的九位 viewer 会话 ////

fn generate_viewer_id() -> Result<i64, getrandom::Error> {
    const VIEWER_ID_RANGE: u32 = 899_999_999;
    const UNBIASED_LIMIT: u32 = u32::MAX - (u32::MAX % VIEWER_ID_RANGE);
    loop {
        let mut random_bytes = [0_u8; 4];
        getrandom::getrandom(&mut random_bytes)?;
        let random_value = u32::from_le_bytes(random_bytes);
        if random_value < UNBIASED_LIMIT {
            return Ok(i64::from(random_value % VIEWER_ID_RANGE) + 100_000_000);
        }
    }
}

fn database_error(error: rusqlite::Error) -> PersonalServiceError {
    PersonalServiceError::new(format!("failed to update CN account storage: {error}"))
}
pub(crate) use local_save_keys::LocalSaveEncryptionKey;
pub(crate) use local_saves::{
    LocalIssuedTransferToken, LocalSaveExport, LocalSaveRevision, LocalSaveSlot, LocalSaveSnapshot,
    LocalSaveState, LocalSaveStoreError, LocalTransferPermission, LocalTransferTokenMetadata,
};
pub(crate) use save_automation::{
    LocalSaveAutomation, LocalSaveAutomationInput, LocalSaveAutomationStoreError,
    DEFAULT_AUTOMATION_INTERVAL_SECONDS, MAX_AUTOMATION_INTERVAL_SECONDS,
    MIN_AUTOMATION_INTERVAL_SECONDS,
};
pub(crate) use save_sync::{
    SaveSyncBinding, SaveSyncStoreError, SaveSyncTarget, SaveSyncTargetInput,
};
pub(crate) use transfer_bindings::{
    CreateTransferBindingInput, TransferBinding, TransferBindingEtagUpdate,
    TransferBindingStoreError, TransferConflict, TransferConflictPolicy, TransferConflictStatus,
    TransferInstanceKind, TransferPullMode, TransferUploadMode, UpdateTransferBindingInput,
    DEFAULT_TRANSFER_INTERVAL_SECONDS, MAX_TRANSFER_INTERVAL_SECONDS,
    MIN_TRANSFER_INTERVAL_SECONDS,
};
