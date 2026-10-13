// audience: internal
// # personal-service-multiplayer-attention
//
// 救援房以真实账号保存 AI 快照归属, 以独立 COM viewer 标识房主.
// 发布和投递期限采用真实墙钟, 战斗参与来源随开战身份保存.

pub(super) mod participation;

use super::room::{allocate_room_sequence, read_room, room_select};
use super::{multiplayer_database_error, MultiplayerAiMateInput, MultiplayerRoom};
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

pub(crate) const BOT_HOST_VIEWER_ID: i64 = 900_000_001;

// //// 保存救援房发布状态和玩家投递凭据 [@x380kkm 2026-09-23] ////
pub(super) fn migrate(connection: &Connection) -> Result<(), PersonalServiceError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS multiplayer_attention_rooms (
             room_number TEXT PRIMARY KEY REFERENCES multiplayer_rooms(room_number) ON DELETE CASCADE,
             attention_key TEXT NOT NULL UNIQUE
         );
         CREATE TABLE IF NOT EXISTS multiplayer_attention_schedule (
             id INTEGER PRIMARY KEY CHECK(id = 1), next_spawn_ms INTEGER NOT NULL
         );
         INSERT OR IGNORE INTO multiplayer_attention_schedule VALUES(1, 0);
         CREATE TABLE IF NOT EXISTS multiplayer_attention_deliveries (
             room_number TEXT NOT NULL REFERENCES multiplayer_attention_rooms(room_number) ON DELETE CASCADE,
             viewer_id INTEGER NOT NULL, delivered_at_ms INTEGER NOT NULL,
             accepted INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(room_number, viewer_id)
         );
         CREATE TABLE IF NOT EXISTS multiplayer_battle_participation (
             account_id INTEGER NOT NULL, play_id TEXT NOT NULL,
             room_number TEXT NOT NULL REFERENCES multiplayer_rooms(room_number) ON DELETE CASCADE,
             is_attention INTEGER NOT NULL, has_newbie INTEGER NOT NULL,
             PRIMARY KEY(account_id, play_id)
         );"
    ).map_err(multiplayer_database_error)
}

impl ServiceDatabase {
    // //// 读取 BOT 队伍来源玩家的新手状态 [@x380kkm 2026-09-23] ////
    pub(crate) fn attention_owner_is_newbie(
        &self,
        account_id: i64,
    ) -> Result<bool, PersonalServiceError> {
        self.connection.query_row(
            "SELECT COALESCE(json_extract(data_json, '$.user_info.is_newbie'), 0) FROM player_snapshots WHERE account_id=?1",
            [account_id], |row| row.get(0),
        ).map_err(multiplayer_database_error)
    }
    // //// 读取可为救援房提供队伍的当前存档账号 [@x380kkm 2026-09-23] ////
    pub(crate) fn attention_owner_account(&self) -> Result<Option<i64>, PersonalServiceError> {
        self.connection
            .query_row(
                "SELECT slots.account_id FROM active_local_save_slots AS active
             JOIN local_save_slots AS slots ON slots.id = active.slot_id
             JOIN players ON players.account_id = slots.account_id
             JOIN player_snapshots AS snapshots ON snapshots.account_id = slots.account_id
             ORDER BY slots.updated_at DESC, slots.id DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(multiplayer_database_error)
    }

    // //// 读取后台救援发布期限 [@x380kkm 2026-09-23] ////
    pub(crate) fn attention_next_spawn_ms(&self) -> Result<i64, PersonalServiceError> {
        self.connection
            .query_row(
                "SELECT next_spawn_ms FROM multiplayer_attention_schedule WHERE id=1",
                [],
                |row| row.get(0),
            )
            .map_err(multiplayer_database_error)
    }

    // //// 保存后台救援发布期限 [@x380kkm 2026-09-23] ////
    pub(crate) fn set_attention_next_spawn_ms(
        &mut self,
        next_spawn_ms: i64,
    ) -> Result<(), PersonalServiceError> {
        self.connection
            .execute(
                "UPDATE multiplayer_attention_schedule SET next_spawn_ms=?1 WHERE id=1",
                [next_spawn_ms],
            )
            .map(|_| ())
            .map_err(multiplayer_database_error)
    }

    // //// 读取所有本地救援房 [@x380kkm 2026-09-23] ////
    pub(crate) fn attention_rooms(&self) -> Result<Vec<MultiplayerRoom>, PersonalServiceError> {
        let mut statement = self.connection.prepare(&format!(
            "{} JOIN multiplayer_attention_rooms AS attention ON attention.room_number=rooms.room_number
             ORDER BY rooms.room_sequence", room_select()
        )).map_err(multiplayer_database_error)?;
        let rooms = statement
            .query_map([], read_room)
            .map_err(multiplayer_database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(multiplayer_database_error)?;
        Ok(rooms)
    }

    // //// 同时保存救援房, BOT 队伍和下次发布时间 [@x380kkm 2026-09-24] ////
    pub(crate) fn create_attention_room(
        &mut self,
        account_id: i64,
        quest: (i64, i64),
        leader_id: i64,
        server_time_ms: i64,
        wall_time_ms: i64,
        next_spawn_ms: i64,
        prepare_mates: impl FnOnce(
            &MultiplayerRoom,
        ) -> Result<Vec<MultiplayerAiMateInput>, PersonalServiceError>,
    ) -> Result<MultiplayerRoom, PersonalServiceError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(multiplayer_database_error)?;
        let (room_number, sequence) = loop {
            let sequence = allocate_room_sequence(&transaction)?;
            let room_number = format!("{:06}", 100_000 + sequence.rem_euclid(900_000));
            let exists: bool = transaction
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM multiplayer_rooms WHERE room_number=?1)",
                    [&room_number],
                    |row| row.get(0),
                )
                .map_err(multiplayer_database_error)?;
            if !exists {
                break (room_number, sequence);
            }
        };
        transaction.execute(
            "INSERT INTO multiplayer_rooms (
                 room_number, room_sequence, access_token, host_account_id, host_viewer_id,
                 host_party_id, host_main_character_id, category_id, quest_id, raising_state,
                 created_at_ms, host_entry_time, expiry_anchor_ms, is_npc_mode
             ) VALUES(?1, ?2, lower(hex(randomblob(16))), ?3, ?4, 1, ?5, ?6, ?7, 1, ?8, ?9, ?10, 1)",
            params![room_number, sequence, account_id, BOT_HOST_VIEWER_ID, leader_id, quest.0, quest.1, server_time_ms, server_time_ms / 1000, wall_time_ms]
        ).map_err(multiplayer_database_error)?;
        transaction
            .execute(
                "INSERT INTO multiplayer_attention_rooms VALUES(?1, lower(hex(randomblob(16))))",
                [&room_number],
            )
            .map_err(multiplayer_database_error)?;
        let room = transaction
            .query_row(
                &format!("{} WHERE rooms.room_number=?1", room_select()),
                [&room_number],
                read_room,
            )
            .map_err(multiplayer_database_error)?;
        let mates = prepare_mates(&room)?;
        if !super::ai::save_ai_mates(&transaction, &room_number, account_id, &mates)? {
            return Err(PersonalServiceError::new("created rescue room is missing"));
        }
        transaction
            .execute(
                "UPDATE multiplayer_attention_schedule SET next_spawn_ms=?1 WHERE id=1",
                [next_spawn_ms],
            )
            .map_err(multiplayer_database_error)?;
        transaction.commit().map_err(multiplayer_database_error)?;
        Ok(room)
    }

    // //// 为玩家投递尚未看过的救援房 [@x380kkm 2026-09-23] ////
    pub(crate) fn attention_last_delivery_ms(
        &self,
        viewer_id: i64,
    ) -> Result<Option<i64>, PersonalServiceError> {
        self.connection.query_row("SELECT MAX(delivered_at_ms) FROM multiplayer_attention_deliveries WHERE viewer_id=?1", [viewer_id], |row| row.get(0))
            .map_err(multiplayer_database_error)
    }

    // //// 保存房间投递与玩家关联 [@x380kkm 2026-09-23] ////
    pub(crate) fn deliver_attention_room(
        &mut self,
        room_number: &str,
        viewer_id: i64,
        wall_time_ms: i64,
    ) -> Result<Option<String>, PersonalServiceError> {
        let updated = self.connection.execute(
            "INSERT OR IGNORE INTO multiplayer_attention_deliveries(room_number, viewer_id, delivered_at_ms)
             VALUES(?1, ?2, ?3)", params![room_number, viewer_id, wall_time_ms]
        ).map_err(multiplayer_database_error)?;
        if updated == 0 {
            return Ok(None);
        }
        self.connection
            .query_row(
                "SELECT attention_key FROM multiplayer_attention_rooms WHERE room_number=?1",
                [room_number],
                |row| row.get(0),
            )
            .optional()
            .map_err(multiplayer_database_error)
    }

    // //// 在铃铛有效期内固定救援入房来源 [@x380kkm 2026-09-23] ////
    pub(crate) fn accept_attention_room(
        &mut self,
        room_number: &str,
        viewer_id: i64,
        wall_time_ms: i64,
    ) -> Result<bool, PersonalServiceError> {
        self.connection
            .execute(
                "UPDATE multiplayer_attention_deliveries SET accepted=1
             WHERE room_number=?1 AND viewer_id=?2 AND (accepted=1 OR delivered_at_ms>=?3)",
                params![room_number, viewer_id, wall_time_ms.saturating_sub(30_000)],
            )
            .map(|updated| updated == 1)
            .map_err(multiplayer_database_error)
    }

    // //// 校验开战请求携带的已接受救援 key [@x380kkm 2026-09-23] ////
    pub(crate) fn attention_key_matches(
        &self,
        room_number: &str,
        viewer_id: i64,
        key: &str,
    ) -> Result<bool, PersonalServiceError> {
        self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM multiplayer_attention_rooms AS rooms
             JOIN multiplayer_attention_deliveries AS delivered USING(room_number)
             WHERE rooms.room_number=?1 AND delivered.viewer_id=?2 AND rooms.attention_key=?3 AND delivered.accepted=1)",
            params![room_number, viewer_id, key], |row| row.get(0)
        ).map_err(multiplayer_database_error)
    }
}
