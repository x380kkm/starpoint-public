// audience: internal
// # personal-service-cn-score-rewards
// 普通奖励的抽取次数应用单人或多人配置倍率后向上取整, 稀有奖励先判定掉落率再按组内概率选择一项.
// 客户端结果保留主表 group_id 和 position, 玩家库存保存解析后的具体物品编号.

use super::{
    apply_reward_at, random_below, require_reward_count, scaled_item_drop_amount, RewardResult,
    ScoreRewardResult,
};
use crate::cn_battle_assets::{BattleQuest, RareScoreReward, Reward, RewardElementMap};
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub(crate) struct ScoreRewardContext<'a> {
    pub(crate) clear_rank: i64,
    pub(crate) common_reward_count_multiplier: f64,
    pub(crate) boost_point_used: bool,
    pub(crate) drop_multiplier: i64,
    pub(crate) server_time: i64,
    pub(crate) reward_elements: &'a RewardElementMap,
}

// //// 按抽取结果发放普通和稀有掉落 [@x380kkm 2026-09-22] ////
pub(crate) fn apply_score_rewards(
    root: &mut Map<String, Value>,
    rare_reward_groups: &BTreeMap<String, Vec<RareScoreReward>>,
    quest: &BattleQuest,
    context: &ScoreRewardContext<'_>,
) -> Result<ScoreRewardResult, PersonalServiceError> {
    let mut result = ScoreRewardResult {
        rewards: RewardResult::default(),
        drop_score_reward_ids: Vec::new(),
        drop_rare_reward_ids: Vec::new(),
    };
    let Some(group_id) = quest.score_reward_group_id else {
        return Ok(result);
    };
    let rank_index = usize::try_from(context.clear_rank - 1)
        .ok()
        .filter(|rank| *rank < quest.score_reward_counts.len())
        .ok_or_else(|| PersonalServiceError::new("CN score reward rank is invalid"))?;
    let common_count = (quest.score_reward_counts[rank_index] as f64
        * context.common_reward_count_multiplier)
        .ceil() as usize;
    for index in draw_common_rewards(quest, common_count, random_below)? {
        let selected = &quest.score_rewards[index];
        let kind = selected
            .reward_type
            .ok_or_else(|| PersonalServiceError::new("CN score reward kind is missing"))?;
        let reward = scaled_reward(
            &Reward {
                kind,
                id: selected.id,
                count: selected.count,
                rarity: None,
            },
            context,
        )?;
        result
            .rewards
            .merge(apply_reward_at(root, &reward, context.server_time)?);
        result.drop_score_reward_ids.push(json!({
            "group_id": group_id, "index": selected.position.unwrap_or(index as i64 + 1),
            "number": require_reward_count(&reward)?,
        }));
    }
    for selected in quest.score_rewards.iter().filter(|reward| reward.kind == 1) {
        let probability = selected
            .rarity
            .ok_or_else(|| PersonalServiceError::new("CN rare drop probability is missing"))?;
        if !wins_probability(probability, random_fraction()?)? {
            continue;
        }
        let rare_id = selected
            .id
            .ok_or_else(|| PersonalServiceError::new("CN rare score reward group ID is missing"))?;
        let group = rare_reward_groups
            .get(&rare_id.to_string())
            .ok_or_else(|| PersonalServiceError::new("CN rare score reward group is missing"))?;
        let selected = choose_rare_reward(group, random_fraction()?)?;
        let reward = resolve_element_reward(
            &selected.reward,
            selected.element_rarity,
            quest.element.unwrap_or(0),
            context.reward_elements,
        )?;
        let reward = scaled_reward(&reward, context)?;
        result
            .rewards
            .merge(apply_reward_at(root, &reward, context.server_time)?);
        result.drop_rare_reward_ids.push(json!({
            "group_id": rare_id, "index": selected.position, "number": require_reward_count(&reward)?,
        }));
    }
    Ok(result)
}

// //// 按普通奖励权重抽取指定次数 [@x380kkm 2026-09-22] ////
fn draw_common_rewards(
    quest: &BattleQuest,
    count: usize,
    mut draw: impl FnMut(u32) -> Result<u32, PersonalServiceError>,
) -> Result<Vec<usize>, PersonalServiceError> {
    let mut pool = Vec::new();
    let mut total = 0_u32;
    for (index, reward) in quest
        .score_rewards
        .iter()
        .enumerate()
        .filter(|(_, reward)| reward.kind == 0)
    {
        let weight = reward
            .field5
            .and_then(|weight| u32::try_from(weight).ok())
            .ok_or_else(|| PersonalServiceError::new("CN common reward weight is invalid"))?;
        total = total.checked_add(weight).ok_or_else(|| {
            PersonalServiceError::new("CN common reward weight exceeds the supported range")
        })?;
        pool.push((index, total));
    }
    if total == 0 {
        return Ok(Vec::new());
    }
    (0..count)
        .map(|_| {
            let roll = draw(total)?;
            pool.iter()
                .find(|(_, upper)| roll < *upper)
                .map(|(index, _)| *index)
                .ok_or_else(|| {
                    PersonalServiceError::new("CN common reward draw is outside the pool")
                })
        })
        .collect()
}

// //// 使用半开区间判定概率并选择稀有奖励 [@x380kkm 2026-09-22] ////
fn wins_probability(probability: f64, roll: f64) -> Result<bool, PersonalServiceError> {
    Ok(roll < require_probability(probability)?)
}

fn require_probability(probability: f64) -> Result<f64, PersonalServiceError> {
    if !probability.is_finite() || !(0.0..=1.0).contains(&probability) {
        return Err(PersonalServiceError::new(
            "CN reward probability is invalid",
        ));
    }
    Ok(probability)
}

fn choose_rare_reward(
    group: &[RareScoreReward],
    roll: f64,
) -> Result<&RareScoreReward, PersonalServiceError> {
    let mut cumulative = 0.0;
    for reward in group {
        let probability = reward
            .reward
            .rarity
            .ok_or_else(|| PersonalServiceError::new("CN rare reward probability is missing"))?;
        cumulative += require_probability(probability)?;
        if roll < cumulative {
            return Ok(reward);
        }
    }
    Err(PersonalServiceError::new(
        "CN rare reward draw is outside the pool",
    ))
}

fn random_fraction() -> Result<f64, PersonalServiceError> {
    Ok(f64::from(random_below(u32::MAX)?) / f64::from(u32::MAX))
}

// //// 将元素稀有度转换为关卡对应的物品编号 [@x380kkm 2026-09-22] ////
pub(super) fn resolve_element_reward(
    reward: &Reward,
    element_rarity: Option<i64>,
    element: i64,
    element_map: &RewardElementMap,
) -> Result<Reward, PersonalServiceError> {
    let mut reward = reward.clone();
    let Some(rarity) = element_rarity else {
        return Ok(reward);
    };
    let enemy_element = usize::try_from(element)
        .ok()
        .and_then(|index| [3, 0, 1, 2, 5, 4].get(index).copied())
        .ok_or_else(|| PersonalServiceError::new("CN quest reward element is invalid"))?;
    let map_kind = if reward.kind == 6 { 1 } else { 2 };
    reward.id = element_map
        .get(&map_kind)
        .and_then(|rarities| rarities.get(&rarity))
        .and_then(|elements| elements.get(&enemy_element))
        .and_then(|ids| ids.first())
        .copied();
    if reward.id.is_none() {
        return Err(PersonalServiceError::new(
            "CN element reward mapping is missing",
        ));
    }
    Ok(reward)
}

// //// 应用加成点和个人服务道具倍率 [@x380kkm 2026-09-22] ////
fn scaled_reward(
    reward: &Reward,
    context: &ScoreRewardContext<'_>,
) -> Result<Reward, PersonalServiceError> {
    let mut scaled = reward.clone();
    let count = if reward.kind == 2 {
        1
    } else {
        require_reward_count(reward)?
    };
    let boost_multiplier = if context.boost_point_used && !matches!(reward.kind, 2 | 3) {
        2
    } else {
        1
    };
    let count = count.checked_mul(boost_multiplier).ok_or_else(|| {
        PersonalServiceError::new("CN boosted reward exceeds the supported range")
    })?;
    scaled.count = Some(scaled_item_drop_amount(
        reward.kind,
        count,
        context.drop_multiplier,
    )?);
    Ok(scaled)
}

#[cfg(test)]
#[path = "score_tests.rs"]
mod tests;
