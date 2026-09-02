// audience: internal
// # personal-service-multiplayer-battle-actions
//
// 该模块原子提交联机继续, 结算和终止状态.

use super::receipt::{
    read_receipt_for_identity, response_with_bool, response_with_i64, save_receipt,
};
use super::{
    MultiplayerBattleAbort, MultiplayerBattleContinue, MultiplayerBattleFinish,
    MultiplayerBattleIdentity, MultiplayerBattleReceipt,
};
use crate::database::multiplayer::multiplayer_database_error;
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

impl ServiceDatabase {
    // //// 原子提交联机继续费用, 计数和响应收据 [@x380kkm 2026-08-23] ////
    pub(crate) fn continue_multiplayer_battle_member(
        &mut self,
        input: MultiplayerBattleContinue<'_>,
    ) -> Result<Option<MultiplayerBattleReceipt>, PersonalServiceError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(multiplayer_database_error)?;
        if let Some(receipt) = read_receipt_for_identity(&transaction, "continue", input.identity)?
        {
            return Ok(Some(receipt));
        }
        if !battle_identity_exists(&transaction, input.identity)? {
            return Ok(None);
        }
        let continue_count = transaction
            .query_row(
                "UPDATE active_single_quests
                 SET continue_count = continue_count + 1
                 WHERE account_id = ?1 AND play_id = ?2
                   AND category = ?3 AND quest_id = ?4
                 RETURNING continue_count",
                params![
                    input.identity.account_id,
                    input.identity.play_id,
                    input.identity.category_id,
                    input.identity.quest_id,
                ],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(multiplayer_database_error)?;
        let Some(continue_count) = continue_count else {
            return Ok(None);
        };
        let updated = transaction
            .execute(
                "UPDATE multiplayer_room_members SET continue_count = ?1
                 WHERE room_number = ?2 AND account_id = ?3",
                params![
                    continue_count,
                    input.identity.room_number,
                    input.identity.account_id,
                ],
            )
            .map_err(multiplayer_database_error)?;
        if updated != 1 {
            return Ok(None);
        }
        update_player_snapshot(&transaction, input.identity.account_id, input.snapshot)?;
        let response_json = response_with_i64(input.response, "continue_count", continue_count)?;
        let receipt = MultiplayerBattleReceipt {
            response_json,
            response_time: input.response_time,
        };
        save_receipt(&transaction, "continue", input.identity, &receipt)?;
        transaction.commit().map_err(multiplayer_database_error)?;
        Ok(Some(receipt))
    }
    // //// /原子提交联机继续费用, 计数和响应收据 ////

    // //// 原子结算成员奖励并完成空房战斗 [@x380kkm 2026-08-23] ////
    pub(crate) fn finish_multiplayer_battle_member(
        &mut self,
        input: MultiplayerBattleFinish<'_>,
    ) -> Result<Option<MultiplayerBattleReceipt>, PersonalServiceError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(multiplayer_database_error)?;
        if let Some(receipt) = read_receipt_for_identity(&transaction, "finish", input.identity)? {
            return Ok(Some(receipt));
        }
        if !battle_identity_exists(&transaction, input.identity)? {
            return Ok(None);
        }
        if delete_active_quest(&transaction, input.identity)? != 1 {
            return Ok(None);
        }
        if delete_battle_player(&transaction, input.identity)? != 1 {
            return Ok(None);
        }
        let host_finished = transaction
            .query_row(
                "SELECT host_account_id = ?2
                 FROM multiplayer_rooms
                 WHERE room_number = ?1",
                params![input.identity.room_number, input.identity.account_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(multiplayer_database_error)?;
        update_player_snapshot(&transaction, input.identity.account_id, input.snapshot)?;
        let response_json = response_with_bool(input.response, "host_finished", host_finished)?;
        let receipt = MultiplayerBattleReceipt {
            response_json,
            response_time: input.response_time,
        };
        save_receipt(&transaction, "finish", input.identity, &receipt)?;
        complete_room_if_empty(
            &transaction,
            input.identity.room_number,
            input.expiry_anchor_ms,
        )?;
        transaction.commit().map_err(multiplayer_database_error)?;
        Ok(Some(receipt))
    }
    // //// /原子结算成员奖励并完成空房战斗 ////

    // //// 原子终止成员战斗并保存响应收据 [@x380kkm 2026-08-23] ////
    pub(crate) fn abort_multiplayer_battle_member(
        &mut self,
        input: MultiplayerBattleAbort<'_>,
    ) -> Result<Option<MultiplayerBattleReceipt>, PersonalServiceError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(multiplayer_database_error)?;
        if let Some(receipt) = read_receipt_for_identity(&transaction, "abort", input.identity)? {
            return Ok(Some(receipt));
        }
        if !battle_identity_exists(&transaction, input.identity)? {
            return Ok(None);
        }
        let host_account_id = transaction
            .query_row(
                "SELECT host_account_id FROM multiplayer_rooms WHERE room_number = ?1",
                params![input.identity.room_number],
                |row| row.get::<_, i64>(0),
            )
            .map_err(multiplayer_database_error)?;
        if delete_active_quest(&transaction, input.identity)? != 1 {
            return Ok(None);
        }
        if delete_battle_player(&transaction, input.identity)? != 1 {
            return Ok(None);
        }
        let receipt = MultiplayerBattleReceipt {
            response_json: input.response_json.to_owned(),
            response_time: input.response_time,
        };
        save_receipt(&transaction, "abort", input.identity, &receipt)?;
        if host_account_id == input.identity.account_id {
            transaction
                .execute(
                    "DELETE FROM active_single_quests
                     WHERE EXISTS (
                         SELECT 1 FROM multiplayer_battle_players AS battle_players
                         WHERE battle_players.room_number = ?1
                           AND battle_players.account_id = active_single_quests.account_id
                           AND battle_players.play_id = active_single_quests.play_id
                     )",
                    params![input.identity.room_number],
                )
                .map_err(multiplayer_database_error)?;
            transaction
                .execute(
                    "DELETE FROM multiplayer_rooms WHERE room_number = ?1",
                    params![input.identity.room_number],
                )
                .map_err(multiplayer_database_error)?;
        } else {
            transaction
                .execute(
                    "DELETE FROM multiplayer_room_members
                     WHERE room_number = ?1 AND account_id = ?2",
                    params![input.identity.room_number, input.identity.account_id],
                )
                .map_err(multiplayer_database_error)?;
            complete_room_if_empty(
                &transaction,
                input.identity.room_number,
                input.expiry_anchor_ms,
            )?;
        }
        transaction.commit().map_err(multiplayer_database_error)?;
        Ok(Some(receipt))
    }
    // //// /原子终止成员战斗并保存响应收据 ////
}

fn battle_identity_exists(
    connection: &Connection,
    identity: MultiplayerBattleIdentity<'_>,
) -> Result<bool, PersonalServiceError> {
    connection
        .query_row(
            "SELECT EXISTS (
                 SELECT 1
                 FROM multiplayer_battle_players AS battle_players
                 JOIN multiplayer_room_members AS members
                   ON members.room_number = battle_players.room_number
                  AND members.account_id = battle_players.account_id
                 JOIN multiplayer_rooms AS rooms
                   ON rooms.room_number = battle_players.room_number
                 WHERE battle_players.account_id = ?1
                   AND battle_players.room_number = ?2
                   AND battle_players.play_id = ?3
                   AND rooms.category_id = ?4 AND rooms.quest_id = ?5
                   AND rooms.battle_started = 1
             )",
            params![
                identity.account_id,
                identity.room_number,
                identity.play_id,
                identity.category_id,
                identity.quest_id,
            ],
            |row| row.get(0),
        )
        .map_err(multiplayer_database_error)
}

fn update_player_snapshot(
    transaction: &Transaction<'_>,
    account_id: i64,
    snapshot: &str,
) -> Result<(), PersonalServiceError> {
    let updated = transaction
        .execute(
            "UPDATE player_snapshots SET data_json = ?1 WHERE account_id = ?2",
            params![snapshot, account_id],
        )
        .map_err(multiplayer_database_error)?;
    if updated != 1 {
        return Err(PersonalServiceError::new(
            "failed to update the CN player snapshot during multiplayer battle",
        ));
    }
    Ok(())
}

fn delete_active_quest(
    transaction: &Transaction<'_>,
    identity: MultiplayerBattleIdentity<'_>,
) -> Result<usize, PersonalServiceError> {
    transaction
        .execute(
            "DELETE FROM active_single_quests
             WHERE account_id = ?1 AND play_id = ?2 AND category = ?3 AND quest_id = ?4",
            params![
                identity.account_id,
                identity.play_id,
                identity.category_id,
                identity.quest_id,
            ],
        )
        .map_err(multiplayer_database_error)
}

fn delete_battle_player(
    transaction: &Transaction<'_>,
    identity: MultiplayerBattleIdentity<'_>,
) -> Result<usize, PersonalServiceError> {
    transaction
        .execute(
            "DELETE FROM multiplayer_battle_players
             WHERE account_id = ?1 AND room_number = ?2 AND play_id = ?3",
            params![identity.account_id, identity.room_number, identity.play_id,],
        )
        .map_err(multiplayer_database_error)
}

fn complete_room_if_empty(
    transaction: &Transaction<'_>,
    room_number: &str,
    expiry_anchor_ms: i64,
) -> Result<(), PersonalServiceError> {
    transaction
        .execute(
            "UPDATE multiplayer_rooms
             SET raising_state = 1, battle_started = 0, lobby_started = 0,
                 play_id = NULL, expiry_anchor_ms = ?2
             WHERE room_number = ?1
               AND NOT EXISTS (
                   SELECT 1 FROM multiplayer_battle_players
                   WHERE room_number = ?1
               )",
            params![room_number, expiry_anchor_ms],
        )
        .map_err(multiplayer_database_error)?;
    Ok(())
}
