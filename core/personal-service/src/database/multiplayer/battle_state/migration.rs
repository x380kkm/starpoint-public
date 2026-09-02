// audience: internal
// # personal-service-multiplayer-battle-state-migration
//
// 该模块创建成员战斗身份和动作收据表, 并迁移房间级旧身份.

use crate::database::multiplayer::multiplayer_database_error;
use crate::PersonalServiceError;
use rusqlite::Connection;

pub(super) fn migrate(connection: &Connection) -> Result<(), PersonalServiceError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS multiplayer_battle_players (
                 account_id INTEGER PRIMARY KEY,
                 room_number TEXT NOT NULL,
                 play_id TEXT,
                 UNIQUE (room_number, account_id),
                 FOREIGN KEY (room_number, account_id)
                     REFERENCES multiplayer_room_members (room_number, account_id)
                     ON DELETE CASCADE,
                 FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS multiplayer_battle_players_room
                 ON multiplayer_battle_players (room_number);
             CREATE TABLE IF NOT EXISTS multiplayer_battle_action_receipts (
                 account_id INTEGER NOT NULL,
                 room_number TEXT NOT NULL,
                 action TEXT NOT NULL
                     CHECK (action IN ('start', 'continue', 'finish', 'abort')),
                 play_id TEXT NOT NULL,
                 api_count INTEGER NOT NULL DEFAULT -1,
                 response_time INTEGER NOT NULL CHECK (response_time >= 0),
                 response_json TEXT NOT NULL,
                 PRIMARY KEY (account_id, action, play_id, api_count),
                 FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS multiplayer_battle_receipts_room
                 ON multiplayer_battle_action_receipts (room_number, action);",
        )
        .map_err(multiplayer_database_error)?;
    connection
        .execute_batch(
            "INSERT OR IGNORE INTO multiplayer_battle_players (
                 account_id, room_number, play_id
             )
             SELECT members.account_id,
                    rooms.room_number,
                    quests.play_id
             FROM multiplayer_rooms AS rooms
             JOIN multiplayer_room_members AS members
               ON members.room_number = rooms.room_number
             LEFT JOIN active_single_quests AS quests
               ON quests.account_id = members.account_id
              AND quests.category = rooms.category_id
              AND quests.quest_id = rooms.quest_id
             WHERE rooms.battle_started = 1 AND rooms.play_id IS NOT NULL;
             UPDATE multiplayer_rooms SET play_id = NULL WHERE battle_started = 1;",
        )
        .map_err(multiplayer_database_error)?;
    Ok(())
}
