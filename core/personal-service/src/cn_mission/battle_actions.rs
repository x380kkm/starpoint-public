// audience: internal
// # personal-service-cn-mission-battle-actions
//
// 战斗任务按任务主表的关卡, 队伍和统计条件累计. 当期进度与奖励凭据共同保存.

mod statistics;

use super::{conditions, mission_catalog, mission_reward_event_key, progress, settle_mission_stage, MissionDefinition, MissionKey};
use super::battle_time::battle_mission_window;
use super::window::mission_scope_for_battle;
use crate::cn_battle_assets::BattleQuest;
use crate::cn_battle_state::FinishBattleInput;
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

enum ProgressChange { Add(i64), Maximum(i64), Milliseconds(i64) }

fn party_matches(mission: &MissionDefinition, input: &FinishBattleInput<'_>) -> Result<bool, PersonalServiceError> {
    if !conditions::battle_kind_matches(mission.battle_kind, input.is_multi)
        || mission.leader_character_id.is_some_and(|id| input.main_character_ids.first().copied().flatten() != Some(id)) {
        return Ok(false);
    }
    let characters = input.main_character_ids.iter().chain(input.unison_character_ids).flatten().copied().collect::<BTreeSet<_>>();
    if !mission.required_character_ids.iter().all(|id| characters.contains(id)) { return Ok(false); }
    if !mission.required_races.is_empty() {
        let fixture = conditions::battle_fixture()?;
        let races = characters.iter().filter_map(|id| fixture.characters.get(&id.to_string()))
            .flat_map(|character| &character.races).collect::<BTreeSet<_>>();
        if !mission.required_races.iter().all(|race| races.contains(race)) { return Ok(false); }
    }
    Ok(true)
}

fn progress_change(
    root: &Map<String, Value>, mission: &MissionDefinition, input: &FinishBattleInput<'_>, statistics: &Value, clear_rank: i64,
    participation: (bool, bool),
) -> Result<Option<ProgressChange>, PersonalServiceError> {
    use ProgressChange::{Add, Maximum, Milliseconds};
    let add = |matched| Some(Add(i64::from(matched)));
    Ok(match mission.pattern.as_deref() {
        Some("single_battle_clear_count") => add(!input.is_multi),
        Some("multi_battle_clear_count") => add(input.is_multi),
        Some("multi_battle_host_count") => add(input.is_multi && input.is_host == Some(true)),
        Some("multi_battle_guest_count") => add(input.is_multi && input.is_host == Some(false)),
        Some("multi_battle_mvp_count") => add(input.is_multi && input.is_mvp == Some(true)),
        Some("attention_quest_clear_count") => add(input.is_multi && participation.0),
        Some("multi_battle_newbie_count") => add(input.is_multi && participation.1),
        Some("send_emotion_count") => statistics::sent_emotions(statistics)?.map(Add),
        Some("battle_clear_count" | "battle_clear_with_specific_characters" | "battle_clear_with_specific_races") => add(true),
        Some("ss_rank_count") => add(clear_rank >= 5),
        Some("single_battle_clear_time") => add(!input.is_multi && mission.stages.iter().filter_map(|stage| stage.clear_seconds)
            .any(|seconds| input.elapsed_time_ms <= seconds.saturating_mul(1000))),
        Some("battle_clear_time") => Some(Milliseconds(input.elapsed_time_ms)),
        Some("high_score_achievement") => Some(Maximum(input.score)),
        Some("max_power_achievement") => conditions::battle_power(root, input, statistics).map(Maximum),
        Some("max_combo_achievement") => input.max_combo_count.map(Maximum),
        Some("max_skill_chain_achievement") => input.max_skill_chain_count.map(Maximum),
        Some("battle_zone_statistics_count") => statistics::total(statistics, mission.statistics_kind)?.map(|value| {
            if mission.statistics_kind == Some(15) { Milliseconds(value) } else { Add(value) }
        }),
        Some("battle_zone_statistics_max_value") => statistics::maximum(statistics, mission.statistics_max_kind)?.map(Maximum),
        Some("battle_zone_statistics_no_more_than_zero") => add(statistics::is_zero(statistics, mission.statistics_kind)),
        Some("ss_rank_clear_with_not_received_resistance_debuff") => add(clear_rank >= 5 && statistics::has_no_resistance_debuff(statistics)),
        Some("battle_client_check") => add(clear_rank >= 5 && mission.client_check.as_ref().is_some_and(|expected| {
            statistics.get("client_checks").and_then(Value::as_array).into_iter().flatten().any(|value| value.as_str() == Some(expected))
        })),
        Some("total_attained_drop_mana_count") => Some(Add(input.add_mana)),
        _ => None,
    })
}

fn advance(root: &mut Map<String, Value>, key: &str, previous: i64, change: ProgressChange) -> Result<i64, PersonalServiceError> {
    let progress = root.entry("mission_window_progress".to_owned()).or_insert_with(|| json!({})).as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN mission progress is invalid"))?;
    let value = match change {
        ProgressChange::Add(amount) => previous.checked_add(amount.max(0)),
        ProgressChange::Maximum(value) => Some(previous.max(value)),
        ProgressChange::Milliseconds(amount) => {
            let millis_key = format!("{key}:milliseconds");
            let previous_ms = progress.get(&millis_key).and_then(Value::as_i64).or_else(|| previous.checked_mul(1000));
            let Some(milliseconds) = previous_ms.and_then(|previous| previous.checked_add(amount.max(0))) else {
                return Err(PersonalServiceError::new("CN mission time exceeds supported range"));
            };
            progress.insert(millis_key, json!(milliseconds));
            Some(milliseconds / 1000)
        }
    }.ok_or_else(|| PersonalServiceError::new("CN mission progress exceeds supported range"))?;
    progress.insert(key.to_owned(), json!(value));
    Ok(value)
}

// //// 按实际通关条件累计普通任务并发放阶段奖励 [@x380kkm 2026-09-22] ////
pub(crate) fn record_completed_battle_actions(
    root: &mut Map<String, Value>, database: &mut ServiceDatabase, account_id: i64,
    quest: &BattleQuest, input: &FinishBattleInput<'_>, statistics: &Value, clear_rank: i64,
) -> Result<Vec<Value>, PersonalServiceError> {
    let participation = if input.is_multi {
        match database.get_active_single_quest(account_id)? {
            Some(active) => database.multiplayer_battle_participation(account_id, &active.play_id)?,
            None => (false, false),
        }
    } else { (false, false) };
    let mut mission_info = record_battle_changes(root, database, account_id, quest, input, |root, mission| {
        progress_change(root, mission, input, statistics, clear_rank, participation)
    })?;
    if let Some(stamina) = root.get("user_info").and_then(|user| user.get("total_stamina_used")).and_then(Value::as_i64) {
        mission_info.extend(super::action::record_battle_stamina_progress(root, database, account_id, stamina)?);
    }
    Ok(mission_info)
}
// //// /按实际通关条件累计普通任务并发放阶段奖励 ////

// //// 按主表记录竞速已完成阶段并发放任务奖励 [@x380kkm 2026-09-22] ////
pub(crate) fn record_battle_phase_actions(
    root: &mut Map<String, Value>, database: &mut ServiceDatabase, account_id: i64,
    quest: &BattleQuest, input: &FinishBattleInput<'_>, statistics: &Value,
) -> Result<Vec<Value>, PersonalServiceError> {
    let fixture = conditions::battle_fixture()?;
    let Some(definition) = fixture.reward_rules.phase_quests.get(&format!("{}:{}", quest.category, quest.quest_id)) else { return Ok(Vec::new()); };
    let reported = statistics.get("clear_phase").and_then(Value::as_i64).unwrap_or_default();
    let completed = definition.phases.iter().filter(|phase| input.is_accomplished || phase.phase <= reported)
        .map(|phase| phase.phase).max().unwrap_or_default();
    if completed == 0 { return Ok(Vec::new()); }
    record_battle_changes(root, database, account_id, quest, input, |_, mission| {
        let required = match mission.pattern.as_deref() {
            Some("time_attack_clear_phase_1") => 1,
            Some("time_attack_clear_phase_2") => 2,
            Some("time_attack_clear_phase_3") => 3,
            Some("time_attack_clear_phase_4") => 4,
            _ => return Ok(None),
        };
        Ok(Some(ProgressChange::Add(i64::from(completed >= required))))
    })
}
// //// /按主表记录竞速已完成阶段并发放任务奖励 ////

fn record_battle_changes(
    root: &mut Map<String, Value>, database: &mut ServiceDatabase, account_id: i64,
    quest: &BattleQuest, input: &FinishBattleInput<'_>,
    evaluate: impl Fn(&Map<String, Value>, &MissionDefinition) -> Result<Option<ProgressChange>, PersonalServiceError>,
) -> Result<Vec<Value>, PersonalServiceError> {
    let catalog = mission_catalog()?;
    let items_before = super::capture_collect_item_counts(root)?;
    let stored = database.mission_progress(account_id)?;
    let now = database.current_server_time_millis()?.div_euclid(1000);
    let window = battle_mission_window(root, database, account_id, now)?;
    let mut updates = BTreeMap::new();
    let mut mission_info = Vec::new();
    let mut receipts = Vec::new();
    for (category, missions) in &catalog.categories {
        for mission in missions {
            if !progress::quest_conditions(mission).matches(quest.category, quest.quest_id)? || !party_matches(mission, input)? { continue; }
            let key = MissionKey { category: *category, mission_id: mission.id };
            let Some(scope) = mission_scope_for_battle(key, mission, database, &window, now)? else { continue; };
            if !super::dependency_received_in_scope(root, key, mission, database, &scope)? { continue; }
            let Some(change) = evaluate(root, mission)? else { continue; };
            if matches!(&change, ProgressChange::Add(value) | ProgressChange::Milliseconds(value) if *value <= 0) { continue; }
            let previous = root.get("mission_window_progress").and_then(|values| values.get(&scope.key)).and_then(Value::as_i64)
                .unwrap_or_else(|| if scope.is_lifetime { stored.get(&(*category, mission.id)).copied().unwrap_or_default() } else { 0 });
            let value = advance(root, &scope.key, previous, change)?;
            updates.insert((*category, mission.id), value);
            for stage in mission.stages.iter().filter(|stage| stage.target.is_some_and(|target| value >= target)) {
                let receipt = scope.receipt(root, stage.stage);
                if settle_mission_stage(root, key, stage, &receipt, now)?.is_some() {
                    receipts.push(receipt);
                    mission_info.push(json!({"mission_category_id": category, "mission_id": mission.id, "mission_reward_id":stage.reward_id}));
                }
            }
        }
    }
    let context = super::build_context(root, catalog, &database.mission_counters(account_id)?)?;
    for (category, missions) in &catalog.categories {
        for mission in missions.iter().filter(|mission| matches!(mission.pattern.as_deref(), Some("chapter_clear" | "target_mission_clear"))) {
            let key = MissionKey { category: *category, mission_id: mission.id };
            let Some(scope) = mission_scope_for_battle(key, mission, database, &window, now)? else { continue; };
            if !super::dependency_received_in_scope(root, key, mission, database, &scope)? { continue; }
            let value = if scope.is_lifetime {
                super::compute_progress(*category, mission, &context, &BTreeMap::new(), 0, catalog)
            } else { super::scoped_progress(root, key, mission, &scope, database)? };
            for stage in mission.stages.iter().filter(|stage| stage.target.is_some_and(|target| value >= target)) {
                let receipt = scope.receipt(root, stage.stage);
                if settle_mission_stage(root, key, stage, &receipt, now)?.is_some() {
                    receipts.push(receipt);
                    updates.insert((*category, mission.id), value);
                    mission_info.push(json!({"mission_category_id": category, "mission_id": mission.id, "mission_reward_id":stage.reward_id}));
                }
            }
        }
    }
    mission_info.extend(super::record_battle_collected_items(root, database, account_id, &items_before)?);
    if !receipts.is_empty() {
        let snapshot = serde_json::to_string(root).map_err(|error| PersonalServiceError::new(format!("failed to encode mission state: {error}")))?;
        database.save_mission_rewards_with_receive_history(account_id, &snapshot, &updates, &mission_reward_event_key(&receipts), now, &[])?;
    } else {
        database.set_mission_progress(account_id, &BTreeMap::new(), &updates)?;
    }
    Ok(mission_info)
}
