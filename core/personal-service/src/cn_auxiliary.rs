// audience: internal
// # personal-service-cn-auxiliary
//
// 该模块实现 CN 客户端辅助页面需要的类型化响应和生日存档更新.

use crate::cn::{decode_request, deserialize_optional_i64, msgpack_response_at, server_time};
use crate::cn_tutorial::{decode_player_data, encode_player_data, player_snapshot, require_root};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Deserialize)]
struct OptionalViewerRequest {
    #[serde(default, deserialize_with = "deserialize_optional_i64")]
    viewer_id: Option<i64>,
}

#[derive(Deserialize)]
struct UpdateBirthRequest {
    viewer_id: i64,
    birth: i64,
}

// //// 分派 CN 辅助页面请求 [@x380kkm 2026-08-24] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    let response = match request.path() {
        "/api/index.php/payment/update_birth" => update_birth(request, database),
        "/api/index.php/payment/detail_items" => respond(
            request,
            database,
            json!({"mana_detail_items": {}, "vmoney_detail_items": {}}),
        ),
        "/api/index.php/Payment_rebate/rebate" => payment_rebate(request, database),
        "/api/index.php/tool/agreement" => respond(
            request,
            database,
            json!({
                "required_privacy_version": null,
                "required_terms_version": null,
                "terms_text": "",
                "terms_url": null,
            }),
        ),
        "/api/index.php/tool/get_survey_url" => {
            respond(request, database, json!({"survey_url": ""}))
        }
        "/api/index.php/tool/get_qq_group_url" => {
            respond(request, database, json!({"qq_group_url": ""}))
        }
        "/api/index.php/tool/get_bug_report_url" => {
            respond(request, database, json!({"bug_report_url": ""}))
        }
        "/api/index.php/debug/get_characters" => {
            respond(request, database, json!({"user_character_list": []}))
        }
        "/api/index.php/debug/get_ios_tier_list" => {
            respond(request, database, json!({"ios_tier_list": {}}))
        }
        "/api/index.php/follow/search_id" => follow_search_id(request, database),
        "/api/index.php/follow/search_twitter" => {
            respond(request, database, json!({"search_result": []}))
        }
        "/api/index.php/follow/bulk_edit" => respond(
            request,
            database,
            json!({"max_follower_user_viewer_id_list": []}),
        ),
        "/api/index.php/take_over_register/get_take_over_setting" => respond(
            request,
            database,
            json!({
                "exists_user_take_over_data": false,
                "social_account": {
                    "is_apple_linked": false,
                    "is_facebook_linked": false,
                    "is_google_linked": false,
                },
            }),
        ),
        "/api/index.php/take_over_register/register_social_account" => {
            take_over_viewer_response(request, database, "registered_viewer_id")
        }
        "/api/index.php/take_over_register/disable_social_account" => {
            take_over_viewer_response(request, database, "disconnected_viewer_id")
        }
        "/api/index.php/take_over/get_user_data_by_take_over_data"
        | "/api/index.php/take_over/get_user_data_by_social_account" => respond(
            request,
            database,
            json!({"current_user": null, "linked_user": null}),
        ),
        "/api/index.php/take_over/take_over_by_take_over_data"
        | "/api/index.php/take_over/take_over_by_social_account" => {
            take_over_result(request, database)
        }
        "/api/index.php/feedback/suggest"
        | "/api/index.php/feedback/finish"
        | "/api/index.php/follow/add"
        | "/api/index.php/follow/delete"
        | "/api/index.php/follow/delete_followed"
        | "/api/index.php/user/delete_apply"
        | "/api/index.php/user/delete_cancel"
        | "/api/index.php/sns/update_twitter"
        | "/api/index.php/tool/change_active"
        | "/api/index.php/lounge/share" => respond(request, database, json!({})),
        _ => return None,
    };
    Some(response)
}
// //// /分派 CN 辅助页面请求 ////

// //// 保存支付年龄检查使用的生日 [@x380kkm 2026-08-24] ////
fn update_birth(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<UpdateBirthRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.birth > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    require_root(&mut player_data)?.insert("birth".to_owned(), Value::from(body.birth));
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(body.viewer_id, false, server_time(database)?, json!({}))
}
// //// /保存支付年龄检查使用的生日 ////

fn payment_rebate(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let viewer_id = optional_viewer_id(request)?;
    let after_vmoney = if let Some(viewer_id) = viewer_id.filter(|viewer_id| *viewer_id > 0) {
        match player_snapshot(database, viewer_id)? {
            Ok(snapshot) => decode_player_data(&snapshot.data)?
                .get("user_info")
                .and_then(|value| value.get("vmoney"))
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            Err(response) => return Ok(response),
        }
    } else {
        0
    };
    msgpack_response_at(
        viewer_id.unwrap_or_default().max(0),
        false,
        server_time(database)?,
        json!({"status": 0, "after_vmoney": after_vmoney}),
    )
}

fn follow_search_id(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let viewer_id = optional_viewer_id(request)?.unwrap_or_default();
    let snapshot = match player_snapshot(database, viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let player_data = decode_player_data(&snapshot.data)?;
    let user_info = player_data
        .get("user_info")
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("stored CN user_info is missing"))?;
    let leader_character_id = user_info
        .get("leader_character_id")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    msgpack_response_at(
        viewer_id,
        false,
        server_time(database)?,
        json!({
            "search_result": {
                "comment": user_info.get("comment").and_then(Value::as_str).unwrap_or(""),
                "degree_id": user_info.get("degree_id").and_then(Value::as_i64).unwrap_or_default(),
                "follow_state": 0,
                "follow_time": null,
                "followed_time": null,
                "last_login_region": null,
                "last_login_time": 0,
                "leader_character_evolution_img_level": 0,
                "leader_character_id": leader_character_id,
                "name": user_info.get("name").and_then(Value::as_str).unwrap_or("冒险者"),
                "profile_image_url": null,
                "rank": user_info.get("rank").and_then(Value::as_i64).unwrap_or(1),
                "role": user_info.get("role").and_then(Value::as_i64),
                "viewer_id": viewer_id,
            },
        }),
    )
}

fn take_over_viewer_response(
    request: &HttpRequest,
    database: &ServiceDatabase,
    key: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let viewer_id = optional_viewer_id(request)?.unwrap_or_default().max(0);
    let mut data = Map::new();
    data.insert(key.to_owned(), Value::from(viewer_id));
    msgpack_response_at(
        viewer_id,
        false,
        server_time(database)?,
        Value::Object(data),
    )
}

fn take_over_result(
    request: &HttpRequest,
    database: &ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let viewer_id = optional_viewer_id(request)?.unwrap_or_default().max(0);
    msgpack_response_at(
        viewer_id,
        false,
        server_time(database)?,
        json!({
            "abolished_viewer_id": viewer_id,
            "linked_viewer_id": viewer_id,
            "short_udid": 0,
        }),
    )
}

fn respond(
    request: &HttpRequest,
    database: &ServiceDatabase,
    data: Value,
) -> Result<HttpResponse, PersonalServiceError> {
    let viewer_id = optional_viewer_id(request)?.unwrap_or_default().max(0);
    msgpack_response_at(viewer_id, false, server_time(database)?, data)
}

fn optional_viewer_id(request: &HttpRequest) -> Result<Option<i64>, PersonalServiceError> {
    decode_request::<OptionalViewerRequest>(request)
        .map(|body| body.viewer_id)
        .map_err(|_| PersonalServiceError::new("invalid CN auxiliary request body"))
}

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}
