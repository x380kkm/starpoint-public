// audience: internal
// # personal-service-cn-reference-read
//
// 该模块实现 CN 查询接口, 并持久化百科已读状态和物品使用结果.

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, format_client_time, player_snapshot,
};
use crate::database::{ServiceDatabase, StoredReceiveHistory, ViewerSessionPlayer};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

const ENCYCLOPEDIA_DATA: &str = include_str!("../assets/cn-encyclopedia.json");
const ITEM_EFFECT_DATA: &str = include_str!("../assets/cn-item-effects.json");
const STAMINA_RECOVERY_SECONDS: i64 = 300;
const MAX_STAMINA_OVERFLOW: i64 = 999;

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
    ("/api/index.php/comic/get_list", comic_get_list),
    ("/api/index.php/history/receive", history_receive),
    ("/api/index.php/history/practice_battle", history_battle),
    (
        "/api/index.php/history/score_attack_event_battle",
        history_battle,
    ),
    ("/api/index.php/contents_guide/start", empty_object),
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
    ("/api/index.php/item/use_item", item_use),
    ("/api/index.php/bonus/shown", supplemental_content_query),
    (
        "/api/index.php/assistant/get_assistant_list",
        supplemental_content_query,
    ),
    (
        "/api/index.php/character_election/get_vote_status",
        supplemental_content_query,
    ),
    (
        "/api/index.php/character_election/vote",
        supplemental_content_query,
    ),
    ("/api/index.php/follow/lists", supplemental_content_query),
    (
        "/api/index.php/gacha/crazy_gacha_save",
        supplemental_content_query,
    ),
    (
        "/api/index.php/gacha/crazy_gacha_select",
        supplemental_content_query,
    ),
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
struct ComicListRequest {
    viewer_id: i64,
    page_index: Option<i64>,
}

impl HasViewerId for ComicListRequest {
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

#[derive(Deserialize)]
struct ItemUseRequest {
    viewer_id: i64,
    items: Vec<ItemUseEntry>,
}

impl HasViewerId for ItemUseRequest {
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

#[derive(Deserialize)]
struct ItemUseEntry {
    id: i64,
    number: i64,
}

#[derive(Deserialize)]
struct ItemEffect {
    #[serde(rename = "effectKind")]
    effect_kind: i64,
    #[serde(rename = "effectValue")]
    effect_value: i64,
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
    let (body, _) = authenticated!(request, database, ViewerRequest);
    respond(
        database,
        body.viewer_id,
        json!({
            "config": {
                "attention_recruitment_interval_seconds": 15,
                "attention_recruitment_redeliver_limit": 20,
                "attention_polling_interval_seconds_normal": 10,
                "attention_polling_interval_seconds_battle": 15,
                "multi_attention_lifetime_seconds": 30,
                "contribution_score_rate_to_parasite": 0.25,
                "attention_log_interval_seconds": 600,
                "disable_finish_duration_seconds": 5,
                "disable_decline_count_seconds": 60,
                "disable_decline_count_limit": 14,
                "disable_decline_duration_seconds": 30,
                "disable_intent_disconnect_duration_seconds": 300,
                "disable_unintent_disconnect_duration_seconds": 5,
                "disable_remote_error_duration_seconds": 300,
                "attention_animation_time_seconds": 6,
                "disable_expire_count_limit": 4,
                "disable_expire_duration_seconds": 180,
                "polling_delay_normal_seconds_range_min": 1,
                "polling_delay_normal_seconds_range_max": 10,
                "polling_delay_battle_seconds_range_min": 1,
                "polling_delay_battle_seconds_range_max": 15,
                "return_attention_max_num": 3
            }
        }),
    )
}
// //// /返回多人邀请配置 ////

// //// 返回多人邀请优先分数 [@x380kkm 2026-08-22] ////
fn attention_action(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let viewer_id = match viewer_id_only(request) {
        Ok(viewer_id) => viewer_id,
        Err(response) => return Ok(response),
    };
    respond(
        database,
        viewer_id,
        json!({"priority_action_score": 0, "priority_playing_score": 0}),
    )
}
// //// /返回多人邀请优先分数 ////

fn attention_logger(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let viewer_id = match viewer_id_only(request) {
        Ok(viewer_id) => viewer_id,
        Err(response) => return Ok(response),
    };
    respond(database, viewer_id, json!({}))
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

// //// 返回空漫画分页 [@x380kkm 2026-08-22] ////
fn comic_get_list(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let (body, _) = authenticated!(request, database, ComicListRequest);
    let page_index = body.page_index.unwrap_or_default().max(0);
    respond(
        database,
        body.viewer_id,
        json!({"comic_list": [], "current_page_index": page_index, "total_count": 0}),
    )
}
// //// /返回空漫画分页 ////

// //// 返回已领取邮件生成的领取记录 [@x380kkm 2026-08-22] ////
fn history_receive(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ViewerRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
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
    let history = database
        .receive_history(account_id, 500)?
        .iter()
        .map(receive_history_entry)
        .collect::<Vec<_>>();
    let total_count = history.len();
    respond(
        database,
        body.viewer_id,
        json!({"history": history, "total_count": total_count}),
    )
}
// //// /返回已领取邮件生成的领取记录 ////

fn receive_history_entry(entry: &StoredReceiveHistory) -> Value {
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

// //// 返回战斗记录数组 [@x380kkm 2026-08-22] ////
fn history_battle(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let viewer_id = match viewer_id_only(request) {
        Ok(viewer_id) => viewer_id,
        Err(response) => return Ok(response),
    };
    respond(database, viewer_id, json!({"history": []}))
}
// //// /返回战斗记录数组 ////

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
    let degree_id = match database.lookup_viewer_session_player(body.viewer_id)? {
        ViewerSessionPlayer::InvalidSession => {
            return Ok(error_response("400 Bad Request", "invalid_viewer_session"));
        }
        ViewerSessionPlayer::Present(snapshot) => decode_player_data(&snapshot.data)?
            .get("user_info")
            .and_then(Value::as_object)
            .and_then(|user_info| user_info.get("degree_id"))
            .and_then(Value::as_i64)
            .unwrap_or(1),
        ViewerSessionPlayer::MissingPlayer | ViewerSessionPlayer::MissingPlayerData(_) => 1,
    };
    respond(
        database,
        body.viewer_id,
        json!({"degree_ids": [1, degree_id]}),
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

// //// 使用库存中的体力物品 [@x380kkm 2026-08-22] ////
fn item_use(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let (body, snapshot) = authenticated!(request, database, ItemUseRequest);
    if body.items.is_empty() {
        return Ok(error_response("400 Bad Request", "invalid_request_body"));
    }
    let effects: BTreeMap<String, ItemEffect> =
        serde_json::from_str(ITEM_EFFECT_DATA).map_err(|error| {
            PersonalServiceError::new(format!("failed to decode CN item effects: {error}"))
        })?;
    let mut requested = BTreeMap::<i64, i64>::new();
    for item in body.items {
        if item.id <= 0 || item.number <= 0 {
            continue;
        }
        if !effects
            .get(&item.id.to_string())
            .is_some_and(|effect| matches!(effect.effect_kind, 2 | 3))
        {
            continue;
        }
        let total = requested.entry(item.id).or_default();
        *total = total.checked_add(item.number).ok_or_else(|| {
            PersonalServiceError::new("CN item count exceeds the supported range")
        })?;
    }
    if requested.is_empty() {
        return Ok(error_response(
            "400 Bad Request",
            "invalid_stamina_recovery",
        ));
    }

    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = player_data
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN player data is not an object"))?;
    let user_info = root
        .get("user_info")
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("stored CN user_info data is missing"))?;
    let stamina = required_i64(user_info, "stamina")?;
    let heal_time = required_i64(user_info, "stamina_heal_time")?;
    let now = server_time(database)?;
    let recovered = now.saturating_sub(heal_time) / STAMINA_RECOVERY_SECONDS;
    let current_stamina = stamina.saturating_add(recovered).min(MAX_STAMINA_OVERFLOW);
    if current_stamina >= MAX_STAMINA_OVERFLOW {
        return Ok(error_response("400 Bad Request", "stamina_already_full"));
    }

    let item_list = root
        .get_mut("item_list")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN item_list data is missing"))?;
    let mut recovery = 0_i64;
    let mut updates = Map::new();
    for (item_id, count) in requested {
        let effect = effects
            .get(&item_id.to_string())
            .ok_or_else(|| PersonalServiceError::new("CN item effect disappeared"))?;
        let owned = item_list
            .get(&item_id.to_string())
            .and_then(Value::as_i64)
            .unwrap_or_default();
        if owned < count {
            return Ok(error_response("400 Bad Request", "insufficient_items"));
        }
        let per_item = if effect.effect_kind == 2 {
            effect.effect_value
        } else {
            MAX_STAMINA_OVERFLOW.saturating_mul(effect.effect_value.max(0)) / 100
        };
        recovery = recovery
            .checked_add(per_item.saturating_mul(count))
            .ok_or_else(|| {
                PersonalServiceError::new("CN stamina recovery exceeds the supported range")
            })?;
        updates.insert(item_id.to_string(), Value::from(owned - count));
    }
    if recovery <= 0 {
        return Ok(error_response(
            "400 Bad Request",
            "invalid_stamina_recovery",
        ));
    }
    for (item_id, count) in &updates {
        item_list.insert(item_id.clone(), count.clone());
    }
    let next_stamina = current_stamina
        .saturating_add(recovery)
        .min(MAX_STAMINA_OVERFLOW);
    let user_info = root
        .get_mut("user_info")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN user_info data is missing"))?;
    user_info.insert("stamina".to_owned(), Value::from(next_stamina));
    user_info.insert("stamina_heal_time".to_owned(), Value::from(now));
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        now,
        json!({
            "user_info": {"stamina": next_stamina, "stamina_heal_time": now},
            "item_list": updates
        }),
    )
}
// //// /使用库存中的体力物品 ////

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
        "/api/index.php/bonus/shown" | "/api/index.php/sns/get" => json!([]),
        "/api/index.php/assistant/get_assistant_list" => json!({
            "all_box_info": [],
            "mission_progress_list": [],
            "records": [],
            "sales_list": [],
            "user_notice_list": [],
        }),
        "/api/index.php/character_election/get_vote_status" => json!({"is_voted": false}),
        "/api/index.php/character_election/vote" | "/api/index.php/gacha/crazy_gacha_select" => {
            json!({})
        }
        "/api/index.php/gacha/crazy_gacha_save" => {
            json!({"crazy_gacha_result_list": []})
        }
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
            "unselected_lineup_shop_sales_list": [],
            "shop_sales_list": shop_sales_list,
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

fn required_i64(object: &Map<String, Value>, key: &str) -> Result<i64, PersonalServiceError> {
    object
        .get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new(format!("stored CN {key} data is invalid")))
}

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}
