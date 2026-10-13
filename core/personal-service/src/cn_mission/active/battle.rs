// audience: internal
// # personal-service-cn-active-mission-battle
//
// 战斗任务按客户端关卡范围编码, 队伍限制和实战统计选择进度. 剧情完成与战斗完成共享关卡条件.

use super::{
    advance, catalog, complete_dependencies, complete_dependencies_with, is_available,
    is_available_in_window, progress, stage_time_limit, Mission,
};
use crate::cn_battle_assets::BattleQuest;
use crate::cn_battle_state::FinishBattleInput;
use crate::cn_mission::battle_time::battle_mission_window;
use crate::cn_mission::conditions::{
    battle_kind_matches, battle_power, character_level, has_complete_skill_gauges,
    has_second_board_abilities, party_slots_match, PartyConditions, QuestConditions,
};
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use serde_json::{Map, Value};

fn quest_matches(
    mission: &Mission,
    category: i64,
    quest_id: i64,
) -> Result<bool, PersonalServiceError> {
    QuestConditions {
        kind: mission.quest_kind,
        parts: [
            &mission.quest_parts[0],
            &mission.quest_parts[1],
            &mission.quest_parts[2],
        ],
        categories: &mission.quest_categories,
        ids: &mission.quest_ids,
        rank: mission.quest_rank,
    }
    .matches(category, quest_id)
}

fn sum_statistic(statistics: &Value, field: &str) -> i64 {
    statistics
        .get("zones")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|zone| zone.get(field).and_then(Value::as_i64))
        .fold(0_i64, |sum, count| sum.saturating_add(count.max(0)))
}

fn statistic_value(kind: i64, input: &FinishBattleInput<'_>, statistics: &Value) -> Option<i64> {
    match kind {
        0 => Some(sum_statistic(statistics, "weak_point_attack_count")),
        2 => input.dash_count,
        1 => input.power_flip_count,
        4 => input.skill_count,
        7 => Some(sum_statistic(statistics, "enemy_kill_count")),
        _ => None,
    }
}

fn completed_stories(
    root: &Map<String, Value>,
    mission: &Mission,
) -> Result<i64, PersonalServiceError> {
    let fixture = super::battle_fixture()?;
    let mut expected = Vec::new();
    for quest in fixture.quests.values() {
        if quest.b_rank_time > 0 && quest_matches(mission, quest.category, quest.quest_id)? {
            expected.push(quest);
        }
    }
    if expected.is_empty() {
        return Ok(0);
    }
    Ok(i64::from(expected.into_iter().all(|quest| {
        root.get("quest_progress")
            .and_then(|categories| categories.get(quest.category.to_string()))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|record| {
                record.get("quest_id").and_then(Value::as_i64) == Some(quest.quest_id)
                    && record
                        .get("clear_rank")
                        .and_then(Value::as_i64)
                        .unwrap_or_default()
                        >= 5
            })
    })))
}

// //// 按关卡范围累计挑战和完成动作 [@x380kkm 2026-09-21] ////
fn record_quest_action(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    category: i64,
    quest_id: i64,
    pattern: &str,
) -> Result<Vec<Value>, PersonalServiceError> {
    let mut updates = Vec::new();
    for (id, mission) in &catalog()?.missions {
        if mission.pattern != pattern
            || !quest_matches(mission, category, quest_id)?
            || !is_available(root, database, mission)?
        {
            continue;
        }
        if let Some(update) = advance(root, *id, progress(root, *id).saturating_add(1))? {
            updates.push(update);
        }
    }
    complete_dependencies(root, database, &mut updates)?;
    Ok(updates)
}

pub(crate) fn record_active_quest_challenge(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    category: i64,
    quest_id: i64,
) -> Result<Vec<Value>, PersonalServiceError> {
    record_quest_action(root, database, category, quest_id, "quest_challenge")
}

pub(crate) fn record_active_quest_clear(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    category: i64,
    quest_id: i64,
) -> Result<Vec<Value>, PersonalServiceError> {
    record_quest_action(root, database, category, quest_id, "quest_clear")
}
// //// /按关卡范围累计挑战和完成动作 ////

// //// 用实际战斗队伍和统计刷新生效的主动任务 [@x380kkm 2026-09-21] ////
pub(crate) fn record_active_battle(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    account_id: i64,
    quest: &BattleQuest,
    input: &FinishBattleInput<'_>,
    statistics: &Value,
    clear_rank: i64,
) -> Result<Vec<Value>, PersonalServiceError> {
    let completed_at = database.current_server_time_millis()?.div_euclid(1000);
    let window = battle_mission_window(root, database, account_id, completed_at)?;
    let mut updates = Vec::new();
    for (id, mission) in &catalog()?.missions {
        if !is_available_in_window(root, database, mission, &window)?
            || !quest_matches(mission, quest.category, quest.quest_id)?
        {
            continue;
        }
        if !battle_kind_matches(mission.battle_kind, input.is_multi) {
            continue;
        }
        let previous = progress(root, *id);
        let characters = input
            .main_character_ids
            .iter()
            .chain(input.unison_character_ids)
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        let required_character = mission.character_id.filter(|id| characters.contains(id));
        let party = PartyConditions {
            character_id: mission.character_id,
            character_element: mission.character_element,
            equipment_element: mission.equipment_element,
            action_effects: &mission.action_effects,
            effects_ignore_characters: &mission.effects_ignore_characters,
        };
        let delta = match mission.pattern.as_str() {
            "single_battle_clear_count" => Some(i64::from(!input.is_multi)),
            "multi_battle_clear_count" => Some(i64::from(input.is_multi)),
            "multi_battle_host_count" => {
                Some(i64::from(input.is_multi && input.is_host == Some(true)))
            }
            "battle_clear_count" | "quest_clear" => Some(1),
            "ss_rank_count" => Some(i64::from(clear_rank >= 5)),
            "single_battle_clear_time" => Some(i64::from(
                !input.is_multi
                    && stage_time_limit(*id)?
                        .is_some_and(|limit| input.elapsed_time_ms <= limit * 1000),
            )),
            "battle_zone_statistics_count" => mission
                .statistics_kind
                .and_then(|kind| statistic_value(kind, input, statistics)),
            "battle_clear_with_specific_party" => Some(i64::from(
                party_slots_match(&mission.main_character_ids, input.main_character_ids)
                    && party_slots_match(&mission.unison_character_ids, input.unison_character_ids)
                    && party_slots_match(&mission.party_equipment_ids, input.equipment_ids),
            )),
            "battle_clear_with_specific_character" => {
                Some(i64::from(party.has_character(root, input)?))
            }
            "battle_clear_with_specific_skill" => Some(i64::from(party.has_skill(root, input)?)),
            "battle_clear_with_level80_character" => Some(i64::from(
                required_character.is_some_and(|id| character_level(root, id) >= 80),
            )),
            "battle_clear_with_level100_character" => Some(i64::from(
                required_character.is_some_and(|id| character_level(root, id) >= 100),
            )),
            "battle_clear_with_character_learned_all_abilities_in_mana_board_2nd" => {
                Some(match required_character {
                    Some(id) => i64::from(has_second_board_abilities(root, id)?),
                    None => 0,
                })
            }
            "battle_clear_with_skill_gauge_over" => {
                Some(i64::from(has_complete_skill_gauges(statistics)))
            }
            _ => None,
        };
        let value = match mission.pattern.as_str() {
            "max_skill_chain_achievement" => input.max_skill_chain_count,
            "max_power_achievement" => battle_power(root, input, statistics),
            "chapter_complete" => Some(completed_stories(root, mission)?),
            _ => delta.map(|delta| previous.saturating_add(delta.max(0))),
        };
        if let Some(value) = value {
            if let Some(update) = advance(root, *id, value)? {
                updates.push(update);
            }
        }
    }
    complete_dependencies_with(root, &mut updates, |root, mission| {
        is_available_in_window(root, database, mission, &window)
    })?;
    Ok(updates)
}
// //// /用实际战斗队伍和统计刷新生效的主动任务 ////
