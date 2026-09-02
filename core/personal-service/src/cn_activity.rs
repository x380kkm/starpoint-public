// audience: internal
// # personal-service-cn-activity
//
// 该模块保存活动日历和可由管理 API 调整的活动 Boss 状态, 并返回 CN 客户端活动查询结果.
// 未配置日历的活动保持开放, 已配置活动按当前虚拟时间判断状态.

mod carnival;
mod party;
mod raid;
mod rush;
mod state;

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{decode_player_data, player_snapshot};
use crate::database::{ActivityWindowStatus, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::management;
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::path::Path;

const RAID_BOSS_MANAGEMENT_PREFIX: &str = "/v1/activities/raid-boss/";
const RANKING_EVENT_CATEGORY: i64 = 14;

// //// 提供活动战斗结算状态 [@x380kkm 2026-08-25] ////
pub(crate) fn activate_battle_event_state<'a>(
    root: &'a mut Map<String, Value>,
    family: &str,
    event_id: i64,
) -> Result<&'a mut Map<String, Value>, PersonalServiceError> {
    state::set_current_event(root, family, event_id)?;
    state::event_state_mut(root, family, event_id)
}

pub(crate) fn rush_battle_folder_rewards(
    event_id: i64,
    folder_id: i64,
) -> Result<Option<Vec<(i64, i64)>>, PersonalServiceError> {
    rush::battle_folder_rewards(event_id, folder_id)
}
// //// /提供活动战斗结算状态 ////

#[derive(Deserialize)]
struct GetRaidBossRequest {
    viewer_id: i64,
    event_id: Option<i64>,
}

#[derive(Deserialize)]
struct UpdateRaidBossRequest {
    hp_percentage: i64,
    total_kill_count: i64,
}

#[derive(Deserialize)]
struct RankingEventRequest {
    viewer_id: i64,
    ranking_event_id: i64,
    quest_kind: Option<i64>,
    api_count: Option<i64>,
}

// //// 分派 CN 活动查询请求 [@x380kkm 2026-07-24] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    let existing = match request.path() {
        "/api/index.php/event/raid/get_boss" => Some(get_raid_boss(request, database, asset_root)),
        "/api/index.php/ranking_event/get_summary" => {
            Some(get_ranking_summary(request, database, asset_root, false))
        }
        "/api/index.php/ranking_event/receive_reward" => {
            Some(get_ranking_summary(request, database, asset_root, true))
        }
        _ => None,
    };
    if existing.is_some() {
        return existing;
    }
    if let Some(response) = carnival::route(request, database, asset_root) {
        return Some(response);
    }
    if let Some(response) = raid::route(request, database, asset_root) {
        return Some(response);
    }
    if let Some(response) = rush::route(request, database, asset_root) {
        return Some(response);
    }
    None
}
// //// /分派 CN 活动查询请求 ////

// //// 分派活动 Boss 管理请求 [@x380kkm 2026-07-24] ////
pub(crate) fn management_route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if !request.path().starts_with(RAID_BOSS_MANAGEMENT_PREFIX) {
        return None;
    }
    if !management::is_authorized(request, database) {
        return Some(Ok(management::unauthorized_response()));
    }
    let event_id = match request.path().strip_prefix(RAID_BOSS_MANAGEMENT_PREFIX) {
        Some(value) => match value.parse::<i64>() {
            Ok(event_id) if event_id > 0 => event_id,
            _ => return Some(Ok(error_response("400 Bad Request", "invalid_event_id"))),
        },
        None => return None,
    };
    let response = match request.method() {
        "GET" => read_raid_boss(database, event_id),
        "PUT" => update_raid_boss(request, database, event_id),
        _ => Ok(error_response(
            "405 Method Not Allowed",
            "method_not_allowed",
        )),
    };
    Some(response)
}
// //// /分派活动 Boss 管理请求 ////

fn get_raid_boss(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<GetRaidBossRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.event_id.map_or(true, |event_id| event_id > 0) => {
            body
        }
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    match player_snapshot(database, body.viewer_id)? {
        Ok(_) => {}
        Err(response) => return Ok(response),
    }
    let (hp_percentage, total_kill_count) = match body.event_id {
        Some(event_id) => {
            if let Some(response) =
                closed_activity_response(database, asset_root, &format!("raid:{event_id}"))?
            {
                return Ok(response);
            }
            let state = database.raid_boss_state(event_id)?;
            (state.hp_percentage, state.total_kill_count)
        }
        None => (100, 0),
    };
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({
            "raid_boss": {
                "hp_percentage": hp_percentage,
                "total_kill_count": total_kill_count,
            }
        }),
    )
}

fn get_ranking_summary(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
    receive_reward: bool,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<RankingEventRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.ranking_event_id > 0
                && body.quest_kind.unwrap_or(RANKING_EVENT_CATEGORY) > 0
                && body.api_count.unwrap_or(0) >= 0 =>
        {
            body
        }
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let (quest_id, top_time) = match ranking_event_definition(body.ranking_event_id) {
        Some(definition) => definition,
        None => return Ok(error_response("400 Bad Request", "ranking_event_not_found")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    if let Some(response) = closed_activity_response(
        database,
        asset_root,
        &format!("ranking:{}", body.ranking_event_id),
    )? {
        return Ok(response);
    }
    let player_data = decode_player_data(&snapshot.data)?;
    let root = player_data
        .as_object()
        .ok_or_else(|| PersonalServiceError::new("stored CN player data is not an object"))?;
    let summary = ranking_summary(root, quest_id, top_time);
    let mut data = summary
        .as_object()
        .cloned()
        .ok_or_else(|| PersonalServiceError::new("CN ranking summary is not an object"))?;
    if receive_reward {
        data.insert("status".to_owned(), Value::from(1));
    }
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        Value::Object(data),
    )
}

pub(super) fn closed_activity_response(
    database: &ServiceDatabase,
    _asset_root: &Path,
    activity_id: &str,
) -> Result<Option<HttpResponse>, PersonalServiceError> {
    let now_ms = database.current_server_time_millis()?;
    let status = database.activity_window_status(activity_id, now_ms)?;
    let code = match status {
        ActivityWindowStatus::Unscheduled | ActivityWindowStatus::Open => return Ok(None),
        ActivityWindowStatus::Disabled => "activity_disabled",
        ActivityWindowStatus::NotStarted => "activity_not_started",
        ActivityWindowStatus::Ended => "activity_ended",
    };
    Ok(Some(error_response("400 Bad Request", code)))
}

fn ranking_summary(root: &Map<String, Value>, quest_id: i64, top_time: i64) -> Value {
    let progress = root
        .get("quest_progress")
        .and_then(Value::as_object)
        .and_then(|progress| progress.get(&RANKING_EVENT_CATEGORY.to_string()))
        .and_then(Value::as_array)
        .and_then(|progress_list| {
            progress_list
                .iter()
                .find(|entry| entry.get("quest_id").and_then(Value::as_i64) == Some(quest_id))
        });
    let best_time = progress
        .and_then(|entry| entry.get("best_elapsed_time_ms"))
        .and_then(Value::as_i64)
        .filter(|time| *time > 0);
    let accomplished = best_time.is_some();
    let elapsed_time = best_time.unwrap_or_default();
    let rank_percentage = best_time
        .map(|time| 1.0 - (top_time as f64 / time as f64))
        .unwrap_or(100.0);
    json!({
        "best_record": {
            "elapsed_time_ms": elapsed_time,
            "is_accomplished": accomplished,
            "score": progress
                .and_then(|entry| entry.get("high_score"))
                .and_then(Value::as_i64)
                .unwrap_or_default(),
        },
        "leader_character_evolution_img_level": 1,
        "leader_character_id": 1,
        "rank_border_top": {
            "elapsed_time_ms": top_time,
            "is_accomplished": true,
            "score": 1_110_111,
        },
        "rank_percentage": rank_percentage,
    })
}

fn ranking_event_definition(event_id: i64) -> Option<(i64, i64)> {
    match event_id {
        1 => Some((1_001, 54_410)),
        2 => Some((2_001, 25_800)),
        3 => Some((3_001, 18_880)),
        4 => Some((4_001, 31_720)),
        5 => Some((5_001, 6_540)),
        1_000 => Some((1_000_001, 0)),
        1_001 => Some((1_001_001, 0)),
        _ => None,
    }
}

fn read_raid_boss(
    database: &ServiceDatabase,
    event_id: i64,
) -> Result<HttpResponse, PersonalServiceError> {
    let state = database.raid_boss_state(event_id)?;
    json_response(
        "200 OK",
        json!({
            "event_id": event_id,
            "hp_percentage": state.hp_percentage,
            "total_kill_count": state.total_kill_count,
        }),
    )
}

fn update_raid_boss(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    event_id: i64,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match serde_json::from_slice::<UpdateRaidBossRequest>(request.body()) {
        Ok(body) => body,
        Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    if !(0..=100).contains(&body.hp_percentage) || body.total_kill_count < 0 {
        return Ok(error_response("400 Bad Request", "invalid_raid_boss_state"));
    }
    database.set_raid_boss_state(event_id, body.hp_percentage, body.total_kill_count)?;
    read_raid_boss(database, event_id)
}

fn json_response(
    status: &'static str,
    value: serde_json::Value,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = serde_json::to_string(&value).map_err(|error| {
        PersonalServiceError::new(format!("failed to encode activity response: {error}"))
    })?;
    Ok(HttpResponse::json(status, body))
}

pub(super) fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}

// //// 验证长期活动入口与显式时间规则一致 [@x380kkm 2026-08-30] ////
#[cfg(test)]
mod tests {
    use super::closed_activity_response;
    use crate::database::{ActivityWindowStatus, ServiceDatabase};
    use tempfile::TempDir;

    #[test]
    fn keeps_permanent_activity_gate_consistent_with_projection() {
        let root = TempDir::new().expect("temporary service directory is created");
        let asset_root = TempDir::new().expect("temporary CN asset directory is created");
        let mut database = ServiceDatabase::open(root.path()).expect("service database opens");
        database
            .set_virtual_time(true, 1_575_259_260_000, 1.0)
            .expect("virtual time is set");

        assert!(
            closed_activity_response(&database, asset_root.path(), "daily-week:1")
                .expect("unscheduled permanent activity status loads")
                .is_none()
        );

        database
            .upsert_activity_schedule("daily-week:1", true, 1, 2)
            .expect("ended permanent activity schedule is stored");
        let ended = closed_activity_response(&database, asset_root.path(), "daily-week:1")
            .expect("ended permanent activity status loads")
            .expect("ended permanent activity is rejected");
        assert!(!ended.is_success());
        assert_eq!(ended.body(), br#"{"error":"activity_ended"}"#);

        database
            .upsert_activity_schedule(
                "challenge-dungeon:1",
                false,
                1_575_259_000_000,
                1_575_260_000_000,
            )
            .expect("disabled permanent activity schedule is stored");
        let disabled =
            closed_activity_response(&database, asset_root.path(), "challenge-dungeon:1")
                .expect("disabled permanent activity status loads")
                .expect("disabled permanent activity is rejected");
        assert!(!disabled.is_success());
        assert_eq!(disabled.body(), br#"{"error":"activity_disabled"}"#);

        database
            .delete_activity_schedule("challenge-dungeon:1")
            .expect("disabled schedule is removed");
        database
            .create_activity_temporary_open_lease("challenge-dungeon:1")
            .expect("temporary permanent activity lease is stored");
        assert!(
            closed_activity_response(&database, asset_root.path(), "challenge-dungeon:1")
                .expect("temporary permanent activity status loads")
                .is_none()
        );

        assert_eq!(
            database
                .activity_window_status("challenge-dungeon:1", 1_575_259_260_000)
                .expect("temporary status loads"),
            ActivityWindowStatus::Open
        );
        assert!(
            closed_activity_response(&database, asset_root.path(), "ranking:1")
                .expect("ranking status loads")
                .is_none()
        );
    }
}
// //// /验证长期活动入口与显式时间规则一致 ////
