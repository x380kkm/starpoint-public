// audience: internal
// # personal-service-cn-party-group
//
// 该模块实现 CN 编队组颜色编辑协议, 并保存编队组快照.

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, player_snapshot, require_object, require_root,
};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct PartyGroupEditRequest {
    viewer_id: i64,
    #[serde(default)]
    api_count: i64,
    #[serde(default)]
    retry_count: i64,
    party_group_edit_params_list: Vec<PartyGroupEdit>,
}

#[derive(Deserialize)]
struct PartyGroupEdit {
    party_group_id: i64,
    #[serde(rename = "party_category")]
    _party_category: i64,
    party_group_color_id: i64,
}

// //// 分派 CN 编队组编辑请求 [@x380kkm 2026-07-24] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" || request.path() != "/api/index.php/party_group/edit" {
        return None;
    }
    Some(edit(request, database))
}
// //// /分派 CN 编队组编辑请求 ////

fn edit(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<PartyGroupEditRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.api_count >= 0
                && body.retry_count >= 0
                && body.party_group_edit_params_list.len() <= 100 =>
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
    let groups = require_object(root, "user_party_group_list")?;
    for edit in &body.party_group_edit_params_list {
        if edit.party_group_id <= 0 || edit.party_group_color_id < 0 {
            return Ok(error_response("400 Bad Request", "invalid_party_group"));
        }
        let Some(group) = groups.get_mut(&edit.party_group_id.to_string()) else {
            return Ok(error_response("400 Bad Request", "party_group_not_found"));
        };
        let group = group
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN party group is invalid"))?;
        group.insert(
            "color_id".to_owned(),
            serde_json::Value::from(edit.party_group_color_id),
        );
    }
    let response_time = server_time(database)?;
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(body.viewer_id, false, response_time, json!({}))
}

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}
