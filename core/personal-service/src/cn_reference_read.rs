// audience: internal
// # personal-service-cn-reference-read
//
// 该模块实现 CN 查询接口, 并持久化百科已读状态和物品使用结果.

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, format_client_time, player_snapshot,
};
use crate::database::{ServiceDatabase, ViewerSessionPlayer};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::path::Path;

mod item_use;
mod receive_history;

const ENCYCLOPEDIA_DATA: &str = include_str!("../assets/cn-encyclopedia.json");

type Handler = fn(&HttpRequest, &mut ServiceDatabase) -> Result<HttpResponse, PersonalServiceError>;

const ROUTES: &[(&str, Handler)] = &[
    ("/api/index.php/attention/check", attention_check),
    ("/api/index.php/attention/action", attention_action),
    ("/api/index.php/attention/logger", attention_logger),
    ("/api/index.php/encyclopedia/index", encyclopedia_index),
    (
        "/api/index.php/encyclopedia/read_keyword",
        encyclopedia_read_keyword,
    ),
    (
        "/api/index.php/encyclopedia/unlock_keyword",
        encyclopedia_read_keyword,
    ),
    ("/api/index.php/history/receive", receive_history::get),
    ("/api/index.php/player_history/index", crate::cn_player_history::index),
    ("/api/index.php/player_history/edit", crate::cn_player_history::edit),
    ("/api/index.php/history/practice_battle", crate::cn_battle_history::history),
    (
        "/api/index.php/history/score_attack_event_battle",
        crate::cn_battle_history::history,
    ),
    ("/api/index.php/contents_guide/start", start_contents_guide),
    (
        "/api/index.php/profile/get_last_login_region",
        profile_last_login_region,
    ),
    (
        "/api/index.php/profile/get_degree_list",
        profile_degree_list,
    ),
    ("/api/index.php/news/index", news_index),
    ("/api/index.php/news/system_index", news_system_index),
    ("/api/index.php/news/get_system_info", empty_object),
    ("/api/index.php/news/latest_forced", empty_object),
    ("/api/index.php/news/latest_forced_system", empty_object),
    ("/api/index.php/reproduce/post", reproduce_post),
    ("/api/index.php/item/use_item", item_use::use_items),
    ("/api/index.php/bonus/shown", crate::cn_login_bonus::shown),
    (
        "/api/index.php/character_election/get_vote_status",
        crate::cn_character_election::get_vote_status,
    ),
    (
        "/api/index.php/character_election/vote",
        crate::cn_character_election::vote,
    ),
    ("/api/index.php/follow/lists", supplemental_content_query),
    ("/api/index.php/gift/receive", supplemental_content_query),
    ("/api/index.php/lounge/get_list", supplemental_content_query),
    ("/api/index.php/sns/get", supplemental_content_query),
];

#[derive(Deserialize)]
struct ViewerRequest {
    viewer_id: i64,
}

#[derive(Deserialize)]
struct ReproduceRequest {
    #[serde(default)]
    viewer_id: Option<i64>,
}

#[derive(Deserialize)]
struct HowToGetRequest {
    viewer_id: i64,
    item_id: Option<i64>,
    equipment_id: Option<i64>,
}

impl HasViewerId for HowToGetRequest {
    fn viewer_id(&self) -> i64 {
        self.viewer_id
    }
}

trait HasViewerId {
    fn viewer_id(&self) -> i64;
}

impl HasViewerId for ViewerRequest {
    fn viewer_id(&self) -> i64 {
        self.viewer_id
    }
}

#[derive(Deserialize)]
struct EncyclopediaReadRequest {
    viewer_id: i64,
    encyclopedia_ids: Vec<i64>,
}

impl HasViewerId for EncyclopediaReadRequest {
    fn viewer_id(&self) -> i64 {
        self.viewer_id
    }
}

#[derive(Deserialize)]
struct NewsIndexRequest {
    viewer_id: i64,
    page_index: Option<i64>,
    current_page: Option<i64>,
}

impl HasViewerId for NewsIndexRequest {
    fn viewer_id(&self) -> i64 {
        self.viewer_id
    }
}

macro_rules! authenticated {
    ($request:expr, $database:expr, $request_type:ty) => {{
        let body = match decode_request::<$request_type>($request) {
            Ok(body) if body.viewer_id() > 0 => body,
            Ok(_) | Err(_) => {
                return Ok(error_response("400 Bad Request", "invalid_request_body"));
            }
        };
        let snapshot = match player_snapshot($database, body.viewer_id())? {
            Ok(snapshot) => snapshot,
            Err(response) => return Ok(response),
        };
        (body, snapshot)
    }};
}

// //// 分派 CN 查询请求 [@x380kkm 2026-08-22] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    if request.path() == "/api/index.php/how_to_get/get_list" {
        return Some(how_to_get(request, database, asset_root));
    }
    ROUTES
        .iter()
        .find(|(path, _)| *path == request.path())
        .map(|(_, handler)| handler(request, database))
}
// //// /分派 CN 查询请求 ////

// //// 返回多人邀请配置 [@x380kkm 2026-08-22] ////
fn attention_check(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    crate::cn_multi::attention_check(request, database)
}
// //// /返回多人邀请配置 ////

// //// 返回多人邀请优先分数 [@x380kkm 2026-08-22] ////
fn attention_action(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let (body, _) = authenticated!(request, database, ViewerRequest);
    respond(
        database,
        body.viewer_id,
        json!({"priority_action_score": 0, "priority_playing_score": 0}),
    )
}
// //// /返回多人邀请优先分数 ////

fn attention_logger(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let (body, _) = authenticated!(request, database, ViewerRequest);
    respond(database, body.viewer_id, json!({}))
}

// //// 返回百科目录并合并玩家已读状态 [@x380kkm 2026-08-22] ////
fn encyclopedia_index(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ViewerRequest>(request) {
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
    let mut encyclopedia = decode_object(ENCYCLOPEDIA_DATA, "CN encyclopedia")?;
    if let Some(snapshot) = snapshot {
        let player_data = decode_player_data(&snapshot.data)?;
        if let Some(stored) = player_data
            .get("encyclopedia_list")
            .and_then(Value::as_object)
        {
            encyclopedia.extend(stored.clone());
        }
    }
    respond(
        database,
        body.viewer_id,
        json!({"encyclopedia_list": encyclopedia, "mail_arrived": false}),
    )
}
// //// /返回百科目录并合并玩家已读状态 ////

// //// 保存百科已读状态 [@x380kkm 2026-08-22] ////
fn encyclopedia_read_keyword(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<EncyclopediaReadRequest>(request) {
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
    let mut response = Map::new();
    for id in &body.encyclopedia_ids {
        let value = json!({"read": true});
        response.insert(id.to_string(), value);
    }
    if let Some(snapshot) = snapshot {
        let mut player_data = decode_player_data(&snapshot.data)?;
        let root = player_data
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN player data is not an object"))?;
        let stored = root
            .entry("encyclopedia_list".to_owned())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN encyclopedia data is invalid"))?;
        stored.extend(response.clone());
        database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    }
    respond(
        database,
        body.viewer_id,
        json!({"encyclopedia_list": response}),
    )
}
// //// /保存百科已读状态 ////

// //// 返回玩家最后登录区域 [@x380kkm 2026-08-22] ////
fn profile_last_login_region(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let viewer_id = match session_viewer_id(request, database)? {
        Ok(viewer_id) => viewer_id,
        Err(response) => return Ok(response),
    };
    respond(database, viewer_id, json!({"region": "CN"}))
}
// //// /返回玩家最后登录区域 ////

// //// 返回玩家持有的称号编号 [@x380kkm 2026-08-22] ////
fn profile_degree_list(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ViewerRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let mut degree_ids = std::collections::BTreeSet::from([1_i64]);
    match database.lookup_viewer_session_player(body.viewer_id)? {
        ViewerSessionPlayer::InvalidSession => {
            return Ok(error_response("400 Bad Request", "invalid_viewer_session"));
        }
        ViewerSessionPlayer::Present(snapshot) => {
            let player = decode_player_data(&snapshot.data)?;
            degree_ids = crate::cn_profile::owned_degree_ids(&player);
        }
        ViewerSessionPlayer::MissingPlayer | ViewerSessionPlayer::MissingPlayerData(_) => {}
    }
    respond(
        database,
        body.viewer_id,
        json!({"degree_ids": degree_ids}),
    )
}
// //// /返回玩家持有的称号编号 ////

// //// 返回包内普通公告分页 [@x380kkm 2026-08-22] ////
fn news_index(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<NewsIndexRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    match session_viewer_id_value(body.viewer_id, database)? {
        Ok(_) => {}
        Err(response) => return Ok(response),
    }
    let current_page = body
        .page_index
        .filter(|page| *page > 0)
        .or_else(|| body.current_page.filter(|page| *page > 0))
        .unwrap_or(1);
    let response_time = server_time(database)?;
    let packaged_news = crate::cn_news::packaged_news_entries()?;
    let news_count = packaged_news.len();
    let news = if current_page == 1 {
        packaged_news
    } else {
        Vec::new()
    };
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        json!({"current_page": current_page, "news": news, "news_count": news_count}),
    )
}
// //// /返回包内普通公告分页 ////

// //// 返回空系统公告分页 [@x380kkm 2026-08-22] ////
fn news_system_index(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let viewer_id = match session_viewer_id(request, database)? {
        Ok(viewer_id) => viewer_id,
        Err(response) => return Ok(response),
    };
    respond(
        database,
        viewer_id,
        json!({"current_page": 1, "news": [], "news_count": 0}),
    )
}
// //// /返回空系统公告分页 ////

// //// 保存内容指南的开启进度并返回任务变化 [@x380kkm 2026-09-21] ////
fn start_contents_guide(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let (body, snapshot) = authenticated!(request, database, ViewerRequest);
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = crate::cn_tutorial::require_root(&mut player_data)?;
    let active_mission_list = crate::cn_mission::record_active_counters(
        root, database, &[("contents_guide_start", 1)],
    )?;
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    respond(database, body.viewer_id, json!({"active_mission_list": active_mission_list}))
}
// //// /保存内容指南的开启进度并返回任务变化 ////

fn empty_object(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let viewer_id = match session_viewer_id(request, database)? {
        Ok(viewer_id) => viewer_id,
        Err(response) => return Ok(response),
    };
    respond(database, viewer_id, json!({}))
}

// //// 返回内容发现和社交查询的固定结构 [@x380kkm 2026-08-23] ////
fn supplemental_content_query(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let viewer_id = match session_viewer_id(request, database)? {
        Ok(viewer_id) => viewer_id,
        Err(response) => return Ok(response),
    };
    let data = match request.path() {
        "/api/index.php/sns/get" => json!([]),
        "/api/index.php/gift/receive" => json!({"all_gift_info": [], "result_code": 1}),
        "/api/index.php/follow/lists" => {
            json!({ "follow_info": [], "followed_count": 0 })
        }
        "/api/index.php/lounge/get_list" => json!({ "lounge_list": [] }),
        _ => return Err(PersonalServiceError::new("unsupported CN content query")),
    };
    respond(database, viewer_id, data)
}
// //// /返回内容发现和社交查询的固定结构 ////

// //// 返回物品和装备的箱池及商店来源 [@x380kkm 2026-08-24] ////
fn how_to_get(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let (body, snapshot) = authenticated!(request, database, HowToGetRequest);
    let has_item = body.item_id.is_some_and(|item_id| item_id > 0) && body.equipment_id.is_none();
    let has_equipment = body
        .equipment_id
        .is_some_and(|equipment_id| equipment_id > 0)
        && body.item_id.is_none();
    if !has_item && !has_equipment {
        return Ok(error_response("400 Bad Request", "invalid_request_body"));
    }
    let response_time = server_time(database)?;
    let player_data = decode_player_data(&snapshot.data)?;
    let root = player_data
        .as_object()
        .ok_or_else(|| PersonalServiceError::new("stored CN player data is not an object"))?;
    let box_gacha_id_list = crate::cn_box_gacha::how_to_get_box_gacha_ids(
        body.item_id,
        body.equipment_id,
        database,
        asset_root,
    )?;
    let shop_sales_list = crate::cn_shop::how_to_get_sales(
        root,
        body.item_id,
        body.equipment_id,
        &format_client_time(response_time),
        database,
        asset_root,
    )?;
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        json!({
            "box_gacha_id_list": box_gacha_id_list,
            "unselected_lineup_shop_sales_list": shop_sales_list.unselected,
            "shop_sales_list": shop_sales_list.available,
        }),
    )
}
// //// /返回物品和装备的箱池及商店来源 ////

fn reproduce_post(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ReproduceRequest>(request) {
        Ok(body) => body,
        Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    respond(
        database,
        body.viewer_id.unwrap_or_default().max(0),
        json!([]),
    )
}

fn viewer_id_only(request: &HttpRequest) -> Result<i64, HttpResponse> {
    match decode_request::<ViewerRequest>(request) {
        Ok(body) if body.viewer_id > 0 => Ok(body.viewer_id),
        Ok(_) | Err(_) => Err(error_response("400 Bad Request", "invalid_request_body")),
    }
}

fn session_viewer_id(
    request: &HttpRequest,
    database: &ServiceDatabase,
) -> Result<Result<i64, HttpResponse>, PersonalServiceError> {
    let viewer_id = match viewer_id_only(request) {
        Ok(viewer_id) => viewer_id,
        Err(response) => return Ok(Err(response)),
    };
    session_viewer_id_value(viewer_id, database)
}

fn session_viewer_id_value(
    viewer_id: i64,
    database: &ServiceDatabase,
) -> Result<Result<i64, HttpResponse>, PersonalServiceError> {
    match database.lookup_viewer_session_player(viewer_id)? {
        ViewerSessionPlayer::InvalidSession => Ok(Err(error_response(
            "400 Bad Request",
            "invalid_viewer_session",
        ))),
        _ => Ok(Ok(viewer_id)),
    }
}

fn respond(
    database: &ServiceDatabase,
    viewer_id: i64,
    data: Value,
) -> Result<HttpResponse, PersonalServiceError> {
    msgpack_response_at(viewer_id, false, server_time(database)?, data)
}

fn decode_object(source: &str, label: &str) -> Result<Map<String, Value>, PersonalServiceError> {
    serde_json::from_str::<Value>(source)
        .map_err(|error| PersonalServiceError::new(format!("failed to decode {label}: {error}")))?
        .as_object()
        .cloned()
        .ok_or_else(|| PersonalServiceError::new(format!("{label} is not an object")))
}

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}
