// audience: internal
// # personal-service-cn-profile
//
// 该模块从玩家快照生成 CN 个人资料和编队摘要.

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{decode_player_data, encode_player_data, player_snapshot, require_root};
use crate::database::{ServiceDatabase, ViewerSessionPlayer};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Deserialize)]
struct ProfileRequest {
    viewer_id: i64,
}

#[derive(Deserialize)]
struct TargetProfileRequest {
    viewer_id: i64,
    target_viewer_id: i64,
}

#[derive(Clone, Default, Deserialize)]
struct ProfileSettings {
    #[serde(default)]
    show_opened_mana_board_second_count: bool,
    #[serde(default)]
    show_owned_character_count: bool,
    #[serde(default)]
    show_owned_degree_count: bool,
}

#[derive(Deserialize)]
struct UpdateDegreeRequest {
    viewer_id: i64,
    degree_id: i64,
}

#[derive(Deserialize)]
struct UpdateProfileSettingsRequest {
    viewer_id: i64,
    profile_settings: Option<ProfileSettings>,
}

#[derive(Deserialize)]
struct UpdateCommentRequest {
    viewer_id: i64,
    #[serde(default)]
    comment: String,
}

#[derive(Deserialize)]
struct RenameRequest {
    viewer_id: i64,
    #[serde(default)]
    name: String,
}

// //// 分派 CN 个人资料请求 [@x380kkm 2026-08-22] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    let response = match request.path() {
        "/api/index.php/profile/get_my_profile" => get_my_profile(request, database),
        "/api/index.php/profile/get_profile" => get_profile(request, database),
        "/api/index.php/profile/update_degree" => update_degree(request, database),
        "/api/index.php/profile/update_profile_settings" => {
            update_profile_settings(request, database)
        }
        "/api/index.php/profile/update_comment" => update_comment(request, database),
        "/api/index.php/profile/rename" => rename(request, database),
        _ => return None,
    };
    Some(response)
}
// //// /分派 CN 个人资料请求 ////

// //// 返回当前玩家的资料计数和编队摘要 [@x380kkm 2026-08-22] ////
fn get_my_profile(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ProfileRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match database.lookup_viewer_session_player(body.viewer_id)? {
        ViewerSessionPlayer::Present(snapshot) => snapshot,
        ViewerSessionPlayer::InvalidSession => {
            return Ok(error_response("400 Bad Request", "invalid_viewer_session"));
        }
        ViewerSessionPlayer::MissingPlayer => {
            return Ok(error_response("400 Bad Request", "no_player"));
        }
        ViewerSessionPlayer::MissingPlayerData(_) => {
            return Ok(error_response(
                "500 Internal Server Error",
                "no_player_data",
            ));
        }
    };
    let player_data = decode_player_data(&snapshot.data)?;
    let root = player_data
        .as_object()
        .ok_or_else(|| PersonalServiceError::new("stored CN player data is not an object"))?;
    let character_count = i64::try_from(required_object(root, "user_character_list")?.len())
        .map_err(|_| PersonalServiceError::new("stored CN character count is out of range"))?;
    let degree_count = 1;
    let party_groups = profile_party_groups(root)?;
    let profile_settings = root
        .get("profile_settings")
        .and_then(Value::as_object)
        .map(|settings| {
            json!({
                "show_opened_mana_board_second_count": settings
                    .get("show_opened_mana_board_second_count")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                "show_owned_character_count": settings
                    .get("show_owned_character_count")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                "show_owned_degree_count": settings
                    .get("show_owned_degree_count")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
            })
        })
        .unwrap_or_else(|| {
            json!({
                "show_opened_mana_board_second_count": false,
                "show_owned_character_count": true,
                "show_owned_degree_count": true,
            })
        });

    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({
            "profile_info": {
                "max_opened_mana_board_second_count": 0,
                "max_owned_character_count": character_count,
                "max_owned_degree_count": degree_count,
                "opened_mana_board_second_count": 0,
                "owned_character_count": character_count,
                "owned_degree_count": degree_count,
            },
            "profile_settings": profile_settings,
            "user_party_group_list": party_groups,
        }),
    )
}
// //// /返回当前玩家的资料计数和编队摘要 ////

// //// 返回离线玩家资料 [@x380kkm 2026-08-24] ////
fn get_profile(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<TargetProfileRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.target_viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match database.lookup_viewer_session_player(body.viewer_id)? {
        ViewerSessionPlayer::Present(snapshot) => snapshot,
        ViewerSessionPlayer::InvalidSession => {
            return Ok(error_response("400 Bad Request", "invalid_viewer_session"));
        }
        ViewerSessionPlayer::MissingPlayer => {
            return Ok(error_response("400 Bad Request", "no_player"));
        }
        ViewerSessionPlayer::MissingPlayerData(_) => {
            return Ok(error_response(
                "500 Internal Server Error",
                "no_player_data",
            ));
        }
    };
    let player_data = decode_player_data(&snapshot.data)?;
    let root = player_data
        .as_object()
        .ok_or_else(|| PersonalServiceError::new("stored CN player data is not an object"))?;
    let user_info = required_object(root, "user_info")?;
    let characters = required_object(root, "user_character_list")?;
    let owned_character_count = i64::try_from(characters.len())
        .map_err(|_| PersonalServiceError::new("stored CN character count is out of range"))?;
    let leader_character_id = user_info
        .get("leader_character_id")
        .and_then(Value::as_i64)
        .filter(|character_id| characters.contains_key(&character_id.to_string()))
        .or_else(|| {
            characters
                .keys()
                .filter_map(|character_id| character_id.parse::<i64>().ok())
                .min()
        });
    let favorite_character_ids = leader_character_id.into_iter().collect::<Vec<_>>();

    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({
            "favorite_character": {
                "character_ex_boost": [],
                "character_ids": favorite_character_ids,
                "unison_character_ex_boost": [],
                "unison_character_ids": [],
            },
            "target_user_info": {
                "comment": user_info.get("comment").and_then(Value::as_str).unwrap_or(""),
                "degree_id": user_info.get("degree_id").and_then(Value::as_i64).unwrap_or_default(),
                "follow_state": 0,
                "last_login_region": null,
                "leader_character_full_shot_evolution_level": 0,
                "max_opened_mana_board_second_count": 0,
                "max_owned_character_count": owned_character_count,
                "max_owned_degree_count": 1,
                "name": user_info.get("name").and_then(Value::as_str).unwrap_or("冒险者"),
                "opened_mana_board_second_count": 0,
                "owned_character_count": owned_character_count,
                "owned_degree_count": 1,
                "rank": 1,
                "role": user_info.get("role").and_then(Value::as_i64).unwrap_or(1),
                "viewer_id": body.target_viewer_id,
            },
        }),
    )
}
// //// /返回离线玩家资料 ////

// //// 更新当前玩家的展示称号 [@x380kkm 2026-08-22] ////
fn update_degree(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<UpdateDegreeRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    crate::cn_tutorial::require_object(require_root(&mut player_data)?, "user_info")?
        .insert("degree_id".to_owned(), Value::from(body.degree_id));
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({"user_info": {"degree_id": body.degree_id}}),
    )
}
// //// /更新当前玩家的展示称号 ////

// //// 保存当前玩家的资料展示设置 [@x380kkm 2026-08-22] ////
fn update_profile_settings(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<UpdateProfileSettingsRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match database.lookup_viewer_session_player(body.viewer_id)? {
        ViewerSessionPlayer::InvalidSession => {
            return Ok(error_response("400 Bad Request", "invalid_viewer_session"));
        }
        ViewerSessionPlayer::Present(snapshot) => Some(snapshot),
        ViewerSessionPlayer::MissingPlayer | ViewerSessionPlayer::MissingPlayerData(_) => None,
    };
    let profile_settings = body.profile_settings.unwrap_or_default();
    let settings = json!({
        "show_opened_mana_board_second_count": profile_settings
            .show_opened_mana_board_second_count,
        "show_owned_character_count": profile_settings.show_owned_character_count,
        "show_owned_degree_count": profile_settings.show_owned_degree_count,
    });
    if let Some(snapshot) = snapshot {
        let mut player_data = decode_player_data(&snapshot.data)?;
        require_root(&mut player_data)?.insert("profile_settings".to_owned(), settings.clone());
        database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    }
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({"profile_settings": settings}),
    )
}
// //// /保存当前玩家的资料展示设置 ////

// //// 保存当前玩家的资料留言 [@x380kkm 2026-08-22] ////
fn update_comment(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<UpdateCommentRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    update_user_info_text(database, body.viewer_id, "comment", &body.comment, 100)
}
// //// /保存当前玩家的资料留言 ////

// //// 保存当前玩家的名称 [@x380kkm 2026-08-22] ////
fn rename(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<RenameRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    update_user_info_text(database, body.viewer_id, "name", &body.name, 20)
}
// //// /保存当前玩家的名称 ////

fn update_user_info_text(
    database: &mut ServiceDatabase,
    viewer_id: i64,
    key: &str,
    value: &str,
    max_characters: usize,
) -> Result<HttpResponse, PersonalServiceError> {
    let snapshot = match database.lookup_viewer_session_player(viewer_id)? {
        ViewerSessionPlayer::Present(snapshot) => snapshot,
        ViewerSessionPlayer::InvalidSession => {
            return Ok(error_response("400 Bad Request", "invalid_viewer_session"));
        }
        ViewerSessionPlayer::MissingPlayer => {
            return Ok(error_response("400 Bad Request", "no_player"));
        }
        ViewerSessionPlayer::MissingPlayerData(_) => {
            return Ok(error_response(
                "500 Internal Server Error",
                "no_player_data",
            ));
        }
    };
    let value = value.chars().take(max_characters).collect::<String>();
    let mut player_data = decode_player_data(&snapshot.data)?;
    crate::cn_tutorial::require_object(require_root(&mut player_data)?, "user_info")?
        .insert(key.to_owned(), Value::String(value.clone()));
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    let mut response = Map::new();
    response.insert(key.to_owned(), Value::String(value));
    msgpack_response_at(
        viewer_id,
        false,
        server_time(database)?,
        Value::Object(response),
    )
}

// //// 将持久化编队映射为个人资料协议数组 [@x380kkm 2026-08-22] ////
fn profile_party_groups(root: &Map<String, Value>) -> Result<Vec<Value>, PersonalServiceError> {
    let groups = required_object(root, "user_party_group_list")?;
    let mut indexed_groups = parse_indexed_values(groups, "party group")?;
    indexed_groups.sort_by_key(|(group_id, _)| *group_id);
    let mut response = Vec::with_capacity(indexed_groups.len());

    for (group_id, group_value) in indexed_groups {
        let group = group_value
            .as_object()
            .ok_or_else(|| PersonalServiceError::new("stored CN party group is invalid"))?;
        let color_id = required_i64(group, "color_id", "party group color")?;
        let parties = required_object(group, "list")?;
        let mut indexed_parties = parse_indexed_values(parties, "party")?;
        indexed_parties.sort_by_key(|(party_id, _)| *party_id);
        let mut party_list = Vec::with_capacity(indexed_parties.len());

        for (party_id, party_value) in indexed_parties {
            let party = party_value
                .as_object()
                .ok_or_else(|| PersonalServiceError::new("stored CN party is invalid"))?;
            let options = required_object(party, "options")?;
            party_list.push(json!({
                "ability_soul_ids": required_array(party, "ability_soul_ids")?,
                "character_ids": required_array(party, "character_ids")?,
                "equipment_ids": required_array(party, "equipment_ids")?,
                "options": {
                    "allow_other_players_to_heal_me": required_bool(
                        options,
                        "allow_other_players_to_heal_me",
                        "party healing option",
                    )?,
                },
                "party_edited": required_bool(party, "edited", "party edited state")?,
                "party_id": party_id,
                "party_name": required_str(party, "name", "party name")?,
                "unison_character_ids": required_array(party, "unison_character_ids")?,
            }));
        }

        response.push(json!({
            "party_group_color_id": color_id,
            "party_group_id": group_id,
            "party_list": party_list,
        }));
    }
    Ok(response)
}
// //// /将持久化编队映射为个人资料协议数组 ////

fn parse_indexed_values<'a>(
    values: &'a Map<String, Value>,
    kind: &str,
) -> Result<Vec<(i64, &'a Value)>, PersonalServiceError> {
    values
        .iter()
        .map(|(key, value)| {
            key.parse::<i64>()
                .map(|id| (id, value))
                .map_err(|_| PersonalServiceError::new(format!("stored CN {kind} id is invalid")))
        })
        .collect()
}

fn required_object<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a Map<String, Value>, PersonalServiceError> {
    object
        .get(key)
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new(format!("stored CN {key} is missing")))
}

fn required_array(
    object: &Map<String, Value>,
    key: &str,
) -> Result<Vec<Value>, PersonalServiceError> {
    object
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| PersonalServiceError::new(format!("stored CN {key} is missing")))
}

fn required_i64(
    object: &Map<String, Value>,
    key: &str,
    label: &str,
) -> Result<i64, PersonalServiceError> {
    object
        .get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new(format!("stored CN {label} is invalid")))
}

fn required_bool(
    object: &Map<String, Value>,
    key: &str,
    label: &str,
) -> Result<bool, PersonalServiceError> {
    object
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| PersonalServiceError::new(format!("stored CN {label} is invalid")))
}

fn required_str<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    label: &str,
) -> Result<&'a str, PersonalServiceError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| PersonalServiceError::new(format!("stored CN {label} is invalid")))
}

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}
