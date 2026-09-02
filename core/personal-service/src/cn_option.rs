// audience: internal
// # personal-service-cn-option
//
// 该模块持久化 CN 客户端的用户选项更新.

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{decode_player_data, encode_player_data, player_snapshot, require_root};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct UpdateRequest {
    viewer_id: i64,
    api_count: Option<i64>,
    option_params: BTreeMap<String, bool>,
}

// //// 分派 CN 用户选项更新请求 [@x380kkm 2026-07-24] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    if !matches!(
        request.path(),
        "/api/index.php/option/update" | "/api/index.php/option/update_in_battle"
    ) {
        return None;
    }
    Some(update(request, database))
}
// //// /分派 CN 用户选项更新请求 ////

fn update(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<UpdateRequest>(request) {
        Ok(body)
            if body.viewer_id > 0 && body.api_count.map_or(true, |api_count| api_count >= 0) =>
        {
            body
        }
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let option = root
        .entry("user_option".to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN user options are invalid"))?;
    let response_options = body.option_params.clone();
    for (key, value) in body.option_params {
        option.insert(key, Value::Bool(value));
    }
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        serde_json::json!({"user_option": response_options}),
    )
}

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}
