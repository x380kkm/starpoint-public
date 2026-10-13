// audience: internal
// # personal-service-cn-ranking
// 竞速查询使用各期最佳成绩, 原始 CN 期兼容分类 11 的通关记录. 奖励邮件, 称号和领取记录原子保存.
// 排名百分数使用本地参考成绩, 奖励档位与客户端主表一致.

mod period;
mod presentation;
mod management;
mod settlement;
pub(super) use management::entries as management_entries;
pub(super) use settlement::settle;
mod records;
mod rewards;
pub(crate) use records::record_battle_result;
pub(crate) use records::preserve_original_record;

use super::error_response;
use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{decode_player_data, encode_player_data, player_snapshot, require_root};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

const RECEIPTS: &str = "ranking_reward_receipts";
static CATALOG: OnceLock<Result<BTreeMap<i64, Event>, String>> = OnceLock::new();

#[derive(Deserialize)]
struct Request {
    viewer_id: i64,
    ranking_event_id: i64,
    quest_kind: Option<i64>,
    api_count: Option<i64>,
}

#[derive(Deserialize)]
struct Event {
    name: String,
    quest_ids: Vec<i64>,
    reference_time_ms: i64,
    start_at_ms: i64,
    play_end_at_ms: i64,
    aggregation_end_at_ms: i64,
    reward_end_at_ms: i64,
    tiers: Vec<Tier>,
}

#[derive(Deserialize)]
struct Tier {
    id: i64,
    border_percentage: f64,
    rewards: Vec<Reward>,
}

#[derive(Deserialize, Serialize)]
struct Reward {
    kind: i64,
    id: Option<i64>,
    amount: i64,
}

// //// 分派竞速成绩查询和奖励领取 [@x380kkm 2026-09-21] ////
pub(super) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    match request.path() {
        "/api/index.php/ranking_event/get_summary" => Some(get_summary(request, database)),
        "/api/index.php/ranking_event/receive_reward" => Some(receive(request, database)),
        _ => None,
    }
}

fn catalog() -> Result<&'static BTreeMap<i64, Event>, PersonalServiceError> {
    CATALOG.get_or_init(|| serde_json::from_str(include_str!("../../assets/cn-ranking-rewards.json"))
        .map_err(|error| error.to_string()))
        .as_ref().map_err(|error| PersonalServiceError::new(error.clone()))
}

fn parse_request(request: &HttpRequest) -> Result<Request, HttpResponse> {
    match decode_request::<Request>(request) {
        Ok(body) if body.viewer_id > 0 && body.ranking_event_id > 0
            && body.quest_kind.map_or(true, |kind| kind == 1)
            && body.api_count.unwrap_or_default() >= 0 => Ok(body),
        _ => Err(error_response("400 Bad Request", "invalid_request_body")),
    }
}

fn get_summary(request: &HttpRequest, database: &mut ServiceDatabase) -> Result<HttpResponse, PersonalServiceError> {
    let body = match parse_request(request) { Ok(body) => body, Err(response) => return Ok(response) };
    let Some(event) = catalog()?.get(&body.ranking_event_id) else {
        return Ok(error_response("400 Bad Request", "ranking_event_not_found"));
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot, Err(response) => return Ok(response),
    };
    if let Some(code) = period::summary_error(database, body.ranking_event_id)? {
        return Ok(error_response("400 Bad Request", code));
    }
    let mut data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut data)?;
    super::state::select_event_period(root, database, "ranking", body.ranking_event_id)?;
    let summary = recorded_summary(root, body.ranking_event_id)
        .unwrap_or_else(|| summary(root, body.ranking_event_id, event));
    let activity_id = format!("ranking:{}", body.ranking_event_id);
    let mut history = summary.clone();
    history["period_start_ms"] = json!(super::state::event_period_start(root, "ranking", body.ranking_event_id));
    database.record_activity_summary(snapshot.account_id, &activity_id,
        crate::activity_history::participation_round(root, &activity_id), &history)?;
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&data)?)?;
    msgpack_response_at(body.viewer_id, false, server_time(database)?, summary)
}

// //// 按成绩档位投递一次竞速奖励 [@x380kkm 2026-09-21] ////
fn receive(request: &HttpRequest, database: &mut ServiceDatabase) -> Result<HttpResponse, PersonalServiceError> {
    let body = match parse_request(request) { Ok(body) => body, Err(response) => return Ok(response) };
    let Some(event) = catalog()?.get(&body.ranking_event_id) else {
        return Ok(error_response("400 Bad Request", "ranking_event_not_found"));
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot, Err(response) => return Ok(response),
    };
    if let Some(code) = period::receive_error(database, body.ranking_event_id, event)? {
        return Ok(error_response("400 Bad Request", code));
    }
    let mut player = super::state::ActivityPlayer { account_id: snapshot.account_id, data: decode_player_data(&snapshot.data)? };
    let response = settle(&mut player, database, body.viewer_id, body.ranking_event_id, None, "client")?;
    msgpack_response_at(body.viewer_id, false, server_time(database)?, response)
}

fn recorded_summary(root: &Map<String, Value>, event_id: i64) -> Option<Value> {
    let scope = super::state::reward_scope(root, "ranking", event_id);
    let mut summary = root.get(RECEIPTS)?.get(scope.strip_prefix("ranking:")?)?.get("summary").cloned()?;
    presentation::normalize(root, &mut summary);
    Some(summary)
}

// //// 从竞速通关记录计算客户端百分数 [@x380kkm 2026-09-21] ////
fn summary(root: &Map<String, Value>, event_id: i64, event: &Event) -> Value {
    let record = records::best_record(root, event_id, event);
    let Some(record) = record else { return json!({"best_record":null}); };
    let best_time = record.is_accomplished.then_some(record.elapsed_time_ms);
    let percentage = best_time.map(|time| (100.0 * (1.0 - event.reference_time_ms as f64 / time as f64)).clamp(0.0, 100.0)).unwrap_or(100.0);
    let mut summary = json!({
        "best_record": record.wire_value(),
        "leader_character_id": record.leader(),
        "rank_border_top": {"elapsed_time_ms": event.reference_time_ms, "is_accomplished": true, "score": 0},
        "rank_percentage": percentage,
    });
    presentation::normalize(root, &mut summary);
    summary
}
