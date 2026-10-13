// audience: internal
// # personal-service-cn-raid-rush-rewards
// CN 奖励主表使用 GeneralRewardKind 编码, 响应使用客户端邮件奖励编码.
// 物品奖励与玩家领取凭据原子投递, 称号保存在同一玩家快照中.

use crate::database::MailReward;
use crate::PersonalServiceError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

const MASTER: &str = include_str!("../../../assets/cn-raid-rush-rewards.json");
static CATALOG: OnceLock<Result<Catalog, String>> = OnceLock::new();

#[derive(Deserialize)]
pub(in crate::cn_activity) struct Catalog {
    pub(in crate::cn_activity) raid: BTreeMap<i64, RaidEvent>,
    pub(in crate::cn_activity) rush: BTreeMap<i64, RushEvent>,
}

#[derive(Deserialize)]
pub(in crate::cn_activity) struct EventPeriod {
    pub(in crate::cn_activity) start_at_ms: i64,
    pub(in crate::cn_activity) play_end_at_ms: Option<i64>,
    pub(in crate::cn_activity) reward_end_at_ms: Option<i64>,
    #[serde(default)]
    pub(in crate::cn_activity) aggregation_end_at_ms: Option<i64>,
}

#[derive(Deserialize)]
pub(in crate::cn_activity) struct RaidEvent {
    #[serde(flatten)]
    pub(in crate::cn_activity) period: EventPeriod,
    pub(in crate::cn_activity) auto_start_point: i64,
    pub(in crate::cn_activity) quest_ids: Vec<i64>,
    pub(in crate::cn_activity) overall_rewards: Vec<KillReward>,
}

#[derive(Deserialize)]
pub(in crate::cn_activity) struct RushEvent {
    #[serde(flatten)]
    pub(in crate::cn_activity) period: EventPeriod,
    pub(in crate::cn_activity) ranking_rewards: Vec<RankReward>,
}

#[derive(Deserialize)]
pub(in crate::cn_activity) struct RankReward {
    pub(in crate::cn_activity) rank_start: i64,
    pub(in crate::cn_activity) rank_end: i64,
    pub(in crate::cn_activity) rewards: Vec<EventReward>,
}

#[derive(Deserialize, Serialize)]
pub(in crate::cn_activity) struct KillReward {
    id: i64,
    requirement: i64,
    first_kill: Option<i64>,
    interval: Option<i64>,
    rewards: Vec<EventReward>,
}

#[derive(Clone, Deserialize, Serialize)]
pub(in crate::cn_activity) struct EventReward {
    kind: i64,
    id: Option<i64>,
    amount: i64,
}

// //// 读取当前客户端活动奖励配置 [@x380kkm 2026-09-21] ////
pub(in crate::cn_activity) fn catalog() -> Result<&'static Catalog, PersonalServiceError> {
    CATALOG
        .get_or_init(|| serde_json::from_str(MASTER).map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| {
            PersonalServiceError::new(format!("CN raid/rush rewards are invalid: {error}"))
        })
}

// //// 汇总首次达到和周期达到的总讨伐奖励 [@x380kkm 2026-09-21] ////
pub(in crate::cn_activity) fn kill_rewards(
    event: &RaidEvent,
    previous: i64,
    current: i64,
) -> Result<(Vec<EventReward>, Vec<Value>), PersonalServiceError> {
    let mut rewards = Vec::new();
    let mut tiers = Vec::new();
    for tier in &event.overall_rewards {
        let occurrences = |kills: i64| -> i64 {
            let first = tier.first_kill.unwrap_or(tier.interval.unwrap_or(1));
            if kills < first {
                return 0;
            }
            if tier.requirement == 0 {
                1
            } else {
                (kills - first) / tier.interval.unwrap_or(1) + 1
            }
        };
        let count = occurrences(current)
            .saturating_sub(occurrences(previous))
            .max(0);
        if count == 0 {
            continue;
        }
        tiers.push(json!({"reward_id": tier.id, "count": count}));
        for reward in &tier.rewards {
            let mut reward = reward.clone();
            reward.amount = reward.amount.checked_mul(count).ok_or_else(|| {
                PersonalServiceError::new("CN raid rewards exceed the supported range")
            })?;
            rewards.push(reward);
        }
    }
    Ok((rewards, tiers))
}

// //// 按客户端奖励编码组装邮件和称号 [@x380kkm 2026-09-21] ////
pub(in crate::cn_activity) fn prepare(
    root: &mut Map<String, Value>,
    rewards: &[EventReward],
    viewer_id: i64,
) -> Result<(MailReward, Vec<Value>, Vec<Value>), PersonalServiceError> {
    let mut mail = MailReward::default();
    let mut response = Vec::new();
    let mut degrees = Vec::new();
    for reward in rewards {
        let required_id = || {
            reward
                .id
                .filter(|id| *id > 0)
                .ok_or_else(|| PersonalServiceError::new("CN event reward id is invalid"))
        };
        let response_kind = match reward.kind {
            0 => {
                add(&mut mail.item_list, required_id()?, reward.amount)?;
                1
            }
            1 => {
                add(&mut mail.equipment_list, required_id()?, reward.amount)?;
                6
            }
            2 => {
                mail.free_vmoney = sum(mail.free_vmoney, reward.amount)?;
                4
            }
            3 => {
                mail.free_mana = sum(mail.free_mana, reward.amount)?;
                8
            }
            4 => {
                mail.exp_pool = sum(mail.exp_pool, reward.amount)?;
                9
            }
            6 => {
                mail.character_list
                    .extend(std::iter::repeat(required_id()?).take(reward.amount as usize));
                5
            }
            7 => {
                let id = required_id()?;
                let owned = root
                    .entry("degree_ids".to_owned())
                    .or_insert_with(|| json!([]))
                    .as_array_mut()
                    .ok_or_else(|| PersonalServiceError::new("stored CN degrees are invalid"))?;
                if !owned.iter().any(|value| value.as_i64() == Some(id)) {
                    owned.push(json!(id));
                }
                degrees.push(json!({"viewer_id": viewer_id, "degree_id": id}));
                13
            }
            _ => {
                return Err(PersonalServiceError::new(
                    "unsupported CN event reward kind",
                ))
            }
        };
        response
            .push(json!({"kind": response_kind, "kind_id": reward.id, "number": reward.amount}));
    }
    Ok((mail, response, degrees))
}

pub(in crate::cn_activity) fn has_mail_rewards(mail: &MailReward) -> bool {
    mail.free_vmoney > 0
        || mail.free_mana > 0
        || mail.exp_pool > 0
        || !mail.item_list.is_empty()
        || !mail.equipment_list.is_empty()
        || !mail.character_list.is_empty()
}

fn add(
    values: &mut BTreeMap<String, i64>,
    id: i64,
    amount: i64,
) -> Result<(), PersonalServiceError> {
    let entry = values.entry(id.to_string()).or_default();
    *entry = sum(*entry, amount)?;
    Ok(())
}

fn sum(previous: i64, amount: i64) -> Result<i64, PersonalServiceError> {
    previous
        .checked_add(amount)
        .ok_or_else(|| PersonalServiceError::new("CN event rewards exceed the supported range"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // //// 累计总讨伐奖励只结算新跨越的固定和周期门槛 [@x380kkm 2026-09-21] ////
    #[test]
    fn settles_new_fixed_and_recurring_kill_thresholds() {
        let catalog = catalog().unwrap();
        let event = &catalog.raid[&1];
        let (rewards, _) = kill_rewards(event, 1, 2).unwrap();
        assert!(rewards
            .iter()
            .any(|reward| reward.kind == 0 && reward.id == Some(49100) && reward.amount == 10));
        assert!(!rewards.iter().any(|reward| reward.kind == 2));
        assert!(kill_rewards(event, 2, 2).unwrap().0.is_empty());
        assert!(kill_rewards(event, 2, 1).unwrap().0.is_empty());
        let event = &catalog.raid[&7];
        let tier = event
            .overall_rewards
            .iter()
            .find(|tier| tier.id == 72)
            .unwrap();
        let first = tier.first_kill.unwrap();
        let step = tier.interval.unwrap();
        let (_, awards) = kill_rewards(event, first - 1, first + step).unwrap();
        assert!(awards
            .iter()
            .any(|award| award["reward_id"] == tier.id && award["count"] == 2));
        let (_, awards) = kill_rewards(event, first, first + step - 1).unwrap();
        assert!(!awards.iter().any(|award| award["reward_id"] == tier.id));
    }
}
