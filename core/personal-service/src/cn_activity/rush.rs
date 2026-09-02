// audience: internal
// # personal-service-cn-rush-event
//
// 该模块实现 CN rush 活动摘要, 排行, 队伍, folder, 奖励和战斗状态.

mod rewards;

use super::{closed_activity_response, error_response, party, state};
use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{encode_player_data, format_client_time};
use crate::database::{ActiveSingleQuest, CreateMailInput, MailReward, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

const FAMILY: &str = "rush";
const QUEST_CATEGORY: i64 = 24;
const RUSH_QUEST_ASSET: &str = include_str!("../../../../assets/rush_event_quest.json");
static RUSH_QUESTS: OnceLock<Result<Value, String>> = OnceLock::new();

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
    api_count: Option<i64>,
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
        "/api/index.php/event/rush/reward" => reward(request, database, asset_root),
        "/api/index.php/event/rush/endless_battle" => endless_battle(request, database, asset_root),
        _ => return None,
    };
    Some(response)
}

// //// 返回 rush 活动摘要 [@x380kkm 2026-08-22] ////
fn summary(
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
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<RankingRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.event_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let mut player = match load_open_player(database, asset_root, body.viewer_id, body.event_id)? {
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
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<RankingPartyRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.event_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let mut player = match load_open_player(database, asset_root, body.viewer_id, body.event_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    let root = player.root_mut()?;
    state::set_current_event(root, FAMILY, body.event_id)?;
    let party_list =
        if body.rank_number == 1 && player_ranking(root, FAMILY, body.event_id, 1).is_some() {
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
    let Some(quest_event_id) = rush_quest_event_id(body.quest_id)? else {
        return Ok(error_response("400 Bad Request", "quest_not_found"));
    };
    let mut player = match state::load_player(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    let event_id = body
        .event_id
        .filter(|event_id| *event_id > 0)
        .unwrap_or(quest_event_id);
    if event_id != quest_event_id {
        return Ok(error_response("400 Bad Request", "invalid_event_id"));
    }
    if let Some(response) =
        closed_activity_response(database, asset_root, &format!("{FAMILY}:{event_id}"))?
    {
        return Ok(response);
    }
    let response_time = server_time(database)?;
    let root = player.root_mut()?;
    state::set_current_event(root, FAMILY, event_id)?;
    party::rush_party_groups(root, FAMILY, event_id)?;
    state::record_battle(
        state::event_state_mut(root, FAMILY, event_id)?,
        body.quest_id,
        body.party_id,
        &body.play_id,
        response_time,
    );
    player.start_battle(
        database,
        &ActiveSingleQuest {
            play_id: body.play_id,
            quest_id: body.quest_id,
            category: QUEST_CATEGORY,
            use_boss_boost_point: false,
            use_boost_point: false,
            is_auto_start_mode: body.is_auto_start_mode,
        },
    )?;
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        json!({
            "user_info": { "last_main_quest_id": body.quest_id },
            "is_multi": "single",
            "start_time": response_time,
            "quest_name": "",
        }),
    )
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

fn reward(
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
    let rank_number = state::event_state(player.root()?, FAMILY, body.event_id)
        .and_then(|event| event.get("rank_number"))
        .and_then(Value::as_i64)
        .or_else(|| {
            player_ranking(player.root().ok()?, FAMILY, body.event_id, 0)
                .and_then(|ranking| ranking.get("rank_number").and_then(Value::as_i64))
        });
    let reward_list = rank_number
        .filter(|rank| *rank > 0)
        .map(|rank| rewards::ranking_rewards(body.event_id, rank))
        .transpose()?
        .unwrap_or_default();
    let reward_was_requested = state::event_state(player.root()?, FAMILY, body.event_id)
        .and_then(|event| event.get("ranking_reward_requested"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let response_time = server_time(database)?;
    if !reward_was_requested && !reward_list.is_empty() {
        state::event_state_mut(player.root_mut()?, FAMILY, body.event_id)?
            .insert("ranking_reward_requested".to_owned(), Value::Bool(true));
        let item_list = reward_list
            .iter()
            .filter(|reward| reward.kind == 7 && reward.kind_id > 0 && reward.number > 0)
            .map(|reward| (reward.kind_id.to_string(), reward.number))
            .collect::<BTreeMap<_, _>>();
        if !item_list.is_empty() {
            database.deliver_reward_mail_with_snapshot_once(
                &CreateMailInput {
                    account_id: player.account_id,
                    title: "竞速活动排名奖励".to_owned(),
                    body: "竞速活动排名奖励已送达.".to_owned(),
                    sender: "Starpoint".to_owned(),
                    rewards: MailReward {
                        item_list,
                        ..MailReward::default()
                    },
                    expires_at: None,
                    created_at: response_time,
                },
                &encode_player_data(&player.data)?,
                &format!(
                    "rush:ranking:{}:{}:{}",
                    body.event_id,
                    rank_number.unwrap_or_default(),
                    body.api_count.unwrap_or_default()
                ),
            )?;
        }
    }
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        json!({
            "rank_number": rank_number,
            "ranking_reward": {
                "reward_list": reward_list.iter().map(|reward| json!({
                    "kind": reward.kind,
                    "kind_id": reward.kind_id,
                    "number": reward.number,
                })).collect::<Vec<_>>(),
                "status": 0,
            },
        }),
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
    let player = match load_open_player(database, asset_root, body.viewer_id, body.event_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
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

fn rush_quest_event_id(quest_id: i64) -> Result<Option<i64>, PersonalServiceError> {
    let quests = RUSH_QUESTS.get_or_init(|| {
        serde_json::from_str::<Value>(RUSH_QUEST_ASSET)
            .map_err(|error| format!("failed to decode CN rush quests: {error}"))
    });
    let quests = quests
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    let event_id = quests
        .get(quest_id.to_string())
        .filter(|quest| quest.get("rankPointReward").is_some())
        .and_then(|quest| quest.get("rushEventId"))
        .and_then(Value::as_i64);
    if event_id.is_some() {
        return Ok(event_id);
    }
    let event_id = quest_id / 1_000;
    let quest_number = quest_id % 1_000;
    let quest_count = match event_id {
        700_001 | 700_011 => 7,
        700_002..=700_007 | 700_012..=700_017 => 8,
        _ => 0,
    };
    Ok((1..=quest_count)
        .contains(&quest_number)
        .then_some(event_id))
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
            Some(json!({
                "character_id": character_id.as_i64()?,
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
        "rank_number": rank,
        "user_rank": 215,
    }))
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
