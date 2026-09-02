// audience: internal
// # personal-service-cn-story
//
// 该模块实现 CN 剧情任务的普通结算和跳过结算. 任务和奖励来自仓库内的真实资产表.

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_battle_assets::{load_battle_fixture, BattleFixture, Reward};
use crate::cn_battle_rewards::{apply_reward, RewardResult};
use crate::cn_player::finish_quest_progress;
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, player_snapshot, require_object, require_root,
};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;

const ADVENT_EVENT_SINGLE_CATEGORY: i64 = 7;
const ADVENT_EVENT_MULTI_CATEGORY: i64 = 8;

static STORY_ASSETS: OnceLock<Result<BattleFixture, String>> = OnceLock::new();

#[derive(Deserialize)]
struct FinishRequest {
    #[serde(rename = "party_id")]
    _party_id: i64,
    quest_id: i64,
    viewer_id: i64,
    category: i64,
    api_count: Option<i64>,
    retry_count: Option<i64>,
}

// //// 分派 CN 剧情结算请求 [@x380kkm 2026-07-24] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST"
        || !matches!(
            request.path(),
            "/api/index.php/story_quest/finish" | "/api/index.php/story_quest/finish_with_skip"
        )
    {
        return None;
    }
    Some(finish(request, database))
}
// //// /分派 CN 剧情结算请求 ////

fn finish(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<FinishRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.quest_id > 0
                && body.category > 0
                && body.api_count.map_or(true, |api_count| api_count >= 0)
                && body
                    .retry_count
                    .map_or(true, |retry_count| retry_count >= 0) =>
        {
            body
        }
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let assets = story_assets()?;
    let asset_category = match body.category {
        ADVENT_EVENT_MULTI_CATEGORY => ADVENT_EVENT_SINGLE_CATEGORY,
        category => category,
    };
    let quest_key = format!("{asset_category}:{}", body.quest_id);
    let Some(quest) = assets.quests.get(&quest_key) else {
        return Ok(error_response("400 Bad Request", "quest_not_found"));
    };
    if quest.s_plus_reward_id.is_some() {
        return Ok(error_response("400 Bad Request", "quest_is_battle_quest"));
    }
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let was_finished = is_quest_finished(root, body.category, body.quest_id)?;
    let reward_result = if was_finished {
        RewardResult::default()
    } else {
        let reward_result = apply_clear_reward(root, quest.clear_reward.as_ref())?;
        finish_quest_progress(root, body.category, body.quest_id)?;
        reward_result
    };
    let (mission_info, active_mission_list) = if !was_finished && body.category == 3 {
        let mission_delta =
            crate::cn_mission::record_story_action(root, database, snapshot.account_id, 1)?;
        (
            mission_delta.mission_info,
            mission_delta.active_mission_list,
        )
    } else {
        (Vec::new(), Vec::new())
    };
    let response = if was_finished {
        json!([])
    } else {
        let user_info = require_object(root, "user_info")?;
        json!({
            "user_info": {
                "free_vmoney": user_info.get("free_vmoney").and_then(Value::as_i64).unwrap_or_default(),
                "free_mana": user_info.get("free_mana").and_then(Value::as_i64).unwrap_or_default(),
            },
            "character_list": reward_result.character_list,
            "joined_character_id_list": reward_result.joined_character_ids,
            "equipment_list": reward_result.equipment_list,
            "item_list": reward_result.items,
            "presigned_quest_category": [],
            "mission_info": mission_info,
            "active_mission_list": active_mission_list,
            "mail_arrived": false,
        })
    };
    let response_time = server_time(database)?;
    if !was_finished {
        database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    }
    msgpack_response_at(body.viewer_id, false, response_time, response)
}

fn story_assets() -> Result<&'static BattleFixture, PersonalServiceError> {
    STORY_ASSETS
        .get_or_init(|| load_battle_fixture().map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

fn apply_clear_reward(
    root: &mut Map<String, Value>,
    reward: Option<&Reward>,
) -> Result<crate::cn_battle_rewards::RewardResult, PersonalServiceError> {
    let Some(reward) = reward else {
        return Ok(RewardResult::default());
    };
    let mut reward = reward.clone();
    reward.count.get_or_insert(1);
    apply_reward(root, &reward)
}

fn is_quest_finished(
    root: &Map<String, Value>,
    category: i64,
    quest_id: i64,
) -> Result<bool, PersonalServiceError> {
    let Some(progress_list) = root
        .get("quest_progress")
        .and_then(Value::as_object)
        .and_then(|progress| progress.get(&category.to_string()))
    else {
        return Ok(false);
    };
    let progress_list = progress_list
        .as_array()
        .ok_or_else(|| PersonalServiceError::new("stored CN quest category progress is invalid"))?;
    Ok(progress_list.iter().any(|progress| {
        progress.get("quest_id").and_then(Value::as_i64) == Some(quest_id)
            && progress.get("finished").and_then(Value::as_bool) == Some(true)
    }))
}

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}
