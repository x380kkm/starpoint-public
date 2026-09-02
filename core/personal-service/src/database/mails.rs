// audience: internal
// # personal-service-mail-storage
//
// 该模块保存个人服务的 CN 玩家邮件和奖励收藏. 邮件领取和玩家快照更新使用同一 SQLite 事务.

use super::receive_history::{insert_receive_history_in_transaction, mail_reward_history_entries};
use super::ServiceDatabase;
use crate::PersonalServiceError;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const MAX_MAIL_PAGE_SIZE: i64 = 100;
pub(crate) const MAX_MAIL_PAGE: i64 = 1_000_000;
pub(crate) const MAX_MAIL_REWARD_ENTRIES: usize = 100;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MailReward {
    #[serde(rename = "itemList", alias = "item_list", default)]
    pub(crate) item_list: BTreeMap<String, i64>,
    #[serde(rename = "equipmentList", alias = "equipment_list", default)]
    pub(crate) equipment_list: BTreeMap<String, i64>,
    #[serde(rename = "characterList", alias = "character_list", default)]
    pub(crate) character_list: Vec<i64>,
    #[serde(rename = "freeMana", alias = "free_mana", default)]
    pub(crate) free_mana: i64,
    #[serde(rename = "paidMana", alias = "paid_mana", default)]
    pub(crate) paid_mana: i64,
    #[serde(rename = "freeVmoney", alias = "free_vmoney", default)]
    pub(crate) free_vmoney: i64,
    #[serde(rename = "vmoney", alias = "paid_vmoney", default)]
    pub(crate) vmoney: i64,
    #[serde(rename = "expPool", alias = "exp_pool", default)]
    pub(crate) exp_pool: i64,
    #[serde(rename = "starCrumb", alias = "star_crumb", default)]
    pub(crate) star_crumb: i64,
    #[serde(rename = "bondToken", alias = "bond_token", default)]
    pub(crate) bond_token: i64,
    #[serde(rename = "bossBoostPoint", alias = "boss_boost_point", default)]
    pub(crate) boss_boost_point: i64,
    #[serde(rename = "boostPoint", alias = "boost_point", default)]
    pub(crate) boost_point: i64,
    #[serde(rename = "rankPoint", alias = "rank_point", default)]
    pub(crate) rank_point: i64,
}

pub(crate) struct CreateMailInput {
    pub(crate) account_id: i64,
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) sender: String,
    pub(crate) rewards: MailReward,
    pub(crate) expires_at: Option<i64>,
    pub(crate) created_at: i64,
}

#[derive(Debug)]
pub(crate) struct StoredMail {
    pub(crate) id: i64,
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) sender: String,
    pub(crate) rewards: MailReward,
    pub(crate) created_at: i64,
    pub(crate) expires_at: Option<i64>,
    pub(crate) received_at: Option<i64>,
}

pub(crate) struct MailPage {
    pub(crate) mails: Vec<StoredMail>,
    pub(crate) total: i64,
}

#[derive(Default)]
pub(crate) struct MailClaimReward {
    pub(crate) item_list: BTreeMap<String, i64>,
    pub(crate) equipment_list: Vec<Value>,
    pub(crate) character_list: Vec<Value>,
}

pub(crate) struct MailClaimResult {
    pub(crate) mail_ids: Vec<i64>,
    pub(crate) reward: MailClaimReward,
    pub(crate) expired_mail_count: i64,
    pub(crate) total_count: i64,
    pub(crate) remaining_count: i64,
}

// //// 创建 CN 玩家邮件表 [@x380kkm 2026-07-24] ////
pub(super) fn migrate(connection: &Connection) -> Result<(), PersonalServiceError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS player_mails (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 account_id INTEGER NOT NULL,
                 title TEXT NOT NULL CHECK (length(trim(title)) BETWEEN 1 AND 200),
                 body TEXT NOT NULL CHECK (length(trim(body)) BETWEEN 1 AND 5000),
                 sender TEXT NOT NULL CHECK (length(trim(sender)) BETWEEN 1 AND 100),
                 rewards_json TEXT NOT NULL,
                 created_at INTEGER NOT NULL CHECK (created_at >= 0),
                 expires_at INTEGER CHECK (expires_at IS NULL OR expires_at >= 0),
                 received_at INTEGER CHECK (received_at IS NULL OR received_at >= 0),
                 FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS player_mails_pending_index
                 ON player_mails (account_id, received_at, expires_at, id);
             CREATE TABLE IF NOT EXISTS mail_reward_favorites (
                 reward_key TEXT PRIMARY KEY NOT NULL
                     CHECK (length(reward_key) BETWEEN 1 AND 100)
             );
             CREATE TABLE IF NOT EXISTS player_reward_mail_events (
                 account_id INTEGER NOT NULL,
                 event_key TEXT NOT NULL CHECK (length(event_key) BETWEEN 1 AND 200),
                 mail_id INTEGER NOT NULL UNIQUE,
                 PRIMARY KEY (account_id, event_key),
                 FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE,
                 FOREIGN KEY (mail_id) REFERENCES player_mails (id) ON DELETE CASCADE
             );",
        )
        .map_err(mail_database_error)
}
// //// /创建 CN 玩家邮件表 ////

impl ServiceDatabase {
    // //// 创建管理员发放的 CN 玩家邮件 [@x380kkm 2026-07-24] ////
    pub(crate) fn create_mail(
        &mut self,
        input: &CreateMailInput,
    ) -> Result<StoredMail, PersonalServiceError> {
        let rewards_json = serde_json::to_string(&input.rewards).map_err(|error| {
            PersonalServiceError::new(format!("failed to encode mail rewards: {error}"))
        })?;
        self.connection
            .execute(
                "INSERT INTO player_mails (
                     account_id, title, body, sender, rewards_json,
                     created_at, expires_at, received_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)",
                params![
                    input.account_id,
                    input.title,
                    input.body,
                    input.sender,
                    rewards_json,
                    input.created_at,
                    input.expires_at,
                ],
            )
            .map_err(mail_database_error)?;
        self.get_mail(self.connection.last_insert_rowid())?
            .ok_or_else(|| PersonalServiceError::new("created mail is missing"))
    }

    // //// 原子保存玩家快照并幂等投递奖励邮件 [@x380kkm 2026-08-22] ////
    pub(crate) fn deliver_reward_mail_with_snapshot_once(
        &mut self,
        input: &CreateMailInput,
        player_data: &str,
        event_key: &str,
    ) -> Result<bool, PersonalServiceError> {
        let transaction = self.connection.transaction().map_err(mail_database_error)?;
        let delivered = transaction
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM player_reward_mail_events
                     WHERE account_id = ?1 AND event_key = ?2
                 )",
                params![input.account_id, event_key],
                |row| row.get::<_, bool>(0),
            )
            .map_err(mail_database_error)?;
        if delivered {
            return Ok(false);
        }
        let rewards_json = serde_json::to_string(&input.rewards).map_err(|error| {
            PersonalServiceError::new(format!("failed to encode mail rewards: {error}"))
        })?;
        transaction
            .execute(
                "INSERT INTO player_mails (
                     account_id, title, body, sender, rewards_json,
                     created_at, expires_at, received_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)",
                params![
                    input.account_id,
                    input.title,
                    input.body,
                    input.sender,
                    rewards_json,
                    input.created_at,
                    input.expires_at,
                ],
            )
            .map_err(mail_database_error)?;
        let mail_id = transaction.last_insert_rowid();
        transaction
            .execute(
                "INSERT INTO player_snapshots (account_id, data_json) VALUES (?1, ?2)
                 ON CONFLICT(account_id) DO UPDATE SET data_json = excluded.data_json",
                params![input.account_id, player_data],
            )
            .map_err(mail_database_error)?;
        transaction
            .execute(
                "INSERT INTO player_reward_mail_events (account_id, event_key, mail_id)
                 VALUES (?1, ?2, ?3)",
                params![input.account_id, event_key, mail_id],
            )
            .map_err(mail_database_error)?;
        transaction.commit().map_err(mail_database_error)?;
        Ok(true)
    }
    // //// /原子保存玩家快照并幂等投递奖励邮件 ////

    pub(crate) fn list_mails(
        &self,
        account_id: i64,
        page: i64,
        page_size: i64,
        _now: i64,
    ) -> Result<MailPage, PersonalServiceError> {
        let offset = (page - 1).checked_mul(page_size).ok_or_else(|| {
            PersonalServiceError::new("mail page offset exceeds the supported range")
        })?;
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, title, body, sender, rewards_json,
                        created_at, expires_at, received_at
                 FROM player_mails
                 WHERE account_id = ?1
                 ORDER BY id DESC LIMIT ?2 OFFSET ?3",
            )
            .map_err(mail_database_error)?;
        let raw_mails = statement
            .query_map(params![account_id, page_size, offset], read_raw_mail)
            .map_err(mail_database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(mail_database_error)?;
        let mails = raw_mails
            .into_iter()
            .map(decode_mail)
            .collect::<Result<Vec<_>, _>>()?;
        let total = self
            .connection
            .query_row(
                "SELECT COUNT(*) FROM player_mails WHERE account_id = ?1",
                params![account_id],
                |row| row.get(0),
            )
            .map_err(mail_database_error)?;
        Ok(MailPage { mails, total })
    }

    // //// 判断账号是否存在可领取邮件 [@x380kkm 2026-08-24] ////
    pub(crate) fn has_unreceived_mail(
        &self,
        account_id: i64,
        now: i64,
    ) -> Result<bool, PersonalServiceError> {
        let count = self
            .connection
            .query_row(
                "SELECT COUNT(*) FROM player_mails
                 WHERE account_id = ?1 AND received_at IS NULL
                   AND (expires_at IS NULL OR expires_at > ?2)",
                params![account_id, now],
                |row| row.get::<_, i64>(0),
            )
            .map_err(mail_database_error)?;
        Ok(count > 0)
    }
    // //// /判断账号是否存在可领取邮件 ////

    // //// 原子领取 CN 玩家邮件和奖励 [@x380kkm 2026-07-24] ////
    pub(crate) fn claim_mails<F>(
        &mut self,
        account_id: i64,
        mail_ids: Option<&[i64]>,
        now: i64,
        mut apply_rewards: F,
    ) -> Result<MailClaimResult, PersonalServiceError>
    where
        F: FnMut(&mut Value, &MailReward) -> Result<MailClaimReward, PersonalServiceError>,
    {
        let transaction = self.connection.transaction().map_err(mail_database_error)?;
        let ids = match mail_ids {
            Some(ids) => ids.to_vec(),
            None => list_pending_ids(&transaction, account_id)?,
        };
        let mut player_data = None;
        let mut result = MailClaimResult {
            mail_ids: Vec::new(),
            reward: MailClaimReward::default(),
            expired_mail_count: 0,
            total_count: 0,
            remaining_count: 0,
        };
        for mail_id in ids {
            let Some(raw_mail) = read_raw_mail_by_id(&transaction, account_id, mail_id)? else {
                continue;
            };
            if raw_mail.received_at.is_some() {
                continue;
            }
            let changed = transaction
                .execute(
                    "UPDATE player_mails SET received_at = ?1
                     WHERE id = ?2 AND account_id = ?3 AND received_at IS NULL",
                    params![now, mail_id, account_id],
                )
                .map_err(mail_database_error)?;
            if changed != 1 {
                continue;
            }
            result.mail_ids.push(mail_id);
            if raw_mail
                .expires_at
                .is_some_and(|expires_at| expires_at <= now)
            {
                result.expired_mail_count += 1;
                continue;
            }
            if player_data.is_none() {
                let data_json: String = transaction
                    .query_row(
                        "SELECT data_json FROM player_snapshots WHERE account_id = ?1",
                        params![account_id],
                        |row| row.get(0),
                    )
                    .map_err(mail_database_error)?;
                player_data = Some(serde_json::from_str::<Value>(&data_json).map_err(|error| {
                    PersonalServiceError::new(format!(
                        "failed to decode player mail snapshot: {error}"
                    ))
                })?);
            }
            let reward = decode_mail(raw_mail)?.rewards;
            let applied = apply_rewards(
                player_data.as_mut().expect("player data is loaded"),
                &reward,
            )?;
            insert_receive_history_in_transaction(
                &transaction,
                account_id,
                &format!("mail:{mail_id}"),
                now,
                &mail_reward_history_entries(&reward),
            )?;
            merge_claim_reward(&mut result.reward, applied)?;
        }
        if let Some(player_data) = player_data {
            let data_json = serde_json::to_string(&player_data).map_err(|error| {
                PersonalServiceError::new(format!("failed to encode player mail snapshot: {error}"))
            })?;
            let updated = transaction
                .execute(
                    "UPDATE player_snapshots SET data_json = ?1 WHERE account_id = ?2",
                    params![data_json, account_id],
                )
                .map_err(mail_database_error)?;
            if updated != 1 {
                return Err(PersonalServiceError::new(
                    "player snapshot disappeared during mail claim",
                ));
            }
        }
        result.remaining_count = transaction
            .query_row(
                "SELECT COUNT(*) FROM player_mails
                 WHERE account_id = ?1 AND received_at IS NULL
                   AND (expires_at IS NULL OR expires_at > ?2)",
                params![account_id, now],
                |row| row.get(0),
            )
            .map_err(mail_database_error)?;
        result.total_count = transaction
            .query_row(
                "SELECT COUNT(*) FROM player_mails WHERE account_id = ?1",
                params![account_id],
                |row| row.get(0),
            )
            .map_err(mail_database_error)?;
        transaction.commit().map_err(mail_database_error)?;
        Ok(result)
    }
    // //// /原子领取 CN 玩家邮件和奖励 ////

    fn get_mail(&self, mail_id: i64) -> Result<Option<StoredMail>, PersonalServiceError> {
        self.connection
            .query_row(
                "SELECT id, title, body, sender, rewards_json,
                        created_at, expires_at, received_at
                 FROM player_mails WHERE id = ?1",
                params![mail_id],
                read_raw_mail,
            )
            .optional()
            .map_err(mail_database_error)?
            .map(decode_mail)
            .transpose()
    }

    // //// 保存本地邮件奖励收藏 [@x380kkm 2026-08-20] ////
    pub(crate) fn mail_reward_favorite_keys(
        &self,
    ) -> Result<BTreeSet<String>, PersonalServiceError> {
        let mut statement = self
            .connection
            .prepare("SELECT reward_key FROM mail_reward_favorites ORDER BY reward_key")
            .map_err(mail_database_error)?;
        let favorites = statement
            .query_map([], |row| row.get(0))
            .map_err(mail_database_error)?
            .collect::<Result<BTreeSet<_>, _>>()
            .map_err(mail_database_error)?;
        Ok(favorites)
    }

    pub(crate) fn set_mail_reward_favorite(
        &mut self,
        reward_key: &str,
        favorite: bool,
    ) -> Result<(), PersonalServiceError> {
        if favorite {
            self.connection
                .execute(
                    "INSERT OR IGNORE INTO mail_reward_favorites (reward_key) VALUES (?1)",
                    params![reward_key],
                )
                .map_err(mail_database_error)?;
        } else {
            self.connection
                .execute(
                    "DELETE FROM mail_reward_favorites WHERE reward_key = ?1",
                    params![reward_key],
                )
                .map_err(mail_database_error)?;
        }
        Ok(())
    }
    // //// /保存本地邮件奖励收藏 ////
}

struct RawMail {
    id: i64,
    title: String,
    body: String,
    sender: String,
    rewards_json: String,
    created_at: i64,
    expires_at: Option<i64>,
    received_at: Option<i64>,
}

fn read_raw_mail(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawMail> {
    Ok(RawMail {
        id: row.get(0)?,
        title: row.get(1)?,
        body: row.get(2)?,
        sender: row.get(3)?,
        rewards_json: row.get(4)?,
        created_at: row.get(5)?,
        expires_at: row.get(6)?,
        received_at: row.get(7)?,
    })
}

fn read_raw_mail_by_id(
    transaction: &Transaction<'_>,
    account_id: i64,
    mail_id: i64,
) -> Result<Option<RawMail>, PersonalServiceError> {
    transaction
        .query_row(
            "SELECT id, title, body, sender, rewards_json,
                    created_at, expires_at, received_at
             FROM player_mails WHERE id = ?1 AND account_id = ?2",
            params![mail_id, account_id],
            read_raw_mail,
        )
        .optional()
        .map_err(mail_database_error)
}

fn decode_mail(raw: RawMail) -> Result<StoredMail, PersonalServiceError> {
    let rewards = serde_json::from_str(&raw.rewards_json).map_err(|error| {
        PersonalServiceError::new(format!("stored mail rewards are invalid: {error}"))
    })?;
    Ok(StoredMail {
        id: raw.id,
        title: raw.title,
        body: raw.body,
        sender: raw.sender,
        rewards,
        created_at: raw.created_at,
        expires_at: raw.expires_at,
        received_at: raw.received_at,
    })
}

fn list_pending_ids(
    transaction: &Transaction<'_>,
    account_id: i64,
) -> Result<Vec<i64>, PersonalServiceError> {
    let mut statement = transaction
        .prepare(
            "SELECT id FROM player_mails
             WHERE account_id = ?1 AND received_at IS NULL ORDER BY id",
        )
        .map_err(mail_database_error)?;
    let ids = statement
        .query_map(params![account_id], |row| row.get(0))
        .map_err(mail_database_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(mail_database_error)?;
    Ok(ids)
}

fn merge_claim_reward(
    result: &mut MailClaimReward,
    applied: MailClaimReward,
) -> Result<(), PersonalServiceError> {
    for (item_id, count) in applied.item_list {
        let current = result.item_list.get(&item_id).copied().unwrap_or_default();
        let total = current.checked_add(count).ok_or_else(|| {
            PersonalServiceError::new("mail item count exceeds the supported range")
        })?;
        result.item_list.insert(item_id, total);
    }
    result.equipment_list.extend(applied.equipment_list);
    result.character_list.extend(applied.character_list);
    Ok(())
}

fn mail_database_error(error: rusqlite::Error) -> PersonalServiceError {
    PersonalServiceError::new(format!("failed to access CN mail storage: {error}"))
}
