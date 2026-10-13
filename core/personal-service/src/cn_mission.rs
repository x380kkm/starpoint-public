// audience: internal
// # personal-service-cn-mission
//
// 该模块按 CN mission master 计算任务进度和阶段. 阶段奖励与领取记录使用同一事务提交.

mod action;
mod activity_actions;
pub(crate) use activity_actions::{record_election_vote_action, record_player_history_action, record_raid_party_actions, record_raid_top_action};
mod battle_actions;
mod battle_time;
pub(crate) use battle_time::{capture_battle_mission_window, battle_mission_window, activity_window_status, BattleMissionWindow};
mod progress;
mod query;
pub(crate) use query::project_progress;
mod awake;
pub(crate) use awake::{awake_stage_received, character_awake_qualification, inject_load_awake_summary};
mod economy;
mod history;
pub(crate) mod conditions;
mod active;
mod collect;
mod login;
mod reward;
mod window;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod battle_tests;
#[cfg(test)]
mod battle_window_tests;
#[cfg(test)]
mod collection_tests;

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, player_snapshot,
};
use crate::database::{ReceiveHistoryEntry, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

pub(crate) use action::{
    record_bond_token_action, record_character_level_action,
    record_character_over_limit_action, record_equipment_upgrade_action,
    record_mana_board_open_action, record_mana_node_action,
    record_story_action,
};
pub(crate) use battle_actions::{record_battle_phase_actions, record_completed_battle_actions};
pub(crate) use economy::{plan_economic_missions, record_economic_actions, record_acquired_mana, total_mana, EconomicActions};
pub(crate) use history::collect_mission_reward_history;
pub(crate) use reward::{apply_mission_reward_response, apply_reward_response, settle_rewards};
pub(crate) use active::{record_active_counters, record_active_campaign_action, refresh_active_missions};
pub(crate) use active::{record_active_battle, record_active_quest_challenge, record_active_quest_clear};
pub(crate) use collect::{capture_collect_item_counts, record_collected_items, record_battle_collected_items, plan_collect_item_missions, record_collected_item_rewards};
use window::{mission_scope, scoped_progress};
pub(crate) use login::record_activity_login_days;

const MAX_MISSION_PARAMETERS: usize = 100;
const MISSION_MASTER: &str = include_str!("../assets/cn-mission-master.json");
static MISSION_CATALOG: OnceLock<Result<MissionCatalog, String>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct MissionKey {
    category: i64,
    mission_id: i64,
}

struct MissionCatalog {
    categories: BTreeMap<i64, Vec<MissionDefinition>>,
    pattern_index: BTreeMap<String, Vec<MissionKey>>,
    character_stories: BTreeMap<String, Vec<i64>>,
    rank_thresholds: Vec<RankThreshold>,
}

struct MissionDefinition {
    id: i64,
    event_id: Option<i64>,
    start_time: Option<i64>,
    end_time: Option<i64>,
    pattern: Option<String>,
    degree_target: Option<i64>,
    quest_categories: Vec<i64>,
    quest_kind: Option<i64>,
    quest_parts: [Vec<i64>; 3],
    quest_rank: Option<i64>,
    battle_kind: Option<i64>,
    statistics_kind: Option<i64>,
    statistics_max_kind: Option<i64>,
    leader_character_id: Option<i64>,
    required_character_ids: Vec<i64>,
    required_races: Vec<String>,
    mission_ids: Vec<i64>,
    item_id: Option<i64>,
    character_id: Option<i64>,
    character_quest_ids: Vec<i64>,
    client_check: Option<String>,
    need_mission_id: Option<i64>,
    need_mission_stage: Option<i64>,
    stages: Vec<MissionStage>,
}

#[derive(Deserialize)]
struct MissionMasterDocument {
    categories: BTreeMap<String, Vec<MissionDocument>>,
    character_stories: BTreeMap<String, Vec<i64>>,
    rank_thresholds: Vec<RankThreshold>,
}

#[derive(Deserialize)]
struct MissionDocument {
    id: i64,
    event_id: Option<i64>,
    start_time: Option<i64>,
    end_time: Option<i64>,
    pattern: Option<String>,
    degree_target: Option<i64>,
    #[serde(default)]
    quest_categories: Vec<i64>,
    quest_kind: Option<i64>,
    #[serde(default)]
    quest_parts: [Vec<i64>; 3],
    quest_rank: Option<i64>,
    #[serde(default)]
    battle_kind: Option<i64>,
    #[serde(default)]
    statistics_kind: Option<i64>,
    statistics_max_kind: Option<i64>,
    #[serde(default)]
    leader_character_id: Option<i64>,
    #[serde(default)]
    required_character_ids: Vec<i64>,
    #[serde(default)]
    required_races: Vec<String>,
    #[serde(default)]
    mission_ids: Vec<i64>,
    item_id: Option<i64>,
    character_id: Option<i64>,
    #[serde(default)]
    character_quest_ids: Vec<i64>,
    client_check: Option<String>,
    need_mission_id: Option<i64>,
    need_mission_stage: Option<i64>,
    stages: Vec<MissionStage>,
}

#[derive(Deserialize)]
struct MissionStage {
    stage: i64,
    reward_id: i64,
    target: Option<i64>,
    clear_seconds: Option<i64>,
    #[serde(default)]
    rewards: Vec<MissionReward>,
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
pub(crate) struct MissionReward {
    pub(crate) kind: i64,
    pub(crate) amount: i64,
    pub(crate) item_id: Option<i64>,
    pub(crate) character_id: Option<i64>,
    pub(crate) equipment_id: Option<i64>,
    pub(crate) degree_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) pass_card_event_id: Option<i64>,
}

#[derive(Deserialize)]
struct RankThreshold {
    degree: i64,
    threshold: i64,
}

struct ComputeContext {
    characters: BTreeMap<i64, progress::CharacterProgress>,
    equipment_count: i64,
    fully_awakened_equipment_count: i64,
    window_progress: BTreeMap<String, i64>,
    finished_quests: BTreeSet<i64>,
    quest_progress: BTreeMap<i64, Vec<QuestProgressEntry>>,
    character_clears: BTreeMap<String, i64>,
    leader_clears: BTreeMap<String, i64>,
    leader_multi_clears: BTreeMap<String, i64>,
    leader_powerflips: BTreeMap<String, i64>,
    co_clears: BTreeMap<String, i64>,
    rank_counts: BTreeMap<&'static str, i64>,
    rank_degree: i64,
    total_quest_clears: i64,
    total_stamina_used: i64,
    total_stories: i64,
}

struct QuestProgressEntry {
    quest_id: i64,
    best_elapsed_time_ms: Option<i64>,
    leader_character_id: Option<i64>,
    multi_clear_count: Option<i64>,
    clear_rank: Option<i64>,
    best_leader_character_id: Option<i64>,
}

#[derive(Deserialize)]
pub(crate) struct MissionCategory {
    pub(crate) category: i64,
    pub(crate) event_id: Option<i64>,
    #[serde(default)]
    character_id: Option<Value>,
}

impl MissionCategory {
    pub(crate) fn is_valid(&self) -> bool {
        self.category > 0 && (self.category != 4 || self.event_id.is_some_and(|id| id > 0))
    }
}

#[derive(Deserialize)]
struct GetMissionProgressRequest {
    viewer_id: i64,
    api_count: i64,
    category_list: Option<Vec<MissionCategory>>,
}

#[derive(Deserialize)]
struct MissionParameter {
    progress_value: i64,
    mission_pattern: String,
}

#[derive(Deserialize)]
struct UpdateMissionProgressRequest {
    viewer_id: i64,
    api_count: i64,
    mission_param_list: Vec<MissionParameter>,
}

// //// 分派 CN 任务进度请求 [@x380kkm 2026-07-24] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    let response = match request.path() {
        "/api/index.php/mission/get_mission_progress" => get_progress(request, database),
        "/api/index.php/mission/update_mission_progress" => update_progress(request, database),
        _ => return None,
    };
    Some(response)
}
// //// /分派 CN 任务进度请求 ////

// //// 计算任务进度并结算新完成阶段 [@x380kkm 2026-08-22] ////
fn get_progress(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<GetMissionProgressRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.api_count >= 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let categories = body.category_list.unwrap_or_else(|| {
        vec![MissionCategory {
            category: 1,
            event_id: None,
            character_id: None,
        }]
    });
    if categories.iter().any(|entry| !entry.is_valid()) {
        return Ok(error_response("400 Bad Request", "invalid_request_body"));
    }
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let response_time = server_time(database)?;
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = player_data
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN player data is not an object"))?;
    let evaluator = query::ProgressEvaluator::new(root, database, snapshot.account_id)?;
    let mana_before = total_mana(root)?;
    let items_before = capture_collect_item_counts(root)?;
    window::record_login_day(root, response_time)?;
    record_activity_login_days(root, database, response_time)?;
    let mut mission_info = Vec::new();
    let mut rewarded_progress = BTreeMap::new();
    let mut rewarded_receipts = Vec::new();
    let mut snapshot_changed = false;

    for (key, mission) in evaluator.entries(&categories) {
            let entry = evaluator.evaluate(root, database, key, mission)?;
            let query::EvaluatedMission { key, mission, scope, progress } = entry;
            let Some(scope) = scope else { continue; };
            if !dependency_received(root, key, mission, database)? { continue; }
            for stage in mission
                .stages
                .iter()
                .filter(|stage| stage.target.is_some_and(|target| progress >= target))
            {
                let receipt = scope.receipt(root, stage.stage);
                if settle_mission_stage(root, key, stage, &receipt, response_time)?.is_none() { continue; }
                mission_info.push(json!({"mission_category_id":key.category,"mission_id":mission.id,"mission_reward_id":stage.reward_id}));
                rewarded_receipts.push(receipt);
                rewarded_progress.insert((key.category, key.mission_id), progress);
                snapshot_changed = true;
            }
    }

    mission_info.extend(record_collected_items(root, database, snapshot.account_id, &items_before)?);
    let acquired_mana = total_mana(root)?.saturating_sub(mana_before);
    mission_info.extend(record_acquired_mana(root, database, snapshot.account_id, acquired_mana, response_time)?);
    let evaluator = query::ProgressEvaluator::new(root, database, snapshot.account_id)?;
    let mission_progress_list = evaluator.entries(&categories).into_iter()
        .map(|(key, mission)| Ok(evaluator.evaluate(root, database, key, mission)?.response()))
        .collect::<Result<Vec<_>, PersonalServiceError>>()?;
    let mut response = json!({"mission_progress_list": mission_progress_list,"mail_arrived": database.has_unreceived_mail(snapshot.account_id, response_time)?});
    apply_mission_reward_response(&mut response, root, &mission_info, body.viewer_id, response_time)?;
    if snapshot_changed {
        database.save_mission_rewards_with_receive_history(
            snapshot.account_id,
            &encode_player_data(&player_data)?,
            &rewarded_progress,
            &mission_reward_event_key(&rewarded_receipts),
            response_time,
            &[],
        )?;
    } else {
        database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    }
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        response,
    )
}
// //// /计算任务进度并结算新完成阶段 ////

// //// 按 mission master 模式保存匹配任务进度 [@x380kkm 2026-08-22] ////
fn update_progress(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<UpdateMissionProgressRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.api_count >= 0
                && body.mission_param_list.len() <= MAX_MISSION_PARAMETERS =>
        {
            body
        }
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let counter_updates = match validated_pattern_values(body.mission_param_list) {
        Ok(counters) => counters,
        Err(response) => return Ok(response),
    };
    let response_time = server_time(database)?;
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = player_data.as_object_mut().ok_or_else(|| PersonalServiceError::new("stored CN player data is invalid"))?;
    let mut mission_info = Vec::new();
    for (pattern, value) in counter_updates {
        let progress = database.mission_counters(snapshot.account_id)?.get(&pattern).copied().unwrap_or_default()
            .checked_add(value).ok_or_else(|| PersonalServiceError::new("CN mission progress exceeds range"))?;
        mission_info.extend(action::record_pattern_progress(root, database, snapshot.account_id, &pattern, progress)?);
    }
    let mail_arrived = database.has_unreceived_mail(snapshot.account_id, response_time)?;
    let mut response = json!({"mission_info": mission_info,"degree_list": [],"mail_arrived": mail_arrived});
    let completed = response["mission_info"].as_array().cloned().unwrap_or_default();
    apply_mission_reward_response(&mut response, root, &completed, body.viewer_id, response_time)?;
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        response,
    )
}
// //// /按 mission master 模式保存匹配任务进度 ////

fn validated_pattern_values(
    parameters: Vec<MissionParameter>,
) -> Result<BTreeMap<String, i64>, HttpResponse> {
    let mut values = BTreeMap::new();
    for parameter in parameters {
        let pattern = parameter.mission_pattern.trim();
        if pattern.is_empty()
            || pattern.chars().count() > 128
            || pattern.chars().any(char::is_control)
            || parameter.progress_value < 0
        {
            return Err(error_response(
                "400 Bad Request",
                "invalid_mission_parameter",
            ));
        }
        if !is_client_increment_pattern(pattern) {
            return Err(error_response("400 Bad Request", "invalid_mission_parameter"));
        }
        let accumulated = values.entry(pattern.to_owned()).or_insert(0_i64);
        *accumulated = accumulated.checked_add(parameter.progress_value)
            .ok_or_else(|| error_response("400 Bad Request", "invalid_mission_parameter"))?;
    }
    Ok(values)
}

fn is_client_increment_pattern(pattern: &str) -> bool {
    matches!(pattern, "character_detail_zoom_illust_for_1min_count" | "character_detail_play_dot_sp_motion_count" | "home_tap_town_character_count" | "home_change_voice_count" | "twitter_check")
}


fn mission_catalog() -> Result<&'static MissionCatalog, PersonalServiceError> {
    MISSION_CATALOG
        .get_or_init(build_mission_catalog)
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

// //// 根据玩家累计经验读取当前等级 [@x380kkm 2026-09-22] ////
pub(crate) fn player_rank(root: &Map<String, Value>) -> Result<i64, PersonalServiceError> {
    let rank_point = root.get("user_info").and_then(|user| user.get("rank_point"))
        .and_then(Value::as_i64).unwrap_or_default();
    Ok(rank_degree(rank_point, &mission_catalog()?.rank_thresholds))
}

// //// 返回角色觉醒任务的阶段奖励 [@x380kkm 2026-08-24] ////
pub(crate) fn awake_mission_stage_rewards(
    mission_id: i64,
    stage: i64,
) -> Result<&'static [MissionReward], PersonalServiceError> {
    let rewards = mission_catalog()?
        .categories
        .get(&9)
        .and_then(|missions| missions.iter().find(|mission| mission.id == mission_id))
        .and_then(|mission| mission.stages.iter().find(|entry| entry.stage == stage))
        .map(|entry| entry.rewards.as_slice())
        .unwrap_or_default();
    Ok(rewards)
}
// //// /返回角色觉醒任务的阶段奖励 ////

pub(crate) fn awake_mission_progress(
    root: &Map<String, Value>, mission_id: i64, stage: i64,
) -> Result<Option<(i64, i64)>, PersonalServiceError> {
    let catalog = mission_catalog()?;
    let Some(target) = catalog.categories.get(&9)
        .and_then(|missions| missions.iter().find(|mission| mission.id == mission_id))
        .and_then(|mission| mission.stages.iter().find(|entry| entry.stage == stage))
        .and_then(|entry| entry.target) else { return Ok(None); };
    let context = build_context(root, catalog, &BTreeMap::new())?;
    let progress = awake_progress(mission_id, &context, catalog);
    Ok(Some((progress, target)))
}

fn build_mission_catalog() -> Result<MissionCatalog, String> {
    let document = serde_json::from_str::<MissionMasterDocument>(MISSION_MASTER)
        .map_err(|error| format!("failed to decode CN mission master: {error}"))?;
    let mut categories = BTreeMap::new();
    let mut pattern_index = BTreeMap::<String, Vec<MissionKey>>::new();
    for (category_text, missions) in document.categories {
        let category = category_text
            .parse::<i64>()
            .map_err(|error| format!("invalid CN mission category {category_text}: {error}"))?;
        let mut definitions = Vec::with_capacity(missions.len());
        for mission in missions {
            if let Some(pattern) = &mission.pattern {
                let pattern = if pattern == "battle_zone_statistics_count" {
                    format!("{pattern}:{}", mission.statistics_kind.unwrap_or_default())
                } else if pattern == "get_item_count" && mission.item_id.is_some() {
                    format!("{pattern}:{}", mission.item_id.unwrap())
                } else { pattern.clone() };
                pattern_index
                    .entry(pattern)
                    .or_default()
                    .push(MissionKey {
                        category,
                        mission_id: mission.id,
                    });
            }
            let event_id = if mission.pattern.as_deref() == Some("character_election_vote_count") {
                crate::cn_character_election::event_for_period(mission.start_time, mission.end_time)
                    .map_err(|error| error.to_string())?
            } else {
                mission.event_id
            };
            let mut stages = mission.stages;
            stages.sort_by(|left, right| match (left.target, right.target) {
                (Some(left), Some(right)) => left.cmp(&right),
                _ => Ordering::Equal,
            });
            definitions.push(MissionDefinition {
                id: mission.id,
                event_id,
                start_time: mission.start_time,
                end_time: mission.end_time,
                pattern: mission.pattern,
                degree_target: mission.degree_target,
                quest_categories: mission.quest_categories,
                quest_kind: mission.quest_kind,
                quest_parts: mission.quest_parts,
                quest_rank: mission.quest_rank,
                battle_kind: mission.battle_kind,
                statistics_kind: mission.statistics_kind,
                statistics_max_kind: mission.statistics_max_kind,
                leader_character_id: mission.leader_character_id,
                required_character_ids: mission.required_character_ids,
                required_races: mission.required_races,
                mission_ids: mission.mission_ids,
                item_id: mission.item_id,
                character_id: mission.character_id,
                character_quest_ids: mission.character_quest_ids,
                client_check: mission.client_check,
                need_mission_id: mission.need_mission_id,
                need_mission_stage: mission.need_mission_stage,
                stages,
            });
        }
        categories.insert(category, definitions);
    }
    Ok(MissionCatalog {
        categories,
        pattern_index,
        character_stories: document.character_stories,
        rank_thresholds: document.rank_thresholds,
    })
}

// //// 按请求条目筛选活动和角色任务 [@x380kkm 2026-09-21] ////
fn mission_matches_category(mission: &MissionDefinition, requested: &MissionCategory) -> bool {
    match requested.category {
        4 => mission.event_id == requested.event_id,
        6..=8 => requested.event_id.map_or(true, |event_id| mission.event_id == Some(event_id)),
        9 => requested
            .character_id
            .as_ref()
            .map_or(true, |character_id| {
                mission_character_id(mission.id) == value_text(character_id)
            }),
        _ => true,
    }
}
// //// /按请求条目筛选活动和角色任务 ////

fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Null => "null".to_owned(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

fn build_context(
    root: &Map<String, Value>,
    catalog: &MissionCatalog,
    counters: &BTreeMap<String, i64>,
) -> Result<ComputeContext, PersonalServiceError> {
    let mut finished_quests = BTreeSet::new();
    let mut quest_progress = BTreeMap::<i64, Vec<QuestProgressEntry>>::new();
    let mut total_quest_clears = 0;
    let mut total_stories = 0;
    let mut rank_ss = 0;
    if let Some(categories) = root.get("quest_progress").and_then(Value::as_object) {
        for (category, entries) in categories {
            let Some(entries) = entries.as_array() else {
                continue;
            };
            let category_id = category.parse::<i64>().ok();
            for entry in entries {
                if entry.get("finished").and_then(Value::as_bool) != Some(true) {
                    continue;
                }
                total_quest_clears += 1;
                if category == "3" {
                    total_stories += 1;
                }
                if let Some(quest_id) = entry.get("quest_id").and_then(Value::as_i64) {
                    finished_quests.insert(quest_id);
                    if let Some(category_id) = category_id {
                        quest_progress
                            .entry(category_id)
                            .or_default()
                            .push(QuestProgressEntry {
                                quest_id,
                                best_elapsed_time_ms: entry
                                    .get("best_elapsed_time_ms")
                                    .and_then(Value::as_i64),
                                leader_character_id: entry
                                    .get("leader_character_id")
                                    .and_then(Value::as_i64),
                                multi_clear_count: entry
                                    .get("multi_clear_count")
                                    .and_then(Value::as_i64),
                                clear_rank: entry.get("clear_rank").and_then(Value::as_i64),
                                best_leader_character_id: entry.get("best_leader_character_id").and_then(Value::as_i64),
                            });
                    }
                }
                match entry.get("clear_rank").and_then(Value::as_i64) {
                    Some(5..=6) => rank_ss += 1,
                    _ => {}
                }
            }
        }
    }
    let user_info = root
        .get("user_info")
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("stored CN user_info data is missing"))?;
    let rank_point = user_info
        .get("rank_point")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let total_stamina_used = user_info
        .get("total_stamina_used")
        .or_else(|| user_info.get("stamina_used"))
        .and_then(Value::as_i64)
        .unwrap_or_else(|| {
            counters
                .iter()
                .filter(|(pattern, _)| pattern.as_str() == "used_stamina_count")
                .map(|(_, value)| *value)
                .max()
                .unwrap_or_default()
        });
    let mut window_progress = named_counts(root, "mission_window_progress");
    awake::merge_recorded_progress(root, catalog, &mut window_progress);
    Ok(ComputeContext {
        characters: progress::character_progress(root)?,
        equipment_count: root.get("user_equipment_list").and_then(Value::as_object).map_or(0, |items| items.len() as i64),
        fully_awakened_equipment_count: conditions::fully_awakened_equipment_count(root)?,
        window_progress,
        finished_quests,
        quest_progress,
        character_clears: character_clear_counts(root),
        leader_clears: named_counts(root, "character_leader_clear_counts"),
        leader_multi_clears: named_counts(root, "character_leader_multi_clear_counts"),
        leader_powerflips: named_counts(root, "character_leader_power_flip_counts"),
        co_clears: named_counts(root, "party_member_co_clear_counts"),
        rank_counts: BTreeMap::from([("ss_rank_count", rank_ss)]),
        rank_degree: rank_degree(rank_point, &catalog.rank_thresholds),
        total_quest_clears,
        total_stamina_used,
        total_stories,
    })
}

fn character_clear_counts(root: &Map<String, Value>) -> BTreeMap<String, i64> {
    let mut counts = BTreeMap::new();
    for key in ["character_clear_counts", "player_character_clear_list"] {
        if let Some(entries) = root.get(key).and_then(Value::as_object) {
            for (character_id, value) in entries {
                if let Some(clear_count) = value
                    .as_i64()
                    .or_else(|| value.get("clear_count").and_then(Value::as_i64))
                {
                    counts.insert(character_id.clone(), clear_count);
                }
            }
        }
    }
    if let Some(characters) = root.get("user_character_list").and_then(Value::as_object) {
        for (character_id, character) in characters {
            if let Some(clear_count) = character.get("clear_count").and_then(Value::as_i64) {
                counts.insert(character_id.clone(), clear_count);
            }
        }
    }
    counts
}

fn named_counts(root: &Map<String, Value>, field: &str) -> BTreeMap<String, i64> {
    root.get(field)
        .and_then(Value::as_object)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|(key, value)| value.as_i64().map(|count| (key.clone(), count)))
                .collect()
        })
        .unwrap_or_default()
}

fn rank_degree(rank_point: i64, thresholds: &[RankThreshold]) -> i64 {
    let mut degree = 1;
    for threshold in thresholds {
        if rank_point < threshold.threshold {
            break;
        }
        degree = threshold.degree;
    }
    degree
}

fn compute_progress(
    category: i64,
    mission: &MissionDefinition,
    context: &ComputeContext,
    counters: &BTreeMap<String, i64>,
    database_progress: i64,
    catalog: &MissionCatalog,
) -> i64 {
    if category == 9 {
        return awake_progress(mission.id, context, catalog);
    }
    if let Some(value) = progress::state_progress(mission, context) { return value; }
    if let Some(value) = context.window_progress.get(&format!("{category}:{}", mission.id)) { return *value; }
    if category == 5 && mission.degree_target.is_some() {
        return context.rank_degree;
    }
    if mission.pattern.as_deref() == Some("chapter_clear") {
        return progress::quest_progress(mission, context).unwrap_or_default();
    }
    if progress::has_quest_conditions(mission) && matches!(mission.pattern.as_deref(), Some("ss_rank_count" | "single_battle_clear_time" | "battle_clear_count")) {
        return progress::quest_progress(mission, context).unwrap_or_default();
    }
    if mission.pattern.as_deref() == Some("target_mission_clear") {
        return mission.mission_ids.iter().filter(|id| {
            catalog.categories.get(&category).and_then(|missions| missions.iter().find(|target| target.id == **id))
                .is_some_and(|target| target.stages.last().and_then(|stage| stage.target).is_some_and(|threshold| {
                    compute_progress(category, target, context, counters, 0, catalog) >= threshold
                }))
        }).count() as i64;
    }
    if is_scoped_battle_mission(mission) {
        return scoped_battle_progress(mission, context, counters);
    }
    if category == 1 {
        if let Some(pattern) = mission.pattern.as_deref() {
            if pattern == "single_battle_clear_count" {
                return context.total_quest_clears.max(counters.get(pattern).copied().unwrap_or_default());
            }
            if pattern == "used_stamina_count" {
                return context.total_stamina_used;
            }
            if let Some(progress) = context.rank_counts.get(pattern) {
                return *progress;
            }
        }
    }
    if mission.pattern.as_deref() == Some("battle_zone_statistics_count") {
        return counters.get(&format!("battle_zone_statistics_count:{}", mission.statistics_kind.unwrap_or_default())).copied().unwrap_or(database_progress).max(database_progress);
    }
    mission
        .pattern
        .as_ref()
        .and_then(|pattern| counters.get(pattern))
        .copied()
        .unwrap_or(database_progress)
        .max(database_progress)
}

fn is_scoped_battle_mission(mission: &MissionDefinition) -> bool {
    progress::has_quest_conditions(mission)
        && matches!(
            mission.pattern.as_deref(),
            Some("single_battle_clear_count") | Some("multi_battle_clear_count")
        )
}

// //// 计算任务关卡范围内的战斗次数 [@x380kkm 2026-08-29] ////
fn scoped_battle_progress(
    mission: &MissionDefinition,
    context: &ComputeContext,
    counters: &BTreeMap<String, i64>,
) -> i64 {
    let pattern = mission.pattern.as_deref().unwrap_or_default();
    if mission.quest_rank.is_some() || mission.quest_parts.iter().any(|parts| !parts.is_empty()) {
        return progress::quest_progress(mission, context).unwrap_or_default();
    }
    mission
        .quest_categories
        .iter()
        .map(|quest_category| {
            counters
                .get(&scoped_battle_counter_key(pattern, *quest_category))
                .copied()
                .unwrap_or_else(|| battle_progress_for_category(context, pattern, *quest_category))
        })
        .fold(0, i64::saturating_add)
}

pub(super) fn scoped_battle_counter_key(pattern: &str, quest_category: i64) -> String {
    format!("{pattern}:quest_category:{quest_category}")
}

fn battle_progress_for_category(
    context: &ComputeContext,
    pattern: &str,
    quest_category: i64,
) -> i64 {
    context
        .quest_progress
        .get(&quest_category)
        .map_or(0, |entries| {
            if pattern == "multi_battle_clear_count" {
                entries.iter().fold(0, |total, entry| {
                    total.saturating_add(entry.multi_clear_count.unwrap_or(1).max(0))
                })
            } else {
                i64::try_from(entries.len()).unwrap_or(i64::MAX)
            }
        })
}
// //// /计算任务关卡范围内的战斗次数 ////

fn awake_progress(mission_id: i64, context: &ComputeContext, catalog: &MissionCatalog) -> i64 {
    let Some(mission) = catalog.categories.get(&9)
        .and_then(|missions| missions.iter().find(|mission| mission.id == mission_id)) else { return 0; };
    let recorded = context.window_progress.get(&format!("9:{mission_id}")).copied().unwrap_or_default();
    recorded.max(current_awake_progress(mission, context, catalog))
}

// //// 按当前角色与通关状态计算觉醒任务进度 [@x380kkm 2026-09-29] ////
fn current_awake_progress(mission: &MissionDefinition, context: &ComputeContext, catalog: &MissionCatalog) -> i64 {
    if let Some(value) = progress::state_progress(mission, context) { return value; }
    let mission_id = mission.id;
    let character_id = mission_character_id(mission_id);
    match mission.pattern.as_deref() {
        Some("character_quest_finish") => {
            let quests = if mission.character_quest_ids.is_empty() {
                catalog.character_stories.get(&character_id).map(Vec::as_slice).unwrap_or_default()
            } else { &mission.character_quest_ids };
            quests.iter().filter(|id| context.finished_quests.contains(id)).count() as i64
        }
        Some("episode_clear_count") => context.total_stories,
        Some("target_mission_clear") => mission.mission_ids.iter().filter(|id| {
            catalog.categories[&9].iter().find(|target| target.id == **id)
                .and_then(|target| target.stages.last()).and_then(|stage| stage.target)
                .is_some_and(|target| awake_progress(**id, context, catalog) >= target)
        }).count() as i64,
        Some("battle_clear_with_specific_characters" | "battle_clear_count" | "single_battle_clear_time" | "ss_rank_count")
            if progress::has_quest_conditions(mission) => progress::quest_progress(mission, context).unwrap_or_default(),
        Some("battle_clear_with_specific_characters" | "battle_clear_count") => {
            if !mission.required_character_ids.is_empty() {
                if mission.leader_character_id.is_some() { return 0; }
                if mission.required_character_ids.len() > 2 { return 0; }
                if mission.required_character_ids.len() == 2 {
                    return minimum_co_clears(&mission.required_character_ids, &context.co_clears);
                }
                return context.character_clears.get(&mission.required_character_ids[0].to_string()).copied().unwrap_or_default();
            }
            mission.leader_character_id.and_then(|id| {
                let counts = if mission.battle_kind == Some(2) { &context.leader_multi_clears } else { &context.leader_clears };
                counts.get(&id.to_string()).copied()
            }).unwrap_or_default()
        }
        Some("battle_zone_statistics_count") if mission.statistics_kind == Some(1) => mission.leader_character_id
            .and_then(|id| context.leader_powerflips.get(&id.to_string()).copied()).unwrap_or_default(),
        _ => 0,
    }
}

fn minimum_co_clears(character_ids: &[i64], co_clears: &BTreeMap<String, i64>) -> i64 {
    let mut minimum = None;
    for (index, character_id) in character_ids.iter().enumerate() {
        for other_character_id in &character_ids[index + 1..] {
            let key = if character_id < other_character_id {
                format!("{character_id}_{other_character_id}")
            } else {
                format!("{other_character_id}_{character_id}")
            };
            let count = co_clears.get(&key).copied().unwrap_or_default();
            minimum = Some(minimum.map_or(count, |current: i64| current.min(count)));
        }
    }
    minimum.unwrap_or_default()
}


fn mission_character_id(mission_id: i64) -> String {
    let text = mission_id.to_string();
    text.get(..text.len().saturating_sub(1))
        .unwrap_or(&text)
        .to_owned()
}

fn current_stage(mission: &MissionDefinition, progress: i64) -> i64 {
    let mut current = mission.stages.last().map_or(1, |stage| stage.stage);
    for stage in &mission.stages {
        if stage.target.is_some_and(|target| progress < target) {
            current = stage.stage;
            break;
        }
    }
    current
}

fn dependency_received(root: &Map<String, Value>, key: MissionKey, mission: &MissionDefinition, database: &ServiceDatabase) -> Result<bool, PersonalServiceError> {
    dependency_received_at(root, key, mission, database, database.current_server_time_millis()?.div_euclid(1000))
}

fn dependency_received_at(root: &Map<String, Value>, key: MissionKey, mission: &MissionDefinition, database: &ServiceDatabase, at: i64) -> Result<bool, PersonalServiceError> {
    if mission.need_mission_id.is_none() { return Ok(true); }
    let Some(scope) = window::mission_scope_at(key, mission, database, at)? else { return Ok(false); };
    dependency_received_in_scope(root, key, mission, database, &scope)
}

fn dependency_received_in_scope(root: &Map<String, Value>, key: MissionKey, mission: &MissionDefinition, database: &ServiceDatabase, scope: &window::MissionScope) -> Result<bool, PersonalServiceError> {
    let Some(id) = mission.need_mission_id else { return Ok(true); };
    let Some(target) = mission_catalog()?.categories.get(&key.category).and_then(|missions| missions.iter().find(|target| target.id == id)) else { return Ok(false); };
    let Some(scope) = scope.related(MissionKey { category: key.category, mission_id: id }, target, database)? else { return Ok(false); };
    let receipt = scope.receipt(root, mission.need_mission_stage.unwrap_or(1));
    Ok(root.get("mission_stage_receipts").and_then(|receipts| receipts.get(receipt)).is_some())
}

// //// 按阶段凭据结算普通和觉醒任务奖励 [@x380kkm 2026-09-21] ////
fn settle_mission_stage(
    root: &mut Map<String, Value>, key: MissionKey, stage: &MissionStage, receipt: &str, response_time: i64,
) -> Result<Option<Vec<ReceiveHistoryEntry>>, PersonalServiceError> {
    if key.category == 9 {
        if awake::has_active_receipt(root, key.mission_id, stage.stage) {
            return Ok(None);
        }
    }
    let recorded = if key.category == 9 { stage.rewards.as_slice() } else { &[] };
    let history = settle_rewards(root, "mission_stage_receipts", receipt, &stage.rewards, recorded, response_time)?;
    if key.category == 1 {
        let completed = root.entry("cleared_regular_mission_list".to_owned()).or_insert_with(|| json!({}))
            .as_object_mut().ok_or_else(|| PersonalServiceError::new("stored CN regular mission stages are invalid"))?;
        let previous = completed.get(&key.mission_id.to_string()).and_then(Value::as_i64).unwrap_or_default();
        completed.insert(key.mission_id.to_string(), Value::from(previous.max(stage.stage)));
    }
    Ok(history)
}
// //// /按阶段凭据结算普通和觉醒任务奖励 ////

// //// 按明确的任务领取凭据恢复未结算奖励 [@x380kkm 2026-09-21] ////
pub(crate) fn recover_recorded_mission_rewards(
    root: &mut Map<String, Value>, response_time: i64,
) -> Result<(), PersonalServiceError> {
    window::record_login_day(root, response_time)?;
    let receipts = root.get("mission_stage_receipts").and_then(Value::as_object).cloned().unwrap_or_default();
    let catalog = mission_catalog()?;
    for (receipt, value) in receipts {
        if value != Value::Bool(true) { continue; }
        let components = receipt.split(':').filter_map(|part| part.parse::<i64>().ok()).collect::<Vec<_>>();
        let [category, mission_id, stage_id] = components.as_slice() else { continue; };
        if !matches!(*category, 1 | 5) { continue; }
        let Some(stage) = catalog.categories.get(category)
            .and_then(|missions| missions.iter().find(|mission| mission.id == *mission_id))
            .and_then(|mission| mission.stages.iter().find(|stage| stage.stage == *stage_id)) else { continue; };
        settle_rewards(root, "mission_stage_receipts", &receipt, &stage.rewards, &[], response_time)?;
    }
    crate::cn_reference_state_misc::recover_recorded_active_mission_rewards(root, response_time)
}
// //// /按明确的任务领取凭据恢复未结算奖励 ////

// //// 清除指定活动的任务进度并返回数据库任务范围 [@x380kkm 2026-09-21] ////
pub(crate) fn reset_activity_missions(root: &mut Map<String, Value>, activity_id: &str, regional_start_ms: Option<i64>) -> Result<Vec<(i64, i64)>, PersonalServiceError> {
    let Some((family, event_id)) = activity_id.split_once(':').and_then(|(family, id)| Some((family, id.parse::<i64>().ok()?))) else { return Ok(Vec::new()); };
    if matches!(family, "collect-item" | "pass-card" | "raid" | "character-election") {
        let categories: &[i64] = match family { "collect-item" => &[4], "pass-card" => &[6, 7, 8], _ => &[3] };
        let catalog = mission_catalog()?;
        let ids = categories.iter().flat_map(|category| catalog.categories.get(category).into_iter().flatten()
            .filter(move |mission| window::mission_activity_id(*category, mission).as_deref() == Some(activity_id))
            .map(move |mission| (*category, mission.id))).collect::<BTreeSet<_>>();
        for field in ["mission_window_progress", "mission_stage_receipts", "mission_activity_login_days"] {
            if let Some(entries) = root.get_mut(field).and_then(Value::as_object_mut) {
                entries.retain(|key, _| {
                    let mut parts = key.split(':');
                    let category = parts.next().and_then(|part| part.parse::<i64>().ok());
                    let mission_id = parts.next().and_then(|part| part.parse::<i64>().ok());
                    let regional = parts.next() == Some("event") && parts.next() == Some("regional");
                    let selected = match regional_start_ms {
                        Some(start) => regional && parts.next().and_then(|part| part.parse::<i64>().ok()) == Some(start),
                        None => !regional,
                    };
                    !selected || !category.zip(mission_id).is_some_and(|key| ids.contains(&key))
                });
            }
        }
        if family == "collect-item" && regional_start_ms.is_none() {
            if let Some(entries) = root.get_mut("cleared_collect_item_event_mission_list").and_then(Value::as_object_mut) {
                entries.remove(&event_id.to_string());
                entries.retain(|key, _| key.parse::<i64>().ok().map_or(true, |id| !ids.contains(&(4, id))));
            }
        }
        return Ok(if regional_start_ms.is_none() { ids.into_iter().collect() } else { Vec::new() });
    }
    if family == "active-mission" {
        let ids = crate::cn_reference_state_misc::active_mission_ids(event_id)?.into_iter().collect::<BTreeSet<_>>();
        for field in ["all_active_mission_list", "active_mission_receipts", "active_mission_incentive_receipts", "win_lottery_active_mission_list"] {
            if let Some(entries) = root.get_mut(field).and_then(Value::as_object_mut) {
                entries.retain(|key, _| key.split(':').next().and_then(|part| part.parse::<i64>().ok()).map_or(true, |id| !ids.contains(&id)));
            }
        }
        if let Some(entries) = root.get_mut("active_mission_list").and_then(Value::as_array_mut) {
            entries.retain(|entry| entry.get("mission_id").and_then(Value::as_i64).map_or(true, |id| !ids.contains(&id)));
        }
        if let Some(entries) = root.get_mut("active_mission_login_days").and_then(Value::as_object_mut) {
            entries.remove(&event_id.to_string());
        }
    }
    Ok(Vec::new())
}
// //// /清除指定活动的任务进度并返回数据库任务范围 ////

// //// 从任务凭据解析主表定义的活动归属 [@x380kkm 2026-09-21] ////
pub(crate) fn receipt_activity_id(receipt_field: &str, receipt_key: &str) -> Result<Option<String>, PersonalServiceError> {
    let mut parts = receipt_key.split(':');
    if receipt_field == "mission_stage_receipts" {
        let Some(category) = parts.next().and_then(|part| part.parse::<i64>().ok()) else { return Ok(None); };
        let mission_id = parts.next().and_then(|part| part.parse::<i64>().ok());
        return Ok(mission_catalog()?.categories.get(&category).into_iter().flatten()
            .find(|mission| Some(mission.id) == mission_id)
            .and_then(|mission| window::mission_activity_id(category, mission)));
    }
    if matches!(receipt_field, "active_mission_receipts" | "active_mission_incentive_receipts") {
        if let Some(mission_id) = parts.next().and_then(|part| part.parse::<i64>().ok()) {
            return Ok(crate::cn_reference_state_misc::active_mission_event_id(mission_id)?.map(|event_id| format!("active-mission:{event_id}")));
        }
    }
    Ok(None)
}
// //// /从任务凭据解析主表定义的活动归属 ////

fn mission_reward_event_key(receipts: &[String]) -> String {
    let digest = Sha256::digest(receipts.join(",").as_bytes());
    format!("mission:{digest:x}")
}


fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // //// 应用任务等级目标并保证阶段奖励幂等 [@x380kkm 2026-08-23] ////
    #[test]
    fn applies_mission_rewards_once_per_receipt() {
        let mut root = Map::from_iter([
            ("item_list".to_owned(), json!({})),
            (
                "user_info".to_owned(),
                json!({"free_mana": 10, "exp_pool": 20}),
            ),
            ("user_character_list".to_owned(), json!({})),
        ]);
        let catalog = mission_catalog().unwrap();
        let rank_mission = catalog.categories[&5]
            .iter()
            .find(|mission| mission.id == 1_000)
            .unwrap();
        assert_eq!(rank_mission.degree_target, Some(50));
        let mission = catalog.categories[&5]
            .iter()
            .find(|mission| mission.id == 11_010)
            .unwrap();
        let stage = mission
            .stages
            .iter()
            .find(|stage| stage.stage == 1)
            .unwrap();
        assert_eq!(stage.target, Some(1));
        let receipt = "5:11010:1";
        assert!(settle_rewards(&mut root, "mission_stage_receipts", receipt, &stage.rewards, &[], 1).unwrap().is_some());
        assert!(settle_rewards(&mut root, "mission_stage_receipts", receipt, &stage.rewards, &[], 1).unwrap().is_none());
        assert_eq!(root["degree_ids"], json!([11_010]));
        assert_eq!(root["user_info"]["free_mana"], 10);
    }
    // //// /应用任务等级目标并保证阶段奖励幂等 ////

    // //// 按战斗队伍和动作统计计算觉醒任务 [@x380kkm 2026-08-25] ////
    #[test]
    fn computes_awake_progress_from_battle_contract() {
        let root = Map::from_iter([
            (
                "quest_progress".to_owned(),
                json!({
                    "2": [{
                        "quest_id": 1_028_004,
                        "finished": true,
                        "best_elapsed_time_ms": 80_000,
                        "leader_character_id": 111_001,
                    }],
                }),
            ),
            ("user_info".to_owned(), json!({"rank_point": 0})),
            (
                "character_clear_counts".to_owned(),
                json!({"151006": 3, "161002": 4}),
            ),
            (
                "character_leader_clear_counts".to_owned(),
                json!({"151006": 3, "161002": 4}),
            ),
            (
                "character_leader_multi_clear_counts".to_owned(),
                json!({"131005": 2}),
            ),
            (
                "character_leader_power_flip_counts".to_owned(),
                json!({"121001": 7}),
            ),
            (
                "party_member_co_clear_counts".to_owned(),
                json!({"151006_263002": 2, "211001_231001": 4}),
            ),
            (
                "party_race_clear_counts".to_owned(),
                json!({"Devil+Dragon+Human": 5}),
            ),
        ]);
        let catalog = mission_catalog().unwrap();
        let context = build_context(&root, catalog, &BTreeMap::new()).unwrap();
        for (mission_id, expected) in [
            (1_110_013, 1),
            (1_210_012, 7),
            (1_310_053, 2),
            (1_510_062, 0),
            (1_610_023, 4),
            (2_110_012, 4),
            (2_310_012, 0),
        ] {
            assert_eq!(awake_progress(mission_id, &context, catalog), expected, "{mission_id}");
        }
    }
    // //// /按战斗队伍和动作统计计算觉醒任务 ////

    // //// 按关卡范围计算摇曳的迷宫任务 [@x380kkm 2026-08-29] ////
    #[test]
    fn scopes_labyrinth_progress_to_matching_quest_categories() {
        let catalog = mission_catalog().unwrap();
        let mission = catalog.categories[&2]
            .iter()
            .find(|mission| mission.id == 2)
            .unwrap();
        assert_eq!(mission.quest_categories, vec![6, 14, 13, 20]);

        let main_only = Map::from_iter([
            (
                "quest_progress".to_owned(),
                json!({"1": [{"quest_id": 1_001_001, "finished": true}]}),
            ),
            ("user_info".to_owned(), json!({"rank_point": 0})),
        ]);
        let context = build_context(&main_only, catalog, &BTreeMap::new()).unwrap();
        assert_eq!(
            compute_progress(2, mission, &context, &BTreeMap::new(), 1, catalog),
            0
        );

        let labyrinth = Map::from_iter([
            (
                "quest_progress".to_owned(),
                json!({
                    "1": [{"quest_id": 1_001_001, "finished": true}],
                    "13": [{"quest_id": 1_001, "finished": true}],
                }),
            ),
            ("user_info".to_owned(), json!({"rank_point": 0})),
        ]);
        let counters = BTreeMap::from([(
            scoped_battle_counter_key("single_battle_clear_count", 13),
            3,
        )]);
        let context = build_context(&labyrinth, catalog, &counters).unwrap();
        assert_eq!(
            compute_progress(2, mission, &context, &counters, 1, catalog),
            3
        );
    }
    // //// /按关卡范围计算摇曳的迷宫任务 ////
}
