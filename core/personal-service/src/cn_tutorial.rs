// audience: internal
// # personal-service-cn-tutorial
//
// 该模块持久化 CN 客户端教程步骤, 教程扭蛋角色和教程完成记录.

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_character_reward::grant_character;
use crate::cn_gacha::{draw_tutorial_gacha, resolve_tutorial_gacha, TutorialGachaPlan};
use crate::database::{
    CreateMailInput, MailReward, PlayerSnapshot, ServiceDatabase, ViewerSessionPlayer,
};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};

const FREE_TUTORIAL_CHARACTER_ID: i64 = 243001;

#[derive(Deserialize)]
struct FinishTriggerRequest {
    viewer_id: i64,
    tutorial_ids: Vec<i64>,
}

#[derive(Deserialize)]
struct UpdateStepRequest {
    viewer_id: i64,
    step: i64,
    skip: Option<bool>,
    gacha_id: Option<i64>,
    name: Option<String>,
}

// //// 分派 CN 教程请求 [@x380kkm 2026-07-24] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    let response = match request.path() {
        "/api/index.php/tutorial/finish_trigger" => finish_trigger(request, database),
        "/api/index.php/tutorial/update_step" => update_step(request, database),
        _ => return None,
    };
    Some(response)
}
// //// /分派 CN 教程请求 ////

// //// 记录 CN 客户端首次完成的教程 [@x380kkm 2026-07-24] ////
fn finish_trigger(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<FinishTriggerRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let triggered = root
        .get_mut("user_triggered_tutorial")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN tutorial records are missing"))?;
    for tutorial_id in body.tutorial_ids {
        if !triggered
            .iter()
            .any(|value| value.as_i64() == Some(tutorial_id))
        {
            triggered.push(Value::from(tutorial_id));
        }
    }
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        Value::Array(Vec::new()),
    )
}
// //// /记录 CN 客户端首次完成的教程 ////

// //// 更新 CN 教程步骤并发放教程奖励 [@x380kkm 2026-08-23] ////
fn update_step(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<UpdateStepRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.step >= 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let stored_next_step = body
        .step
        .checked_add(1)
        .ok_or_else(|| PersonalServiceError::new("CN tutorial step exceeds the supported range"))?;
    let skip = body.skip.unwrap_or(false);
    let response_step = client_tutorial_step(stored_next_step, skip)?;
    let tutorial_gacha_phase = is_tutorial_gacha_phase(response_step);
    let tutorial_gacha = if tutorial_gacha_phase {
        match body.gacha_id {
            Some(gacha_id) => match resolve_tutorial_gacha(gacha_id)? {
                Some(gacha) => Some(gacha),
                None => return Ok(error_response("400 Bad Request", "invalid_gacha_id")),
            },
            None => None,
        }
    } else {
        None
    };
    let server_time = server_time(database)?;
    let tutorial_is_complete = root
        .get("user_tutorial")
        .ok_or_else(|| PersonalServiceError::new("stored CN user_tutorial data is missing"))?
        .is_null()
        || root
            .get("user_triggered_tutorial")
            .and_then(Value::as_array)
            .is_some_and(|tutorials| {
                tutorials
                    .iter()
                    .any(|tutorial_id| tutorial_id.as_i64() == Some(12))
            });
    let tutorial_end = response_step == 16;
    let tutorial_gacha_is_saved = root
        .get("tutorial_gacha")
        .and_then(Value::as_object)
        .and_then(|gacha| gacha.get("character_id"))
        .and_then(Value::as_i64)
        .is_some();
    if tutorial_is_complete {
        return Ok(error_response(
            "400 Bad Request",
            "tutorial_already_completed",
        ));
    }

    let stored_current_step = require_object(root, "user_tutorial")?
        .get("tutorial_step")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    if stored_current_step > stored_next_step {
        return Ok(error_response("400 Bad Request", "previous_tutorial_step"));
    }
    if tutorial_end {
        let tutorial = require_object(root, "user_tutorial")?;
        tutorial.insert("tutorial_step".to_owned(), Value::from(stored_next_step));
        tutorial.insert("skip_flag".to_owned(), Value::Bool(skip));
        tutorial.insert("powerflip_failure".to_owned(), Value::from(0));
        if let Some(name) = body.name.as_deref().and_then(normalize_name) {
            require_object(root, "user_info")?.insert("name".to_owned(), Value::String(name));
        }
        let tutorial_reward_is_saved = root
            .get("user_character_list")
            .and_then(Value::as_object)
            .is_some_and(|characters| {
                characters.contains_key(&FREE_TUTORIAL_CHARACTER_ID.to_string())
            });
        let response = if tutorial_reward_is_saved {
            tutorial_end_response(root, body.viewer_id, response_step, server_time)?
        } else {
            finish_tutorial_reward(root, body.viewer_id, response_step, server_time)?
        };
        let encoded_player_data = encode_player_data(&player_data)?;
        let delivered = database.deliver_reward_mail_with_snapshot_once(
            &CreateMailInput {
                account_id: snapshot.account_id,
                title: "教程完成奖励".to_owned(),
                body: "教程完成奖励已送达.".to_owned(),
                sender: "Starpoint".to_owned(),
                rewards: MailReward {
                    free_vmoney: 500,
                    ..MailReward::default()
                },
                expires_at: None,
                created_at: server_time,
            },
            &encoded_player_data,
            "tutorial:completion:free-vmoney-500",
        )?;
        if !delivered {
            database.save_player_snapshot(snapshot.account_id, &encoded_player_data)?;
        }
        return msgpack_response_at(body.viewer_id, false, server_time, response);
    }

    if stored_current_step >= stored_next_step {
        let response = if tutorial_gacha_phase && body.gacha_id.is_some() && tutorial_gacha_is_saved
        {
            tutorial_gacha_response(
                root,
                body.viewer_id,
                body.gacha_id
                    .expect("validated tutorial gacha id is present"),
                response_step,
                server_time,
            )?
        } else if stored_current_step == stored_next_step
            && tutorial_gacha_phase
            && tutorial_gacha.is_some()
        {
            let response = match finish_tutorial_gacha(
                root,
                body.viewer_id,
                tutorial_gacha.expect("validated tutorial gacha plan is present"),
                response_step,
                server_time,
            )? {
                Ok(response) => response,
                Err(code) => return Ok(error_response("400 Bad Request", code)),
            };
            database
                .save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
            response
        } else {
            tutorial_step_response(
                root,
                body.viewer_id,
                response_step,
                body.gacha_id,
                tutorial_gacha_phase,
                server_time,
            )?
        };
        return msgpack_response_at(body.viewer_id, false, server_time, response);
    }

    let tutorial = require_object(root, "user_tutorial")?;
    tutorial.insert("tutorial_step".to_owned(), Value::from(stored_next_step));
    tutorial.insert("skip_flag".to_owned(), Value::Bool(skip));
    tutorial.insert("powerflip_failure".to_owned(), Value::from(0));
    if let Some(name) = body.name.as_deref().and_then(normalize_name) {
        require_object(root, "user_info")?.insert("name".to_owned(), Value::String(name));
    }

    let response = if tutorial_gacha_phase {
        match tutorial_gacha {
            Some(gacha) => match finish_tutorial_gacha(
                root,
                body.viewer_id,
                gacha,
                response_step,
                server_time,
            )? {
                Ok(response) => response,
                Err(code) => return Ok(error_response("400 Bad Request", code)),
            },
            None => basic_tutorial_step_response(response_step, server_time),
        }
    } else {
        basic_tutorial_step_response(response_step, server_time)
    };
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(body.viewer_id, false, server_time, response)
}
// //// /更新 CN 教程步骤并发放教程奖励 ////

// //// 映射持久教程步骤到客户端步骤 [@x380kkm 2026-08-23] ////
fn client_tutorial_step(stored_step: i64, skip: bool) -> Result<i64, PersonalServiceError> {
    if skip {
        stored_step.checked_add(11).ok_or_else(|| {
            PersonalServiceError::new("CN tutorial step exceeds the supported range")
        })
    } else {
        Ok(stored_step)
    }
}
// //// /映射持久教程步骤到客户端步骤 ////

// //// 识别客户端教程扭蛋阶段 [@x380kkm 2026-08-23] ////
fn is_tutorial_gacha_phase(response_step: i64) -> bool {
    response_step == 15
}
// //// /识别客户端教程扭蛋阶段 ////

// //// 构造已保存教程步骤的客户端响应 [@x380kkm 2026-08-23] ////
fn tutorial_step_response(
    root: &Map<String, Value>,
    viewer_id: i64,
    next_step: i64,
    gacha_id: Option<i64>,
    tutorial_gacha_phase: bool,
    server_time: i64,
) -> Result<Value, PersonalServiceError> {
    match gacha_id.filter(|_| tutorial_gacha_phase) {
        Some(gacha_id) => {
            tutorial_gacha_response(root, viewer_id, gacha_id, next_step, server_time)
        }
        None => Ok(basic_tutorial_step_response(next_step, server_time)),
    }
}
// //// /构造已保存教程步骤的客户端响应 ////

// //// 构造普通教程步骤的客户端响应 [@x380kkm 2026-08-23] ////
fn basic_tutorial_step_response(step: i64, server_time: i64) -> Value {
    json!({
        "step": step,
        "mail_arrived": true,
        "start_time": server_time,
    })
}
// //// /构造普通教程步骤的客户端响应 ////

// //// 生成教程扭蛋结果并保存角色 [@x380kkm 2026-08-23] ////
fn finish_tutorial_gacha(
    root: &mut Map<String, Value>,
    viewer_id: i64,
    gacha: TutorialGachaPlan,
    step: i64,
    server_time: i64,
) -> Result<Result<Value, &'static str>, PersonalServiceError> {
    let draw = match draw_tutorial_gacha(root, viewer_id, gacha, server_time)? {
        Ok(draw) => draw,
        Err(code) => return Ok(Err(code)),
    };
    let mut tutorial_gacha = json!({
        "gacha_id": gacha.gacha_id,
        "character_id": draw.character_id,
    });
    if let Some(item) = draw.duplicate_item {
        tutorial_gacha["duplicate_item"] =
            json!({"id": item.id, "count": item.count, "total": item.total});
    }
    root.insert("tutorial_gacha".to_owned(), tutorial_gacha);
    tutorial_gacha_response(root, viewer_id, gacha.gacha_id, step, server_time).map(Ok)
}
// //// /生成教程扭蛋结果并保存角色 ////

// //// 构造教程扭蛋的客户端响应 [@x380kkm 2026-08-23] ////
fn tutorial_gacha_response(
    root: &Map<String, Value>,
    viewer_id: i64,
    gacha_id: i64,
    step: i64,
    server_time: i64,
) -> Result<Value, PersonalServiceError> {
    let character_id = root
        .get("tutorial_gacha")
        .and_then(Value::as_object)
        .and_then(|gacha| gacha.get("character_id"))
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new("stored CN tutorial gacha result is missing"))?;
    let stored_character = root
        .get("user_character_list")
        .and_then(Value::as_object)
        .and_then(|characters| characters.get(&character_id.to_string()))
        .ok_or_else(|| {
            PersonalServiceError::new("stored CN tutorial gacha character is missing")
        })?;
    let character =
        create_character_response(viewer_id, character_id, stored_character, server_time);
    let free_vmoney = user_info_value(root, "free_vmoney")?;
    let duplicate_item = root
        .get("tutorial_gacha")
        .and_then(Value::as_object)
        .and_then(|gacha| gacha.get("duplicate_item"))
        .and_then(Value::as_object);
    let mut draw = json!({
        "character_id": character_id,
        "movie_id": "normal_guarantee",
        "seed": 10007656,
        "entry_count": 1,
    });
    let mut item_list = Map::new();
    if let Some(item) = duplicate_item {
        let item_id = item.get("id").and_then(Value::as_i64).ok_or_else(|| {
            PersonalServiceError::new("stored tutorial duplicate item is invalid")
        })?;
        let count = item.get("count").and_then(Value::as_i64).unwrap_or(1);
        let total = item.get("total").and_then(Value::as_i64).ok_or_else(|| {
            PersonalServiceError::new("stored tutorial duplicate total is invalid")
        })?;
        draw["ex_boost_item"] = json!({"id": item_id, "count": count});
        item_list.insert(item_id.to_string(), Value::from(total));
    }
    Ok(json!({
        "step": step,
        "user_info": {"free_vmoney": free_vmoney},
        "gacha": {
            "draw": [draw],
            "gacha_info_list": [{
                "gacha_id": gacha_id,
                "is_account_first": false,
                "is_daily_first": false,
            }],
        },
        "character_list": [character],
        "item_list": item_list,
        "encyclopedia_info": [],
        "mail_arrived": false,
        "start_time": server_time,
    }))
}
// //// /构造教程扭蛋的客户端响应 ////

// //// 保存教程完成后的免费角色和货币 [@x380kkm 2026-08-23] ////
fn finish_tutorial_reward(
    root: &mut Map<String, Value>,
    viewer_id: i64,
    step: i64,
    server_time: i64,
) -> Result<Value, PersonalServiceError> {
    let free_vmoney = user_info_value(root, "free_vmoney")?;
    let new_free_vmoney = free_vmoney.checked_add(1_500).ok_or_else(|| {
        PersonalServiceError::new("CN tutorial reward currency exceeds the supported range")
    })?;
    set_user_info_value(root, "free_vmoney", new_free_vmoney)?;
    grant_character(root, viewer_id, FREE_TUTORIAL_CHARACTER_ID, server_time)?;
    tutorial_end_response(root, viewer_id, step, server_time)
}
// //// /保存教程完成后的免费角色和货币 ////

// //// 构造教程结束的客户端响应 [@x380kkm 2026-08-23] ////
fn tutorial_end_response(
    root: &Map<String, Value>,
    viewer_id: i64,
    step: i64,
    server_time: i64,
) -> Result<Value, PersonalServiceError> {
    let free_vmoney = user_info_value(root, "free_vmoney")?;
    let stored_character = root
        .get("user_character_list")
        .and_then(Value::as_object)
        .and_then(|characters| characters.get(&FREE_TUTORIAL_CHARACTER_ID.to_string()))
        .ok_or_else(|| {
            PersonalServiceError::new("stored CN tutorial reward character is missing")
        })?;
    let character = create_character_response(
        viewer_id,
        FREE_TUTORIAL_CHARACTER_ID,
        stored_character,
        server_time,
    );
    let mut encyclopedia_info = Map::new();
    encyclopedia_info.insert(
        format!("1{FREE_TUTORIAL_CHARACTER_ID}01"),
        json!({"read": false}),
    );
    Ok(json!({
        "step": step,
        "user_info": {"free_vmoney": free_vmoney},
        "character_list": [character],
        "encyclopedia_info": encyclopedia_info,
        "mail_arrived": step == 16,
        "start_time": server_time,
    }))
}
// //// /构造教程结束的客户端响应 ////

// //// 按 CN 角色主表构造持久化角色 [@x380kkm 2026-08-23] ////
pub(crate) fn create_stored_character(
    character_id: i64,
    server_time: i64,
) -> Result<Value, PersonalServiceError> {
    let bond_token_list = (1..=crate::cn_character::character_mana_board_count(character_id)?)
        .map(|mana_board_index| json!({"mana_board_index": mana_board_index, "status": 0}))
        .collect::<Vec<_>>();
    Ok(json!({
        "entry_count": 1,
        "exp": 0,
        "evolution_level": 0,
        "bond_token_list": bond_token_list,
        "mana_board_index": 1,
        "over_limit_step": 0,
        "protection": false,
        "stack": 0,
        "join_time": server_time,
        "update_time": server_time,
    }))
}
// //// /按 CN 角色主表构造持久化角色 ////

pub(crate) fn create_character_response(
    _viewer_id: i64,
    character_id: i64,
    stored_character: &Value,
    server_time: i64,
) -> Value {
    let entry_count = stored_character
        .get("entry_count")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    let exp = stored_character
        .get("exp")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let mana_board_index = stored_character
        .get("mana_board_index")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    let formatted_time = format_client_time(server_time);
    let mut response = json!({
        "viewer_id": 0,
        "character_id": character_id,
        "entry_count": entry_count,
        "exp": exp,
        "exp_total": exp,
        "mana_board_index": mana_board_index,
        "create_time": formatted_time.clone(),
        "update_time": formatted_time.clone(),
        "join_time": formatted_time,
    });
    if let Some(bond_token_list) = stored_character
        .get("bond_token_list")
        .and_then(Value::as_array)
    {
        response
            .as_object_mut()
            .expect("CN character response is an object")
            .insert(
                "bond_token_list".to_owned(),
                Value::Array(bond_token_list.clone()),
            );
    }
    response
}

pub(crate) fn format_client_time(server_time: i64) -> String {
    let seconds = server_time.max(0);
    let days = seconds / 86_400;
    let day_seconds = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        day_seconds / 3_600,
        (day_seconds % 3_600) / 60,
        day_seconds % 60,
    )
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = month_part + if month_part < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    (year, month, day)
}

pub(crate) fn player_snapshot(
    database: &ServiceDatabase,
    viewer_id: i64,
) -> Result<Result<PlayerSnapshot, HttpResponse>, PersonalServiceError> {
    match database.lookup_viewer_session_player(viewer_id)? {
        ViewerSessionPlayer::Present(snapshot) => Ok(Ok(snapshot)),
        ViewerSessionPlayer::InvalidSession => Ok(Err(error_response(
            "400 Bad Request",
            "invalid_viewer_session",
        ))),
        ViewerSessionPlayer::MissingPlayer => Ok(Err(error_response(
            "500 Internal Server Error",
            "no_player",
        ))),
        ViewerSessionPlayer::MissingPlayerData(_) => Ok(Err(error_response(
            "500 Internal Server Error",
            "no_player_data",
        ))),
    }
}

pub(crate) fn decode_player_data(serialized: &str) -> Result<Value, PersonalServiceError> {
    serde_json::from_str(serialized).map_err(|error| {
        PersonalServiceError::new(format!("failed to decode CN tutorial data: {error}"))
    })
}

pub(crate) fn encode_player_data(player_data: &Value) -> Result<String, PersonalServiceError> {
    serde_json::to_string(player_data).map_err(|error| {
        PersonalServiceError::new(format!("failed to encode CN tutorial data: {error}"))
    })
}

pub(crate) fn require_root(
    player_data: &mut Value,
) -> Result<&mut Map<String, Value>, PersonalServiceError> {
    player_data
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN tutorial data is not an object"))
}

pub(crate) fn require_object<'a>(
    root: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>, PersonalServiceError> {
    root.get_mut(key)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new(format!("stored CN {key} data is missing")))
}

pub(crate) fn user_info_value(
    root: &Map<String, Value>,
    key: &str,
) -> Result<i64, PersonalServiceError> {
    root.get("user_info")
        .and_then(Value::as_object)
        .and_then(|user_info| user_info.get(key))
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new(format!("stored CN {key} value is missing")))
}

pub(crate) fn set_user_info_value(
    root: &mut Map<String, Value>,
    key: &str,
    value: i64,
) -> Result<(), PersonalServiceError> {
    require_object(root, "user_info")?.insert(key.to_owned(), Value::from(value));
    Ok(())
}

fn normalize_name(value: &str) -> Option<String> {
    let name = value.trim();
    (!name.is_empty() && name.chars().count() <= 32 && !name.chars().any(char::is_control))
        .then(|| name.to_owned())
}

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}
