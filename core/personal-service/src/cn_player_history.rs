// audience: internal
// # personal-service-cn-player-history
// 回忆录目录来自客户端主表. 展示资格使用独立活动时窗, 任务资格使用任务所属活动时窗.
// 每轮首次合法访问在领奖前固定统计值. 卡片选项, 统计和任务奖励使用同一玩家事务.

mod capture;
mod catalog;
mod edit;
mod statistics;

use crate::cn::{
    decode_request, msgpack_response_at, msgpack_result_code_response_at, server_time,
};
use crate::cn_tutorial::{decode_player_data, encode_player_data, player_snapshot, require_root};
use crate::database::{ActivityWindowStatus, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub(crate) use catalog::{activity_definitions, default_activity_window};
const STATE_FIELD: &str = "player_history_cards";
const INVALID_PERIOD: i64 = 11101;

#[derive(Deserialize)]
struct ViewerRequest {
    viewer_id: i64,
}

#[derive(Clone, Deserialize, Serialize)]
struct FavoriteCharacters {
    character_ids: Vec<Option<i64>>,
    unison_character_ids: Vec<Option<i64>>,
}

#[derive(Deserialize, Serialize)]
struct CardSettings {
    background_card_id: i64,
    degree_id: i64,
    favorite_character: FavoriteCharacters,
    #[serde(default)]
    visible: BTreeMap<i64, bool>,
}

// //// 读取合法回忆录并原子结算访问任务 [@x380kkm 2026-09-26] ////
pub(crate) fn index(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    database
        .with_player_settlement(|database| index_and_settle(request, database).map(Some))?
        .ok_or_else(|| PersonalServiceError::new("CN player history response is missing"))
}

fn index_and_settle(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ViewerRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        _ => return Ok(invalid_request()),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let now = server_time(database)?;
    let Some(history) = available_history(database, now)? else {
        return msgpack_result_code_response_at(body.viewer_id, now, INVALID_PERIOD);
    };
    let mut player = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player)?;
    let settings = card_settings(root, history.id)?;
    let topics = capture::topic_list(
        root,
        database,
        snapshot.account_id,
        history.id,
        &settings.visible,
    )?;
    let completed =
        crate::cn_mission::record_player_history_action(root, database, snapshot.account_id)?;
    let mut response = json!({
        "player_history_id": history.id,
        "background_card_id": settings.background_card_id,
        "degree_id": settings.degree_id,
        "favorite_character": settings.favorite_character,
        "player_history_topic_list": topics,
        "mission_info": completed,
    });
    crate::cn_mission::apply_mission_reward_response(
        &mut response,
        root,
        &completed,
        body.viewer_id,
        now,
    )?;
    save_settings(root, history.id, &settings)?;
    database.save_player_snapshot(
        snapshot.account_id,
        &encode_player_data(&Value::Object(root.clone()))?,
    )?;
    msgpack_response_at(body.viewer_id, false, now, response)
}

pub(crate) use edit::edit;

fn available_history(
    database: &ServiceDatabase,
    now: i64,
) -> Result<Option<&'static catalog::History>, PersonalServiceError> {
    for history in &catalog::definitions()?.histories {
        let open = match database
            .activity_window_status(&format!("player-history:{}", history.id), now * 1000)?
        {
            ActivityWindowStatus::Unscheduled => {
                history.start_time <= now && now <= history.end_time
            }
            ActivityWindowStatus::Open => true,
            _ => false,
        };
        if open {
            return Ok(Some(history));
        }
    }
    Ok(None)
}

// //// 保存合法卡片选项并修正已失效的角色引用 [@x380kkm 2026-09-26] ////
fn card_settings(
    root: &Map<String, Value>,
    history_id: i64,
) -> Result<CardSettings, PersonalServiceError> {
    let data = catalog::definitions()?;
    let default_background = data
        .backgrounds
        .iter()
        .find(|background| background.is_default)
        .ok_or_else(|| {
            PersonalServiceError::new("CN player history default background is missing")
        })?
        .id;
    let owned = root.get("user_character_list").and_then(Value::as_object);
    let leader = root
        .get("user_info")
        .and_then(|info| info.get("leader_character_id"))
        .and_then(Value::as_i64)
        .filter(|id| owned.is_some_and(|characters| characters.contains_key(&id.to_string())));
    let default_degree = root
        .get("user_info")
        .and_then(|info| info.get("degree_id"))
        .and_then(Value::as_i64)
        .unwrap_or(1);
    let mut settings = root
        .get(STATE_FIELD)
        .and_then(|cards| cards.get(history_id.to_string()))
        .cloned()
        .map(serde_json::from_value::<CardSettings>)
        .transpose()
        .map_err(|error| {
            PersonalServiceError::new(format!("invalid CN player history settings: {error}"))
        })?
        .unwrap_or_else(|| CardSettings {
            background_card_id: default_background,
            degree_id: default_degree,
            favorite_character: FavoriteCharacters {
                character_ids: vec![leader, None, None],
                unison_character_ids: vec![None; 3],
            },
            visible: BTreeMap::new(),
        });
    if !data
        .backgrounds
        .iter()
        .any(|background| background.id == settings.background_card_id)
    {
        settings.background_card_id = default_background;
    }
    if !edit::owns_degree(root, settings.degree_id) {
        settings.degree_id = default_degree;
    }
    for characters in [
        &mut settings.favorite_character.character_ids,
        &mut settings.favorite_character.unison_character_ids,
    ] {
        characters.resize(3, None);
        characters.truncate(3);
        for id in characters {
            if id.is_some_and(|id| !owned.is_some_and(|owned| owned.contains_key(&id.to_string())))
            {
                *id = None;
            }
        }
    }
    Ok(settings)
}

fn save_settings(
    root: &mut Map<String, Value>,
    id: i64,
    settings: &CardSettings,
) -> Result<(), PersonalServiceError> {
    let encoded = serde_json::to_value(settings)
        .map_err(|error| PersonalServiceError::new(error.to_string()))?;
    root.entry(STATE_FIELD.to_owned())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("CN player history cards are invalid"))?
        .insert(id.to_string(), encoded);
    Ok(())
}

fn invalid_request() -> HttpResponse {
    HttpResponse::json(
        "400 Bad Request",
        json!({"error":"invalid_request_body"}).to_string(),
    )
}
