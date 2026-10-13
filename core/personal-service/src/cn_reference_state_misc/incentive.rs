// audience: internal
// # personal-service-cn-active-mission-incentive
//
// 主动任务激励按活动奖励池抽取游戏内奖励. 领取状态与奖励邮件使用同一事务保存.

use super::active_mission::{active_mission_stage_received, qualifying_incentive_stage};
use super::common::error_response;
use crate::cn::{decode_request, msgpack_response_at, msgpack_result_code_response_at, server_time};
use crate::cn_tutorial::{decode_player_data, encode_player_data, player_snapshot, require_root};
use crate::database::{evaluate_activity_schedule, ActivityMode, ActivityWindowStatus, CreateMailInput, MailReward, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

const INCENTIVES: &str = include_str!("../../assets/cn-active-mission-incentives.json");
static CATALOG: OnceLock<Result<IncentiveCatalog, String>> = OnceLock::new();

#[derive(Deserialize)]
struct IncentiveCatalog {
    missions: BTreeMap<i64, IncentiveMission>,
    events: BTreeMap<i64, IncentiveEvent>,
    rewards: Vec<IncentiveReward>,
}

#[derive(Deserialize)]
struct IncentiveMission { event_id: i64 }

#[derive(Deserialize)]
struct IncentiveEvent { end_time: Option<i64>, lottery_end_time: Option<i64> }

#[derive(Deserialize)]
struct IncentiveReward {
    id: i64,
    event_id: i64,
    weight: u64,
    items: BTreeMap<String, i64>,
    free_vmoney: i64,
    free_mana: i64,
    exp_pool: i64,
}

#[derive(Deserialize)]
struct ReceiveRequest { viewer_id: i64, mission_id: i64 }

fn catalog() -> Result<&'static IncentiveCatalog, PersonalServiceError> {
    CATALOG.get_or_init(|| serde_json::from_str(INCENTIVES).map_err(|error| error.to_string()))
        .as_ref().map_err(|error| PersonalServiceError::new(error.clone()))
}

pub(crate) fn active_mission_ids(event_id: i64) -> Result<Vec<i64>, PersonalServiceError> {
    Ok(catalog()?.missions.iter().filter_map(|(id, mission)| (mission.event_id == event_id).then_some(*id)).collect())
}

pub(crate) fn active_mission_event_id(mission_id: i64) -> Result<Option<i64>, PersonalServiceError> {
    Ok(catalog()?.missions.get(&mission_id).map(|mission| mission.event_id))
}

// //// 校验激励资格并原子投递游戏内奖励 [@x380kkm 2026-09-21] ////
pub(super) fn receive(request: &HttpRequest, database: &mut ServiceDatabase) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ReceiveRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.mission_id > 0 => body,
        _ => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? { Ok(snapshot) => snapshot, Err(response) => return Ok(response) };
    let now = server_time(database)?;
    let mut player = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player)?;
    let key = body.mission_id.to_string();
    if let Some(reward_id) = root.get("active_mission_incentive_receipts").and_then(|receipts| receipts.get(&key)).and_then(Value::as_i64) {
        return response(database, body.viewer_id, snapshot.account_id, reward_id, now);
    }
    let Some(stage) = qualifying_incentive_stage(body.mission_id)? else {
        return Ok(error_response("400 Bad Request", "mission_has_no_incentive"));
    };
    if !active_mission_stage_received(root, body.mission_id, stage)
        && root.get("active_mission_receipts").and_then(|receipts| receipts.get(format!("{}:{stage}", body.mission_id))).is_none() {
        return Ok(error_response("400 Bad Request", "mission_not_completed"));
    }
    let catalog = catalog()?;
    let Some(mission) = catalog.missions.get(&body.mission_id) else { return Ok(error_response("400 Bad Request", "mission_not_found")); };
    let activity_id = format!("active-mission:{}", mission.event_id);
    if !is_claim_period(database, &activity_id, catalog.events.get(&mission.event_id), now)? {
        return msgpack_result_code_response_at(body.viewer_id, now, 2479);
    }
    let reward = draw_reward(&catalog.rewards, mission.event_id)?;
    let participation_round = crate::activity_history::participation_round(root, &activity_id);
    let event_key = format!("active-mission:{}:{participation_round}:incentive:{}", mission.event_id, body.mission_id);
    root.entry("active_mission_incentive_receipts".to_owned()).or_insert_with(|| json!({})).as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN incentive receipts are invalid"))?.insert(key.clone(), json!(reward.id));
    let missions = root.entry("all_active_mission_list".to_owned()).or_insert_with(|| json!({})).as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN active missions are invalid"))?;
    let mission = missions.entry(key).or_insert_with(|| json!({"progress":0,"stages":{}})).as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN active mission is invalid"))?;
    mission.insert("ingame_status".to_owned(), json!(2));
    mission.insert("ingame_reward_id".to_owned(), json!(reward.id));
    database.deliver_reward_mail_with_snapshot_once(&CreateMailInput {
        account_id: snapshot.account_id,
        title: "主动任务奖励".to_owned(), body: "主动任务游戏内奖励已送达.".to_owned(), sender: "Starpoint".to_owned(),
        rewards: MailReward { free_vmoney: reward.free_vmoney, free_mana: reward.free_mana, exp_pool: reward.exp_pool, item_list: reward.items.clone(), ..MailReward::default() },
        expires_at: None, created_at: now,
    }, &encode_player_data(&player)?, &event_key, Some(&activity_id))?;
    response(database, body.viewer_id, snapshot.account_id, reward.id, now)
}
// //// /校验激励资格并原子投递游戏内奖励 ////

// //// 按抽奖统计结束和活动结束确定激励领奖窗口 [@x380kkm 2026-09-21] ////
fn is_claim_period(
    database: &ServiceDatabase, activity_id: &str, event: Option<&IncentiveEvent>, now: i64,
) -> Result<bool, PersonalServiceError> {
    use crate::activity_projection::{AGGREGATION_DELAY_MS, REWARD_DELAY_MS};
    if let Some((_, expires_at_ms)) = database.activity_temporary_reward_window(activity_id, REWARD_DELAY_MS)? {
        let wall_now = database.current_wall_time_millis()?;
        let elapsed = wall_now.saturating_sub(expires_at_ms) as f64 * database.current_server_time_rate()?;
        return Ok(wall_now < expires_at_ms || elapsed > AGGREGATION_DELAY_MS as f64);
    }
    if let Some(mut schedule) = database.get_activity_schedule(activity_id)? {
        if !schedule.enabled { return Ok(false); }
        if matches!(schedule.mode, ActivityMode::Manual | ActivityMode::Always) { return Ok(true); }
        schedule.end_at_ms = schedule.end_at_ms.saturating_add(REWARD_DELAY_MS);
        let window = evaluate_activity_schedule(&schedule, now * 1000);
        return Ok(window.status == ActivityWindowStatus::Open && window.active_end_ms.is_some_and(|end| {
            let lottery_end = end.saturating_sub(REWARD_DELAY_MS).saturating_add(AGGREGATION_DELAY_MS);
            now * 1000 > lottery_end && now * 1000 <= end
        }));
    }
    Ok(event.is_some_and(|event| {
        event.lottery_end_time.is_some_and(|end| now > end) && event.end_time.map_or(true, |end| now <= end)
    }))
}
// //// /按抽奖统计结束和活动结束确定激励领奖窗口 ////

fn response(database: &ServiceDatabase, viewer_id: i64, account_id: i64, reward_id: i64, now: i64) -> Result<HttpResponse, PersonalServiceError> {
    msgpack_response_at(viewer_id, false, now, json!({
        "active_mission_incentive":{"ingame_reward_id":reward_id,"real_reward_id":null,"url":null},
        "mail_arrived": database.has_unreceived_mail(account_id, now)?,
    }))
}

// //// 按主表权重抽取游戏内奖励 [@x380kkm 2026-09-21] ////
fn draw_reward(rewards: &[IncentiveReward], event_id: i64) -> Result<&IncentiveReward, PersonalServiceError> {
    let pool = rewards.iter().filter(|reward| reward.event_id == event_id && reward.weight > 0).collect::<Vec<_>>();
    let total = pool.iter().try_fold(0_u64, |sum, reward| sum.checked_add(reward.weight))
        .filter(|total| *total > 0).ok_or_else(|| PersonalServiceError::new("CN incentive reward weights are invalid"))?;
    let limit = u64::MAX - u64::MAX % total;
    let mut selected = loop {
        let mut bytes = [0_u8; 8];
        getrandom::getrandom(&mut bytes).map_err(|error| PersonalServiceError::new(format!("CN incentive random source failed: {error}")))?;
        let value = u64::from_le_bytes(bytes);
        if value < limit { break value % total; }
    };
    for reward in pool {
        if selected < reward.weight { return Ok(reward); }
        selected -= reward.weight;
    }
    Err(PersonalServiceError::new("CN incentive reward selection is invalid"))
}
// //// /按主表权重抽取游戏内奖励 ////
