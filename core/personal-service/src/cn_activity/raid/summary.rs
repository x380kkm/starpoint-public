// audience: internal
// # personal-service-cn-raid-summary
// 团队战摘要按全局领主讨伐数投递总奖励, 并返回个人各关卡讨伐次数.

use super::{period, rewards, EventRequest, FAMILY};
use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_activity::{closed_activity_response, error_response, state};
use crate::cn_tutorial::format_client_time;
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde_json::{json, Value};
use std::path::Path;

// //// 结算达到的总讨伐奖励并读取活动摘要 [@x380kkm 2026-09-21] ////
pub(super) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    database.with_player_settlement(|database| summary(request, database, asset_root).map(Some))?
        .ok_or_else(|| PersonalServiceError::new("CN raid summary response is missing"))
}

// //// 按活动资格汇总讨伐结果并结算首页任务 [@x380kkm 2026-09-26] ////
fn summary(request: &HttpRequest, database: &mut ServiceDatabase, asset_root: &Path) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<EventRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.event_id > 0 => body,
        _ => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let mut player = match state::load_player(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    let activity_id = format!("raid:{}", body.event_id);
    let definition = rewards::catalog()?.raid.get(&body.event_id);
    if let Some(definition) = definition {
        if let Some(code) = period::summary_error(database, &activity_id, &definition.period)? {
            return Ok(error_response("400 Bad Request", code));
        }
    } else if let Some(response) = closed_activity_response(database, asset_root, &activity_id)? {
        return Ok(response);
    }
    let response_time = server_time(database)?;
    let account_id = player.account_id;
    let root = player.root_mut()?;
    state::select_event_period(root, database, FAMILY, body.event_id)?;
    state::set_current_event(root, FAMILY, body.event_id)?;
    let period_start_ms = state::event_period_start(root, FAMILY, body.event_id);
    let boss = database.raid_boss_state_for_period(body.event_id, period_start_ms)?;
    let round = crate::activity_history::participation_round(root, &activity_id);
    let award = super::settlement::prepare(root, database, body.viewer_id, body.event_id, "client")?;
    let current = &award.response["received_up_to"];
    let response_rewards = &award.response["reward_list"];
    let degrees = &award.response["degree_list"];
    let event = state::event_state_mut(root, FAMILY, body.event_id)?;
    let next_round = event
        .get("endless_battle_next_round")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    let mut quest_list = event
        .get("quest_list")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if let (Some(definition), Some(quests)) = (definition, quest_list.as_object_mut()) {
        for id in &definition.quest_ids {
            quests
                .entry(id.to_string())
                .or_insert_with(|| json!({"kill_count": 0}));
        }
    }
    let mut response = json!({
        "aggregated_time": format_client_time(response_time),
        "auto_start_point": definition.map_or(0, |definition| definition.auto_start_point),
        "kill_count_reward_data": {"received_up_to": current, "reward_list": response_rewards},
        "quest_list": quest_list,
        "raid_boss": {"hp_percentage": boss.hp_percentage, "total_kill_count": boss.total_kill_count},
        "endless_battle_next_round": next_round,
        "active_rush_battle_folder_id": event.get("active_folder_id").cloned().unwrap_or(Value::Null),
        "endless_battle_played_max_round": next_round,
        "cleared_folder_id_list": event.get("cleared_folder_id_list").cloned().unwrap_or_else(|| json!([])),
        "endless_battle_played_party_list": event.get("endless_played_party_list").cloned().unwrap_or_else(|| json!({})),
        "rush_battle_played_party_list": event.get("folder_played_party_list").cloned().unwrap_or_else(|| json!({})),
        "endless_battle_my_ranking": null,
        "degree_list": degrees,
    });
    let completed = crate::cn_mission::record_raid_top_action(root, database, account_id, body.event_id)?;
    response["mission_info"] = json!(completed);
    crate::cn_mission::apply_mission_reward_response(&mut response, root, &completed, body.viewer_id, response_time)?;
    award.deliver(&player, database)?;
    response["mail_arrived"] =
        json!(database.has_unreceived_mail(player.account_id, response_time)?);
    let mut history = response.clone();
    history["period_start_ms"] = json!(period_start_ms);
    database.record_activity_summary(player.account_id, &activity_id, round, &history)?;
    msgpack_response_at(body.viewer_id, false, response_time, response)
}
