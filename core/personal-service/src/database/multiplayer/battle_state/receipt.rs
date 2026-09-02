// audience: internal
// # personal-service-multiplayer-battle-receipts
//
// 该模块按账号, 动作, 战斗和请求计数保存完整响应收据.

use super::{MultiplayerBattleIdentity, MultiplayerBattleReceipt};
use crate::database::multiplayer::multiplayer_database_error;
use crate::PersonalServiceError;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::Value;

pub(super) fn read_receipt_for_identity(
    connection: &Connection,
    action: &str,
    identity: MultiplayerBattleIdentity<'_>,
) -> Result<Option<MultiplayerBattleReceipt>, PersonalServiceError> {
    read_receipt(
        connection,
        identity.account_id,
        action,
        identity.play_id,
        identity.api_count,
    )
}

// //// 按战斗身份回放联机开始收据 [@x380kkm 2026-08-29] ////
pub(super) fn read_start_receipt_for_identity(
    connection: &Connection,
    identity: MultiplayerBattleIdentity<'_>,
) -> Result<Option<MultiplayerBattleReceipt>, PersonalServiceError> {
    connection
        .query_row(
            "SELECT response_json, response_time
             FROM multiplayer_battle_action_receipts
             WHERE account_id = ?1 AND room_number = ?2
               AND action = 'start' AND play_id = ?3
             ORDER BY api_count DESC
             LIMIT 1",
            params![identity.account_id, identity.room_number, identity.play_id],
            |row| {
                Ok(MultiplayerBattleReceipt {
                    response_json: row.get(0)?,
                    response_time: row.get(1)?,
                })
            },
        )
        .optional()
        .map_err(multiplayer_database_error)
}
// //// /按战斗身份回放联机开始收据 ////

pub(super) fn read_receipt(
    connection: &Connection,
    account_id: i64,
    action: &str,
    play_id: &str,
    api_count: Option<i64>,
) -> Result<Option<MultiplayerBattleReceipt>, PersonalServiceError> {
    connection
        .query_row(
            "SELECT response_json, response_time
             FROM multiplayer_battle_action_receipts
             WHERE account_id = ?1 AND action = ?2 AND play_id = ?3 AND api_count = ?4",
            params![account_id, action, play_id, api_count_key(api_count)],
            |row| {
                Ok(MultiplayerBattleReceipt {
                    response_json: row.get(0)?,
                    response_time: row.get(1)?,
                })
            },
        )
        .optional()
        .map_err(multiplayer_database_error)
}

pub(super) fn save_receipt(
    transaction: &Transaction<'_>,
    action: &str,
    identity: MultiplayerBattleIdentity<'_>,
    receipt: &MultiplayerBattleReceipt,
) -> Result<(), PersonalServiceError> {
    transaction
        .execute(
            "INSERT INTO multiplayer_battle_action_receipts (
                 account_id, room_number, action, play_id, api_count,
                 response_time, response_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                identity.account_id,
                identity.room_number,
                action,
                identity.play_id,
                api_count_key(identity.api_count),
                receipt.response_time,
                receipt.response_json,
            ],
        )
        .map_err(multiplayer_database_error)?;
    Ok(())
}

pub(super) fn response_with_i64(
    response: &Value,
    key: &str,
    value: i64,
) -> Result<String, PersonalServiceError> {
    let mut response = response
        .as_object()
        .cloned()
        .ok_or_else(|| PersonalServiceError::new("multiplayer battle response is invalid"))?;
    response.insert(key.to_owned(), Value::from(value));
    encode_response(Value::Object(response))
}

pub(super) fn response_with_bool(
    response: &Value,
    key: &str,
    value: bool,
) -> Result<String, PersonalServiceError> {
    let mut response = response
        .as_object()
        .cloned()
        .ok_or_else(|| PersonalServiceError::new("multiplayer battle response is invalid"))?;
    response.insert(key.to_owned(), Value::Bool(value));
    encode_response(Value::Object(response))
}

fn api_count_key(api_count: Option<i64>) -> i64 {
    api_count.unwrap_or(-1)
}

fn encode_response(response: Value) -> Result<String, PersonalServiceError> {
    serde_json::to_string(&response).map_err(|error| {
        PersonalServiceError::new(format!(
            "failed to encode multiplayer battle receipt: {error}"
        ))
    })
}
