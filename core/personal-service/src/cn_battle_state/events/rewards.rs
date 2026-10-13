// audience: internal
// # personal-service-cn-event-rewards
//
// 该模块发放活动档位奖励, 并返回本次领取的档位和新增称号.

use crate::cn_mission::{settle_rewards, MissionReward};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::BTreeSet;

#[derive(Deserialize)]
pub(super) struct RewardTier {
    pub(super) id: i64,
    pub(super) rewards: Vec<MissionReward>,
}

#[derive(Default)]
pub(super) struct EventRewards {
    pub(super) reward_ids: Vec<i64>,
    pub(super) new_degree_ids: BTreeSet<i64>,
    pub(super) granted: Vec<MissionReward>,
}

// //// 发放达到条件且尚未领取的奖励档位 [@x380kkm 2026-09-21] ////
pub(super) fn settle_reward_tiers<'a>(
    root: &mut Map<String, Value>,
    receipt_field: &str,
    tiers: impl IntoIterator<Item = &'a RewardTier>,
    server_time: i64,
) -> Result<EventRewards, PersonalServiceError> {
    let owned_degrees = root
        .get("degree_ids")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_i64)
        .collect::<BTreeSet<_>>();
    let mut result = EventRewards::default();
    for tier in tiers {
        if settle_rewards(
            root,
            receipt_field,
            &tier.id.to_string(),
            &tier.rewards,
            &[],
            server_time,
        )?
        .is_none()
        {
            continue;
        }
        result.reward_ids.push(tier.id);
        for reward in &tier.rewards {
            if let Some(degree_id) = reward.degree_id.filter(|id| !owned_degrees.contains(id)) {
                result.new_degree_ids.insert(degree_id);
            }
        }
        result.granted.extend(tier.rewards.iter().cloned());
    }
    Ok(result)
}
// //// /发放达到条件且尚未领取的奖励档位 ////

// //// 移除指定档位的领取凭据 [@x380kkm 2026-09-21] ////
pub(super) fn reset_reward_receipts(
    root: &mut Map<String, Value>,
    receipt_field: &str,
    reward_ids: impl IntoIterator<Item = i64>,
) -> Result<usize, PersonalServiceError> {
    let Some(receipts) = root.get_mut(receipt_field) else {
        return Ok(0);
    };
    let receipts = receipts
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN event reward receipts are invalid"))?;
    Ok(reward_ids
        .into_iter()
        .filter_map(|id| receipts.remove(&id.to_string()))
        .count())
}
// //// /移除指定档位的领取凭据 ////
