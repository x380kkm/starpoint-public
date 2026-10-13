// audience: internal
// # personal-service-cn-expod-settlement
// 经验操作的响应凭据随资产保存, 转换所得 EXP 和星粒写入领取历史, 无计数操作使用独立记录标识.

use super::REWARD_ITEM_ID;
use crate::cn::msgpack_response_at;
use crate::cn_request_receipt::RequestReceipt;
use crate::cn_tutorial::{encode_player_data, require_root};
use crate::database::{ReceiveHistoryEntry, ServiceDatabase};
use crate::http::HttpResponse;
use crate::PersonalServiceError;
use serde_json::Value;

pub(super) struct ExpodSettlement {
    pub(super) viewer_id: i64,
    pub(super) account_id: i64,
    pub(super) response_time: i64,
    pub(super) receipt: Option<RequestReceipt>,
}

// //// 保存经验操作的资产, 响应凭据和实际所得奖励 [@x380kkm 2026-09-23] ////
impl ExpodSettlement {
    pub(super) fn save(
        self,
        database: &mut ServiceDatabase,
        player_data: &mut Value,
        response: Value,
        history: &[ReceiveHistoryEntry],
    ) -> Result<HttpResponse, PersonalServiceError> {
        let key = self
            .receipt
            .map(|receipt| {
                receipt.record(require_root(player_data)?, &response, self.response_time)
            })
            .transpose()?;
        if history.is_empty() {
            database.save_player_snapshot(self.account_id, &encode_player_data(player_data)?)?;
        } else {
            let event_key = match key {
                Some(key) => key,
                None => {
                    let mut bytes = [0_u8; 16];
                    getrandom::getrandom(&mut bytes).map_err(|error| {
                        PersonalServiceError::new(format!(
                            "CN EXP history identifier failed: {error}"
                        ))
                    })?;
                    format!("expod:conversion:{:032x}", u128::from_le_bytes(bytes))
                }
            };
            database.save_player_snapshot_with_receive_history(
                self.account_id,
                &encode_player_data(player_data)?,
                &event_key,
                self.response_time,
                history,
            )?;
        }
        msgpack_response_at(self.viewer_id, false, self.response_time, response)
    }
}
// //// /保存经验操作的资产, 响应凭据和实际所得奖励 ////

// //// 将实际转换产出映射为玩家领取历史 [@x380kkm 2026-09-23] ////
pub(super) fn conversion_history(exp: i64, star_grain: i64) -> Vec<ReceiveHistoryEntry> {
    let mut history = Vec::new();
    if exp > 0 {
        history.push(ReceiveHistoryEntry::reward(9, None, exp));
    }
    if star_grain > 0 {
        history.push(ReceiveHistoryEntry::reward(
            1,
            Some(REWARD_ITEM_ID.parse().expect("star grain item identifier")),
            star_grain,
        ));
    }
    history
}
// //// /将实际转换产出映射为玩家领取历史 ////
