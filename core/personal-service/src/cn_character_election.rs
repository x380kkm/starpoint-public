// audience: internal
// # personal-service-cn-character-election
// 每轮投票保存首次选择, 投票与活动历史使用同一快照事务. 候选遵循百科开放日期和活动排除表.

use crate::cn::{
    decode_request, msgpack_response_at, msgpack_result_code_response_at, server_time,
};
use crate::cn_tutorial::{decode_player_data, encode_player_data, player_snapshot, require_root};
use crate::database::{ActivityWindowStatus, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

pub(crate) const VOTES_FIELD: &str = "character_election_votes";
const INVALID_PERIOD: i64 = 11003;

#[derive(Deserialize)]
struct ElectionRequest {
    viewer_id: i64,
    election_id: i64,
}

#[derive(Deserialize)]
struct VoteRequest {
    #[serde(flatten)]
    election: ElectionRequest,
    keyword_id: i64,
}

#[derive(Deserialize)]
struct Election {
    start_at: i64,
    end_at: i64,
    excluded: BTreeSet<i64>,
}

#[derive(Deserialize)]
struct Candidate {
    available_at: Option<i64>,
}

#[derive(Deserialize)]
struct Definitions {
    elections: BTreeMap<i64, Election>,
    candidates: BTreeMap<i64, Candidate>,
}

fn definitions() -> Result<&'static Definitions, PersonalServiceError> {
    static DATA: OnceLock<Result<Definitions, String>> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/cn-character-election.json"))
            .map_err(|error| error.to_string())
    })
    .as_ref()
    .map_err(|error| PersonalServiceError::new(error.clone()))
}

pub(crate) fn has_election(activity_id: &str) -> Result<bool, PersonalServiceError> {
    let data = definitions()?;
    Ok(activity_id
        .strip_prefix("character-election:")
        .and_then(|id| id.parse::<i64>().ok())
        .is_some_and(|id| data.elections.contains_key(&id)))
}

// //// 根据主表相同时间范围定位投票任务的活动 [@x380kkm 2026-09-26] ////
pub(crate) fn event_for_period(start: Option<i64>, end: Option<i64>) -> Result<Option<i64>, PersonalServiceError> {
    Ok(definitions()?.elections.iter().find(|(_, event)| {
        Some(event.start_at) == start && Some(event.end_at) == end
    }).map(|(id, _)| *id))
}

fn is_open(
    database: &ServiceDatabase,
    id: i64,
    election: &Election,
    now: i64,
) -> Result<bool, PersonalServiceError> {
    Ok(
        match database.activity_window_status(&format!("character-election:{id}"), now * 1000)? {
            ActivityWindowStatus::Unscheduled => election.start_at <= now && now <= election.end_at,
            ActivityWindowStatus::Open => true,
            _ => false,
        },
    )
}

// //// 按活动窗口读取本轮投票状态 [@x380kkm 2026-09-23] ////
pub(crate) fn get_vote_status(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    database.with_player_settlement(|database| vote_status(request, database).map(Some))?
        .ok_or_else(|| PersonalServiceError::new("CN election status response is missing"))
}

// //// 读取有效投票状态并结算尚缺的投票任务 [@x380kkm 2026-09-26] ////
fn vote_status(request: &HttpRequest, database: &mut ServiceDatabase) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ElectionRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.election_id > 0 => body,
        _ => return Ok(invalid_request()),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let now = server_time(database)?;
    let Some(election) = definitions()?.elections.get(&body.election_id) else {
        return msgpack_result_code_response_at(body.viewer_id, now, INVALID_PERIOD);
    };
    if !is_open(database, body.election_id, election, now)? {
        return msgpack_result_code_response_at(body.viewer_id, now, INVALID_PERIOD);
    }
    let mut player = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player)?;
    let voted = root
        .get(VOTES_FIELD)
        .and_then(|votes| votes.get(body.election_id.to_string()))
        .is_some();
    if voted {
        return settle_vote_response(root, database, snapshot.account_id, body.viewer_id, body.election_id, now, json!({"is_voted": true}));
    }
    msgpack_response_at(body.viewer_id, false, now, json!({"is_voted": false}))
}

// //// 保存有效候选的首次投票并保留重复请求结果 [@x380kkm 2026-09-23] ////
pub(crate) fn vote(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    database.with_player_settlement(|database| vote_and_settle(request, database).map(Some))?
        .ok_or_else(|| PersonalServiceError::new("CN election settlement response is missing"))
}

// //// 验证候选并保存当前轮次首次投票 [@x380kkm 2026-09-26] ////
fn vote_and_settle(request: &HttpRequest, database: &mut ServiceDatabase) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<VoteRequest>(request) {
        Ok(body)
            if body.election.viewer_id > 0
                && body.election.election_id > 0
                && body.keyword_id > 0 =>
        {
            body
        }
        _ => return Ok(invalid_request()),
    };
    let ElectionRequest {
        viewer_id,
        election_id,
    } = body.election;
    let snapshot = match player_snapshot(database, viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let now = server_time(database)?;
    let data = definitions()?;
    let Some(election) = data.elections.get(&election_id) else {
        return msgpack_result_code_response_at(viewer_id, now, INVALID_PERIOD);
    };
    let mut player = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player)?;
    if root
        .get(VOTES_FIELD)
        .and_then(|votes| votes.get(election_id.to_string()))
        .is_some()
    {
        return settle_vote_response(root, database, snapshot.account_id, viewer_id, election_id, now, json!({}));
    }
    if !is_open(database, election_id, election, now)? {
        return msgpack_result_code_response_at(viewer_id, now, INVALID_PERIOD);
    }
    let eligible = data
        .candidates
        .get(&body.keyword_id)
        .is_some_and(|candidate| candidate.available_at.map_or(true, |start| start <= now));
    if !eligible || election.excluded.contains(&body.keyword_id) {
        return Ok(invalid_request());
    }
    let activity_id = format!("character-election:{election_id}");
    let round = crate::activity_history::participation_round(root, &activity_id);
    root.entry(VOTES_FIELD.to_owned())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("invalid CN character election votes"))?
        .insert(
            election_id.to_string(),
            json!({
                "keyword_id": body.keyword_id, "created_at": now, "participation_round": round,
            }),
        );
    settle_vote_response(root, database, snapshot.account_id, viewer_id, election_id, now, json!({}))
}

// //// 返回投票任务奖励并保存投票与领取凭据 [@x380kkm 2026-09-26] ////
fn settle_vote_response(
    root: &mut Map<String, Value>, database: &mut ServiceDatabase, account_id: i64,
    viewer_id: i64, event_id: i64, now: i64, mut response: Value,
) -> Result<HttpResponse, PersonalServiceError> {
    let completed = crate::cn_mission::record_election_vote_action(root, database, account_id, event_id)?;
    response["mission_info"] = json!(completed);
    crate::cn_mission::apply_mission_reward_response(&mut response, root, &completed, viewer_id, now)?;
    database.save_player_snapshot(account_id, &encode_player_data(&Value::Object(root.clone()))?)?;
    msgpack_response_at(viewer_id, false, now, response)
}

fn invalid_request() -> HttpResponse {
    HttpResponse::json(
        "400 Bad Request",
        json!({"error":"invalid_request_body"}).to_string(),
    )
}
