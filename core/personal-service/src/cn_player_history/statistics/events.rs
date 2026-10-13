// audience: internal
// # personal-service-cn-player-history-events
// 活动统计关联服务端关卡主表和玩家最佳成绩. 超过客户端整数范围的值由上层保留为无记录.

use crate::cn_battle_assets::{BattleFixture, BattleQuest};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

fn fixture() -> Result<&'static BattleFixture, PersonalServiceError> {
    static DATA: OnceLock<Result<BattleFixture, String>> = OnceLock::new();
    DATA.get_or_init(|| {
        crate::cn_battle_assets::load_battle_fixture().map_err(|error| error.to_string())
    })
    .as_ref()
    .map_err(|error| PersonalServiceError::new(error.clone()))
}

fn event_states<'a>(
    root: &'a Map<String, Value>,
    family: &str,
) -> impl Iterator<Item = (i64, &'a Value)> {
    root.get("cn_activity_state")
        .and_then(|state| state.get("event_families"))
        .and_then(|families| families.get(family))
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(id, state)| id.parse::<i64>().ok().map(|id| (id, state)))
}

// //// 读取无限演武的最佳轮数及对应编队 [@x380kkm 2026-09-26] ////
pub(super) fn best_rush(root: &Map<String, Value>) -> Option<(i64, i64, &Value)> {
    event_states(root, "rush")
        .filter_map(|(id, state)| {
            let round = state
                .get("endless_battle_max_round")
                .and_then(Value::as_i64)?;
            let valid = fixture()
                .ok()?
                .quests
                .values()
                .any(|quest| quest.rush_event_id == Some(id));
            (valid && round > 0).then_some((id, round, state))
        })
        .max_by_key(|entry| entry.1)
}

// //// 合计土偶活动各关卡组的真实最佳分数 [@x380kkm 2026-09-26] ////
pub(super) fn best_carnival(root: &Map<String, Value>) -> Option<(i64, i64)> {
    event_states(root, "carnival")
        .filter_map(|(id, state)| {
            if !fixture()
                .ok()?
                .quests
                .values()
                .any(|quest| quest.carnival_event_id == Some(id))
            {
                return None;
            }
            let records = state.get("carnival_records")?.as_array()?;
            if records.is_empty() {
                return None;
            }
            let total = records.iter().try_fold(0_i64, |total, record| {
                total.checked_add(record.get("best_score")?.as_i64()?)
            })?;
            Some((id, total))
        })
        .max_by_key(|entry| entry.1)
}

#[derive(Deserialize)]
struct SoloRewards {
    quests: BTreeMap<i64, Vec<SoloTier>>,
}
#[derive(Deserialize)]
struct SoloTier {
    elapsed_time_ms: i64,
    rewards: Vec<SoloReward>,
}
#[derive(Deserialize)]
struct SoloReward {
    degree_id: Option<i64>,
}

// //// 分别统计极时试炼的通关称号与精通称号 [@x380kkm 2026-09-26] ////
pub(super) fn solo_degree_counts(
    root: &Map<String, Value>,
) -> Result<Vec<Option<i64>>, PersonalServiceError> {
    static DATA: OnceLock<Result<SoloRewards, String>> = OnceLock::new();
    let data = DATA
        .get_or_init(|| {
            serde_json::from_str(include_str!(
                "../../../assets/cn-solo-time-attack-rewards.json"
            ))
            .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    let owned: BTreeSet<i64> = root
        .get("degree_ids")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_i64)
        .collect();
    let mut clear = 0;
    let mut complete = 0;
    for tiers in data.quests.values() {
        let mut degrees: Vec<_> = tiers
            .iter()
            .flat_map(|tier| {
                tier.rewards.iter().filter_map(move |reward| {
                    reward
                        .degree_id
                        .map(|degree| (tier.elapsed_time_ms, degree))
                })
            })
            .collect();
        degrees.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        if degrees
            .first()
            .is_some_and(|entry| owned.contains(&entry.1))
        {
            clear += 1;
        }
        if degrees.len() > 1 && degrees.last().is_some_and(|entry| owned.contains(&entry.1)) {
            complete += 1;
        }
    }
    Ok(vec![Some(clear), Some(complete)])
}

fn progress<'a>(root: &'a Map<String, Value>, quest: &BattleQuest) -> Option<&'a Value> {
    root.get("quest_progress")?
        .get(quest.category.to_string())?
        .as_array()?
        .iter()
        .find(|entry| entry.get("quest_id").and_then(Value::as_i64) == Some(quest.quest_id))
}

fn cleared(root: &Map<String, Value>, quest: &BattleQuest) -> bool {
    progress(root, quest).is_some_and(|record| {
        record.get("finished").and_then(Value::as_bool) == Some(true)
            || record
                .get("clear_rank")
                .and_then(Value::as_i64)
                .is_some_and(|rank| rank > 0)
    })
}

// //// 将无限演武分数按真实档位阈值换算为档位记录 [@x380kkm 2026-09-26] ////
pub(super) fn score_attack_borders(
    root: &Map<String, Value>,
) -> Result<Vec<Option<i64>>, PersonalServiceError> {
    let mut borders = Vec::new();
    for quest in fixture()?
        .quests
        .values()
        .filter(|quest| !quest.score_attack_border_rewards.is_empty())
    {
        let Some(score) = progress(root, quest)
            .and_then(|record| record.get("high_score"))
            .and_then(Value::as_i64)
        else {
            continue;
        };
        borders.push(
            quest
                .score_attack_border_rewards
                .iter()
                .filter(|border| border.score <= score)
                .count() as i64,
        );
    }
    Ok(vec![
        borders.iter().max().copied(),
        (!borders.is_empty()).then(|| borders.iter().sum()),
    ])
}

// //// 按关卡所属活动统计通过的高难关卡 [@x380kkm 2026-09-26] ////
pub(super) fn expert_quest_count(
    root: &Map<String, Value>,
) -> Result<Option<i64>, PersonalServiceError> {
    Ok(Some(
        fixture()?
            .quests
            .values()
            .filter(|quest| {
                quest
                    .activity_id
                    .as_deref()
                    .is_some_and(|id| id.starts_with("expert-single:"))
                    && quest.b_rank_time > 0
                    && cleared(root, quest)
            })
            .count() as i64,
    ))
}

// //// 仅在整个支线活动的战斗关卡完成后计入完成数 [@x380kkm 2026-09-26] ////
pub(super) fn side_story_count(
    root: &Map<String, Value>,
) -> Result<Option<i64>, PersonalServiceError> {
    let mut activities: BTreeMap<&str, Vec<&BattleQuest>> = BTreeMap::new();
    for quest in fixture()?
        .quests
        .values()
        .filter(|quest| quest.b_rank_time > 0)
    {
        if let Some(activity) = quest
            .activity_id
            .as_deref()
            .filter(|id| id.starts_with("world-story:"))
        {
            activities.entry(activity).or_default().push(quest);
        }
    }
    Ok(Some(
        activities
            .values()
            .filter(|quests| quests.iter().all(|quest| cleared(root, quest)))
            .count() as i64,
    ))
}

// //// 读取各爬塔活动已完成的关卡层数 [@x380kkm 2026-09-26] ////
pub(super) fn tower_floor_count(
    root: &Map<String, Value>,
) -> Result<Option<i64>, PersonalServiceError> {
    Ok(Some(
        fixture()?
            .quests
            .values()
            .filter(|quest| {
                quest
                    .activity_id
                    .as_deref()
                    .is_some_and(|id| id.starts_with("tower-dungeon:"))
                    && quest.b_rank_time > 0
                    && cleared(root, quest)
            })
            .count() as i64,
    ))
}
