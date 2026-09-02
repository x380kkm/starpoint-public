// audience: internal
// # personal-service-cn-box-gacha
//
// 该模块实现 CN 箱池查询、抽取和关闭. 箱池状态与奖励记录保存在玩家快照.

mod catalog;
mod reward;

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, player_snapshot, require_object, require_root,
};
use crate::database::{ActivityWindowStatus, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use catalog::{
    all_box_info, box_available_count, box_gacha, box_gacha_ids_for_reward, box_rewards,
    choose_reward, drawn_reward_counts, ensure_box_status, save_drawn_reward_counts,
};
use reward::apply_rewards;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Deserialize)]
struct BoxListRequest {
    viewer_id: i64,
    box_gacha_id: i64,
}

#[derive(Deserialize)]
struct BoxCloseRequest {
    viewer_id: i64,
    box_gacha_id: i64,
    box_id: i64,
}

#[derive(Deserialize)]
struct BoxExecRequest {
    viewer_id: i64,
    box_gacha_id: i64,
    box_id: i64,
    number: i64,
    stop_on_featured_rewards: bool,
}

// //// 分派 CN 箱池请求 [@x380kkm 2026-08-22] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    let response = match request.path() {
        "/api/index.php/box_gacha/get_box_list" => get_box_list(request, database, asset_root),
        "/api/index.php/box_gacha/close" => close(request, database, asset_root),
        "/api/index.php/box_gacha/reset" => reset(request, database, asset_root),
        "/api/index.php/box_gacha/exec" => execute(request, database, asset_root),
        _ => return None,
    };
    Some(response)
}
// //// /分派 CN 箱池请求 ////

// //// 返回包含指定奖励的 CN 箱池编号 [@x380kkm 2026-08-24] ////
pub(crate) fn how_to_get_box_gacha_ids(
    item_id: Option<i64>,
    equipment_id: Option<i64>,
    database: &ServiceDatabase,
    asset_root: &Path,
) -> Result<Vec<i64>, PersonalServiceError> {
    box_gacha_ids_for_reward(item_id, equipment_id)?
        .into_iter()
        .filter_map(|box_gacha_id| {
            match box_gacha_source_status(database, asset_root, box_gacha_id) {
                Ok(ActivityWindowStatus::Open | ActivityWindowStatus::Unscheduled) => {
                    Some(Ok(box_gacha_id))
                }
                Ok(_) => None,
                Err(error) => Some(Err(error)),
            }
        })
        .collect()
}
// //// /返回包含指定奖励的 CN 箱池编号 ////

fn get_box_list(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<BoxListRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.box_gacha_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    if let Some(response) = closed_activity_response(database, asset_root, body.box_gacha_id)? {
        return Ok(response);
    }
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let player_data = decode_player_data(&snapshot.data)?;
    let root = player_data
        .as_object()
        .ok_or_else(|| PersonalServiceError::new("stored CN player data is not an object"))?;
    let gacha = box_gacha(body.box_gacha_id)?;
    let rewards = box_rewards(body.box_gacha_id)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({"all_box_info": all_box_info(root, body.box_gacha_id, &gacha, &rewards)?}),
    )
}

// //// 关闭指定 CN 箱池阶段 [@x380kkm 2026-08-22] ////
fn close(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<BoxCloseRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.box_gacha_id > 0 && body.box_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    if let Some(response) = closed_activity_response(database, asset_root, body.box_gacha_id)? {
        return Ok(response);
    }
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let gacha = box_gacha(body.box_gacha_id)?;
    let rewards = box_rewards(body.box_gacha_id)?;
    if !rewards.contains_key(&body.box_id.to_string()) {
        return Ok(error_response("400 Bad Request", "box_not_found"));
    }
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let available = box_available_count(&gacha, body.box_id);
    let status = ensure_box_status(root, body.box_gacha_id, body.box_id, available)?;
    if status
        .get("is_closed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(error_response("400 Bad Request", "box_already_closed"));
    }
    status.insert("is_closed".to_owned(), Value::Bool(true));
    let all_box_info = all_box_info(root, body.box_gacha_id, &gacha, &rewards)?;
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({"all_box_info": all_box_info}),
    )
}
// //// /关闭指定 CN 箱池阶段 ////

// //// 重置指定 CN 箱池阶段 [@x380kkm 2026-08-24] ////
fn reset(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<BoxCloseRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.box_gacha_id > 0 && body.box_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    if let Some(response) = closed_activity_response(database, asset_root, body.box_gacha_id)? {
        return Ok(response);
    }
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let gacha = box_gacha(body.box_gacha_id)?;
    let rewards = box_rewards(body.box_gacha_id)?;
    if !rewards.contains_key(&body.box_id.to_string()) {
        return Ok(error_response("400 Bad Request", "box_not_found"));
    }
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let available = box_available_count(&gacha, body.box_id);
    {
        let status = ensure_box_status(root, body.box_gacha_id, body.box_id, available)?;
        let reset_times = status
            .get("reset_times")
            .and_then(Value::as_i64)
            .unwrap_or_default()
            .saturating_add(1);
        status.insert("reset_times".to_owned(), Value::from(reset_times));
        status.insert("remaining_number".to_owned(), Value::from(available));
        status.insert("is_closed".to_owned(), Value::Bool(false));
    }
    save_drawn_reward_counts(root, body.box_gacha_id, body.box_id, &BTreeMap::new())?;
    let all_box_info = all_box_info(root, body.box_gacha_id, &gacha, &rewards)?;
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({"all_box_info": all_box_info}),
    )
}
// //// /重置指定 CN 箱池阶段 ////

// //// 抽取 CN 箱池并发放奖励 [@x380kkm 2026-08-22] ////
fn execute(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<BoxExecRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.box_gacha_id > 0
                && body.box_id > 0
                && body.number > 0
                && body.number <= 100 =>
        {
            body
        }
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    if let Some(response) = closed_activity_response(database, asset_root, body.box_gacha_id)? {
        return Ok(response);
    }
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let gacha = box_gacha(body.box_gacha_id)?;
    let rewards_by_box = box_rewards(body.box_gacha_id)?;
    let Some(rewards) = rewards_by_box
        .get(&body.box_id.to_string())
        .and_then(Value::as_object)
    else {
        return Ok(error_response("400 Bad Request", "box_not_found"));
    };
    let currency_id = gacha
        .get("itemId")
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new("CN box currency id is missing"))?;
    let currency_per_draw = gacha
        .get("count")
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new("CN box currency cost is missing"))?;
    let total_cost = currency_per_draw
        .checked_mul(body.number)
        .ok_or_else(|| PersonalServiceError::new("CN box currency cost exceeds range"))?;
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let currency_key = currency_id.to_string();
    let remaining_currency = match require_object(root, "item_list")?
        .get(&currency_key)
        .and_then(Value::as_i64)
        .unwrap_or_default()
        .checked_sub(total_cost)
    {
        Some(value) if value >= 0 => value,
        _ => return Ok(error_response("400 Bad Request", "not_enough_box_currency")),
    };
    if ensure_box_status(
        root,
        body.box_gacha_id,
        body.box_id,
        box_available_count(&gacha, body.box_id),
    )?
    .get("is_closed")
    .and_then(Value::as_bool)
    .unwrap_or(false)
    {
        return Ok(error_response("400 Bad Request", "box_closed"));
    }
    let mut drawn = drawn_reward_counts(root, body.box_gacha_id, body.box_id)?;
    let mut current_draw = BTreeMap::<i64, i64>::new();
    for _ in 0..body.number {
        let Some(reward) = choose_reward(rewards, &drawn)? else {
            break;
        };
        *drawn.entry(reward).or_default() += 1;
        *current_draw.entry(reward).or_default() += 1;
        let featured = rewards
            .get(&reward.to_string())
            .and_then(|reward| reward.get("tier"))
            .and_then(Value::as_i64)
            .unwrap_or_default()
            >= 2;
        if body.stop_on_featured_rewards && featured {
            break;
        }
    }
    save_drawn_reward_counts(root, body.box_gacha_id, body.box_id, &drawn)?;
    let remaining_number = box_available_count(&gacha, body.box_id)
        .saturating_sub(drawn.values().sum::<i64>())
        .max(0);
    let status = ensure_box_status(
        root,
        body.box_gacha_id,
        body.box_id,
        box_available_count(&gacha, body.box_id),
    )?;
    status.insert("remaining_number".to_owned(), Value::from(remaining_number));
    status.insert("is_closed".to_owned(), Value::Bool(remaining_number == 0));
    require_object(root, "item_list")?
        .insert(currency_key.clone(), Value::from(remaining_currency));
    let reward_result = apply_rewards(
        root,
        body.viewer_id,
        rewards,
        &current_draw,
        server_time(database)?,
    )?;
    let all_box_info = all_box_info_with_current_last(
        root,
        body.box_gacha_id,
        body.box_id,
        &gacha,
        &rewards_by_box,
    )?;
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    let mut item_list = reward_result.item_list;
    item_list.insert(currency_key, Value::from(remaining_currency));
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({
            "user_info": reward_result.user_info,
            "drawn_reward_list": current_draw.into_iter().map(|(reward_id, number)| json!({"reward_id": reward_id, "number": number})).collect::<Vec<_>>(),
            "all_box_info": all_box_info,
            "joined_character_id_list": reward_result.joined_character_ids,
            "character_list": reward_result.character_list,
            "equipment_list": reward_result.equipment_list,
            "item_list": item_list,
            "mail_arrived": false,
        }),
    )
}
// //// /抽取 CN 箱池并发放奖励 ////

fn closed_activity_response(
    database: &ServiceDatabase,
    asset_root: &Path,
    box_gacha_id: i64,
) -> Result<Option<HttpResponse>, PersonalServiceError> {
    let status = box_gacha_activity_status(database, asset_root, box_gacha_id)?;
    let code = match status {
        ActivityWindowStatus::Unscheduled | ActivityWindowStatus::Open => return Ok(None),
        ActivityWindowStatus::Disabled => "activity_disabled",
        ActivityWindowStatus::NotStarted => "activity_not_started",
        ActivityWindowStatus::Ended => "activity_ended",
    };
    Ok(Some(error_response("400 Bad Request", code)))
}

fn box_gacha_activity_status(
    database: &ServiceDatabase,
    _asset_root: &Path,
    box_gacha_id: i64,
) -> Result<ActivityWindowStatus, PersonalServiceError> {
    let activity_id = format!("box-gacha:{box_gacha_id}");
    let now_ms = database.current_server_time_millis()?;
    database.activity_window_status(&activity_id, now_ms)
}

// //// 按客户端目录窗口筛选如何获得来源 [@x380kkm 2026-08-30] ////
fn box_gacha_source_status(
    database: &ServiceDatabase,
    asset_root: &Path,
    box_gacha_id: i64,
) -> Result<ActivityWindowStatus, PersonalServiceError> {
    let status = box_gacha_activity_status(database, asset_root, box_gacha_id)?;
    if status != ActivityWindowStatus::Unscheduled {
        return Ok(status);
    }
    let activity_id = format!("box-gacha:{box_gacha_id}");
    Ok(crate::activity_catalog::default_activity_window_status(
        asset_root,
        &activity_id,
        database.current_server_time_millis()?,
    )
    .unwrap_or(status))
}
// //// /按客户端目录窗口筛选如何获得来源 ////

// //// 将当前箱放到参考响应的末尾 [@x380kkm 2026-08-23] ////
fn all_box_info_with_current_last(
    root: &Map<String, Value>,
    box_gacha_id: i64,
    current_box_id: i64,
    gacha: &Map<String, Value>,
    rewards: &Map<String, Value>,
) -> Result<Vec<Value>, PersonalServiceError> {
    let mut boxes = all_box_info(root, box_gacha_id, gacha, rewards)?;
    if let Some(index) = boxes
        .iter()
        .position(|box_info| box_info.get("box_id").and_then(Value::as_i64) == Some(current_box_id))
    {
        let current_box = boxes.remove(index);
        boxes.push(current_box);
    }
    Ok(boxes)
}
// //// /将当前箱放到参考响应的末尾 ////

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}
