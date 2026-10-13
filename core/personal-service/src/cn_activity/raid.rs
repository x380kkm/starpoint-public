// audience: internal
// # personal-service-cn-raid-event
//
// 该模块实现 CN raid 活动入口, 队伍, 排行, folder 和战斗状态.

pub(super) mod period;
pub(super) mod rewards;
pub(super) mod settlement;
mod summary;

use super::{closed_activity_response, error_response, party, state};
use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::database::{ActiveSingleQuest, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

const FAMILY: &str = "raid";
const QUEST_CATEGORY: i64 = 23;

#[derive(Deserialize)]
struct EventRequest {
    event_id: i64,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct OptionalEventRequest {
    event_id: Option<i64>,
    quest_id: Option<i64>,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct SelectFolderRequest {
    event_id: i64,
    folder_id: i64,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct BattleStartRequest {
    event_id: Option<i64>,
    is_auto_start_mode: bool,
    party_group_id: i64,
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
        "/api/index.php/event/raid/summary" => summary::route(request, database, asset_root),
        "/api/index.php/event/raid/ranking_reward" => ranking_reward(request, database, asset_root),
        "/api/index.php/event/raid/party" => event_party(request, database, asset_root),
        "/api/index.php/event/raid/ranking" => ranking(request, database, asset_root),
        "/api/index.php/event/raid/ranking/party" => ranking_party(request, database, asset_root),
        "/api/index.php/event/raid/battle/start" => battle_start(request, database, asset_root),
        "/api/index.php/event/raid/select_folder" => select_folder(request, database, asset_root),
        "/api/index.php/event/raid/reset" => reset(request, database, asset_root),
        _ => return None,
    };
    Some(response)
}

fn ranking_reward(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    _asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<OptionalEventRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let _player = match state::load_player(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({ "reward_list": [], "status": 0 }),
    )
}

fn event_party(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<OptionalEventRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let mut player = match state::load_player(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    let managed_event = state::managed_event_id(database, FAMILY)?;
    let event_id =
        resolve_event_id(&player, body.event_id, body.quest_id, managed_event).unwrap_or_default();
    if event_id > 0 {
        if let Some(response) =
            closed_activity_response(database, asset_root, &format!("{FAMILY}:{event_id}"))?
        {
            return Ok(response);
        }
    }
    let root = player.root_mut()?;
    if event_id > 0 {
        activate_event(root, database, event_id)?;
    }
    let party_groups = party::raid_party_groups(root, FAMILY, event_id)?;
    player.save(database)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({ "user_party_group_list": party_groups }),
    )
}

fn ranking(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let (body, mut player, event_id) = match load_optional_event(request, database, asset_root)? {
        Ok(context) => context,
        Err(response) => return Ok(response),
    };
    if event_id > 0 {
        activate_event(player.root_mut()?, database, event_id)?;
        player.save(database)?;
    }
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({ "aggregated_time": "", "quest_list": {} }),
    )
}

fn ranking_party(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let (body, mut player, event_id) = match load_optional_event(request, database, asset_root)? {
        Ok(context) => context,
        Err(response) => return Ok(response),
    };
    if event_id > 0 {
        activate_event(player.root_mut()?, database, event_id)?;
        player.save(database)?;
    }
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({ "raid_ranking_party": [] }),
    )
}

// //// 持久化 raid 活动战斗 [@x380kkm 2026-08-22] ////
fn battle_start(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<BattleStartRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.quest_id > 0
                && body.party_group_id > 0
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
    let Some(event_id) = quest.raid_event_id else {
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
    activate_event(root, database, event_id)?;
    party::raid_party_groups(root, FAMILY, event_id)?;
    state::record_battle(
        state::event_state_mut(root, FAMILY, event_id)?,
        body.quest_id,
        body.party_group_id,
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
// //// /持久化 raid 活动战斗 ////

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
    let root = player.root_mut()?;
    activate_event(root, database, body.event_id)?;
    state::event_state_mut(root, FAMILY, body.event_id)?
        .insert("active_folder_id".to_owned(), Value::from(body.folder_id));
    player.save(database)?;
    msgpack_response_at(body.viewer_id, false, server_time(database)?, json!({}))
}

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
    activate_event(root, database, body.event_id)?;
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
    msgpack_response_at(body.viewer_id, false, server_time(database)?, json!({}))
}

fn activate_event(
    root: &mut serde_json::Map<String, Value>,
    database: &ServiceDatabase,
    event_id: i64,
) -> Result<(), PersonalServiceError> {
    state::select_event_period(root, database, FAMILY, event_id)?;
    state::set_current_event(root, FAMILY, event_id)
}

fn load_open_player(
    database: &ServiceDatabase,
    asset_root: &Path,
    viewer_id: i64,
    event_id: i64,
) -> Result<Result<state::ActivityPlayer, HttpResponse>, PersonalServiceError> {
    let player = match state::load_player(database, viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(Err(response)),
    };
    if let Some(response) =
        closed_activity_response(database, asset_root, &format!("{FAMILY}:{event_id}"))?
    {
        return Ok(Err(response));
    }
    Ok(Ok(player))
}

fn load_optional_event(
    request: &HttpRequest,
    database: &ServiceDatabase,
    asset_root: &Path,
) -> Result<
    Result<(OptionalEventRequest, state::ActivityPlayer, i64), HttpResponse>,
    PersonalServiceError,
> {
    let body = match decode_request::<OptionalEventRequest>(request) {
        Ok(body) => body,
        Err(_) => {
            return Ok(Err(error_response(
                "400 Bad Request",
                "invalid_request_body",
            )))
        }
    };
    if body.viewer_id <= 0 {
        return Ok(Err(error_response(
            "400 Bad Request",
            "invalid_request_body",
        )));
    }
    let player = match state::load_player(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(Err(response)),
    };
    let managed_event = state::managed_event_id(database, FAMILY)?;
    let event_id =
        resolve_event_id(&player, body.event_id, body.quest_id, managed_event).unwrap_or_default();
    if event_id > 0 {
        if let Some(response) =
            closed_activity_response(database, asset_root, &format!("{FAMILY}:{event_id}"))?
        {
            return Ok(Err(response));
        }
    }
    Ok(Ok((body, player, event_id)))
}

fn resolve_event_id(
    player: &state::ActivityPlayer,
    event_id: Option<i64>,
    quest_id: Option<i64>,
    managed_event_id: Option<i64>,
) -> Option<i64> {
    event_id
        .filter(|event_id| *event_id > 0)
        .or_else(|| state::current_event_id(player.root().ok()?, FAMILY))
        .or(managed_event_id)
        .or_else(|| {
            quest_id
                .filter(|quest_id| *quest_id > 0)
                .map(|quest_id| quest_id / 1000)
        })
        .filter(|event_id| *event_id > 0)
}
