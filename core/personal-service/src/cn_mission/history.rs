// audience: internal
// # personal-service-cn-mission-history
//
// 奖励凭据区分主表奖励与直接发放奖励. 库存奖励由容量投递记录保存领取历史, 其他奖励从阶段凭据增量生成历史.

use super::{receipt_activity_id, reward::reward_history_entries, MissionReward};
use crate::database::ReceiveHistoryEntry;
use crate::PersonalServiceError;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub(crate) struct MissionRewardHistory {
    pub(crate) event_key: String,
    pub(crate) received_at: i64,
    pub(crate) entries: Vec<ReceiveHistoryEntry>,
}

// //// 从新增奖励凭据读取实发奖励并区分参与轮次 [@x380kkm 2026-09-26] ////
pub(crate) fn collect_mission_reward_history(
    before: &Map<String, Value>, after: &Map<String, Value>,
) -> Result<Vec<MissionRewardHistory>, PersonalServiceError> {
    let mut history = Vec::new();
    for field in ["mission_stage_receipts", "active_mission_receipts", "score_attack_reward_receipts"] {
        for (key, receipt) in after.get(field).and_then(Value::as_object).into_iter().flatten() {
            if before.get(field).and_then(|receipts| receipts.get(key)).is_some_and(Value::is_object) {
                continue;
            }
            let Some(granted) = receipt.get("granted_rewards") else { continue; };
            let rewards: Vec<MissionReward> = serde_json::from_value(granted.clone())
                .map_err(|error| PersonalServiceError::new(format!("invalid granted mission rewards: {error}")))?;
            let entries = reward_history_entries(&rewards)?;
            if entries.is_empty() { continue; }
            let received_at = receipt.get("received_at").and_then(Value::as_i64).filter(|time| *time >= 0)
                .ok_or_else(|| PersonalServiceError::new("CN mission receipt time is invalid"))?;
            let activity_id = match crate::cn_battle_state::reward_receipt_activity_id(field, key)? {
                Some(activity_id) => Some(activity_id),
                None => receipt_activity_id(field, key)?,
            };
            let round = activity_id.map(|activity_id| {
                crate::activity_history::participation_round(after, &activity_id)
            }).unwrap_or_default();
            let digest = Sha256::digest(format!("{field}:{key}:round:{round}").as_bytes());
            history.push(MissionRewardHistory { event_key: format!("mission-receipt:{digest:x}"), received_at, entries });
        }
    }
    Ok(history)
}
// //// /从新增奖励凭据读取实发奖励并区分参与轮次 ////

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cn_mission::settle_rewards;
    use serde_json::json;

    // //// 按历史领取状态记录实际补发并区分活动重置轮次 [@x380kkm 2026-09-22] ////
    #[test]
    fn records_only_granted_legacy_rewards_and_new_participation() {
        let rewards = vec![
            MissionReward { kind: 0, amount: 5, item_id: None, character_id: None, equipment_id: None, degree_id: None, pass_card_event_id: None },
            MissionReward { kind: 3, amount: 100, item_id: None, character_id: None, equipment_id: None, degree_id: None, pass_card_event_id: None },
        ];
        let before = json!({"user_info":{"free_vmoney":0,"free_mana":100},"active_mission_receipts":{"21010:1":true}}).as_object().unwrap().clone();
        let mut after = before.clone();
        settle_rewards(&mut after, "active_mission_receipts", "21010:1", &rewards, &rewards[1..], 1).unwrap();
        let first = collect_mission_reward_history(&before, &after).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].entries, vec![ReceiveHistoryEntry::reward(4, None, 5)]);
        assert!(collect_mission_reward_history(&after, &after).unwrap().is_empty());
        assert_eq!(after["user_info"]["free_mana"], 100);
        after["active_mission_receipts"] = json!({});
        after.insert("activity_participation_rounds".to_owned(), json!({"active-mission:3":1}));
        let reset = after.clone();
        settle_rewards(&mut after, "active_mission_receipts", "21010:1", &rewards, &[], 2).unwrap();
        let repeated = collect_mission_reward_history(&reset, &after).unwrap();
        assert_eq!(repeated[0].entries, vec![ReceiveHistoryEntry::reward(4, None, 5)]);
        let deliveries: Vec<crate::cn_reward_overflow::Delivery> = serde_json::from_value(after["reward_deliveries"].clone()).unwrap();
        assert_eq!(deliveries.len(), 1);
        assert_eq!(deliveries[0].credited_reward().free_mana, 100);
        assert_ne!(first[0].event_key, repeated[0].event_key);
    }
    // //// /按历史领取状态记录实际补发并区分活动重置轮次 ////
}
