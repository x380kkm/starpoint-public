// audience: internal
// # personal-service-cn-mission-progress
//
// 历史战斗只恢复保存字段能表达的任务条件. 角色状态任务取当前角色, 玛纳节点和装备数据.

use super::{conditions, ComputeContext, MissionDefinition};
use crate::PersonalServiceError;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub(super) struct CharacterProgress {
    level: i64,
    complete_second_board: bool,
    bond_token_claimed: bool,
}

pub(super) fn character_progress(root: &Map<String, Value>) -> Result<BTreeMap<i64, CharacterProgress>, PersonalServiceError> {
    root.get("user_character_list").and_then(Value::as_object).into_iter().flat_map(|items| items.iter())
        .filter_map(|(id, value)| id.parse::<i64>().ok().map(|id| (id, value)))
        .map(|(id, value)| Ok((id, CharacterProgress {
            level: conditions::character_level(root, id),
            complete_second_board: conditions::has_complete_second_board(root, id)?,
            bond_token_claimed: value.get("bond_token_list").and_then(Value::as_array).into_iter().flatten()
                .any(|token| token.get("status").and_then(Value::as_i64) == Some(2)),
        }))).collect()
}

// //// 根据主表的角色条件读取当前养成状态 [@x380kkm 2026-09-22] ////
pub(super) fn state_progress(mission: &MissionDefinition, context: &ComputeContext) -> Option<i64> {
    let characters = || context.characters.iter().filter(|(id, _)| mission.character_id.map_or(true, |expected| expected == **id));
    Some(match mission.pattern.as_deref()? {
        "mana_board_2nd_complete_count" => characters().filter(|(_, character)| character.complete_second_board).count() as i64,
        "character_level_100_and_got_bond_token" => characters().filter(|(_, character)| character.level >= 100 && character.bond_token_claimed).count() as i64,
        "character_level_achievement" => characters().map(|(_, character)| character.level).max().unwrap_or_default(),
        "level_80_character_count" => characters().filter(|(_, character)| character.level >= 80).count() as i64,
        "characters_count" => characters().count() as i64,
        "got_equip_kind_count" => context.equipment_count,
        "level_max_equipment_count" => context.fully_awakened_equipment_count,
        "player_rank_achievement" => context.rank_degree,
        _ => return None,
    })
}
// //// /根据主表的角色条件读取当前养成状态 ////

pub(super) fn quest_conditions(mission: &MissionDefinition) -> conditions::QuestConditions<'_> {
    conditions::QuestConditions {
        kind: mission.quest_kind,
        parts: [&mission.quest_parts[0], &mission.quest_parts[1], &mission.quest_parts[2]],
        categories: &mission.quest_categories,
        ids: &[],
        rank: mission.quest_rank,
    }
}

pub(super) fn has_quest_conditions(mission: &MissionDefinition) -> bool {
    mission.quest_kind.is_some() || !mission.quest_categories.is_empty() || mission.quest_rank.is_some()
        || mission.quest_parts.iter().any(|parts| !parts.is_empty())
}

// //// 只从匹配关卡和关联队长的历史记录恢复进度 [@x380kkm 2026-09-22] ////
pub(super) fn quest_progress(mission: &MissionDefinition, context: &ComputeContext) -> Result<i64, PersonalServiceError> {
    if !mission.required_character_ids.is_empty() || !mission.required_races.is_empty() {
        return Ok(0);
    }
    let pattern = mission.pattern.as_deref().unwrap_or_default();
    let conditions = quest_conditions(mission);
    if pattern == "chapter_clear" {
        let mut expected = Vec::new();
        for quest in conditions::battle_fixture()?.quests.values() {
            if quest.b_rank_time > 0 && conditions.matches(quest.category, quest.quest_id)? {
                expected.push((quest.category, quest.quest_id));
            }
        }
        return Ok(i64::from(!expected.is_empty() && expected.iter().all(|(category, id)| {
            context.quest_progress.get(category).into_iter().flatten().any(|entry| entry.quest_id == *id)
        })));
    }
    let mut count = 0_i64;
    for (category, entries) in &context.quest_progress {
        for entry in entries {
            if !conditions.matches(*category, entry.quest_id)? { continue; }
            if mission.battle_kind == Some(2) && entry.multi_clear_count.unwrap_or_default() == 0 { continue; }
            let timed = pattern == "single_battle_clear_time";
            let leader = if timed { entry.best_leader_character_id } else { entry.leader_character_id };
            if mission.leader_character_id.is_some_and(|expected| leader != Some(expected)) { continue; }
            if timed && !mission.stages.iter().filter_map(|stage| stage.clear_seconds)
                .any(|seconds| entry.best_elapsed_time_ms.is_some_and(|ms| ms <= seconds.saturating_mul(1000))) { continue; }
            if pattern == "ss_rank_count" && entry.clear_rank.unwrap_or_default() < 5 { continue; }
            let increment = if pattern == "multi_battle_clear_count" { entry.multi_clear_count.unwrap_or_default().max(0) } else { 1 };
            count = count.saturating_add(increment);
        }
    }
    Ok(count)
}
// //// /只从匹配关卡和关联队长的历史记录恢复进度 ////
