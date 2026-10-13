// audience: internal
// # personal-service-cn-additional-battle-rewards
// 追加掉落沿用入场活动资格, 按关卡范围和敌人等级选择奖励组.
// 响应保存客户端主表的组与位置, 库存通过共享奖励入口保存最终数量.

use super::{
    apply_reward_at, random_below, require_reward_count, scaled_item_drop_amount, RewardResult,
};
use crate::cn_battle_assets::{BattleQuest, Reward, RewardElementMap};
use crate::cn_mission::{activity_window_status, BattleMissionWindow};
use crate::database::{ActivityWindowStatus, ServiceDatabase};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

const CATALOG: &str = include_str!("../../assets/cn-additional-battle-rewards.json");
static REWARDS: OnceLock<Result<AdditionalRewardCatalog, String>> = OnceLock::new();

#[derive(Deserialize)]
struct AdditionalRewardCatalog {
    events: Vec<CollectEvent>,
    reward_relations: BTreeMap<i64, Vec<RewardRelation>>,
    reward_groups: BTreeMap<i64, Vec<AdditionalReward>>,
    quest_enemy_levels: BTreeMap<String, i64>,
}

#[derive(Deserialize)]
struct CollectEvent {
    id: i64,
    start_time: i64,
    end_time: i64,
    required_quest: Option<RequiredQuest>,
    relations: Vec<QuestRelation>,
}

#[derive(Deserialize)]
struct RequiredQuest {
    category: i64,
    quest_id: i64,
}

#[derive(Deserialize)]
struct QuestRelation {
    quest_kind: Option<i64>,
    quest_parts: [Option<Vec<i64>>; 3],
    reward_relation_id: i64,
}

#[derive(Deserialize)]
struct RewardRelation {
    minimum_enemy_level: i64,
    group_id: i64,
}

#[derive(Deserialize)]
struct AdditionalReward {
    position: i64,
    weight: u32,
    reward: Reward,
    element_rarity: Option<i64>,
}

#[derive(Default)]
pub(crate) struct AdditionalRewardResult {
    pub(crate) rewards: RewardResult,
    pub(crate) drop_ids: Vec<Value>,
}

// //// 将追加掉落活动加入入场资格快照 [@x380kkm 2026-09-28] ////
pub(crate) fn additional_activity_ids() -> Result<Vec<String>, PersonalServiceError> {
    Ok(catalog()?
        .events
        .iter()
        .map(|event| format!("collect-item:{}", event.id))
        .collect())
}

// //// 保存入场时已解锁的收集活动 [@x380kkm 2026-09-28] ////
pub(crate) fn unlocked_additional_event_ids(
    root: &Map<String, Value>,
) -> Result<BTreeSet<i64>, PersonalServiceError> {
    Ok(catalog()?
        .events
        .iter()
        .filter(|event| event_unlocked(root, event))
        .map(|event| event.id)
        .collect())
}

// //// 发放当前局匹配的活动追加掉落 [@x380kkm 2026-09-28] ////
pub(crate) fn apply_additional_rewards(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    quest: &BattleQuest,
    window: &BattleMissionWindow,
    drop_multiplier: i64,
    server_time: i64,
    reward_elements: &RewardElementMap,
) -> Result<AdditionalRewardResult, PersonalServiceError> {
    let mut result = AdditionalRewardResult::default();
    if matches!(quest.category, 3 | 9 | 16 | 20) {
        return Ok(result);
    }
    let catalog = catalog()?;
    let Some(&enemy_level) = catalog
        .quest_enemy_levels
        .get(&format!("{}:{}", quest.category, quest.quest_id))
    else {
        return Ok(result);
    };
    for event in &catalog.events {
        let unlocked = window
            .activities
            .as_ref()
            .and_then(|activities| activities.unlocked_additional_events.as_ref())
            .map_or_else(
                || event_unlocked(root, event),
                |ids| ids.contains(&event.id),
            );
        if !unlocked || !event_available(event, window, database)? {
            continue;
        }
        for relation in &event.relations {
            if !matches_quest(relation, quest)? {
                continue;
            }
            let thresholds = catalog
                .reward_relations
                .get(&relation.reward_relation_id)
                .ok_or_else(|| {
                    PersonalServiceError::new("CN additional reward relation is missing")
                })?;
            for threshold in thresholds
                .iter()
                .filter(|entry| entry.minimum_enemy_level <= enemy_level)
            {
                let group = catalog
                    .reward_groups
                    .get(&threshold.group_id)
                    .ok_or_else(|| {
                        PersonalServiceError::new("CN additional reward group is missing")
                    })?;
                let selected = choose_reward(group, random_below)?;
                let mut reward = super::score::resolve_element_reward(
                    &selected.reward,
                    selected.element_rarity,
                    quest.element.unwrap_or(0),
                    reward_elements,
                )?;
                let count = require_reward_count(&reward)?;
                reward.count = Some(scaled_item_drop_amount(
                    reward.kind,
                    count,
                    drop_multiplier,
                )?);
                result
                    .rewards
                    .merge(apply_reward_at(root, &reward, server_time)?);
                result.drop_ids.push(json!({
                    "group_id": threshold.group_id, "index": selected.position, "number": require_reward_count(&reward)?,
                }));
            }
        }
    }
    Ok(result)
}

// //// 按前置关卡的通关记录判定收集活动解锁 [@x380kkm 2026-09-28] ////
fn event_unlocked(root: &Map<String, Value>, event: &CollectEvent) -> bool {
    let Some(required) = &event.required_quest else {
        return true;
    };
    root.get("quest_progress")
        .and_then(|progress| progress.get(required.category.to_string()))
        .and_then(Value::as_array)
        .is_some_and(|progress| {
            progress.iter().any(|entry| {
                entry.get("quest_id").and_then(Value::as_i64) == Some(required.quest_id)
                    && entry.get("finished").and_then(Value::as_bool) == Some(true)
            })
        })
}

// //// 按入场管理状态和原始活动日历判定开放 [@x380kkm 2026-09-28] ////
fn event_available(
    event: &CollectEvent,
    window: &BattleMissionWindow,
    database: &ServiceDatabase,
) -> Result<bool, PersonalServiceError> {
    let activity_id = format!("collect-item:{}", event.id);
    match activity_window_status(
        window.activities.as_deref(),
        database,
        &activity_id,
        window.started_at,
    )? {
        ActivityWindowStatus::Open => Ok(true),
        ActivityWindowStatus::Unscheduled => {
            let at = window.started_at.saturating_mul(1_000);
            if let Some((start, end)) =
                crate::activity_projection::regional_activity_window(&activity_id, at)?
            {
                return Ok(start <= at && at <= end);
            }
            Ok(event.start_time <= window.started_at && window.started_at <= event.end_time)
        }
        _ => Ok(false),
    }
}

// //// 将客户端可选关卡范围交给共享范围匹配器 [@x380kkm 2026-09-28] ////
fn matches_quest(
    relation: &QuestRelation,
    quest: &BattleQuest,
) -> Result<bool, PersonalServiceError> {
    if relation.quest_parts.iter().flatten().any(Vec::is_empty) {
        return Ok(false);
    }
    crate::cn_mission::conditions::QuestConditions {
        kind: relation.quest_kind,
        parts: relation
            .quest_parts
            .each_ref()
            .map(|part| part.as_deref().unwrap_or(&[])),
        ..Default::default()
    }
    .matches(quest.category, quest.quest_id)
}

// //// 每个等级奖励组按整数权重选择一项 [@x380kkm 2026-09-28] ////
fn choose_reward(
    group: &[AdditionalReward],
    mut draw: impl FnMut(u32) -> Result<u32, PersonalServiceError>,
) -> Result<&AdditionalReward, PersonalServiceError> {
    let total = group
        .iter()
        .try_fold(0_u32, |sum, entry| sum.checked_add(entry.weight))
        .filter(|total| *total > 0)
        .ok_or_else(|| PersonalServiceError::new("CN additional reward weights are invalid"))?;
    let mut roll = draw(total)?;
    for entry in group {
        if roll < entry.weight {
            return Ok(entry);
        }
        roll -= entry.weight;
    }
    Err(PersonalServiceError::new(
        "CN additional reward draw is outside the pool",
    ))
}

fn catalog() -> Result<&'static AdditionalRewardCatalog, PersonalServiceError> {
    REWARDS
        .get_or_init(|| serde_json::from_str(CATALOG).map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| {
            PersonalServiceError::new(format!("invalid CN additional battle rewards: {error}"))
        })
}

#[cfg(test)]
#[path = "additional_tests.rs"]
mod tests;
