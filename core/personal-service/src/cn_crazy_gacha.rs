// audience: internal
// # personal-service-cn-crazy-gacha
// 乐享扭蛋以一张券生成至多 50 组候选, 当前组为槽位 0, 保留组为槽位 1 和 2.
// 最终选定的角色, 任务和领取记录在同一快照事务提交.

mod reward;
mod state;

pub(crate) use state::{prepare_load, store_draw, ticket_cost};

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_gacha::receipt::GachaReceipt;
use crate::cn_tutorial::{decode_player_data, encode_player_data, player_snapshot, require_root};
use crate::database::{ReceiveHistoryEntry, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};

const GACHA_ID: i64 = 100;

#[derive(Deserialize)]
struct ResultRequest {
    viewer_id: i64,
    #[serde(default)]
    api_count: i64,
    index: i64,
    #[serde(default)]
    gacha_id: i64,
}

struct Mutation {
    response: Value,
    history: Vec<ReceiveHistoryEntry>,
}

type Operation = fn(
    &mut Map<String, Value>,
    &ResultRequest,
    &ServiceDatabase,
    i64,
) -> Result<Result<Mutation, &'static str>, PersonalServiceError>;

// //// 保存或选定乐享扭蛋结果并重放已提交响应 [@x380kkm 2026-09-23] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    let (action, operation): (&str, Operation) = match request.path() {
        "/api/index.php/gacha/crazy_gacha_save" => ("crazy-save", save),
        "/api/index.php/gacha/crazy_gacha_select" => ("crazy-select", select),
        _ => return None,
    };
    Some(execute(request, database, action, operation))
}

fn execute(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    action: &str,
    operation: Operation,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ResultRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.api_count >= 0 => body,
        _ => return Ok(error_response("invalid_crazy_gacha_request")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player)?;
    let receipt = GachaReceipt::new(
        action,
        body.viewer_id,
        body.api_count,
        json!({"index": body.index, "gacha_id": body.gacha_id}),
    );
    if let Some(response) = receipt.replay(root)? {
        return Ok(response);
    }
    let response_time = server_time(database)?;
    let mut mutation = match operation(root, &body, database, response_time)? {
        Ok(mutation) => mutation,
        Err(code) => return Ok(error_response(code)),
    };
    mutation.response["mail_arrived"] =
        json!(database.has_unreceived_mail(snapshot.account_id, response_time)?);
    let event_key = receipt.record(root, &mutation.response, response_time)?;
    database.save_player_snapshot_with_receive_history(
        snapshot.account_id,
        &encode_player_data(&player)?,
        &event_key,
        response_time,
        &mutation.history,
    )?;
    msgpack_response_at(body.viewer_id, false, response_time, mutation.response)
}
// //// /保存或选定乐享扭蛋结果并重放已提交响应 ////

// //// 将当前候选保存到一个空保留槽位 [@x380kkm 2026-09-23] ////
fn save(
    root: &mut Map<String, Value>,
    request: &ResultRequest,
    _database: &ServiceDatabase,
    _response_time: i64,
) -> Result<Result<Mutation, &'static str>, PersonalServiceError> {
    if let Err(code) = state::save_slot(root, request.index)? {
        return Ok(Err(code));
    }
    Ok(Ok(Mutation {
        response: state::response(root)?,
        history: Vec::new(),
    }))
}
// //// /将当前候选保存到一个空保留槽位 ////

// //// 发放选定组并完成当前乐享扭蛋 [@x380kkm 2026-09-23] ////
fn select(
    root: &mut Map<String, Value>,
    request: &ResultRequest,
    database: &ServiceDatabase,
    response_time: i64,
) -> Result<Result<Mutation, &'static str>, PersonalServiceError> {
    if request.gacha_id != GACHA_ID {
        return Ok(Err("invalid_crazy_gacha_request"));
    }
    let character_ids = match state::selection(root, request.index)? {
        Ok(Some(character_ids)) => character_ids,
        Ok(None) => {
            return Ok(Ok(Mutation {
                response: state::response(root)?,
                history: Vec::new(),
            }));
        }
        Err(code) => return Ok(Err(code)),
    };
    let mut mutation = reward::grant(root, request.viewer_id, &character_ids, response_time)?;
    state::finish(root, request.index)?;
    mutation
        .response
        .as_object_mut()
        .unwrap()
        .extend(state::response(root)?.as_object().unwrap().clone());
    reward::record_missions(
        root,
        request,
        database,
        response_time,
        &mut mutation.response,
    )?;
    Ok(Ok(mutation))
}
// //// /发放选定组并完成当前乐享扭蛋 ////

fn error_response(code: &str) -> HttpResponse {
    HttpResponse::json("400 Bad Request", json!({"error": code}).to_string())
}
