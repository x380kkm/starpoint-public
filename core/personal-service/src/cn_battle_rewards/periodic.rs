// audience: internal
// # personal-service-cn-periodic-battle-rewards
// 周期掉落沿用降临战主表的点数引用, 奖励组和抽选次数.
// 每次抽中奖励消耗一个周期点, 客户端条目保存奖励主表位置和实际数量.

mod points;
#[cfg(test)]
mod tests;

use super::{apply_reward_at, random_below, require_reward_count, RewardResult};
use crate::cn_battle_assets::{BattleQuest, Reward, RewardElementMap};
use crate::cn_mission::BattleMissionWindow;
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

pub(crate) use points::recover_periodic_reward_points;
pub(crate) const RECOVERY_STATE_FIELD: &str = "periodic_reward_recovery_state";
const POINTS_FIELD: &str = "user_periodic_reward_point_list";
const CATALOG: &str = include_str!("../../assets/cn-periodic-battle-rewards.json");
static REWARDS: OnceLock<Result<PeriodicRewardCatalog, String>> = OnceLock::new();

#[derive(Deserialize)]
struct PeriodicRewardCatalog {
    daily_recovery_hour: i64,
    points: BTreeMap<i64, PeriodicPoint>,
    quests: BTreeMap<String, PeriodicQuest>,
    reward_groups: BTreeMap<i64, Vec<PeriodicReward>>,
}

#[derive(Deserialize)]
struct PeriodicPoint {
    activity_id: String,
    max_point: i64,
    recovery_point: i64,
    recovery_cycle: i64,
    start_time: i64,
    end_time: i64,
}

#[derive(Deserialize)]
struct PeriodicQuest {
    point_id: Option<i64>,
    group_id: i64,
    slots: usize,
}

#[derive(Deserialize)]
struct PeriodicReward {
    position: i64,
    probability: f64,
    reward: Reward,
    element_rarity: Option<i64>,
}

#[derive(Default)]
pub(crate) struct PeriodicRewardResult {
    pub(crate) rewards: RewardResult,
    pub(crate) drop_ids: Vec<Value>,
    pub(crate) point_list: Option<Vec<Value>>,
}

// //// 将周期奖励活动加入入场资格快照 [@x380kkm 2026-09-28] ////
pub(crate) fn periodic_activity_ids() -> Result<Vec<String>, PersonalServiceError> {
    Ok(catalog()?
        .points
        .values()
        .map(|point| point.activity_id.clone())
        .collect())
}

// //// 按入场时的活动点数发放周期掉落 [@x380kkm 2026-09-28] ////
pub(crate) fn apply_periodic_rewards(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    quest: &BattleQuest,
    window: &BattleMissionWindow,
    server_time: i64,
    reward_elements: &RewardElementMap,
) -> Result<PeriodicRewardResult, PersonalServiceError> {
    let catalog = catalog()?;
    let Some(periodic) = catalog
        .quests
        .get(&format!("{}:{}", quest.category, quest.quest_id))
    else {
        return Ok(PeriodicRewardResult::default());
    };
    let Some(point_id) = periodic.point_id else {
        return Ok(PeriodicRewardResult::default());
    };
    if !points::recover_battle_points(root, database, catalog, point_id, window)? {
        return Ok(PeriodicRewardResult::default());
    }
    grant_periodic_rewards(
        root,
        periodic,
        catalog,
        quest.element.unwrap_or(0),
        server_time,
        reward_elements,
    )
}

// //// 抽选周期奖励并保存对应剩余点数 [@x380kkm 2026-09-28] ////
fn grant_periodic_rewards(
    root: &mut Map<String, Value>,
    quest: &PeriodicQuest,
    catalog: &PeriodicRewardCatalog,
    element: i64,
    server_time: i64,
    reward_elements: &RewardElementMap,
) -> Result<PeriodicRewardResult, PersonalServiceError> {
    let mut result = PeriodicRewardResult::default();
    let Some(point_id) = quest.point_id else {
        return Ok(result);
    };
    let Some(point) = points::stored_point(root, point_id) else {
        return Ok(result);
    };
    let group = catalog
        .reward_groups
        .get(&quest.group_id)
        .ok_or_else(|| PersonalServiceError::new("CN periodic reward group is missing"))?;
    let slots = quest.slots.min(usize::try_from(point).unwrap_or_default());
    let mut consumed = 0;
    for _ in 0..slots {
        let Some(selected) = choose_reward(group, random_fraction)? else {
            continue;
        };
        let reward = super::score::resolve_element_reward(
            &selected.reward,
            selected.element_rarity,
            element,
            reward_elements,
        )?;
        result
            .rewards
            .merge(apply_reward_at(root, &reward, server_time)?);
        result.drop_ids.push(json!({
            "group_id": quest.group_id, "index": selected.position, "number": require_reward_count(&reward)?,
        }));
        consumed += 1;
    }
    let remaining = point - consumed;
    points::store_point(root, point_id, remaining)?;
    result.point_list = Some(vec![json!({"id": point_id, "point": remaining})]);
    Ok(result)
}

// //// 筛选命中奖励后按候选概率选择一项 [@x380kkm 2026-09-28] ////
fn choose_reward(
    group: &[PeriodicReward],
    mut draw: impl FnMut() -> Result<f64, PersonalServiceError>,
) -> Result<Option<&PeriodicReward>, PersonalServiceError> {
    let mut candidates = Vec::new();
    let mut total = 0.0;
    for reward in group {
        if !reward.probability.is_finite() || !(0.0..=1.0).contains(&reward.probability) {
            return Err(PersonalServiceError::new(
                "CN periodic reward probability is invalid",
            ));
        }
        if reward.probability == 1.0 || draw()? < reward.probability {
            candidates.push(reward);
            total += reward.probability;
        }
    }
    if candidates.len() <= 1 {
        return Ok(candidates.first().copied());
    }
    let mut roll = draw()? * total;
    for reward in candidates {
        if roll < reward.probability {
            return Ok(Some(reward));
        }
        roll -= reward.probability;
    }
    Err(PersonalServiceError::new(
        "CN periodic reward draw is outside the pool",
    ))
}

fn random_fraction() -> Result<f64, PersonalServiceError> {
    Ok(f64::from(random_below(u32::MAX)?) / f64::from(u32::MAX))
}

fn catalog() -> Result<&'static PeriodicRewardCatalog, PersonalServiceError> {
    REWARDS
        .get_or_init(|| serde_json::from_str(CATALOG).map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| {
            PersonalServiceError::new(format!("invalid CN periodic battle rewards: {error}"))
        })
}
