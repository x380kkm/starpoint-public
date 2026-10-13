// audience: internal
// # personal-service-reward-history-storage
// 管理台从领取表, 邮件表和存档订单读取记录, 分别使用记录 ID 或订单顺序分页.

use super::mails::{decode_mail, read_raw_mail};
use super::{ServiceDatabase, StoredMail, StoredReceiveHistory};
use crate::PersonalServiceError;
use rusqlite::params;

const PAGE_SIZE: usize = 50;

pub(crate) struct RewardHistoryRecord {
    pub(crate) id: i64,
    pub(crate) source: String,
    pub(crate) reward: StoredReceiveHistory,
}

pub(crate) struct RewardHistoryPage {
    pub(crate) entries: Vec<RewardHistoryRecord>,
    pub(crate) next_before_id: Option<i64>,
}

pub(crate) struct RewardDeliveryPage {
    pub(crate) mails: Vec<StoredMail>,
    pub(crate) next_before_id: Option<i64>,
    pub(crate) total: i64,
}

impl ServiceDatabase {
    // //// 从当前存档订单读取已发放的商品记录 [@x380kkm 2026-09-23] ////
    pub(crate) fn product_history_page(
        &self,
        account_id: i64,
        before_id: Option<i64>,
    ) -> Result<crate::cn_payment::ProductHistoryPage, PersonalServiceError> {
        let serialized: String = self.connection.query_row(
            "SELECT data_json FROM player_snapshots WHERE account_id=?1",
            [account_id],
            |row| row.get(0),
        ).map_err(history_error)?;
        let data = crate::cn_tutorial::decode_player_data(&serialized)?;
        let root = data.as_object().ok_or_else(|| {
            PersonalServiceError::new("stored CN player snapshot is invalid")
        })?;
        crate::cn_payment::product_history_page(root, before_id, PAGE_SIZE)
    }
    // //// /从当前存档订单读取已发放的商品记录 ////

    // //// 按存档账号和记录游标查询到账资产 [@x380kkm 2026-09-22] ////
    pub(crate) fn reward_history_page(
        &self,
        account_id: i64,
        before_id: Option<i64>,
    ) -> Result<RewardHistoryPage, PersonalServiceError> {
        let mut statement = self.connection.prepare(
            "SELECT id, event_key, created_at, description, number, reason_id, subject, kind, type_id
             FROM player_receive_history
             WHERE account_id = ?1 AND (?2 IS NULL OR id < ?2)
             ORDER BY id DESC LIMIT ?3",
        ).map_err(history_error)?;
        let mut entries = statement
            .query_map(params![account_id, before_id, PAGE_SIZE + 1], |row| {
                let event_key: String = row.get(1)?;
                Ok(RewardHistoryRecord {
                    id: row.get(0)?,
                    source: event_key.split(':').next().unwrap_or_default().to_owned(),
                    reward: StoredReceiveHistory {
                        create_time: row.get(2)?,
                        description: row.get(3)?,
                        number: row.get(4)?,
                        reason_id: row.get(5)?,
                        subject: row.get(6)?,
                        kind: row.get(7)?,
                        type_id: row.get(8)?,
                    },
                })
            })
            .map_err(history_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(history_error)?;
        let has_more = entries.len() > PAGE_SIZE;
        entries.truncate(PAGE_SIZE);
        let next_before_id = entries.last().filter(|_| has_more).map(|entry| entry.id);
        Ok(RewardHistoryPage {
            entries,
            next_before_id,
        })
    }
    // //// /按存档账号和记录游标查询到账资产 ////

    // //// 按邮件游标读取存档的全部投递状态 [@x380kkm 2026-09-22] ////
    pub(crate) fn reward_delivery_page(
        &self,
        account_id: i64,
        before_id: Option<i64>,
    ) -> Result<RewardDeliveryPage, PersonalServiceError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, title, body, sender, rewards_json, created_at, expires_at, received_at
             FROM player_mails
             WHERE account_id = ?1 AND (?2 IS NULL OR id < ?2)
             ORDER BY id DESC LIMIT ?3",
            )
            .map_err(history_error)?;
        let raw = statement
            .query_map(params![account_id, before_id, PAGE_SIZE + 1], read_raw_mail)
            .map_err(history_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(history_error)?;
        let mut mails = raw
            .into_iter()
            .map(decode_mail)
            .collect::<Result<Vec<_>, _>>()?;
        let has_more = mails.len() > PAGE_SIZE;
        mails.truncate(PAGE_SIZE);
        let next_before_id = mails.last().filter(|_| has_more).map(|mail| mail.id);
        let total = self
            .connection
            .query_row(
                "SELECT COUNT(*) FROM player_mails WHERE account_id = ?1",
                [account_id],
                |row| row.get(0),
            )
            .map_err(history_error)?;
        Ok(RewardDeliveryPage {
            mails,
            next_before_id,
            total,
        })
    }
    // //// /按邮件游标读取存档的全部投递状态 ////
}

fn history_error(error: rusqlite::Error) -> PersonalServiceError {
    PersonalServiceError::new(format!("reward history query failed: {error}"))
}
