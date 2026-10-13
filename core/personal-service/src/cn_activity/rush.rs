// audience: internal
// # personal-service-cn-rush-event
//
// 该模块实现 CN rush 活动摘要, 排行, 队伍, folder, 奖励和战斗状态.

mod claim;
pub(super) use claim::settle;
mod rewards;
mod presentation;
pub(super) use presentation::normalize_character_references;
pub(crate) use presentation::normalize_played_parties;

use super::{closed_activity_response, error_response, party, state};
use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::format_client_time;
use crate::database::{ActiveSingleQuest, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

const FAMILY: &str = "rush";
const QUEST_CATEGORY: i64 = 24;

// //// 读取战斗结算使用的 folder 奖励 [@x380kkm 2026-08-25] ////
pub(super) fn battle_folder_rewards(
    event_id: i64,
    folder_id: i64,
) -> Result<Option<Vec<(i64, i64)>>, PersonalServiceError> {
    rewards::folder_rewards(event_id, folder_id)
}
// //// /读取战斗结算使用的 folder 奖励 ////

#[derive(Deserialize)]
struct EventRequest {
    event_id: i64,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct PartyRequest {
    event_id: Option<i64>,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct SelectFolderRequest {
    event_id: i64,
    folder_id: i64,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct RankingRequest {
    event_id: i64,
    page: Option<i64>,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct RankingPartyRequest {
    event_id: i64,
    rank_number: i64,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct BattleStartRequest {
    event_id: Option<i64>,
    is_auto_start_mode: bool,
    party_id: i64,
    play_id: String,
    quest_id: i64,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct ResetRequest {
    event_id: i64,
    is_reset_after_target_round: Option<bool>,
    quest_type: i64,
    reset_target_id: Option<i64>,
    viewer_id: i64,
}

pub(super) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let response = match request.path() {
        "/api/index.php/event/rush/summary" => summary(request, database, asset_root),
        "/api/index.php/event/rush/select_folder" => select_folder(request, database, asset_root),
        "/api/index.php/event/rush/ranking" => ranking(request, database, asset_root),
        "/api/index.php/event/rush/ranking/played_party" => {
            ranking_party(request, database, asset_root)
        }
        "/api/index.php/event/rush/aggregated_time" => {
            aggregated_time(request, database, asset_root)
        }
        "/api/index.php/event/rush/party" => event_party(request, database, asset_root),
        "/api/index.php/event/rush/battle/start" => battle_start(request, database, asset_root),
        "/api/index.php/event/rush/reset" => reset(request, database, asset_root),
        "/api/index.php/event/rush/reward" => claim::route(request, database),
        "/api/index.php/event/rush/endless_battle" => endless_battle(request, database, asset_root),
        _ => return None,
    };
    Some(response)
}

// //// 返回 rush 活动摘要 [@x380kkm 2026-08-22] ////
fn summary(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    _asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<EventRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.event_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let mut player = match load_summary_player(database, body.viewer_id, body.event_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    let response_time = server_time(database)?;
    let root = player.root_mut()?;
    state::set_current_event(root, FAMILY, body.event_id)?;
    let event = state::event_state_mut(root, FAMILY, body.event_id)?;
    event.insert("summary_initialized".to_owned(), Value::Bool(true));
    let next_round = event
        .get("endless_battle_next_round")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    let max_round = event
        .get("endless_battle_max_round")
        .cloned()
        .unwrap_or(Value::Null);
    let active_folder = event
        .get("active_folder_id")
        .cloned()
        .unwrap_or(Value::Null);
    let cleared_folders = event
        .get("cleared_folder_id_list")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let endless_parties = event
        .get("endless_played_party_list")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let folder_parties = event
        .get("folder_played_party_list")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let my_ranking = player_ranking(root, FAMILY, body.event_id, 0);
    player.save(database)?;
    let activity_id = format!("rush:{}", body.event_id);
    database.record_activity_summary(player.account_id, &activity_id,
        crate::activity_history::participation_round(player.root()?, &activity_id),
        &json!({"endless_battle_max_round": max_round, "endless_battle_my_ranking": my_ranking, "cleared_folder_id_list": cleared_folders,
            "period_start_ms":state::event_period_start(player.root()?, FAMILY, body.event_id)}))?;
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        json!({
            "endless_battle_next_round": next_round,
            "endless_battle_max_round": max_round,
            "active_rush_battle_folder_id": active_folder,
            "endless_battle_played_max_round": max_round,
            "cleared_folder_id_list": cleared_folders,
            "endless_battle_played_party_list": endless_parties,
            "rush_battle_played_party_list": folder_parties,
            "endless_battle_my_ranking": my_ranking,
            "aggregated_time": format_client_time(response_time),
        }),
    )
}
// //// /返回 rush 活动摘要 ////

fn select_folder(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<SelectFolderRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.event_id > 0 && body.folder_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let mut player = match load_open_player(database, asset_root, body.viewer_id, body.event_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    let Some(_) = rewards::folder_rewards(body.event_id, body.folder_id)? else {
        return Ok(error_response("400 Bad Request", "folder_not_found"));
    };
    let root = player.root_mut()?;
    if !state::event_state(root, FAMILY, body.event_id)
        .and_then(|event| event.get("summary_initialized"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(error_response(
            "400 Bad Request",
            "rush_event_not_initialized",
        ));
    }
    state::set_current_event(root, FAMILY, body.event_id)?;
    let event = state::event_state_mut(root, FAMILY, body.event_id)?;
    if event
        .get("active_folder_id")
        .is_some_and(|folder| !folder.is_null())
    {
        return Ok(error_response("400 Bad Request", "folder_already_selected"));
    }
    event.insert("active_folder_id".to_owned(), Value::from(body.folder_id));
    let response_time = server_time(database)?;
    player.save(database)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        json!({ "folder_id": body.folder_id, "event_id": body.event_id }),
    )
}

fn ranking(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    _asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<RankingRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.event_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let mut player = match load_summary_player(database, body.viewer_id, body.event_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    let page = body.page.unwrap_or(0);
    let root = player.root_mut()?;
    state::set_current_event(root, FAMILY, body.event_id)?;
    let my_data = player_ranking(root, FAMILY, body.event_id, 0);
    let ranking_list = if page == 0 {
        player_ranking(root, FAMILY, body.event_id, 1)
            .into_iter()
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let page_max = i64::from(my_data.is_some());
    player.save(database)?;
    let activity_id = format!("rush:{}", body.event_id);
    database.record_activity_summary(
        player.account_id,
        &activity_id,
        crate::activity_history::participation_round(player.root()?, &activity_id),
        &json!({"my_data": my_data,"period_start_ms":state::event_period_start(player.root()?, FAMILY, body.event_id)}),
    )?;
    let response_time = server_time(database)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        json!({
            "aggregated_time": format_client_time(response_time),
            "current_page": page + 1,
            "page_max": page_max,
            "my_data": my_data,
            "ranking_list": ranking_list,
        }),
    )
}

fn ranking_party(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    _asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<RankingPartyRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.event_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let mut player = match load_summary_player(database, body.viewer_id, body.event_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    let root = player.root_mut()?;
    state::set_current_event(root, FAMILY, body.event_id)?;
    let party_list = if player_ranking(root, FAMILY, body.event_id, 1)
        .is_some_and(|ranking| ranking["rank_number"].as_i64() == Some(body.rank_number))
    {
        state::event_state(root, FAMILY, body.event_id)
            .and_then(|event| event.get("endless_played_party_list"))
            .cloned()
            .unwrap_or_else(|| json!({}))
    } else {
        Value::Array(Vec::new())
    };
    player.save(database)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({ "rush_ranking_party": party_list }),
    )
}

fn aggregated_time(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    _asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<EventRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.event_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let mut player = match load_summary_player(database, body.viewer_id, body.event_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    state::set_current_event(player.root_mut()?, FAMILY, body.event_id)?;
    player.save(database)?;
    let response_time = server_time(database)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        json!({ "aggregated_time": format_client_time(response_time) }),
    )
}

fn event_party(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<PartyRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let mut player = match state::load_player(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    let current_event = state::current_event_id(player.root()?, FAMILY);
    let managed_event = state::managed_event_id(database, FAMILY)?;
    let event_id = body
        .event_id
        .filter(|event_id| *event_id > 0)
        .or(current_event)
        .or(managed_event)
        .unwrap_or_default();
    if event_id > 0 {
        if let Some(response) =
            closed_activity_response(database, asset_root, &format!("{FAMILY}:{event_id}"))?
        {
            return Ok(response);
        }
    }
    let root = player.root_mut()?;
    if event_id > 0 {
        state::select_event_period(root, database, FAMILY, event_id)?;
        state::set_current_event(root, FAMILY, event_id)?;
    }
    let party_groups = party::rush_party_groups(root, FAMILY, event_id)?;
    player.save(database)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({ "user_party_group_list": party_groups }),
    )
}

// //// 持久化 rush 活动战斗 [@x380kkm 2026-08-22] ////
fn battle_start(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<BattleStartRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.party_id > 0
                && body.quest_id > 0
                && !body.play_id.is_empty() =>
        {
            body
        }
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let mut player = match state::load_player(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    let active = ActiveSingleQuest {
        play_id: body.play_id.clone(),
        quest_id: body.quest_id,
        category: QUEST_CATEGORY,
        use_boss_boost_point: false,
        use_boost_point: false,
        is_auto_start_mode: body.is_auto_start_mode,
    };
    if let Some(response) = super::battle::resume(&player, database, body.viewer_id, &active)? {
        return Ok(response);
    }
    let fixture = crate::cn_battle_assets::load_battle_fixture()?;
    let Some(quest) = fixture
        .quests
        .get(&format!("{QUEST_CATEGORY}:{}", body.quest_id))
    else {
        return Ok(error_response("400 Bad Request", "quest_not_found"));
    };
    let Some(event_id) = quest.rush_event_id else {
        return Ok(error_response("400 Bad Request", "invalid_event_id"));
    };
    if body
        .event_id
        .filter(|id| *id > 0)
        .is_some_and(|id| id != event_id)
    {
        return Ok(error_response("400 Bad Request", "invalid_event_id"));
    }
    if closed_activity_response(database, asset_root, &format!("{FAMILY}:{event_id}"))?.is_some()
        || !crate::cn_quest::is_quest_open(database, QUEST_CATEGORY, body.quest_id)?
    {
        return crate::cn::msgpack_result_code_response_at(
            body.viewer_id,
            server_time(database)?,
            4050,
        );
    }
    let response_time = server_time(database)?;
    let root = player.root_mut()?;
    state::select_event_period(root, database, FAMILY, event_id)?;
    state::set_current_event(root, FAMILY, event_id)?;
    party::rush_party_groups(root, FAMILY, event_id)?;
    state::record_battle(
        state::event_state_mut(root, FAMILY, event_id)?,
        body.quest_id,
        body.party_id,
        &body.play_id,
        response_time,
    );
    let response = match super::battle::start(&mut player, database, &active, quest, response_time)?
    {
        Ok(response) => response,
        Err(response) => return Ok(response),
    };
    msgpack_response_at(body.viewer_id, false, response_time, response)
}
// //// /持久化 rush 活动战斗 ////

fn reset(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ResetRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.event_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let mut player = match load_open_player(database, asset_root, body.viewer_id, body.event_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    let root = player.root_mut()?;
    state::set_current_event(root, FAMILY, body.event_id)?;
    let reset_kind = if body.quest_type == 1 {
        1
    } else if body.reset_target_id.is_some() {
        2
    } else {
        0
    };
    state::reset_progress(
        state::event_state_mut(root, FAMILY, body.event_id)?,
        reset_kind,
        body.reset_target_id,
        body.is_reset_after_target_round.unwrap_or(false),
    )?;
    player.save(database)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        Value::Array(Vec::new()),
    )
}

fn endless_battle(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<EventRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.event_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let mut player = match load_open_player(database, asset_root, body.viewer_id, body.event_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    state::set_current_event(player.root_mut()?, FAMILY, body.event_id)?;
    let event = state::event_state(player.root()?, FAMILY, body.event_id);
    let max_round = event
        .and_then(|event| event.get("endless_battle_max_round"))
        .cloned()
        .unwrap_or(Value::Null);
    let next_round = event
        .and_then(|event| event.get("endless_battle_next_round"))
        .and_then(Value::as_i64)
        .unwrap_or(1);
    let parties = event
        .and_then(|event| event.get("endless_played_party_list"))
        .cloned()
        .unwrap_or(Value::Null);
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({
            "endless_battle_max_round": max_round,
            "endless_battle_next_round": next_round,
            "endless_battle_played_party_list": parties,
        }),
    )
}

pub(super) fn player_ranking(
    root: &serde_json::Map<String, Value>,
    family: &str,
    event_id: i64,
    rank: i64,
) -> Option<Value> {
    let event = state::event_state(root, family, event_id)?;
    let best_round = event.get("endless_battle_max_round")?.as_i64()?;
    let elapsed_time_ms = event.get("endless_battle_max_round_time")?.as_i64()?;
    let character_ids = event
        .get("endless_battle_max_round_character_ids")
        .and_then(Value::as_array);
    let evolution_levels = event
        .get("endless_battle_max_round_character_evolution_img_lvls")
        .and_then(Value::as_array);
    let party_member_list = character_ids
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(index, character_id)| {
            let character_id = character_id.as_i64()
                .filter(|id| crate::cn_character_presentation::is_valid_character(*id))?;
            Some(json!({
                "character_id": character_id,
                "evolution_img_level": evolution_levels
                    .and_then(|levels| levels.get(index))
                    .and_then(Value::as_i64)
                    .unwrap_or_default(),
            }))
        })
        .collect::<Vec<_>>();
    let name = root
        .get("user_info")
        .and_then(Value::as_object)
        .and_then(|user_info| user_info.get("name"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(json!({
        "best_round": best_round,
        "elapsed_time_ms": elapsed_time_ms,
        "name": name,
        "party_member_list": party_member_list,
        "rank_number": event.get("rank_number").and_then(Value::as_i64).filter(|rank| *rank > 0).unwrap_or(rank.max(1)),
        "user_rank": root.get("user_info").and_then(|info| info.get("rank_point")).and_then(Value::as_i64)
            .and_then(|points| crate::cn_battle_assets::rank_degree_and_stamina(points).ok()).map_or(1, |(rank, _, _)| rank),
    }))
}

// //// 在比赛和领奖窗口内读取成绩与参赛队伍 [@x380kkm 2026-09-28] ////
fn load_summary_player(
    database: &ServiceDatabase,
    viewer_id: i64,
    event_id: i64,
) -> Result<Result<state::ActivityPlayer, HttpResponse>, PersonalServiceError> {
    let Some(definition) = super::raid::rewards::catalog()?.rush.get(&event_id) else {
        return Ok(Err(error_response("400 Bad Request", "activity_not_found")));
    };
    if let Some(code) = super::raid::period::summary_error(database, &format!("{FAMILY}:{event_id}"), &definition.period)? {
        return Ok(Err(error_response("400 Bad Request", code)));
    }
    let mut player = match state::load_player(database, viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(Err(response)),
    };
    state::select_event_period(player.root_mut()?, database, FAMILY, event_id)?;
    Ok(Ok(player))
}

fn load_open_player(
    database: &ServiceDatabase,
    asset_root: &Path,
    viewer_id: i64,
    event_id: i64,
) -> Result<Result<state::ActivityPlayer, HttpResponse>, PersonalServiceError> {
    let mut player = match state::load_player(database, viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(Err(response)),
    };
    if let Some(response) =
        closed_activity_response(database, asset_root, &format!("{FAMILY}:{event_id}"))?
    {
        return Ok(Err(response));
    }
    state::select_event_period(player.root_mut()?, database, FAMILY, event_id)?;
    Ok(Ok(player))
}
