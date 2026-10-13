// audience: internal
// # personal-service-cn-rush-claim
// 狂热激战排名奖励按活动参与轮次保存领取凭据, 称号与实物采用各自的投递形式.

use super::{player_ranking, EventRequest, FAMILY};
use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_activity::{
    error_response,
    raid::{period, rewards},
    state,
};
use crate::cn_tutorial::encode_player_data;
use crate::database::{CreateMailInput, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde_json::{json, Value};

// //// 固定本轮排名奖励并一次投递 [@x380kkm 2026-09-21] ////
pub(super) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<EventRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.event_id > 0 => body,
        _ => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let mut player = match state::load_player(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    let Some(definition) = rewards::catalog()?.rush.get(&body.event_id) else {
        return Ok(error_response("400 Bad Request", "activity_not_found"));
    };
    let activity_id = format!("rush:{}", body.event_id);
    if let Some(code) = period::ranking_error(database, &activity_id, &definition.period)? {
        return Ok(error_response("400 Bad Request", code));
    }
    let response = settle(&mut player, database, body.viewer_id, body.event_id, None, "client")?;
    msgpack_response_at(body.viewer_id, false, server_time(database)?, response)
}

// //// 客户端和管理页共用本轮排名奖励凭据 [@x380kkm 2026-09-28] ////
pub(in crate::cn_activity) fn settle(
    player: &mut state::ActivityPlayer,
    database: &mut ServiceDatabase,
    viewer_id: i64,
    event_id: i64,
    selected_rank: Option<i64>,
    source: &str,
) -> Result<Value, PersonalServiceError> {
    let definition = rewards::catalog()?.rush.get(&event_id)
        .ok_or_else(|| PersonalServiceError::new("rush event is missing"))?;
    let activity_id = format!("rush:{event_id}");
    let response_time = server_time(database)?;
    let account_id = player.account_id;
    let root = player.root_mut()?;
    if source == "client" {
        state::select_event_period(root, database, FAMILY, event_id)?;
    }
    let round = crate::activity_history::participation_round(root, &activity_id);
    if let Some(receipt) = state::event_state(root, FAMILY, event_id)
        .and_then(|event| event.get("ranking_reward_receipt"))
    {
        return Ok(json!({
                "rank_number": receipt["rank_number"],
                "ranking_reward": {"status": 2, "reward_list": receipt["rewards"]},
                "degree_list": crate::cn_activity::competition::degree_list_for_viewer(&receipt["degree_list"],viewer_id),
                "mail_arrived": database.has_unreceived_mail(account_id, response_time)?,
            }));
    }
    let statistics = player_ranking(root, FAMILY, event_id, 1);
    if source == "management" && statistics.is_none() {
        return Ok(json!({"rank_number":null,"ranking_reward":{"status":3,"reward_list":[]}}));
    }
    if source == "client" && crate::cn_activity::competition::is_manual(root, &activity_id) {
        return Ok(json!({"rank_number":statistics.as_ref().map(|value| &value["rank_number"]),"ranking_reward":{"status":3,"reward_list":[]}}));
    }
    let rank = selected_rank.filter(|_| statistics.is_some()).or_else(|| state::event_state(root, FAMILY, event_id)
        .and_then(|event| event.get("rank_number"))
        .and_then(Value::as_i64)
        .filter(|rank| *rank > 0)
        .or_else(|| {
            statistics.clone()
                .and_then(|entry| entry["rank_number"].as_i64())
        }));
    let rewards = definition
        .ranking_rewards
        .iter()
        .filter(|tier| rank.is_some_and(|rank| (tier.rank_start..=tier.rank_end).contains(&rank)))
        .flat_map(|tier| tier.rewards.iter().cloned())
        .collect::<Vec<_>>();
    if rewards.is_empty() {
        return Ok(json!({"rank_number": rank, "ranking_reward": {"status": 3, "reward_list": []}}));
    }
    let (mail, response_rewards, degrees) = rewards::prepare(root, &rewards, viewer_id)?;
    let degrees=crate::cn_activity::competition::degree_list_for_viewer(&json!(degrees),viewer_id);
    let degree_receipt=crate::cn_activity::competition::degree_list_for_viewer(&degrees,0);
    let period_start_ms = state::event_period_start(root, FAMILY, event_id);
    let scope = state::reward_scope(root, FAMILY, event_id);
    state::event_state_mut(root, FAMILY, event_id)?.insert("ranking_reward_receipt".to_owned(),
        json!({"received_at": response_time, "rank_number": rank, "rewards": response_rewards, "degree_list": degree_receipt,
            "statistics":statistics,"source":source,"participation_round":round,"period_start_ms":period_start_ms}));
    if rewards::has_mail_rewards(&mail) {
        database.deliver_reward_mail_with_snapshot_once(
            &CreateMailInput {
                account_id: player.account_id,
                title: "狂热激战排名奖励".to_owned(),
                body: "狂热激战排名奖励.".to_owned(),
                sender: "Starpoint".to_owned(),
                rewards: mail,
                expires_at: None,
                created_at: response_time,
            },
            &encode_player_data(&player.data)?,
            &format!("{scope}:round:{round}:ranking"),
            Some(&activity_id),
        )?;
    } else {
        player.save(database)?;
    }
    Ok(json!({
            "rank_number": rank, "ranking_reward": {"status": 1, "reward_list": response_rewards},
            "degree_list": degrees, "mail_arrived": database.has_unreceived_mail(player.account_id, response_time)?,
        }))
}
