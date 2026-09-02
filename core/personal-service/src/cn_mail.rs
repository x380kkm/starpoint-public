// audience: internal
// # personal-service-cn-mail
//
// 该模块实现 CN 邮件索引, 单封领取, 批量领取和受管理的本地发放.

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_character_reward::grant_mailed_character;
use crate::cn_tutorial::format_client_time;
use crate::database::{
    CreateMailInput, MailClaimReward, MailReward, PlayerSnapshot, ServiceDatabase, StoredMail,
    ViewerSessionPlayer, MAX_MAIL_PAGE, MAX_MAIL_PAGE_SIZE, MAX_MAIL_REWARD_ENTRIES,
};
use crate::http::{HttpRequest, HttpResponse};
use crate::management;
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

// //// 定义本地邮件奖励目录 [@x380kkm 2026-08-20] ////
const CN_GACHA_ASSET: &str = include_str!("../../../assets/cn_gacha.json");
const CN_ITEM_CATALOG_ASSET: &str = include_str!("../../../assets/cn_item_catalog.json");
const CN_GACHA_ID: &str = "1";
const PULLS_PER_MULTI_DRAW: i64 = 10;
const QUICK_PULL_COUNT: i64 = 200;
const LARGE_STAMINA_RECOVERY_ITEM_ID: &str = "106";
const QUICK_STAMINA_ITEM_COUNT: i64 = 10;
const STATIC_ITEM_IDS: &[&str] = &[
    "100", "101", "102", "106", "999001", "999003", "999004", "999005",
];
const REWARD_KINDS: &[(&str, &str)] = &[
    ("currency", "货币"),
    ("ticket", "抽卡券"),
    ("growth", "通用养成"),
    ("stamina", "体力"),
    ("event", "活动素材"),
    ("ability-soul", "能力之魂"),
    ("character-growth", "角色养成"),
    ("equipment-growth", "装备养成"),
    ("exchange", "兑换素材"),
    ("craft", "锻造"),
    ("quest", "任务素材"),
    ("star-grain", "星之粒"),
    ("other", "其他"),
];

#[derive(Clone, Copy)]
enum CatalogReward {
    FreeVmoney,
    FreeMana,
    ExpPool,
    StarCrumb,
    BondToken,
    BossBoostPoint,
    BoostPoint,
    RankPoint,
    Item(&'static str),
}

struct RewardCatalogEntry {
    key: &'static str,
    name: &'static str,
    kind: &'static str,
    default_amount: i64,
    reward: CatalogReward,
}

const REWARD_CATALOG: &[RewardCatalogEntry] = &[
    RewardCatalogEntry {
        key: "currency.free-vmoney",
        name: "免费星导石",
        kind: "currency",
        default_amount: 1_500,
        reward: CatalogReward::FreeVmoney,
    },
    RewardCatalogEntry {
        key: "currency.free-mana",
        name: "玛纳",
        kind: "currency",
        default_amount: 100_000,
        reward: CatalogReward::FreeMana,
    },
    RewardCatalogEntry {
        key: "currency.star-crumb",
        name: "星之碎片",
        kind: "currency",
        default_amount: 100,
        reward: CatalogReward::StarCrumb,
    },
    RewardCatalogEntry {
        key: "growth.exp-pool",
        name: "经验池",
        kind: "growth",
        default_amount: 100_000,
        reward: CatalogReward::ExpPool,
    },
    RewardCatalogEntry {
        key: "growth.bond-token",
        name: "羁绊之证 (信赖之证)",
        kind: "character-growth",
        default_amount: 1,
        reward: CatalogReward::BondToken,
    },
    RewardCatalogEntry {
        key: "growth.boss-boost-point",
        name: "Boss Boost 点数",
        kind: "growth",
        default_amount: 1,
        reward: CatalogReward::BossBoostPoint,
    },
    RewardCatalogEntry {
        key: "growth.boost-point",
        name: "Boost 点数",
        kind: "growth",
        default_amount: 1,
        reward: CatalogReward::BoostPoint,
    },
    RewardCatalogEntry {
        key: "growth.rank-point",
        name: "段位点数",
        kind: "growth",
        default_amount: 100,
        reward: CatalogReward::RankPoint,
    },
    RewardCatalogEntry {
        key: "stamina.recovery.tiny",
        name: "体力回复药 (微小)",
        kind: "stamina",
        default_amount: 10,
        reward: CatalogReward::Item("100"),
    },
    RewardCatalogEntry {
        key: "stamina.recovery.small",
        name: "体力回复药 (小)",
        kind: "stamina",
        default_amount: 10,
        reward: CatalogReward::Item("101"),
    },
    RewardCatalogEntry {
        key: "stamina.recovery.medium",
        name: "体力回复药 (中)",
        kind: "stamina",
        default_amount: 10,
        reward: CatalogReward::Item("102"),
    },
    RewardCatalogEntry {
        key: "stamina.recovery.large",
        name: "体力回复药 (大)",
        kind: "stamina",
        default_amount: QUICK_STAMINA_ITEM_COUNT,
        reward: CatalogReward::Item(LARGE_STAMINA_RECOVERY_ITEM_ID),
    },
    RewardCatalogEntry {
        key: "ticket.character.single",
        name: "角色单抽券",
        kind: "ticket",
        default_amount: 1,
        reward: CatalogReward::Item("999003"),
    },
    RewardCatalogEntry {
        key: "ticket.character.multi",
        name: "角色十连券",
        kind: "ticket",
        default_amount: 1,
        reward: CatalogReward::Item("999001"),
    },
    RewardCatalogEntry {
        key: "ticket.weapon.single",
        name: "武器单抽券",
        kind: "ticket",
        default_amount: 1,
        reward: CatalogReward::Item("999005"),
    },
    RewardCatalogEntry {
        key: "ticket.weapon.multi",
        name: "武器十连券",
        kind: "ticket",
        default_amount: 1,
        reward: CatalogReward::Item("999004"),
    },
];

#[derive(Deserialize)]
struct CnItemCatalogAsset {
    items: Vec<CnItemCatalogEntry>,
}

#[derive(Deserialize)]
struct CnItemCatalogEntry {
    id: String,
    string_id: Option<String>,
    name: String,
    thumbnail_id: Option<String>,
    description: Option<String>,
    effect_kind: i64,
    category: i64,
    group: Option<i64>,
    kind: String,
}

#[derive(Clone, Copy)]
struct GachaCost {
    multi_draw: i64,
}
// //// /定义本地邮件奖励目录 ////

#[derive(Deserialize)]
struct IndexRequest {
    viewer_id: Value,
    current_page: Option<Value>,
}

#[derive(Deserialize)]
struct ReceiveRequest {
    viewer_id: Value,
    mail_id: Value,
}

#[derive(Deserialize)]
struct ReceiveAllRequest {
    viewer_id: Value,
    mail_ids: Option<Vec<Value>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManagementCreateMailRequest {
    viewer_id: i64,
    title: String,
    body: String,
    sender: String,
    rewards: Value,
    expires_at: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalSaveCreateMailRequest {
    title: String,
    body: String,
    sender: String,
    rewards: Value,
    expires_at: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FavoriteRewardRequest {
    favorite: bool,
}

struct ManagedMailInput {
    title: String,
    body: String,
    sender: String,
    rewards: Value,
    expires_at: Option<i64>,
}

// //// 分派 CN 邮件客户端请求 [@x380kkm 2026-07-24] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    let response = match request.path() {
        "/api/index.php/mail/index" => index(request, database),
        "/api/index.php/mail/receive" => receive(request, database),
        "/api/index.php/mail/receive_all" => receive_all(request, database),
        _ => return None,
    };
    Some(response)
}
// //// /分派 CN 邮件客户端请求 ////

// //// 分派受保护的本地邮件管理请求 [@x380kkm 2026-07-24] ////
pub(crate) fn management_route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let local_save_slot_id = local_save_mail_slot_id(request.path());
    if local_save_slot_id.is_none()
        && !request.path().starts_with("/v1/mails")
        && !request.path().starts_with("/v1/mail-rewards/")
    {
        return None;
    }
    if !management::is_authorized(request, database) {
        return Some(Ok(management::unauthorized_response()));
    }
    let response = if request.path() == "/v1/mail-rewards/catalog" {
        manage_reward_catalog(request, database)
    } else if let Some(reward_key) = reward_favorite_key(request.path()) {
        manage_reward_favorite(request, database, reward_key)
    } else if let Some(slot_id) = local_save_slot_id {
        manage_local_save_mails(request, database, slot_id)
    } else if request.path() == "/v1/mails" && request.method() == "POST" {
        create_managed_mail(request, database)
    } else if let Some(viewer_id) = request.path().strip_prefix("/v1/mails/") {
        if request.method() != "GET" {
            Ok(error_response(
                "405 Method Not Allowed",
                "method_not_allowed",
            ))
        } else {
            list_managed_mails(database, viewer_id)
        }
    } else if request.path().starts_with("/v1/mail-rewards/") {
        Ok(error_response("404 Not Found", "mail_reward_not_found"))
    } else {
        Ok(error_response(
            "405 Method Not Allowed",
            "method_not_allowed",
        ))
    };
    Some(response)
}
// //// /分派受保护的本地邮件管理请求 ////

// //// 返回邮件奖励目录并保存收藏 [@x380kkm 2026-08-20] ////
fn manage_reward_catalog(
    request: &HttpRequest,
    database: &ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    if request.method() != "GET" {
        return Ok(error_response(
            "405 Method Not Allowed",
            "method_not_allowed",
        ));
    }
    let favorites = database.mail_reward_favorite_keys()?;
    let item_catalog = cn_item_catalog()?;
    let mut items = Vec::with_capacity(REWARD_CATALOG.len() + item_catalog.len());
    items.extend(
        REWARD_CATALOG
            .iter()
            .map(|entry| serialize_reward_catalog_entry(entry, &favorites)),
    );
    items.extend(
        item_catalog
            .iter()
            .filter(|entry| !STATIC_ITEM_IDS.contains(&entry.id.as_str()))
            .map(|entry| serialize_cn_item_catalog_entry(entry, &favorites)),
    );
    let gacha_cost = cn_gacha_cost()?;
    let multi_draw_count = QUICK_PULL_COUNT / PULLS_PER_MULTI_DRAW;
    let quick_pull_cost = gacha_cost
        .multi_draw
        .checked_mul(multi_draw_count)
        .ok_or_else(|| {
            PersonalServiceError::new("CN gacha preset cost exceeds the supported range")
        })?;
    let body = json!({
        "items": items,
        "kinds": REWARD_KINDS
            .iter()
            .map(|(key, name)| json!({"key": key, "name": name}))
            .collect::<Vec<_>>(),
        "presets": [
            {
                "key": "gacha.200-pulls",
                "name": "200 抽资源",
                "description": "按当前 CN 普通卡池十连价格发放 200 抽所需的有偿星导石.",
                "image_url": reward_catalog_image_url("currency.free-vmoney"),
                "rewards": {"vmoney": quick_pull_cost},
                "available": true,
                "source": {
                    "gacha_id": CN_GACHA_ID,
                    "multi_draw_cost": gacha_cost.multi_draw,
                    "pull_count": QUICK_PULL_COUNT,
                },
            },
            {
                "key": "stamina.large-potions",
                "name": "大体力回复药",
                "description": "发放 10 个每个恢复 100 体力的长期回复道具.",
                "image_url": reward_catalog_image_url("stamina.recovery.large"),
                "rewards": catalog_reward_value(
                    CatalogReward::Item(LARGE_STAMINA_RECOVERY_ITEM_ID),
                    QUICK_STAMINA_ITEM_COUNT,
                ),
                "available": true,
                "source": {
                    "item_id": LARGE_STAMINA_RECOVERY_ITEM_ID,
                    "stamina_recovery": 100,
                    "item_count": QUICK_STAMINA_ITEM_COUNT,
                },
            },
        ],
    });
    json_response("200 OK", body, "failed to encode mail reward catalog")
}

fn manage_reward_favorite(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    reward_key: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    if request.method() != "PUT" {
        return Ok(error_response(
            "405 Method Not Allowed",
            "method_not_allowed",
        ));
    }
    if !reward_catalog_contains(reward_key)? {
        return Ok(error_response("404 Not Found", "mail_reward_not_found"));
    }
    let body = match parse_json::<FavoriteRewardRequest>(request) {
        Ok(body) => body,
        Err(_) => return Ok(error_response("400 Bad Request", "invalid_favorite")),
    };
    database.set_mail_reward_favorite(reward_key, body.favorite)?;
    json_response(
        "200 OK",
        json!({"key": reward_key, "favorite": body.favorite}),
        "failed to encode mail reward favorite",
    )
}

fn reward_favorite_key(path: &str) -> Option<&str> {
    let suffix = path.strip_prefix("/v1/mail-rewards/catalog/")?;
    let reward_key = suffix.strip_suffix("/favorite")?;
    (!reward_key.is_empty() && !reward_key.contains('/')).then_some(reward_key)
}

fn serialize_reward_catalog_entry(
    entry: &RewardCatalogEntry,
    favorites: &BTreeSet<String>,
) -> Value {
    json!({
        "key": entry.key,
        "name": entry.name,
        "kind": entry.kind,
        "kind_name": reward_kind_name(entry.kind),
        "default_amount": entry.default_amount,
        "rewards": catalog_reward_value(entry.reward, 1),
        "resource_id": catalog_reward_resource_id(entry.reward),
        "image_url": reward_catalog_image_url(entry.key),
        "favorite": favorites.contains(entry.key),
    })
}

fn serialize_cn_item_catalog_entry(
    entry: &CnItemCatalogEntry,
    favorites: &BTreeSet<String>,
) -> Value {
    let key = format!("item.{}", entry.id);
    json!({
        "key": key,
        "name": entry.name,
        "kind": entry.kind,
        "kind_name": reward_kind_name(&entry.kind),
        "default_amount": 1,
        "rewards": catalog_item_reward_value(&entry.id, 1),
        "resource_id": entry.id,
        "string_id": entry.string_id,
        "thumbnail_id": entry.thumbnail_id,
        "description": entry.description,
        "effect_kind": entry.effect_kind,
        "category": entry.category,
        "group": entry.group,
        "favorite": favorites.contains(&key),
    })
}

fn reward_kind_name(kind: &str) -> &'static str {
    REWARD_KINDS
        .iter()
        .find_map(|(key, name)| (*key == kind).then_some(*name))
        .unwrap_or("其他")
}

fn reward_catalog_contains(key: &str) -> Result<bool, PersonalServiceError> {
    if REWARD_CATALOG.iter().any(|entry| entry.key == key) {
        return Ok(true);
    }
    let Some(item_id) = key.strip_prefix("item.") else {
        return Ok(false);
    };
    if STATIC_ITEM_IDS.contains(&item_id) {
        return Ok(false);
    }
    Ok(cn_item_catalog()?.iter().any(|entry| entry.id == item_id))
}

fn reward_catalog_image_url(key: &str) -> String {
    format!("/manage/assets/item-icons/{key}.png")
}

fn catalog_reward_resource_id(reward: CatalogReward) -> Option<&'static str> {
    match reward {
        CatalogReward::Item(item_id) => Some(item_id),
        CatalogReward::FreeVmoney
        | CatalogReward::FreeMana
        | CatalogReward::ExpPool
        | CatalogReward::StarCrumb
        | CatalogReward::BondToken
        | CatalogReward::BossBoostPoint
        | CatalogReward::BoostPoint
        | CatalogReward::RankPoint => None,
    }
}

fn catalog_reward_value(reward: CatalogReward, amount: i64) -> Value {
    match reward {
        CatalogReward::FreeVmoney => json!({"freeVmoney": amount}),
        CatalogReward::FreeMana => json!({"freeMana": amount}),
        CatalogReward::ExpPool => json!({"expPool": amount}),
        CatalogReward::StarCrumb => json!({"starCrumb": amount}),
        CatalogReward::BondToken => json!({"bondToken": amount}),
        CatalogReward::BossBoostPoint => json!({"bossBoostPoint": amount}),
        CatalogReward::BoostPoint => json!({"boostPoint": amount}),
        CatalogReward::RankPoint => json!({"rankPoint": amount}),
        CatalogReward::Item(item_id) => catalog_item_reward_value(item_id, amount),
    }
}

fn catalog_item_reward_value(item_id: &str, amount: i64) -> Value {
    let mut items = Map::new();
    items.insert(item_id.to_owned(), Value::from(amount));
    json!({"itemList": items})
}

fn cn_item_catalog() -> Result<&'static [CnItemCatalogEntry], PersonalServiceError> {
    static CATALOG: OnceLock<Result<CnItemCatalogAsset, String>> = OnceLock::new();
    match CATALOG.get_or_init(|| {
        serde_json::from_str(CN_ITEM_CATALOG_ASSET)
            .map_err(|error| format!("failed to parse CN item catalog: {error}"))
    }) {
        Ok(catalog) => Ok(&catalog.items),
        Err(error) => Err(PersonalServiceError::new(error.clone())),
    }
}

fn cn_gacha_cost() -> Result<GachaCost, PersonalServiceError> {
    static COST: OnceLock<Result<GachaCost, String>> = OnceLock::new();
    match COST.get_or_init(parse_cn_gacha_cost) {
        Ok(cost) => Ok(*cost),
        Err(error) => Err(PersonalServiceError::new(error.clone())),
    }
}

fn parse_cn_gacha_cost() -> Result<GachaCost, String> {
    let value = serde_json::from_str::<Value>(CN_GACHA_ASSET)
        .map_err(|error| format!("failed to parse CN gacha configuration: {error}"))?;
    let multi_draw = value
        .get(CN_GACHA_ID)
        .and_then(|gacha| gacha.get("multiCost"))
        .and_then(Value::as_i64)
        .filter(|cost| *cost > 0)
        .ok_or_else(|| "CN gacha multi-draw cost is missing".to_owned())?;
    Ok(GachaCost { multi_draw })
}

fn json_response(
    status: &'static str,
    value: Value,
    error_context: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    serde_json::to_string(&value)
        .map(|body| HttpResponse::json(status, body))
        .map_err(|error| PersonalServiceError::new(format!("{error_context}: {error}")))
}
// //// /返回邮件奖励目录并保存收藏 ////

// //// 映射本地存档槽邮件管理请求 [@x380kkm 2026-08-18] ////
fn local_save_mail_slot_id(path: &str) -> Option<&str> {
    let suffix = path.strip_prefix("/v1/local-saves/")?;
    let segments = suffix.split('/').collect::<Vec<_>>();
    match segments.as_slice() {
        [slot_id, "mails"] => Some(slot_id),
        _ => None,
    }
}

fn manage_local_save_mails(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(slot_id) = positive_integer(&Value::String(slot_id.to_owned())) else {
        return Ok(error_response("400 Bad Request", "invalid_local_save_id"));
    };
    let Some(context) = database.local_save_context(slot_id)? else {
        return Ok(error_response("404 Not Found", "local_save_not_found"));
    };
    match request.method() {
        "GET" => list_managed_mails_for_account(database, context.account_id),
        "POST" => create_local_save_mail(request, database, context.account_id),
        _ => Ok(error_response(
            "405 Method Not Allowed",
            "method_not_allowed",
        )),
    }
}
// //// /映射本地存档槽邮件管理请求 ////

// //// 返回 CN 邮件索引 [@x380kkm 2026-07-24] ////
fn index(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<IndexRequest>(request) {
        Ok(body) => body,
        Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let Some(viewer_id) = positive_integer(&body.viewer_id) else {
        return Ok(error_response("400 Bad Request", "invalid_viewer_id"));
    };
    let page_value = body.current_page.as_ref().and_then(positive_integer);
    let page = if body.current_page.is_some() && page_value.is_none() {
        return Ok(error_response("400 Bad Request", "invalid_current_page"));
    } else {
        page_value.unwrap_or(1).min(MAX_MAIL_PAGE)
    };
    let Some(snapshot) = player_snapshot(database, viewer_id)? else {
        return Ok(error_response("400 Bad Request", "invalid_viewer_session"));
    };
    let now = server_time(database)?;
    let page = database.list_mails(snapshot.account_id, page, MAX_MAIL_PAGE_SIZE, now)?;
    msgpack_response_at(
        viewer_id,
        false,
        server_time(database)?,
        json!({
            "mail": page.mails.into_iter().map(serialize_mail).collect::<Vec<_>>(),
            "total_count": page.total,
        }),
    )
}
// //// /返回 CN 邮件索引 ////

// //// 领取单封 CN 邮件 [@x380kkm 2026-07-24] ////
fn receive(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ReceiveRequest>(request) {
        Ok(body) => body,
        Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let Some(viewer_id) = positive_integer(&body.viewer_id) else {
        return Ok(error_response("400 Bad Request", "invalid_viewer_id"));
    };
    let Some(mail_id) = positive_integer(&body.mail_id) else {
        return Ok(error_response("400 Bad Request", "invalid_mail_id"));
    };
    claim_and_respond(database, viewer_id, Some(vec![mail_id]), false)
}
// //// /领取单封 CN 邮件 ////

// //// 批量领取 CN 邮件 [@x380kkm 2026-07-24] ////
fn receive_all(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ReceiveAllRequest>(request) {
        Ok(body) => body,
        Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let Some(viewer_id) = positive_integer(&body.viewer_id) else {
        return Ok(error_response("400 Bad Request", "invalid_viewer_id"));
    };
    let mail_ids = match body.mail_ids {
        None => None,
        Some(values) if values.len() <= MAX_MAIL_PAGE_SIZE as usize => {
            let mut ids = Vec::with_capacity(values.len());
            for value in values {
                let Some(mail_id) = positive_integer(&value) else {
                    return Ok(error_response("400 Bad Request", "invalid_mail_id"));
                };
                ids.push(mail_id);
            }
            Some(ids)
        }
        Some(_) => return Ok(error_response("400 Bad Request", "mail_ids_too_many")),
    };
    claim_and_respond(database, viewer_id, mail_ids, true)
}
// //// /批量领取 CN 邮件 ////

fn claim_and_respond(
    database: &mut ServiceDatabase,
    viewer_id: i64,
    mail_ids: Option<Vec<i64>>,
    receive_all: bool,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(snapshot) = player_snapshot(database, viewer_id)? else {
        return Ok(error_response("400 Bad Request", "invalid_viewer_session"));
    };
    let account_id = snapshot.account_id;
    let now = server_time(database)?;
    let requested_mail_count = mail_ids.as_ref().map(Vec::len);
    let initial_user_info = read_claim_user_info(database, viewer_id)?;
    let initial_encyclopedia_info = read_claim_encyclopedia_info(database, viewer_id)?;
    let result = database.claim_mails(
        account_id,
        mail_ids.as_deref(),
        now,
        |player_data, rewards| apply_rewards(player_data, rewards, viewer_id, now),
    )?;
    if !receive_all && result.mail_ids.is_empty() {
        return Ok(error_response("400 Bad Request", "mail_not_found"));
    }
    let user_info = changed_user_info(
        &initial_user_info,
        &read_claim_user_info(database, viewer_id)?,
    )?;
    let item_updates = read_claim_item_updates(database, viewer_id, &result.reward.item_list)?;
    let encyclopedia_info = changed_encyclopedia_info(
        &initial_encyclopedia_info,
        &read_claim_encyclopedia_info(database, viewer_id)?,
    );
    let mut data = if receive_all {
        let already_mail_count = requested_mail_count
            .map(|count| count.saturating_sub(result.mail_ids.len()))
            .unwrap_or_default();
        json!({
            "ex_boost_item_list": [],
            "mail_ids": result.mail_ids,
            "already_mail_count": already_mail_count,
            "auto_sale_expired_mail_count": 0,
            "deleted_mail_count": 0,
            "dispose_expired_mail_count": 0,
            "max_overed_mail_count": 0,
            "outdated_mail_count": 0,
            "total_count": result.total_count,
            "mail_arrived": result.remaining_count > 0,
        })
    } else {
        json!({
            "total_count": result.total_count,
            "dispose_expired_mail": false,
            "auto_sale_expired_mail": false,
            "mail_arrived": result.remaining_count > 0,
        })
    };
    let data_object = data
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("mail response data is not an object"))?;
    if !result.reward.character_list.is_empty() {
        data_object.insert(
            "character_list".to_owned(),
            Value::Array(result.reward.character_list),
        );
    }
    if !result.reward.equipment_list.is_empty() {
        data_object.insert(
            "equipment_list".to_owned(),
            Value::Array(result.reward.equipment_list),
        );
    }
    if !item_updates.is_empty() {
        data_object.insert("item_list".to_owned(), Value::Object(item_updates));
    }
    if user_info
        .as_object()
        .is_some_and(|values| !values.is_empty())
    {
        data_object.insert("user_info".to_owned(), user_info);
    }
    if !encyclopedia_info.is_empty() {
        data_object.insert(
            "encyclopedia_info".to_owned(),
            Value::Object(encyclopedia_info),
        );
    }
    msgpack_response_at(viewer_id, false, server_time(database)?, data)
}

// //// 创建受管理的本地 CN 邮件 [@x380kkm 2026-07-24] ////
fn create_managed_mail(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match parse_json::<ManagementCreateMailRequest>(request) {
        Ok(body) => body,
        Err(_) => return Ok(error_response("400 Bad Request", "invalid_mail")),
    };
    let Some(snapshot) = player_snapshot(database, body.viewer_id)? else {
        return Ok(error_response("404 Not Found", "viewer_not_found"));
    };
    create_mail_for_account(
        database,
        snapshot.account_id,
        ManagedMailInput {
            title: body.title,
            body: body.body,
            sender: body.sender,
            rewards: body.rewards,
            expires_at: body.expires_at,
        },
    )
}

fn create_local_save_mail(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    account_id: i64,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match parse_json::<LocalSaveCreateMailRequest>(request) {
        Ok(body) => body,
        Err(_) => return Ok(error_response("400 Bad Request", "invalid_mail")),
    };
    create_mail_for_account(
        database,
        account_id,
        ManagedMailInput {
            title: body.title,
            body: body.body,
            sender: body.sender,
            rewards: body.rewards,
            expires_at: body.expires_at,
        },
    )
}

fn create_mail_for_account(
    database: &mut ServiceDatabase,
    account_id: i64,
    input: ManagedMailInput,
) -> Result<HttpResponse, PersonalServiceError> {
    let rewards = match normalize_rewards(input.rewards) {
        Ok(rewards) => rewards,
        Err(code) => return Ok(error_response("400 Bad Request", &code)),
    };
    let created_at = server_time(database)?;
    let title = input.title.trim().to_owned();
    let mail_body = input.body.trim().to_owned();
    let sender = input.sender.trim().to_owned();
    if title.is_empty()
        || title.chars().count() > 200
        || title.chars().any(char::is_control)
        || mail_body.is_empty()
        || mail_body.chars().count() > 5000
        || mail_body.chars().any(char::is_control)
        || sender.is_empty()
        || sender.chars().count() > 100
        || sender.chars().any(char::is_control)
        || input
            .expires_at
            .is_some_and(|expires_at| expires_at <= created_at)
    {
        return Ok(error_response("400 Bad Request", "invalid_mail"));
    }
    let mail = database.create_mail(&CreateMailInput {
        account_id,
        title,
        body: mail_body,
        sender,
        rewards,
        expires_at: input.expires_at,
        created_at,
    })?;
    serialize_mail_response(mail)
}

fn list_managed_mails(
    database: &mut ServiceDatabase,
    viewer_text: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(viewer_id) = positive_integer(&Value::String(viewer_text.to_owned())) else {
        return Ok(error_response("400 Bad Request", "invalid_viewer_id"));
    };
    let Some(snapshot) = player_snapshot(database, viewer_id)? else {
        return Ok(error_response("404 Not Found", "viewer_not_found"));
    };
    list_managed_mails_for_account(database, snapshot.account_id)
}

fn list_managed_mails_for_account(
    database: &mut ServiceDatabase,
    account_id: i64,
) -> Result<HttpResponse, PersonalServiceError> {
    let page = database.list_mails(account_id, 1, MAX_MAIL_PAGE_SIZE, server_time(database)?)?;
    serde_json::to_string(
        &page
            .mails
            .into_iter()
            .map(serialize_management_mail)
            .collect::<Vec<_>>(),
    )
    .map(|body| HttpResponse::json("200 OK", body))
    .map_err(|error| PersonalServiceError::new(format!("failed to encode mail list: {error}")))
}
// //// /创建受管理的本地 CN 邮件 ////

fn normalize_rewards(value: Value) -> Result<MailReward, String> {
    let rewards: MailReward = serde_json::from_value(value).map_err(|_| "invalid_mail_rewards")?;
    if rewards.item_list.len() > MAX_MAIL_REWARD_ENTRIES
        || rewards.equipment_list.len() > MAX_MAIL_REWARD_ENTRIES
        || rewards.character_list.len() > MAX_MAIL_REWARD_ENTRIES
        || rewards.item_list.keys().any(|key| !is_positive_id(key))
        || rewards
            .equipment_list
            .keys()
            .any(|key| !is_positive_id(key))
        || rewards.character_list.iter().any(|value| *value <= 0)
        || rewards.item_list.values().any(|value| *value < 0)
        || rewards.equipment_list.values().any(|value| *value < 0)
        || rewards.free_mana < 0
        || rewards.paid_mana < 0
        || rewards.free_vmoney < 0
        || rewards.vmoney < 0
        || rewards.exp_pool < 0
        || rewards.star_crumb < 0
        || rewards.bond_token < 0
        || rewards.boss_boost_point < 0
        || rewards.boost_point < 0
        || rewards.rank_point < 0
    {
        return Err("invalid_mail_rewards".to_owned());
    }
    let has_reward = rewards.item_list.values().any(|value| *value > 0)
        || rewards.equipment_list.values().any(|value| *value > 0)
        || !rewards.character_list.is_empty()
        || rewards.free_mana > 0
        || rewards.paid_mana > 0
        || rewards.free_vmoney > 0
        || rewards.vmoney > 0
        || rewards.exp_pool > 0
        || rewards.star_crumb > 0
        || rewards.bond_token > 0
        || rewards.boss_boost_point > 0
        || rewards.boost_point > 0
        || rewards.rank_point > 0;
    if !has_reward {
        return Err("mail_rewards_empty".to_owned());
    }
    Ok(rewards)
}

fn apply_rewards(
    player_data: &mut Value,
    rewards: &MailReward,
    viewer_id: i64,
    server_time: i64,
) -> Result<MailClaimReward, PersonalServiceError> {
    let root = require_root(player_data)?;
    let user_info = require_object(root, "user_info")?;
    for (key, amount) in [
        ("free_mana", rewards.free_mana),
        ("paid_mana", rewards.paid_mana),
        ("free_vmoney", rewards.free_vmoney),
        ("vmoney", rewards.vmoney),
        ("exp_pool", rewards.exp_pool),
        ("star_crumb", rewards.star_crumb),
        ("bond_token", rewards.bond_token),
        ("boss_boost_point", rewards.boss_boost_point),
        ("boost_point", rewards.boost_point),
        ("rank_point", rewards.rank_point),
    ] {
        let current = user_info
            .get(key)
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let total = current.checked_add(amount).ok_or_else(|| {
            PersonalServiceError::new("mail currency exceeds the supported range")
        })?;
        user_info.insert(key.to_owned(), Value::from(total));
    }
    if rewards.exp_pool > 0 {
        user_info.insert("exp_pooled_time".to_owned(), Value::from(server_time));
    }
    let item_list = require_object(root, "item_list")?;
    let mut applied = MailClaimReward::default();
    for (item_id, amount) in &rewards.item_list {
        let current = item_list
            .get(item_id)
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let total = current.checked_add(*amount).ok_or_else(|| {
            PersonalServiceError::new("mail item count exceeds the supported range")
        })?;
        item_list.insert(item_id.clone(), Value::from(total));
        if *amount > 0 {
            applied.item_list.insert(item_id.clone(), *amount);
        }
    }

    let equipment_list = require_object(root, "user_equipment_list")?;
    for (equipment_id, amount) in &rewards.equipment_list {
        if *amount == 0 {
            continue;
        }
        let equipment_id_number = equipment_id
            .parse::<i64>()
            .map_err(|_| PersonalServiceError::new("mail equipment id is invalid"))?;
        let existing = equipment_list.get(equipment_id).cloned();
        let mut equipment = existing.clone().unwrap_or_else(|| {
            json!({
                "enhancement_level": 0,
                "level": 1,
                "protection": false,
                "stack": 0,
            })
        });
        let object = equipment
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored equipment data is invalid"))?;
        let stack = object
            .get("stack")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let duplicate_count = if existing.is_some() {
            *amount
        } else {
            amount.saturating_sub(1)
        };
        let new_stack = stack.checked_add(duplicate_count).ok_or_else(|| {
            PersonalServiceError::new("mail equipment count exceeds the supported range")
        })?;
        object.insert("stack".to_owned(), Value::from(new_stack));
        let protection = object
            .get("protection")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let level = object.get("level").and_then(Value::as_i64).unwrap_or(1);
        let enhancement_level = object
            .get("enhancement_level")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        equipment_list.insert(equipment_id.clone(), equipment);
        applied.equipment_list.push(json!({
            "null": 1,
            "viewer_id": 0,
            "equipment_id": equipment_id_number,
            "protection": protection,
            "level": level,
            "enhancement_level": enhancement_level,
            "stack": new_stack,
        }));
    }

    for character_id in &rewards.character_list {
        let joined =
            !require_object(root, "user_character_list")?.contains_key(&character_id.to_string());
        let character = grant_mailed_character(root, viewer_id, *character_id, server_time)?;
        if joined {
            crate::cn_gacha::record_character_encyclopedia_state(root, *character_id)?;
        }
        applied.character_list.push(character);
    }
    Ok(applied)
}

// //// 返回领取邮件前后的百科状态 [@x380kkm 2026-08-28] ////
fn read_claim_encyclopedia_info(
    database: &ServiceDatabase,
    viewer_id: i64,
) -> Result<Map<String, Value>, PersonalServiceError> {
    let Some(snapshot) = player_snapshot(database, viewer_id)? else {
        return Err(PersonalServiceError::new(
            "viewer session disappeared after mail claim",
        ));
    };
    let player_data = decode_player_data(&snapshot.data)?;
    let root = require_root_readonly(&player_data)?;
    match root.get("encyclopedia_list") {
        Some(Value::Object(values)) => Ok(values.clone()),
        Some(_) => Err(PersonalServiceError::new(
            "stored player encyclopedia data is invalid",
        )),
        None => Ok(Map::new()),
    }
}

fn changed_encyclopedia_info(
    previous: &Map<String, Value>,
    current: &Map<String, Value>,
) -> Map<String, Value> {
    current
        .iter()
        .filter(|(key, value)| previous.get(*key) != Some(*value))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}
// //// /返回领取邮件前后的百科状态 ////

fn read_claim_user_info(
    database: &ServiceDatabase,
    viewer_id: i64,
) -> Result<Value, PersonalServiceError> {
    let Some(snapshot) = player_snapshot(database, viewer_id)? else {
        return Err(PersonalServiceError::new(
            "viewer session disappeared after mail claim",
        ));
    };
    let player_data = decode_player_data(&snapshot.data)?;
    let root = require_root_readonly(&player_data)?;
    let user_info = root
        .get("user_info")
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("stored user info is missing"))?;
    Ok(json!({
        "free_mana": user_info.get("free_mana").and_then(Value::as_i64).unwrap_or_default(),
        "paid_mana": user_info.get("paid_mana").and_then(Value::as_i64).unwrap_or_default(),
        "free_vmoney": user_info.get("free_vmoney").and_then(Value::as_i64).unwrap_or_default(),
        "vmoney": user_info.get("vmoney").and_then(Value::as_i64).unwrap_or_default(),
        "exp_pool": user_info.get("exp_pool").and_then(Value::as_i64).unwrap_or_default(),
        "exp_pooled_time": user_info.get("exp_pooled_time").and_then(Value::as_i64).unwrap_or_default(),
        "star_crumb": user_info.get("star_crumb").and_then(Value::as_i64).unwrap_or_default(),
        "bond_token": user_info.get("bond_token").and_then(Value::as_i64).unwrap_or_default(),
        "boss_boost_point": user_info.get("boss_boost_point").and_then(Value::as_i64).unwrap_or_default(),
        "boost_point": user_info.get("boost_point").and_then(Value::as_i64).unwrap_or_default(),
        "rank_point": user_info.get("rank_point").and_then(Value::as_i64).unwrap_or_default(),
    }))
}

// //// 返回领取邮件后的道具余额 [@x380kkm 2026-08-22] ////
fn read_claim_item_updates(
    database: &ServiceDatabase,
    viewer_id: i64,
    claimed_items: &BTreeMap<String, i64>,
) -> Result<Map<String, Value>, PersonalServiceError> {
    let Some(snapshot) = player_snapshot(database, viewer_id)? else {
        return Err(PersonalServiceError::new(
            "viewer session disappeared after mail claim",
        ));
    };
    let player_data = decode_player_data(&snapshot.data)?;
    let root = require_root_readonly(&player_data)?;
    let item_list = root
        .get("item_list")
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("stored item list is missing"))?;
    claimed_items
        .keys()
        .map(|item_id| {
            item_list
                .get(item_id)
                .and_then(Value::as_i64)
                .map(|count| (item_id.clone(), Value::from(count)))
                .ok_or_else(|| PersonalServiceError::new("claimed item balance is missing"))
        })
        .collect()
}
// //// /返回领取邮件后的道具余额 ////

// //// 返回领取邮件后发生变化的玩家余额 [@x380kkm 2026-08-22] ////
fn changed_user_info(previous: &Value, current: &Value) -> Result<Value, PersonalServiceError> {
    let previous = previous
        .as_object()
        .ok_or_else(|| PersonalServiceError::new("stored user info is invalid"))?;
    let current = current
        .as_object()
        .ok_or_else(|| PersonalServiceError::new("stored user info is invalid"))?;
    let changed = current
        .iter()
        .filter(|(key, value)| previous.get(*key) != Some(*value))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Map<_, _>>();
    Ok(Value::Object(changed))
}
// //// /返回领取邮件后发生变化的玩家余额 ////

const UNRECEIVED_MAIL_TIME: &str = "0000-00-00 00:00:00";

fn serialize_mail(mail: StoredMail) -> Value {
    let (kind, type_id, number) = resolve_mail_kind(&mail.rewards);
    json!({
        "create_time": format_client_time(mail.created_at),
        "description": mail.body,
        "id": mail.id,
        "number": number,
        "reason_id": 999998,
        "receive_time": mail.received_at.map(format_client_time).unwrap_or_else(|| UNRECEIVED_MAIL_TIME.to_owned()),
        "reward_limit_time": mail.expires_at.map(format_client_time),
        "reward_period_limited": mail.expires_at.is_some(),
        "subject": mail.title,
        "type": kind,
        "type_id": type_id,
    })
}

fn serialize_management_mail(mail: StoredMail) -> Value {
    json!({
        "id": mail.id,
        "title": mail.title,
        "body": mail.body,
        "sender": mail.sender,
        "created_at": mail.created_at,
        "expires_at": mail.expires_at,
        "received_at": mail.received_at,
        "rewards": mail.rewards,
    })
}

fn serialize_mail_response(mail: StoredMail) -> Result<HttpResponse, PersonalServiceError> {
    serde_json::to_string(&serialize_management_mail(mail))
        .map(|body| HttpResponse::json("201 Created", body))
        .map_err(|error| {
            PersonalServiceError::new(format!("failed to encode created mail: {error}"))
        })
}

fn resolve_mail_kind(rewards: &MailReward) -> (i64, Option<i64>, i64) {
    if let Some((id, count)) = rewards.item_list.iter().next() {
        return (1, id.parse().ok(), *count);
    }
    if let Some((id, count)) = rewards.equipment_list.iter().next() {
        return (6, id.parse().ok(), *count);
    }
    if let Some(character_id) = rewards.character_list.first() {
        return (5, Some(*character_id), rewards.character_list.len() as i64);
    }
    if rewards.free_vmoney > 0 {
        return (4, None, rewards.free_vmoney);
    }
    if rewards.vmoney > 0 {
        return (3, None, rewards.vmoney);
    }
    if rewards.free_mana > 0 {
        return (8, None, rewards.free_mana);
    }
    if rewards.exp_pool > 0 {
        return (9, None, rewards.exp_pool);
    }
    if rewards.star_crumb > 0 {
        return (7, None, rewards.star_crumb);
    }
    if rewards.bond_token > 0 {
        return (10, None, rewards.bond_token);
    }
    if rewards.boss_boost_point > 0 {
        return (11, None, rewards.boss_boost_point);
    }
    if rewards.boost_point > 0 {
        return (12, None, rewards.boost_point);
    }
    if rewards.rank_point > 0 {
        return (15, None, rewards.rank_point);
    }
    (8, None, rewards.paid_mana.max(1))
}

fn player_snapshot(
    database: &ServiceDatabase,
    viewer_id: i64,
) -> Result<Option<PlayerSnapshot>, PersonalServiceError> {
    match database.lookup_viewer_session_player(viewer_id)? {
        ViewerSessionPlayer::Present(snapshot) => Ok(Some(snapshot)),
        ViewerSessionPlayer::InvalidSession
        | ViewerSessionPlayer::MissingPlayer
        | ViewerSessionPlayer::MissingPlayerData(_) => Ok(None),
    }
}

fn positive_integer(value: &Value) -> Option<i64> {
    match value {
        Value::Number(value) => value.as_i64().filter(|value| *value > 0),
        Value::String(value) => value.parse().ok().filter(|value: &i64| *value > 0),
        _ => None,
    }
}

fn parse_json<T: serde::de::DeserializeOwned>(
    request: &HttpRequest,
) -> Result<T, PersonalServiceError> {
    if !request
        .header("content-type")
        .is_some_and(|value| value.starts_with("application/json"))
    {
        return Err(PersonalServiceError::new("invalid_json_content_type"));
    }
    serde_json::from_slice(request.body())
        .map_err(|_| PersonalServiceError::new("invalid_json_body"))
}

fn is_positive_id(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<i64>().ok().is_some_and(|value| value > 0)
}

fn decode_player_data(serialized: &str) -> Result<Value, PersonalServiceError> {
    serde_json::from_str(serialized).map_err(|error| {
        PersonalServiceError::new(format!("failed to decode player mail data: {error}"))
    })
}

fn require_root(player_data: &mut Value) -> Result<&mut Map<String, Value>, PersonalServiceError> {
    player_data
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored player mail data is not an object"))
}

fn require_root_readonly(player_data: &Value) -> Result<&Map<String, Value>, PersonalServiceError> {
    player_data
        .as_object()
        .ok_or_else(|| PersonalServiceError::new("stored player mail data is not an object"))
}

fn require_object<'a>(
    root: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>, PersonalServiceError> {
    root.get_mut(key)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new(format!("stored player {key} data is missing")))
}

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}
