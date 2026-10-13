// audience: internal
// # personal-service-cn-player-history-edit
// 卡片编辑请求与客户端的主题, 称号, 背景及三人主副队字段一致. 引用限定为真实主表与玩家持有对象.

use super::*;
use std::collections::BTreeSet;

#[derive(Deserialize)]
struct EditRequest {
    viewer_id: i64,
    background_card_id: Option<i64>,
    degree_id: Option<i64>,
    party_info: Option<FavoriteCharacters>,
    player_history_topic_visible: Option<BTreeMap<String, bool>>,
}

// //// 原子保存当前开放回忆录的卡片编辑 [@x380kkm 2026-09-26] ////
pub(crate) fn edit(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    database
        .with_player_settlement(|database| edit_and_save(request, database).map(Some))?
        .ok_or_else(|| PersonalServiceError::new("CN player history edit response is missing"))
}

fn edit_and_save(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<EditRequest>(request) {
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
    let data = catalog::definitions()?;
    let mut player = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player)?;
    let mut settings = card_settings(root, history.id)?;
    if let Some(id) = body.background_card_id {
        if !data
            .backgrounds
            .iter()
            .any(|background| background.id == id)
        {
            return Ok(invalid_request());
        }
        settings.background_card_id = id;
    }
    if let Some(id) = body.degree_id {
        if !owns_degree(root, id) {
            return Ok(invalid_request());
        }
        settings.degree_id = id;
    }
    if let Some(party) = body.party_info {
        if party.character_ids.len() != 3 || party.unison_character_ids.len() != 3 {
            return Ok(invalid_request());
        }
        let owned = root.get("user_character_list").and_then(Value::as_object);
        let mut unique = BTreeSet::new();
        if !party
            .character_ids
            .iter()
            .chain(&party.unison_character_ids)
            .flatten()
            .all(|id| {
                owned.is_some_and(|owned| owned.contains_key(&id.to_string())) && unique.insert(*id)
            })
        {
            return Ok(invalid_request());
        }
        settings.favorite_character = party;
    }
    if let Some(visible) = body.player_history_topic_visible {
        let Ok(visible) = visible
            .into_iter()
            .map(|(index, value)| index.parse::<i64>().map(|index| (index, value)))
            .collect::<Result<BTreeMap<_, _>, _>>()
        else {
            return Ok(invalid_request());
        };
        let topics = data
            .topics
            .get(&history.id)
            .ok_or_else(|| PersonalServiceError::new("CN player history topics are missing"))?;
        if !visible
            .keys()
            .all(|index| topics.iter().any(|topic| topic.index == *index))
        {
            return Ok(invalid_request());
        }
        settings.visible.extend(visible);
    }
    save_settings(root, history.id, &settings)?;
    database.save_player_snapshot(
        snapshot.account_id,
        &encode_player_data(&Value::Object(root.clone()))?,
    )?;
    msgpack_response_at(body.viewer_id, false, now, json!({}))
}

pub(super) fn owns_degree(root: &Map<String, Value>, id: i64) -> bool {
    id == 1
        || root
            .get("user_info")
            .and_then(|info| info.get("degree_id"))
            .and_then(Value::as_i64)
            == Some(id)
        || root
            .get("degree_ids")
            .and_then(Value::as_array)
            .is_some_and(|ids| ids.iter().any(|value| value.as_i64() == Some(id)))
}
