// audience: internal
// # personal-service-cn-news
//
// 该模块从同一份包内条目返回 CN 客户端新闻列表和详情数据.

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::database::{ServiceDatabase, ViewerSessionPlayer};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use std::sync::OnceLock;

const NEWS_ASSET: &str = include_str!("../assets/cn-news.json");
static NEWS_ENTRIES: OnceLock<Result<Vec<serde_json::Value>, String>> = OnceLock::new();

#[derive(Deserialize)]
struct NewsRequest {
    viewer_id: i64,
    news_id: i64,
    #[serde(rename = "api_count")]
    _api_count: Option<i64>,
}

// //// 分派 CN 新闻查询请求 [@x380kkm 2026-07-24] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" || request.path() != "/api/index.php/news/get_info" {
        return None;
    }
    Some(get_info(request, database))
}
// //// /分派 CN 新闻查询请求 ////

fn get_info(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<NewsRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.news_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    if matches!(
        database.lookup_viewer_session_player(body.viewer_id)?,
        ViewerSessionPlayer::InvalidSession
    ) {
        return Ok(error_response("400 Bad Request", "invalid_viewer_session"));
    }
    let response_time = server_time(database)?;
    let Some(news) = packaged_news_entry(body.news_id)? else {
        return Ok(error_response("400 Bad Request", "news_not_found"));
    };
    msgpack_response_at(body.viewer_id, false, response_time, news)
}

pub(crate) fn packaged_news_entries() -> Result<Vec<serde_json::Value>, PersonalServiceError> {
    NEWS_ENTRIES
        .get_or_init(|| {
            serde_json::from_str::<Vec<serde_json::Value>>(NEWS_ASSET)
                .map_err(|error| format!("failed to decode CN news asset: {error}"))
        })
        .as_ref()
        .cloned()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

fn packaged_news_entry(news_id: i64) -> Result<Option<serde_json::Value>, PersonalServiceError> {
    Ok(packaged_news_entries()?
        .into_iter()
        .find(|entry| entry.get("id").and_then(serde_json::Value::as_i64) == Some(news_id)))
}

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}
