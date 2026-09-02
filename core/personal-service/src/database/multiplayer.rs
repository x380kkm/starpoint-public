// audience: internal
// # personal-service-multiplayer-storage
//
// 该模块定义 CN 本地联机房间, 真人成员和 COM 快照存储结构.
// 协议时间使用虚拟时钟, 房间到期锚点使用真实墙钟.

mod ai;
mod battle_expected;
mod battle_state;
#[cfg(test)]
mod battle_state_tests;
mod lifecycle;
#[cfg(test)]
mod lifecycle_tests;
mod member;
mod room;

use crate::PersonalServiceError;
use rusqlite::{Connection, OptionalExtension};

pub(crate) use battle_state::{
    MultiplayerBattleAbort, MultiplayerBattleContinue, MultiplayerBattleFinish,
    MultiplayerBattleIdentity, MultiplayerBattleReceipt, MultiplayerBattleStart,
};

pub(super) const MAX_ROOM_MEMBERS: i64 = 3;

#[derive(Clone, Debug)]
pub(crate) struct MultiplayerRoom {
    pub(crate) room_number: String,
    pub(crate) room_sequence: i64,
    pub(crate) access_token: String,
    pub(crate) host_account_id: i64,
    pub(crate) host_viewer_id: i64,
    pub(crate) host_player_id: i64,
    pub(crate) host_party_id: i64,
    pub(crate) host_main_character_id: i64,
    pub(crate) category_id: i64,
    pub(crate) quest_id: i64,
    pub(crate) raising_state: i64,
    pub(crate) host_entry_time: i64,
    pub(crate) share_room_options: i64,
    pub(crate) battle_started: bool,
    pub(crate) lobby_started: bool,
    pub(crate) is_npc_mode: bool,
    pub(crate) member_count: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct MultiplayerMember {
    pub(crate) account_id: i64,
    pub(crate) viewer_id: i64,
    pub(crate) party_id: i64,
    pub(crate) connection_id: Option<String>,
    pub(crate) lobby_player_json: Option<String>,
    pub(crate) entered: bool,
    pub(crate) ready: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct MultiplayerAiMate {
    pub(crate) room_number: String,
    pub(crate) position: i64,
    pub(crate) owner_account_id: i64,
    pub(crate) snapshot_id: String,
    pub(crate) client_mate_json: String,
    pub(crate) lobby_player_json: String,
}

pub(crate) struct MultiplayerAiMateInput {
    pub(crate) snapshot_id: String,
    pub(crate) client_mate_json: String,
    pub(crate) lobby_player_json: String,
}

// //// 描述房间定时器产生的会话事件 [@x380kkm 2026-08-23] ////
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum MultiplayerRoomEventKind {
    Remaining { seconds: i64, deadline_ms: i64 },
    Dismissed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MultiplayerRoomEvent {
    pub(crate) room_number: String,
    pub(crate) room_sequence: i64,
    pub(crate) kind: MultiplayerRoomEventKind,
}
// //// /描述房间定时器产生的会话事件 ////

// //// 创建本地联机状态表 [@x380kkm 2026-08-22] ////
pub(super) fn migrate(connection: &Connection) -> Result<(), PersonalServiceError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS multiplayer_room_sequence (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 next_value INTEGER NOT NULL CHECK (next_value > 0)
             );
             INSERT OR IGNORE INTO multiplayer_room_sequence (id, next_value) VALUES (1, 1);
             CREATE TABLE IF NOT EXISTS multiplayer_rooms (
                 room_number TEXT PRIMARY KEY NOT NULL
                     CHECK (length(room_number) = 6),
                 room_sequence INTEGER NOT NULL UNIQUE CHECK (room_sequence > 0),
                 access_token TEXT NOT NULL UNIQUE CHECK (length(access_token) = 32),
                 host_account_id INTEGER NOT NULL,
                 host_viewer_id INTEGER NOT NULL CHECK (host_viewer_id > 0),
                 host_party_id INTEGER NOT NULL CHECK (host_party_id > 0),
                 host_main_character_id INTEGER NOT NULL CHECK (host_main_character_id > 0),
                 category_id INTEGER NOT NULL CHECK (category_id > 0),
                 quest_id INTEGER NOT NULL CHECK (quest_id > 0),
                 raising_state INTEGER NOT NULL CHECK (raising_state BETWEEN 0 AND 9),
                 created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
                 host_entry_time INTEGER NOT NULL CHECK (host_entry_time >= 0),
                 expiry_anchor_ms INTEGER NOT NULL CHECK (expiry_anchor_ms >= 0),
                 share_room_options INTEGER NOT NULL DEFAULT 0,
                 battle_started INTEGER NOT NULL DEFAULT 0
                     CHECK (battle_started IN (0, 1)),
                 lobby_started INTEGER NOT NULL DEFAULT 0
                     CHECK (lobby_started IN (0, 1)),
                 play_id TEXT,
                 is_npc_mode INTEGER NOT NULL DEFAULT 0
                     CHECK (is_npc_mode IN (0, 1)),
                 FOREIGN KEY (host_account_id) REFERENCES accounts (id) ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS multiplayer_room_members (
                 room_number TEXT NOT NULL,
                 account_id INTEGER NOT NULL,
                 viewer_id INTEGER NOT NULL CHECK (viewer_id > 0),
                 party_id INTEGER NOT NULL CHECK (party_id > 0),
                 member_index INTEGER NOT NULL CHECK (member_index BETWEEN 0 AND 2),
                 connection_id TEXT,
                 lobby_player_json TEXT,
                 entered INTEGER NOT NULL DEFAULT 0 CHECK (entered IN (0, 1)),
                 ready INTEGER NOT NULL DEFAULT 0 CHECK (ready IN (0, 1)),
                 autoplay INTEGER NOT NULL DEFAULT 0 CHECK (autoplay IN (0, 1)),
                 auto_start INTEGER NOT NULL DEFAULT 0 CHECK (auto_start IN (0, 1)),
                 scene_ready INTEGER NOT NULL DEFAULT 0 CHECK (scene_ready IN (0, 1)),
                 continue_count INTEGER NOT NULL DEFAULT 0 CHECK (continue_count >= 0),
                 PRIMARY KEY (room_number, viewer_id),
                 UNIQUE (room_number, account_id),
                 UNIQUE (room_number, member_index),
                 FOREIGN KEY (room_number) REFERENCES multiplayer_rooms (room_number)
                     ON DELETE CASCADE,
                 FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
             );
             CREATE UNIQUE INDEX IF NOT EXISTS multiplayer_member_connection
                 ON multiplayer_room_members (connection_id)
                 WHERE connection_id IS NOT NULL;
             CREATE TABLE IF NOT EXISTS multiplayer_ai_mates (
                 room_number TEXT NOT NULL,
                 position INTEGER NOT NULL CHECK (position IN (1, 2)),
                 owner_account_id INTEGER NOT NULL,
                 snapshot_id TEXT NOT NULL,
                 client_mate_json TEXT NOT NULL,
                 lobby_player_json TEXT NOT NULL,
                 PRIMARY KEY (room_number, position),
                 UNIQUE (room_number, snapshot_id),
                 FOREIGN KEY (room_number) REFERENCES multiplayer_rooms (room_number)
                     ON DELETE CASCADE,
                 FOREIGN KEY (owner_account_id) REFERENCES accounts (id) ON DELETE CASCADE,
                 FOREIGN KEY (snapshot_id) REFERENCES ai_team_snapshots (id) ON DELETE RESTRICT
             );
             CREATE TABLE IF NOT EXISTS multiplayer_battle_expected_members (
                 room_number TEXT NOT NULL,
                 room_sequence INTEGER NOT NULL CHECK (room_sequence > 0),
                 viewer_id INTEGER NOT NULL CHECK (viewer_id > 0),
                 delivered INTEGER NOT NULL DEFAULT 0 CHECK (delivered IN (0, 1)),
                 required INTEGER NOT NULL DEFAULT 1 CHECK (required IN (0, 1)),
                 PRIMARY KEY (room_number, room_sequence, viewer_id),
                 FOREIGN KEY (room_number, viewer_id)
                     REFERENCES multiplayer_room_members (room_number, viewer_id)
                     ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS multiplayer_room_expiry_notices (
                 room_number TEXT PRIMARY KEY NOT NULL,
                 room_sequence INTEGER NOT NULL,
                 deadline_ms INTEGER NOT NULL,
                 FOREIGN KEY (room_number) REFERENCES multiplayer_rooms (room_number)
                     ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS multiplayer_room_dismissals (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 room_number TEXT NOT NULL,
                 room_sequence INTEGER NOT NULL
             );",
        )
        .map_err(multiplayer_database_error)?;
    add_expiry_anchor_if_missing(connection)?;
    add_battle_start_delivery_if_missing(connection)?;
    add_battle_start_requirement_if_missing(connection)?;
    battle_state::migrate(connection)?;
    connection
        .execute_batch(
            "UPDATE multiplayer_rooms
             SET expiry_anchor_ms = CAST(strftime('%s', 'now') AS INTEGER) * 1000
             WHERE expiry_anchor_ms = 0;
             UPDATE multiplayer_room_sequence
             SET next_value = MAX(
                 next_value,
                 COALESCE((SELECT MAX(room_sequence) + 1 FROM multiplayer_rooms), 1),
                 COALESCE((SELECT MAX(room_sequence) + 1 FROM multiplayer_room_dismissals), 1)
             )
             WHERE id = 1;",
        )
        .map_err(multiplayer_database_error)?;
    connection
        .execute(
            "UPDATE multiplayer_room_members
             SET lobby_player_json = NULL, entered = 0, ready = 0,
                 autoplay = 0, auto_start = 0, scene_ready = 0",
            [],
        )
        .map_err(multiplayer_database_error)?;
    Ok(())
}
// //// /创建本地联机状态表 ////

// //// 为现有房间补充真实墙钟锚点 [@x380kkm 2026-08-23] ////
fn add_expiry_anchor_if_missing(connection: &Connection) -> Result<(), PersonalServiceError> {
    let exists = connection
        .prepare(
            "SELECT 1 FROM pragma_table_info('multiplayer_rooms')
             WHERE name = 'expiry_anchor_ms'",
        )
        .and_then(|mut statement| statement.query_row([], |_| Ok(())))
        .optional()
        .map_err(multiplayer_database_error)?
        .is_some();
    if !exists {
        connection
            .execute(
                "ALTER TABLE multiplayer_rooms
                 ADD COLUMN expiry_anchor_ms INTEGER NOT NULL DEFAULT 0
                     CHECK (expiry_anchor_ms >= 0)",
                [],
            )
            .map_err(multiplayer_database_error)?;
    }
    Ok(())
}
// //// /为现有房间补充真实墙钟锚点 ////

// //// 为现有战斗等待集合补充投递状态 [@x380kkm 2026-08-23] ////
fn add_battle_start_delivery_if_missing(
    connection: &Connection,
) -> Result<(), PersonalServiceError> {
    let exists = connection
        .prepare(
            "SELECT 1 FROM pragma_table_info('multiplayer_battle_expected_members')
             WHERE name = 'delivered'",
        )
        .and_then(|mut statement| statement.query_row([], |_| Ok(())))
        .optional()
        .map_err(multiplayer_database_error)?
        .is_some();
    if !exists {
        connection
            .execute(
                "ALTER TABLE multiplayer_battle_expected_members
                 ADD COLUMN delivered INTEGER NOT NULL DEFAULT 0
                     CHECK (delivered IN (0, 1))",
                [],
            )
            .map_err(multiplayer_database_error)?;
    }
    Ok(())
}
// //// /为现有战斗等待集合补充投递状态 ////

// //// 为现有战斗等待集合补充参与状态 [@x380kkm 2026-08-23] ////
fn add_battle_start_requirement_if_missing(
    connection: &Connection,
) -> Result<(), PersonalServiceError> {
    let exists = connection
        .prepare(
            "SELECT 1 FROM pragma_table_info('multiplayer_battle_expected_members')
             WHERE name = 'required'",
        )
        .and_then(|mut statement| statement.query_row([], |_| Ok(())))
        .optional()
        .map_err(multiplayer_database_error)?
        .is_some();
    if !exists {
        connection
            .execute(
                "ALTER TABLE multiplayer_battle_expected_members
                 ADD COLUMN required INTEGER NOT NULL DEFAULT 1
                     CHECK (required IN (0, 1))",
                [],
            )
            .map_err(multiplayer_database_error)?;
    }
    Ok(())
}
// //// /为现有战斗等待集合补充参与状态 ////

pub(super) fn multiplayer_database_error(error: rusqlite::Error) -> PersonalServiceError {
    PersonalServiceError::new(format!("failed to access multiplayer storage: {error}"))
}
