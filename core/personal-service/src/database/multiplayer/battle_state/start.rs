// audience: internal
// # personal-service-multiplayer-battle-start
//
// 该模块按真人成员保存独立的客户端战斗身份和选项.

use super::receipt::{read_start_receipt_for_identity, save_receipt};
use super::{MultiplayerBattleIdentity, MultiplayerBattleReceipt, MultiplayerBattleStart};
use crate::database::multiplayer::multiplayer_database_error;
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use rusqlite::{params, Connection, OptionalExtension};

impl ServiceDatabase {
    // //// 原子保存成员自己的联机战斗身份 [@x380kkm 2026-08-23] ////
    pub(crate) fn start_multiplayer_battle_member(
        &mut self,
        input: MultiplayerBattleStart<'_>,
    ) -> Result<Option<MultiplayerBattleReceipt>, PersonalServiceError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(multiplayer_database_error)?;
        if let Some(receipt) = read_start_receipt_for_identity(&transaction, input.identity)? {
            return Ok(Some(receipt));
        }
        let battle_started = transaction
            .query_row(
                "SELECT rooms.battle_started
                 FROM multiplayer_rooms AS rooms
                 JOIN multiplayer_room_members AS members
                   ON members.room_number = rooms.room_number
                 WHERE rooms.room_number = ?1 AND members.account_id = ?2
                   AND rooms.category_id = ?3 AND rooms.quest_id = ?4",
                params![
                    input.identity.room_number,
                    input.identity.account_id,
                    input.identity.category_id,
                    input.identity.quest_id,
                ],
                |row| row.get::<_, bool>(0),
            )
            .optional()
            .map_err(multiplayer_database_error)?;
        let Some(battle_started) = battle_started else {
            return Ok(None);
        };
        if !battle_started {
            let updated = transaction
                .execute(
                    "UPDATE multiplayer_rooms
                     SET raising_state = 4, battle_started = 1, play_id = NULL
                     WHERE room_number = ?1 AND battle_started = 0",
                    params![input.identity.room_number],
                )
                .map_err(multiplayer_database_error)?;
            if updated != 1 {
                return Ok(None);
            }
            transaction
                .execute(
                    "INSERT OR IGNORE INTO multiplayer_battle_players (
                         account_id, room_number, play_id
                     )
                     SELECT members.account_id, members.room_number, NULL
                     FROM multiplayer_room_members AS members
                     WHERE members.room_number = ?1",
                    params![input.identity.room_number],
                )
                .map_err(multiplayer_database_error)?;
        }
        let stored_play_id = transaction
            .query_row(
                "SELECT play_id FROM multiplayer_battle_players
                 WHERE room_number = ?1 AND account_id = ?2",
                params![input.identity.room_number, input.identity.account_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(multiplayer_database_error)?;
        let Some(stored_play_id) = stored_play_id else {
            return Ok(None);
        };
        if stored_play_id
            .as_deref()
            .is_some_and(|play_id| play_id != input.identity.play_id)
        {
            return Ok(None);
        }
        if stored_play_id.is_none() {
            let updated = transaction
                .execute(
                    "UPDATE multiplayer_battle_players SET play_id = ?1
                     WHERE room_number = ?2 AND account_id = ?3 AND play_id IS NULL",
                    params![
                        input.identity.play_id,
                        input.identity.room_number,
                        input.identity.account_id,
                    ],
                )
                .map_err(multiplayer_database_error)?;
            if updated != 1 {
                return Ok(None);
            }
            transaction
                .execute(
                    "INSERT INTO active_single_quests (
                         account_id, play_id, quest_id, category,
                         use_boss_boost_point, use_boost_point,
                         is_auto_start_mode, continue_count
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
                        input.identity.account_id,
                        input.identity.play_id,
                        input.identity.quest_id,
                        input.identity.category_id,
                        input.use_boss_boost_point,
                        input.use_boost_point,
                        input.is_auto_start_mode,
                    ],
                )
                .map_err(multiplayer_database_error)?;
            let updated = transaction
                .execute(
                    "UPDATE multiplayer_room_members SET continue_count = 0
                     WHERE room_number = ?1 AND account_id = ?2",
                    params![input.identity.room_number, input.identity.account_id],
                )
                .map_err(multiplayer_database_error)?;
            if updated != 1 {
                return Ok(None);
            }
        } else if !active_quest_matches(
            &transaction,
            input.identity,
            input.use_boss_boost_point,
            input.use_boost_point,
            input.is_auto_start_mode,
        )? {
            return Ok(None);
        }
        let receipt = MultiplayerBattleReceipt {
            response_json: input.response_json.to_owned(),
            response_time: input.response_time,
        };
        save_receipt(&transaction, "start", input.identity, &receipt)?;
        transaction.commit().map_err(multiplayer_database_error)?;
        Ok(Some(receipt))
    }
    // //// /原子保存成员自己的联机战斗身份 ////
}

fn active_quest_matches(
    connection: &Connection,
    identity: MultiplayerBattleIdentity<'_>,
    use_boss_boost_point: bool,
    use_boost_point: bool,
    is_auto_start_mode: bool,
) -> Result<bool, PersonalServiceError> {
    connection
        .query_row(
            "SELECT EXISTS (
                 SELECT 1 FROM active_single_quests
                 WHERE account_id = ?1 AND play_id = ?2
                   AND category = ?3 AND quest_id = ?4
                   AND use_boss_boost_point = ?5 AND use_boost_point = ?6
                   AND is_auto_start_mode = ?7
             )",
            params![
                identity.account_id,
                identity.play_id,
                identity.category_id,
                identity.quest_id,
                use_boss_boost_point,
                use_boost_point,
                is_auto_start_mode,
            ],
            |row| row.get(0),
        )
        .map_err(multiplayer_database_error)
}
