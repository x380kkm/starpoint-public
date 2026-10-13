// audience: internal
// # personal-service-reward-delivery-storage
// 转换余量使用唯一投递标识随快照事务保存, 实际到账记录与邮件投递共同提交.

use super::receive_history::{insert_receive_history_in_transaction, mail_reward_history_entries};
use crate::cn_reward_overflow::{Delivery, DELIVERIES_FIELD};
use crate::PersonalServiceError;
use rusqlite::{params, Connection};
use serde_json::Value;
use std::borrow::Cow;

// //// 在调用方事务内投递转换余量并清理快照中的投递数据 [@x380kkm 2026-09-26] ////
pub(super) fn prepare_snapshot<'a>(
    connection: &Connection,
    account_id: i64,
    data: &'a str,
) -> Result<Cow<'a, str>, PersonalServiceError> {
    if !data.contains(DELIVERIES_FIELD) {
        return Ok(Cow::Borrowed(data));
    }
    let mut snapshot: Value =
        serde_json::from_str(data).map_err(|error| PersonalServiceError::new(error.to_string()))?;
    let root = snapshot
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("CN reward snapshot is invalid"))?;
    crate::cn_reward_overflow::reconcile_capacity(root)?;
    let Some(pending) = root.remove(DELIVERIES_FIELD) else {
        return Ok(Cow::Borrowed(data));
    };
    let deliveries: Vec<Delivery> = serde_json::from_value(pending)
        .map_err(|error| PersonalServiceError::new(error.to_string()))?;
    for delivery in &deliveries {
        let direct = delivery.credited_reward();
        let mut history = mail_reward_history_entries(&direct);
        for entry in &mut history {
            entry.subject = Some(delivery.conversion.subject.clone());
            entry.description = Some(delivery.conversion.description.clone());
        }
        insert_receive_history_in_transaction(
            connection,
            account_id,
            &delivery.event_key,
            delivery.conversion.created_at,
            &history,
        )?;
        if !delivery.has_mail() {
            continue;
        }
        let exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM player_reward_mail_events WHERE account_id=?1 AND event_key=?2)",
            params![account_id, delivery.event_key], |row| row.get(0),
        ).map_err(super::database_error)?;
        if exists {
            continue;
        }
        let rewards = serde_json::to_string(&delivery.mail)
            .map_err(|error| PersonalServiceError::new(error.to_string()))?;
        connection.execute(
            "INSERT INTO player_mails (account_id,title,body,sender,rewards_json,created_at,expires_at,received_at)
             VALUES (?1,'持有上限奖励','超出持有上限的奖励存入邮件.','系统',?2,?3,NULL,NULL)",
            params![account_id, rewards, delivery.conversion.created_at],
        ).map_err(super::database_error)?;
        connection.execute(
            "INSERT INTO player_reward_mail_events (account_id,event_key,mail_id) VALUES (?1,?2,?3)",
            params![account_id, delivery.event_key, connection.last_insert_rowid()],
        ).map_err(super::database_error)?;
    }
    Ok(Cow::Owned(serde_json::to_string(&snapshot).map_err(
        |error| PersonalServiceError::new(error.to_string()),
    )?))
}
// //// /在调用方事务内投递转换余量并清理快照中的投递数据 ////
