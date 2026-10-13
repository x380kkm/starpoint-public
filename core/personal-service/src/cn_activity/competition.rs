// audience: internal
// # personal-service-competition-management
// 管理结算绑定存档, 当前期别和参与轮次. 奖励方式与活动开关分别保存, 全服统计属于活动期别.

mod catalog;

use super::{error_response, raid, ranking, rush, state};
use crate::cn_tutorial::decode_player_data;
use crate::database::ServiceDatabase;
use crate::http::{decode_path_segment, HttpRequest, HttpResponse};
use crate::{management, PersonalServiceError};
use serde::Deserialize;
use serde_json::{json, Map, Value};

const MODES: &str = "competition_reward_modes";

pub(super) fn is_manual(root: &Map<String, Value>, activity_id: &str) -> bool {
    root.get(MODES)
        .and_then(|modes| modes.get(activity_id))
        .and_then(Value::as_str)
        == Some("manual")
}

// //// 按当前客户端身份投影领取凭据中的称号 [@x380kkm 2026-09-28] ////
pub(super) fn degree_list_for_viewer(degrees: &Value, viewer_id: i64) -> Value {
    json!(degrees
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|degree| degree.get("degree_id").and_then(Value::as_i64))
        .map(|degree_id| if viewer_id > 0 {
            json!({"degree_id":degree_id,"viewer_id":viewer_id})
        } else {
            json!({"degree_id":degree_id})
        })
        .collect::<Vec<_>>())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettleRequest {
    expected_round: i64,
    expected_period_start_ms: Value,
    tier_id: Option<i64>,
    rank_number: Option<i64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModeRequest {
    mode: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ServerRequest {
    expected_period_start_ms: Value,
    hp_percentage: i64,
    total_kill_count: i64,
}

// //// 分派存档竞赛目录与人工结算请求 [@x380kkm 2026-09-28] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let path = request.path().strip_prefix("/v1/local-saves/")?;
    let (slot, rest) = path.split_once("/competitions")?;
    if !management::is_authorized(request, database) {
        return Some(Ok(management::unauthorized_response()));
    }
    let Some(slot) = slot.parse::<i64>().ok().filter(|slot| *slot > 0) else {
        return Some(Ok(error_response(
            "400 Bad Request",
            "invalid_local_save_id",
        )));
    };
    Some(handle(request, database, slot, rest))
}

fn handle(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    slot_id: i64,
    rest: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(context) = database.local_save_context(slot_id)? else {
        return Ok(error_response("404 Not Found", "local_save_not_found"));
    };
    let mut player = state::ActivityPlayer {
        account_id: context.account_id,
        data: decode_player_data(&database.activity_player_snapshot(context.account_id)?)?,
    };
    let mut entries = catalog::entries(player.root_mut()?, database)?;
    if rest.is_empty() && request.method() == "GET" {
        return Ok(HttpResponse::json(
            "200 OK",
            json!({"slot_id":slot_id,"slot_name":context.slot.name,"entries":entries}).to_string(),
        ));
    }
    let Some((activity, action)) = rest
        .strip_prefix('/')
        .and_then(|rest| rest.rsplit_once('/'))
    else {
        return Ok(error_response("404 Not Found", "competition_not_found"));
    };
    let Some(activity_id) = decode_path_segment(activity) else {
        return Ok(error_response("400 Bad Request", "invalid_activity_id"));
    };
    let Some(entry) = entries
        .iter_mut()
        .find(|entry| entry["activity_id"].as_str() == Some(activity_id.as_str()))
    else {
        return Ok(error_response("404 Not Found", "competition_not_found"));
    };
    let family = entry["family"].as_str().unwrap().to_owned();
    let event_id = entry["event_id"].as_i64().unwrap();
    match (request.method(), action) {
        ("PUT", "reward-mode") => {
            let body = match serde_json::from_slice::<ModeRequest>(request.body()) {
                Ok(body) if matches!(body.mode.as_str(), "manual" | "automatic") => body,
                _ => return Ok(error_response("400 Bad Request", "invalid_reward_mode")),
            };
            player
                .root_mut()?
                .entry(MODES.to_owned())
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .ok_or_else(|| PersonalServiceError::new("competition reward modes are invalid"))?
                .insert(activity_id.clone(), json!(body.mode));
            player.save(database)?;
            Ok(HttpResponse::json(
                "200 OK",
                json!({"activity_id":activity_id,"reward_mode":body.mode}).to_string(),
            ))
        }
        ("PUT", "server-state") if family == "raid" => {
            let body = match serde_json::from_slice::<ServerRequest>(request.body()) {
                Ok(body)
                    if (0..=100).contains(&body.hp_percentage) && body.total_kill_count >= 0 =>
                {
                    body
                }
                _ => return Ok(error_response("400 Bad Request", "invalid_server_state")),
            };
            if body.expected_period_start_ms != entry["period_start_ms"] {
                return Ok(error_response("409 Conflict", "competition_period_changed"));
            }
            database.set_raid_boss_state_for_period(
                event_id,
                entry["period_start_ms"].as_i64(),
                body.hp_percentage,
                body.total_kill_count,
            )?;
            Ok(HttpResponse::json("200 OK",json!({"activity_id":activity_id,"period_start_ms":entry["period_start_ms"],"global_state":{"hp_percentage":body.hp_percentage,"total_kill_count":body.total_kill_count}}).to_string()))
        }
        ("POST", "settle") => {
            let body = match serde_json::from_slice::<SettleRequest>(request.body()) {
                Ok(body) if body.expected_round >= 0 => body,
                _ => return Ok(error_response("400 Bad Request", "invalid_request_body")),
            };
            if body.expected_round != entry["participation_round"].as_i64().unwrap() {
                return Ok(error_response("409 Conflict", "activity_round_changed"));
            }
            if body.expected_period_start_ms != entry["period_start_ms"] {
                return Ok(error_response("409 Conflict", "competition_period_changed"));
            }
            if !valid_selection(&body, entry, &family) {
                return Ok(error_response(
                    "400 Bad Request",
                    "invalid_reward_selection",
                ));
            }
            let viewer = context.viewer_id.unwrap_or_default();
            let response=database.with_player_settlement(|database|{
                let settlement=match family.as_str(){
                    "ranking"=>ranking::settle(&mut player,database,viewer,event_id,body.tier_id,"management")?,
                    "rush"=>rush::settle(&mut player,database,viewer,event_id,body.rank_number,"management")?,
                    "raid"=>raid::settlement::settle(&mut player,database,viewer,event_id)?,
                    _=>unreachable!(),
                };
                let status=settlement.get("status").or_else(||settlement.get("ranking_reward").and_then(|reward|reward.get("status"))).and_then(Value::as_i64).unwrap_or(3);
                let response=json!({"activity_id":activity_id,"participation_round":body.expected_round,"period_start_ms":entry["period_start_ms"],
                    "status":match status{1=>"delivered",2=>"already_received",_=>"no_reward"},"settlement":settlement});
                Ok(Some(HttpResponse::json("200 OK",response.to_string())))
            })?.ok_or_else(||PersonalServiceError::new("competition settlement response is missing"))?;
            Ok(response)
        }
        _ => Ok(error_response(
            "405 Method Not Allowed",
            "method_not_allowed",
        )),
    }
}

fn valid_selection(body: &SettleRequest, entry: &Value, family: &str) -> bool {
    match family {
        "ranking" => {
            body.rank_number.is_none()
                && body.tier_id.map_or(true, |id| {
                    entry["tiers"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|tier| tier["id"].as_i64() == Some(id))
                })
        }
        "rush" => {
            body.tier_id.is_none()
                && body.rank_number.map_or(true, |rank| {
                    entry["tiers"].as_array().unwrap().iter().any(|tier| {
                        tier["rank_from"]
                            .as_i64()
                            .zip(tier["rank_to"].as_i64())
                            .is_some_and(|(start, end)| (start..=end).contains(&rank))
                    })
                })
        }
        "raid" => body.tier_id.is_none() && body.rank_number.is_none(),
        _ => false,
    }
}
