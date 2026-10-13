// audience: internal
// # personal-service-cn-receive-history
// 领取历史按账号隔离, 每页 100 条, 页码从 1 开始. 总数包含该账号的全部领取记录.

use super::{error_response, respond};
use crate::cn::decode_request;
use crate::cn_tutorial::format_client_time;
use crate::database::{ServiceDatabase, StoredReceiveHistory, ViewerSessionPlayer};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Value};

const PAGE_SIZE: i64 = 100;

#[derive(Deserialize)]
struct ReceiveHistoryRequest {
    viewer_id: i64,
    page: Option<i64>,
}

// //// 按客户端页码读取奖励领取记录 [@x380kkm 2026-09-23] ////
pub(super) fn get(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ReceiveHistoryRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let page = body.page.unwrap_or(1);
    let Some(offset) = page
        .checked_sub(1)
        .filter(|page| *page >= 0)
        .and_then(|page| page.checked_mul(PAGE_SIZE))
    else {
        return Ok(error_response("400 Bad Request", "invalid_request_body"));
    };
    let account_id = match database.lookup_viewer_session_player(body.viewer_id)? {
        ViewerSessionPlayer::Present(snapshot) => snapshot.account_id,
        ViewerSessionPlayer::MissingPlayerData(account_id) => account_id,
        ViewerSessionPlayer::InvalidSession => {
            return Ok(error_response("400 Bad Request", "invalid_viewer_session"));
        }
        ViewerSessionPlayer::MissingPlayer => {
            return Ok(error_response("400 Bad Request", "no_player"));
        }
    };
    let page = database.read_receive_history_page(account_id, PAGE_SIZE, offset)?;
    let history = page.entries.iter().map(serialize_entry).collect::<Vec<_>>();
    respond(
        database,
        body.viewer_id,
        json!({
            "history": history,
            "total_count": page.total_count,
        }),
    )
}
// //// /按客户端页码读取奖励领取记录 ////

// //// 将领取记录转换为客户端奖励字段 [@x380kkm 2026-09-23] ////
fn serialize_entry(entry: &StoredReceiveHistory) -> Value {
    json!({
        "create_time": format_client_time(entry.create_time),
        "description": entry.description,
        "number": entry.number,
        "reason_id": entry.reason_id,
        "subject": entry.subject,
        "type": entry.kind,
        "type_id": entry.type_id,
    })
}
// //// /将领取记录转换为客户端奖励字段 ////
