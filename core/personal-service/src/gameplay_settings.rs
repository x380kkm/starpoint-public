// audience: internal
// # personal-service-gameplay-settings
//
// 该模块提供管理页使用的战斗资源掉落倍率 API.

use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::management;
use serde::Deserialize;
use serde_json::json;

const PATH: &str = "/v1/gameplay-settings";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateRequest {
    drop_multiplier: i64,
}

pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, crate::PersonalServiceError>> {
    if request.path() != PATH {
        return None;
    }
    if !management::is_authorized(request, database) {
        return Some(Ok(management::unauthorized_response()));
    }
    let response = match request.method() {
        "GET" => get(database),
        "PUT" => update(request, database),
        _ => Ok(HttpResponse::json(
            "405 Method Not Allowed",
            "{\"error\":\"method_not_allowed\"}".to_owned(),
        )),
    };
    Some(response)
}

fn get(database: &ServiceDatabase) -> Result<HttpResponse, crate::PersonalServiceError> {
    Ok(HttpResponse::json(
        "200 OK",
        json!({ "drop_multiplier": database.drop_multiplier()? }).to_string(),
    ))
}

fn update(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, crate::PersonalServiceError> {
    if !request
        .header("content-type")
        .is_some_and(|value| value.starts_with("application/json"))
    {
        return Ok(HttpResponse::json(
            "400 Bad Request",
            "{\"error\":\"invalid_gameplay_settings_request\"}".to_owned(),
        ));
    }
    let body = match serde_json::from_slice::<UpdateRequest>(request.body()) {
        Ok(body) => body,
        Err(_) => {
            return Ok(HttpResponse::json(
                "400 Bad Request",
                "{\"error\":\"invalid_gameplay_settings_request\"}".to_owned(),
            ));
        }
    };
    if !(1..=100).contains(&body.drop_multiplier) {
        return Ok(HttpResponse::json(
            "400 Bad Request",
            "{\"error\":\"invalid_drop_multiplier\"}".to_owned(),
        ));
    }
    let value = database.set_drop_multiplier(body.drop_multiplier)?;
    Ok(HttpResponse::json(
        "200 OK",
        json!({ "drop_multiplier": value }).to_string(),
    ))
}
