// audience: internal
// # personal-service-multiplayer-battle-state
//
// 该模块定义真人联机战斗身份, 玩家变更和动作收据的存储接口.

mod actions;
mod migration;
mod receipt;
mod start;

use super::multiplayer_database_error;
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MultiplayerBattleReceipt {
    pub(crate) response_json: String,
    pub(crate) response_time: i64,
}

#[derive(Clone, Copy)]
pub(crate) struct MultiplayerBattleIdentity<'a> {
    pub(crate) account_id: i64,
    pub(crate) room_number: &'a str,
    pub(crate) play_id: &'a str,
    pub(crate) category_id: i64,
    pub(crate) quest_id: i64,
    pub(crate) api_count: Option<i64>,
}

pub(crate) struct MultiplayerBattleStart<'a> {
    pub(crate) identity: MultiplayerBattleIdentity<'a>,
    pub(crate) use_boss_boost_point: bool,
    pub(crate) use_boost_point: bool,
    pub(crate) is_auto_start_mode: bool,
    pub(crate) response_time: i64,
    pub(crate) response_json: &'a str,
}

pub(crate) struct MultiplayerBattleContinue<'a> {
    pub(crate) identity: MultiplayerBattleIdentity<'a>,
    pub(crate) snapshot: &'a str,
    pub(crate) response_time: i64,
    pub(crate) response: &'a Value,
}

pub(crate) struct MultiplayerBattleFinish<'a> {
    pub(crate) identity: MultiplayerBattleIdentity<'a>,
    pub(crate) snapshot: &'a str,
    pub(crate) expiry_anchor_ms: i64,
    pub(crate) response_time: i64,
    pub(crate) response: &'a Value,
}

pub(crate) struct MultiplayerBattleAbort<'a> {
    pub(crate) identity: MultiplayerBattleIdentity<'a>,
    pub(crate) expiry_anchor_ms: i64,
    pub(crate) response_time: i64,
    pub(crate) response_json: &'a str,
}

// //// 创建成员战斗身份和动作收据表 [@x380kkm 2026-08-23] ////
pub(super) fn migrate(connection: &Connection) -> Result<(), PersonalServiceError> {
    migration::migrate(connection)
}
// //// /创建成员战斗身份和动作收据表 ////

impl ServiceDatabase {
    // //// 读取指定联机动作的完整响应收据 [@x380kkm 2026-08-23] ////
    pub(crate) fn multiplayer_battle_receipt(
        &self,
        account_id: i64,
        action: &str,
        play_id: &str,
        api_count: Option<i64>,
    ) -> Result<Option<MultiplayerBattleReceipt>, PersonalServiceError> {
        receipt::read_receipt(&self.connection, account_id, action, play_id, api_count)
    }
    // //// /读取指定联机动作的完整响应收据 ////

    // //// 定位账号当前联机战斗房间 [@x380kkm 2026-08-23] ////
    pub(crate) fn multiplayer_battle_room(
        &self,
        account_id: i64,
        play_id: &str,
        category_id: i64,
        quest_id: i64,
    ) -> Result<Option<String>, PersonalServiceError> {
        self.connection
            .query_row(
                "SELECT battle_players.room_number
                 FROM multiplayer_battle_players AS battle_players
                 JOIN multiplayer_room_members AS members
                   ON members.room_number = battle_players.room_number
                  AND members.account_id = battle_players.account_id
                 JOIN multiplayer_rooms AS rooms
                   ON rooms.room_number = battle_players.room_number
                 WHERE battle_players.account_id = ?1
                   AND battle_players.play_id = ?2
                   AND rooms.category_id = ?3 AND rooms.quest_id = ?4
                   AND rooms.battle_started = 1",
                params![account_id, play_id, category_id, quest_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(multiplayer_database_error)
    }
    // //// /定位账号当前联机战斗房间 ////
}
