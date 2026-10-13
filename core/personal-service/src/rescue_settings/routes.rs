// audience: internal
// # personal-service-rescue-management
// 管理接口保存救援选择和房间时限, 目录响应结合当前存档解锁条件和关卡时窗.

use super::{catalog, eligible_quest_keys, policy, RescueQuest};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::{management, PersonalServiceError};
use serde::Deserialize;
use serde_json::{json, Value};

const SETTINGS_PATH: &str = "/v1/rescue-settings";
const QUESTS_PATH: &str = "/v1/rescue-settings/quests";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateRequest {
    enabled: Option<bool>,
    spawn_interval_seconds: Option<i64>,
    room_lifetime_seconds: Option<i64>,
    max_rooms: Option<usize>,
    quests: Option<Vec<RescueQuest>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuestUpdateRequest {
    enabled: bool,
}

// //// 分派本机救援管理请求 [@x380kkm 2026-09-23] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let quest_path = request.path().strip_prefix("/v1/rescue-settings/quests/");
    if !matches!(request.path(), SETTINGS_PATH | QUESTS_PATH) && quest_path.is_none() {
        return None;
    }
    if !management::is_authorized(request, database) {
        return Some(Ok(management::unauthorized_response()));
    }
    if let Some(path) = quest_path {
        return Some(if request.method() == "PUT" {
            update_quest(request, database, path)
        } else {
            Ok(HttpResponse::json(
                "405 Method Not Allowed",
                json!({"error":"method_not_allowed"}).to_string(),
            ))
        });
    }
    Some(match (request.method(), request.path()) {
        ("GET", SETTINGS_PATH) => settings_response(database),
        ("PUT", SETTINGS_PATH) => update(request, database),
        ("GET", QUESTS_PATH) => catalog_response(database),
        _ => Ok(HttpResponse::json(
            "405 Method Not Allowed",
            json!({"error": "method_not_allowed"}).to_string(),
        )),
    })
}
// //// /分派本机救援管理请求 ////

// //// 独立更新开放关卡的铃铛选择 [@x380kkm 2026-09-26] ////
fn update_quest(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    path: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some((category_id, quest_id)) = path.split_once('/').and_then(|(category, quest)| {
        Some((category.parse::<i64>().ok()?, quest.parse::<i64>().ok()?))
    }) else {
        return Ok(invalid("invalid_rescue_quest"));
    };
    let quest = RescueQuest {
        category_id,
        quest_id,
    };
    if !catalog::quests()?.contains_key(&quest) {
        return Ok(invalid("invalid_rescue_quest"));
    }
    let Ok(update) = serde_json::from_slice::<QuestUpdateRequest>(request.body()) else {
        return Ok(invalid("invalid_rescue_settings"));
    };
    if update.enabled && !crate::cn_quest::is_quest_open(database, category_id, quest_id)? {
        return Ok(invalid("rescue_quest_closed"));
    }
    let mut settings = database.rescue_settings()?;
    settings.quests.retain(|selected| *selected != quest);
    if update.enabled {
        settings.quests.push(quest);
        settings.quests.sort_unstable();
        settings.enabled = true;
    }
    database.set_rescue_settings(&settings)?;
    settings_response(database)
}
// //// /独立更新开放关卡的铃铛选择 ////

fn settings_response(database: &ServiceDatabase) -> Result<HttpResponse, PersonalServiceError> {
    let settings = database.rescue_settings()?;
    let mut response = serde_json::to_value(settings).map_err(|error| {
        PersonalServiceError::new(format!("failed to encode rescue settings: {error}"))
    })?;
    response["eligible_quest_count"] = json!(policy(database)?.quests.len());
    Ok(HttpResponse::json("200 OK", response.to_string()))
}

// //// 返回具备开放状态的协战关卡目录 [@x380kkm 2026-09-23] ////
fn catalog_response(database: &ServiceDatabase) -> Result<HttpResponse, PersonalServiceError> {
    let mut quests = Vec::new();
    let open = eligible_quest_keys(database)?;
    for definition in catalog::quests()?.values() {
        let mut entry = serde_json::to_value(definition).map_err(|error| {
            PersonalServiceError::new(format!("failed to encode rescue quest: {error}"))
        })?;
        entry["is_open"] =
            Value::Bool(open.contains(&(definition.quest.category_id, definition.quest.quest_id)));
        quests.push(entry);
    }
    Ok(HttpResponse::json(
        "200 OK",
        json!({"quests": quests}).to_string(),
    ))
}
// //// /返回具备开放状态的协战关卡目录 ////

// //// 合并救援配置并保存有效的关卡选择 [@x380kkm 2026-09-23] ////
fn update(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let update = match serde_json::from_slice::<UpdateRequest>(request.body()) {
        Ok(update) => update,
        Err(_) => return Ok(invalid("invalid_rescue_settings")),
    };
    let mut settings = database.rescue_settings()?;
    if let Some(enabled) = update.enabled {
        settings.enabled = enabled;
    }
    if let Some(interval) = update.spawn_interval_seconds {
        settings.spawn_interval_seconds = interval;
    }
    if let Some(lifetime) = update.room_lifetime_seconds {
        settings.room_lifetime_seconds = lifetime;
    }
    if let Some(max_rooms) = update.max_rooms {
        settings.max_rooms = max_rooms;
    }
    if !(5..=3_600).contains(&settings.spawn_interval_seconds)
        || !(30..=3_600).contains(&settings.room_lifetime_seconds)
        || !(1..=10).contains(&settings.max_rooms)
    {
        return Ok(invalid("invalid_rescue_settings"));
    }
    if let Some(mut quests) = update.quests {
        let catalog = catalog::quests()?;
        if quests.iter().any(|quest| !catalog.contains_key(quest)) {
            return Ok(invalid("invalid_rescue_quest"));
        }
        quests.sort_unstable();
        quests.dedup();
        settings.quests = quests;
    }
    database.set_rescue_settings(&settings)?;
    settings_response(database)
}
// //// /合并救援配置并保存有效的关卡选择 ////

fn invalid(code: &str) -> HttpResponse {
    HttpResponse::json("400 Bad Request", json!({"error": code}).to_string())
}
