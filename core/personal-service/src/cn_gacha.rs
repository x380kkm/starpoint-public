// audience: internal
// # personal-service-cn-gacha
//
// 该模块实现 CN 角色和装备扭蛋的开放条件, 支付, 抽取, 兑换和状态持久化. 配置沿用仓库内 CN 主数据.

mod campaign;
mod movie;
mod region;

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_character_reward::{grant_character, DuplicateCharacterItem};
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, player_snapshot, require_object, require_root,
};
use crate::database::{
    evaluate_activity_schedule, ActivityWindowStatus, ReceiveHistoryEntry, ServiceDatabase,
};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use getrandom::getrandom;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;

const CN_GACHA_ASSET: &str = include_str!("../../../assets/gacha.json");
const CHARACTER_GACHA_TYPE: i64 = 0;
const EQUIPMENT_GACHA_TYPE: i64 = 1;
const FREE_VMONEY_PAYMENT: i64 = 1;
const PAID_VMONEY_PAYMENT: i64 = 2;
const TICKET_PAYMENT: i64 = 3;
const CAMPAIGN_PAYMENT: i64 = 4;
const SINGLE_DRAW: i64 = 1;
const MULTI_DRAW: i64 = 2;
const POOL_SINGLE_TICKET_DRAW: i64 = 3;
const POOL_MULTI_TICKET_DRAW: i64 = 4;
const DAILY_SINGLE_DRAW: i64 = 5;
const ACCOUNT_FIRST_MULTI_DRAW: i64 = 7;
const LEGACY_CAMPAIGN_SINGLE_DRAW: i64 = 7;
const CAMPAIGN_MULTI_DRAW: i64 = 8;
const CHARACTER_MULTI_TICKET_DRAW: i64 = 9;
const CHARACTER_SINGLE_TICKET_DRAW: i64 = 10;
const CAMPAIGN_SINGLE_DRAW: i64 = 11;
const EQUIPMENT_SINGLE_TICKET_DRAW: i64 = 12;
const EQUIPMENT_MULTI_TICKET_DRAW: i64 = 13;
const CRAZY_MULTI_TICKET_DRAW: i64 = 14;
const GUARANTEED_CHARACTER_SINGLE_TICKET_DRAW: i64 = 20;
const EXCHANGE_REQUIRED_POINTS: i64 = 250;
const MAX_CLIENT_PERIOD_SECONDS: i64 = 253_402_300_799;

static CN_GACHA_DATA: OnceLock<Result<Value, String>> = OnceLock::new();

#[derive(Deserialize)]
struct ExecuteRequest {
    #[serde(default)]
    api_count: i64,
    payment_type: i64,
    number_of_exec: i64,
    viewer_id: i64,
    gacha_id: i64,
    r#type: i64,
}

#[derive(Deserialize)]
struct ExchangeCharacterRequest {
    character_id: i64,
    #[serde(default)]
    api_count: i64,
    gacha_id: i64,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct ExchangeEquipmentRequest {
    equipment_id: i64,
    #[serde(default)]
    api_count: i64,
    gacha_id: i64,
    viewer_id: i64,
}

struct PaymentResult {
    pull_count: i64,
    free_vmoney: i64,
    vmoney: i64,
    item_list: Map<String, Value>,
    campaign_list: Vec<Value>,
    is_daily_payment: bool,
    is_account_first_payment: bool,
}

#[derive(Clone, Copy, Default)]
struct GachaCountIncrement {
    daily_one: i64,
    daily_ten: i64,
    crazy_draw: i64,
}

#[derive(Clone, Copy)]
struct GachaAvailability {
    is_daily_first: bool,
    is_account_first: bool,
}

struct TicketPayment {
    item_id: i64,
    pulls_per_ticket: i64,
}

// //// 分派 CN 扭蛋请求 [@x380kkm 2026-07-24] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    let response = match request.path() {
        "/api/index.php/gacha/exec" => execute(request, database),
        "/api/index.php/gacha/exchange_character" => exchange_character(request, database),
        "/api/index.php/gacha/exchange_equipment" => exchange_equipment(request, database),
        _ => return None,
    };
    Some(response)
}
// //// /分派 CN 扭蛋请求 ////

// //// 按当前服务日期刷新 CN 载入响应中的每日扭蛋状态 [@x380kkm 2026-08-18] ////
pub(crate) fn refresh_daily_availability(
    player_data: &mut Value,
    database: &ServiceDatabase,
    account_id: i64,
) -> Result<(), PersonalServiceError> {
    let root = player_data
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN player data is not an object"))?;
    region::normalize_gacha_info(root)?;
    if let Some(gacha_info_list) = root.get_mut("gacha_info_list") {
        let gacha_info_list = gacha_info_list
            .as_array_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN gacha info is invalid"))?;
        for info in gacha_info_list {
            let info = info
                .as_object_mut()
                .ok_or_else(|| PersonalServiceError::new("stored CN gacha info is invalid"))?;
            apply_gacha_count_increment(info, GachaCountIncrement::default())?;
            let Some(gacha_id) = info.get("gacha_id").and_then(Value::as_i64) else {
                continue;
            };
            let Ok(gacha) = gacha_definition(gacha_id) else {
                continue;
            };
            let is_daily_first = gacha_supports_daily_discount(gacha)?
                && database.is_cn_gacha_daily_available(account_id, gacha_id)?;
            info.insert("is_daily_first".to_owned(), Value::Bool(is_daily_first));
        }
    }
    refresh_dynamic_gacha_campaigns(root, database)?;
    campaign::replace_available_for_load(root, |gacha_id| {
        Ok(gacha_activity_status(database, gacha_id)? == ActivityWindowStatus::Open)
    })?;
    inject_account_first_gacha_info(root)?;
    inject_active_temporary_gacha_aliases(root, database, account_id)?;
    inject_regional_coverage_gacha_aliases(root, database, account_id)?;
    Ok(())
}
// //// /按当前服务日期刷新 CN 载入响应中的每日扭蛋状态 ////

// //// 向载入响应投影未消费的账号首次扭蛋 [@x380kkm 2026-08-24] ////
fn inject_account_first_gacha_info(
    root: &mut Map<String, Value>,
) -> Result<(), PersonalServiceError> {
    let account_first_ids = gacha_document()?
        .as_object()
        .ok_or_else(|| PersonalServiceError::new("CN gacha asset is invalid"))?
        .iter()
        .filter_map(|(id, gacha)| {
            (gacha.get("pageKind").and_then(Value::as_i64) == Some(1))
                .then(|| id.parse::<i64>().ok())
                .flatten()
        })
        .collect::<Vec<_>>();
    let list = root
        .entry("gacha_info_list".to_owned())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN gacha info is invalid"))?;
    for gacha_id in account_first_ids {
        if list
            .iter()
            .any(|info| info.get("gacha_id").and_then(Value::as_i64) == Some(gacha_id))
        {
            continue;
        }
        list.push(json!({
            "gacha_id": gacha_id,
            "is_account_first": true,
            "is_daily_first": false,
            "gacha_exchange_point": 0,
            "daily_one_count": 0,
            "daily_ten_count": 0,
            "crazy_draw_count": 0,
        }));
    }
    Ok(())
}
// //// /向载入响应投影未消费的账号首次扭蛋 ////

// //// 仅向客户端合成当前有效的临时卡池入口 [@x380kkm 2026-08-24] ////
fn inject_active_temporary_gacha_aliases(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    account_id: i64,
) -> Result<(), PersonalServiceError> {
    let leases = database.list_active_activity_temporary_open_leases()?;
    if leases.is_empty() {
        return Ok(());
    }
    let mut campaign_aliases = Vec::new();
    {
        let list = root
            .entry("gacha_info_list".to_owned())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN gacha info is invalid"))?;
        for (activity_id, _) in leases {
            let Some(canonical_id) = activity_id
                .strip_prefix("gacha:")
                .and_then(|value| value.parse::<i64>().ok())
            else {
                continue;
            };
            let Some(temporary_id) = region::temporary_alias_for(canonical_id)? else {
                continue;
            };
            if gacha_master_window_status(canonical_id, database.current_server_time_millis()?)?
                == ActivityWindowStatus::Open
            {
                continue;
            }
            let Some(period) = active_temporary_gacha_period(database, canonical_id)? else {
                continue;
            };
            let gacha = direct_gacha_definition(canonical_id)?;
            let is_daily_first = gacha_supports_daily_discount(gacha)?
                && database.is_cn_gacha_daily_available(account_id, canonical_id)?;
            let is_account_first = account_first_available_from_list(list, gacha, canonical_id)?;
            let mut info = list
                .iter()
                .find(|info| info.get("gacha_id").and_then(Value::as_i64) == Some(canonical_id))
                .cloned()
                .unwrap_or_else(|| {
                    json!({
                        "gacha_id": canonical_id,
                        "is_account_first": is_account_first,
                        "is_daily_first": is_daily_first,
                        "gacha_exchange_point": 0,
                        "daily_one_count": 0,
                        "daily_ten_count": 0,
                        "crazy_draw_count": 0,
                    })
                });
            let info = info
                .as_object_mut()
                .ok_or_else(|| PersonalServiceError::new("stored CN gacha info is invalid"))?;
            info.insert("gacha_id".to_owned(), Value::from(temporary_id));
            info.insert("comeback_campaign".to_owned(), period);
            list.push(Value::Object(info.clone()));
            campaign_aliases.push((canonical_id, temporary_id));
        }
    }
    for (canonical_id, temporary_id) in campaign_aliases {
        campaign::project_alias_for_load(root, canonical_id, temporary_id)?;
    }
    Ok(())
}

// //// 在普通卡池时间缺口合成一个 CN 内容入口 [@x380kkm 2026-08-24] ////
fn inject_regional_coverage_gacha_aliases(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    account_id: i64,
) -> Result<(), PersonalServiceError> {
    let aliases = region::coverage_aliases(database.current_server_time_millis()?)?;
    if aliases.is_empty() {
        return Ok(());
    }
    let list = root
        .entry("gacha_info_list".to_owned())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN gacha info is invalid"))?;
    for resolution in aliases {
        let canonical = direct_gacha_definition(resolution.canonical)?;
        let kind = gacha_type(canonical)?;
        if region::has_visible_temporary_for_type(list, kind)? {
            continue;
        }
        let alias = direct_gacha_definition(resolution.requested)?;
        let (start_at_ms, end_at_ms) = gacha_master_times(alias)?;
        let is_daily_first = gacha_supports_daily_discount(canonical)?
            && database.is_cn_gacha_daily_available(account_id, resolution.canonical)?;
        let is_account_first =
            account_first_available_from_list(list, canonical, resolution.canonical)?;
        let mut info = list
            .iter()
            .find(|info| info.get("gacha_id").and_then(Value::as_i64) == Some(resolution.canonical))
            .cloned()
            .unwrap_or_else(|| {
                json!({
                    "gacha_id": resolution.canonical,
                    "is_account_first": is_account_first,
                    "is_daily_first": is_daily_first,
                    "gacha_exchange_point": 0,
                    "daily_one_count": 0,
                    "daily_ten_count": 0,
                    "crazy_draw_count": 0,
                })
            });
        let info = info
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN gacha info is invalid"))?;
        info.insert("gacha_id".to_owned(), Value::from(resolution.requested));
        info.insert(
            "comeback_campaign".to_owned(),
            json!({
                "period_start_time": start_at_ms.div_euclid(1000),
                "period_end_time": end_at_ms.div_euclid(1000),
            }),
        );
        list.push(Value::Object(info.clone()));
    }
    Ok(())
}

// //// /在普通卡池时间缺口合成一个 CN 内容入口 ////

fn active_temporary_gacha_period(
    database: &ServiceDatabase,
    canonical_id: i64,
) -> Result<Option<Value>, PersonalServiceError> {
    let activity_id = format!("gacha:{canonical_id}");
    let Some((opened_wall_ms, expires_wall_ms)) =
        database.activity_temporary_open_window(&activity_id)?
    else {
        return Ok(None);
    };
    Ok(Some(projected_temporary_gacha_period(
        opened_wall_ms,
        expires_wall_ms,
        database.current_wall_time_millis()?,
        database.current_server_time_millis()?,
    )?))
}

fn projected_temporary_gacha_period(
    opened_wall_ms: i64,
    expires_wall_ms: i64,
    wall_now_ms: i64,
    virtual_now_ms: i64,
) -> Result<Value, PersonalServiceError> {
    let elapsed_ms = wall_now_ms.saturating_sub(opened_wall_ms).max(0);
    let remaining_ms = expires_wall_ms.saturating_sub(wall_now_ms);
    if remaining_ms <= 0 {
        return Err(PersonalServiceError::new(
            "CN gacha temporary lease expired during refresh",
        ));
    }
    let start_at_ms = virtual_now_ms
        .checked_sub(elapsed_ms)
        .ok_or_else(|| PersonalServiceError::new("CN gacha temporary period exceeds range"))?;
    let end_at_ms = virtual_now_ms
        .checked_add(remaining_ms)
        .ok_or_else(|| PersonalServiceError::new("CN gacha temporary period exceeds range"))?;
    Ok(json!({
        "period_start_time": start_at_ms.div_euclid(1000),
        "period_end_time": end_at_ms.div_euclid(1000),
    }))
}

fn response_gacha_info(
    mut info: Value,
    resolution: region::ResolvedGachaId,
    database: &ServiceDatabase,
) -> Result<Value, PersonalServiceError> {
    if resolution.requested == resolution.canonical {
        return Ok(info);
    }
    let info = info
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN gacha info is invalid"))?;
    info.insert("gacha_id".to_owned(), Value::from(resolution.requested));
    if resolution.is_temporary {
        let period = active_temporary_gacha_period(database, resolution.canonical)?
            .ok_or_else(|| PersonalServiceError::new("CN gacha temporary lease is not active"))?;
        info.insert("comeback_campaign".to_owned(), period);
    } else if resolution.is_coverage || resolution.is_regional {
        if let Some(period) = alias_gacha_period(database, resolution)? {
            info.insert("comeback_campaign".to_owned(), period);
        } else {
            info.remove("comeback_campaign");
        }
    }
    Ok(Value::Object(info.clone()))
}
// //// /仅向客户端合成当前有效的临时卡池入口 ////

// //// 构造地区别名响应使用的客户端有效期 [@x380kkm 2026-08-29] ////
fn alias_gacha_period(
    database: &ServiceDatabase,
    resolution: region::ResolvedGachaId,
) -> Result<Option<Value>, PersonalServiceError> {
    let canonical = direct_gacha_definition(resolution.canonical)?;
    let now_ms = database.current_server_time_millis()?;
    if let Some(period) =
        dynamic_campaign_period(database, resolution.canonical, canonical, now_ms)?
    {
        return Ok(Some(period));
    }
    let alias = direct_gacha_definition(resolution.requested)?;
    let (start_at_ms, end_at_ms) = gacha_master_times(alias)?;
    if start_at_ms <= now_ms && now_ms < end_at_ms {
        return Ok(Some(json!({
            "period_start_time": start_at_ms.div_euclid(1000),
            "period_end_time": end_at_ms.div_euclid(1000),
        })));
    }
    Ok(None)
}
// //// /构造地区别名响应使用的客户端有效期 ////

// //// 在载入响应中同步回归和群星卡池的客户端有效期 [@x380kkm 2026-08-24] ////
fn refresh_dynamic_gacha_campaigns(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
) -> Result<(), PersonalServiceError> {
    let now_ms = database.current_server_time_millis()?;
    let document = gacha_document()?
        .as_object()
        .ok_or_else(|| PersonalServiceError::new("CN gacha asset root is invalid"))?;
    for (gacha_id, gacha) in document {
        if region::is_hidden(gacha_id)? {
            continue;
        }
        let Some(field) = dynamic_campaign_field(gacha)? else {
            continue;
        };
        let gacha_id = gacha_id
            .parse::<i64>()
            .map_err(|_| PersonalServiceError::new("CN gacha id is invalid"))?;
        let period = dynamic_campaign_period(database, gacha_id, gacha, now_ms)?;
        update_dynamic_campaign(root, gacha_id, field, period)?;
    }
    Ok(())
}
// //// /在载入响应中同步回归和群星卡池的客户端有效期 ////

fn dynamic_campaign_field(gacha: &Value) -> Result<Option<&'static str>, PersonalServiceError> {
    let is_comeback = required_gacha_boolean(gacha, "isComeback")?;
    let is_stars = required_gacha_boolean(gacha, "isStarsGacha")?;
    match (is_comeback, is_stars) {
        (true, false) => Ok(Some("comeback_campaign")),
        (false, true) => Ok(Some("stars_campaign")),
        (false, false) => Ok(None),
        (true, true) => Err(PersonalServiceError::new(
            "CN gacha dynamic campaign kind is ambiguous",
        )),
    }
}

fn dynamic_campaign_period(
    database: &ServiceDatabase,
    gacha_id: i64,
    gacha: &Value,
    now_ms: i64,
) -> Result<Option<Value>, PersonalServiceError> {
    if let Some(period) = active_temporary_gacha_period(database, gacha_id)? {
        return Ok(Some(period));
    }
    let activity_id = format!("gacha:{gacha_id}");
    if let Some(schedule) = database.get_activity_schedule(&activity_id)? {
        let evaluation = evaluate_activity_schedule(&schedule, now_ms);
        if evaluation.status != ActivityWindowStatus::Open {
            return Ok(None);
        }
        return match (evaluation.active_start_ms, evaluation.active_end_ms) {
            (Some(start_at_ms), Some(end_at_ms)) if end_at_ms > start_at_ms => Ok(Some(json!({
                "period_start_time": start_at_ms.div_euclid(1000),
                "period_end_time": end_at_ms.div_euclid(1000),
            }))),
            (None, None) => Ok(Some(json!({
                "period_start_time": 0,
                "period_end_time": MAX_CLIENT_PERIOD_SECONDS,
            }))),
            _ => Err(PersonalServiceError::new(
                "CN gacha activity period is invalid",
            )),
        };
    }
    if gacha_master_window_status_from_definition(gacha, now_ms)? != ActivityWindowStatus::Open {
        return Ok(None);
    }
    let (start_at_ms, end_at_ms) = gacha_master_times(gacha)?;
    Ok(Some(json!({
        "period_start_time": start_at_ms.div_euclid(1000),
        "period_end_time": end_at_ms.div_euclid(1000),
    })))
}

fn update_dynamic_campaign(
    root: &mut Map<String, Value>,
    gacha_id: i64,
    field: &'static str,
    period: Option<Value>,
) -> Result<(), PersonalServiceError> {
    let list = root
        .entry("gacha_info_list".to_owned())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN gacha info is invalid"))?;
    let entry = list
        .iter_mut()
        .find(|info| info.get("gacha_id").and_then(Value::as_i64) == Some(gacha_id));
    if let Some(entry) = entry {
        let entry = entry
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN gacha info is invalid"))?;
        if let Some(period) = period {
            entry.insert(field.to_owned(), period);
        } else {
            entry.remove(field);
        }
        return Ok(());
    }
    let Some(period) = period else {
        return Ok(());
    };
    list.push(json!({
        "gacha_id": gacha_id,
        "is_account_first": true,
        "is_daily_first": false,
        "gacha_exchange_point": 0,
        (field): period,
    }));
    Ok(())
}

// //// 重置玩家的每日扭蛋状态 [@x380kkm 2026-08-23] ////
pub(crate) fn reset_daily_state(root: &mut Map<String, Value>) -> Result<(), PersonalServiceError> {
    let gacha_info_list = root
        .entry("gacha_info_list".to_owned())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN gacha info is invalid"))?;
    for info in gacha_info_list {
        let info = info
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN gacha info is invalid"))?;
        let gacha_id = info
            .get("gacha_id")
            .and_then(Value::as_i64)
            .ok_or_else(|| PersonalServiceError::new("stored CN gacha id is invalid"))?;
        let is_daily_first = match gacha_definition(gacha_id) {
            Ok(gacha) => gacha_supports_daily_discount(gacha)?,
            Err(_) => false,
        };
        apply_gacha_count_increment(info, GachaCountIncrement::default())?;
        info.insert("daily_one_count".to_owned(), Value::from(0));
        info.insert("daily_ten_count".to_owned(), Value::from(0));
        info.insert("is_daily_first".to_owned(), Value::Bool(is_daily_first));
    }
    campaign::reset_daily_counts(root)
}
// //// /重置玩家的每日扭蛋状态 ////

// //// 按正式卡池契约执行教程单抽 [@x380kkm 2026-08-25] ////
#[derive(Clone, Copy)]
pub(crate) struct TutorialGachaPlan {
    pub(crate) gacha_id: i64,
    resolution: region::ResolvedGachaId,
    gacha: &'static Value,
}

pub(crate) struct TutorialGachaDraw {
    pub(crate) character_id: i64,
    pub(crate) duplicate_item: Option<DuplicateCharacterItem>,
}

pub(crate) fn resolve_tutorial_gacha(
    gacha_id: i64,
) -> Result<Option<TutorialGachaPlan>, PersonalServiceError> {
    let resolution = match region::resolve(gacha_id) {
        Ok(resolution) => resolution,
        Err(_) => return Ok(None),
    };
    let gacha = direct_gacha_definition(resolution.canonical)?;
    if gacha_type(gacha)? != CHARACTER_GACHA_TYPE {
        return Ok(None);
    }
    Ok(Some(TutorialGachaPlan {
        gacha_id,
        resolution,
        gacha,
    }))
}

pub(crate) fn draw_tutorial_gacha(
    root: &mut Map<String, Value>,
    viewer_id: i64,
    plan: TutorialGachaPlan,
    server_time: i64,
) -> Result<Result<TutorialGachaDraw, &'static str>, PersonalServiceError> {
    let request = ExecuteRequest {
        api_count: 0,
        payment_type: FREE_VMONEY_PAYMENT,
        number_of_exec: 1,
        viewer_id,
        gacha_id: plan.gacha_id,
        r#type: SINGLE_DRAW,
    };
    match apply_payment(
        root,
        plan.gacha,
        &request,
        plan.resolution.canonical,
        false,
        false,
    )? {
        Ok(_) => {}
        Err(code) => return Ok(Err(code)),
    }
    let character_id = draw_character_id(plan.gacha, 0, false)?;
    let reward = grant_character(root, viewer_id, character_id, server_time)?;
    Ok(Ok(TutorialGachaDraw {
        character_id,
        duplicate_item: reward.duplicate_item,
    }))
}
// //// /按正式卡池契约执行教程单抽 ////

// //// 执行 CN 角色或装备扭蛋 [@x380kkm 2026-08-22] ////
fn execute(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ExecuteRequest>(request) {
        Ok(body) => body,
        Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    if body.viewer_id <= 0 || body.api_count < 0 {
        return Ok(error_response("400 Bad Request", "invalid_gacha_request"));
    }
    let resolution = match region::resolve(body.gacha_id) {
        Ok(resolution) => resolution,
        Err(_) => return Ok(error_response("400 Bad Request", "invalid_gacha_request")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let gacha = direct_gacha_definition(resolution.canonical)?;
    if let Some(response) = closed_gacha_response(database, root, resolution)? {
        return Ok(response);
    }
    let daily_available = gacha_supports_daily_discount(gacha)?
        && database.is_cn_gacha_daily_available(snapshot.account_id, resolution.canonical)?;
    let account_first_available = account_first_available(root, gacha, resolution.canonical)?;
    let mut payment = match apply_payment(
        root,
        gacha,
        &body,
        resolution.canonical,
        daily_available,
        account_first_available,
    )? {
        Ok(payment) => payment,
        Err(code) => return Ok(error_response("400 Bad Request", code)),
    };
    if resolution.is_temporary {
        campaign::project_temporary_response(&mut payment.campaign_list, resolution.requested)?;
    }
    let pull_count = payment.pull_count;
    let count_increment = gacha_count_increment(&body);
    let is_daily_payment = payment.is_daily_payment;
    let availability = GachaAvailability {
        is_daily_first: daily_available && !payment.is_daily_payment,
        is_account_first: account_first_available && !payment.is_account_first_payment,
    };

    let server_time = server_time(database)?;
    let (response, history_entries) = if gacha_type(gacha)? == EQUIPMENT_GACHA_TYPE {
        let mut draws = Vec::with_capacity(pull_count as usize);
        let mut equipment_by_id = Map::new();
        let mut history_entries = Vec::with_capacity(pull_count as usize);
        for draw_number in 0..pull_count {
            let equipment_id = draw_equipment_id(gacha, draw_number)?;
            let stored = add_equipment(root, equipment_id)?;
            equipment_by_id.insert(equipment_id.to_string(), stored);
            history_entries.push(ReceiveHistoryEntry::reward(6, Some(equipment_id), 1));
            draws.push(json!({
                "equipment_id": equipment_id,
                "treasure_up_type": 0,
            }));
        }
        let equipment = equipment_by_id
            .into_iter()
            .map(|(_, value)| value)
            .collect::<Vec<_>>();
        let gacha_info = response_gacha_info(
            update_gacha_info(
                root,
                resolution.canonical,
                pull_count,
                availability,
                count_increment,
            )?,
            resolution,
            database,
        )?;
        (
            json!({
                "user_info": {
                    "free_vmoney": payment.free_vmoney,
                    "vmoney": payment.vmoney,
                },
                "is_erupt": false,
                "draw_equipment": draws,
                "item_list": payment.item_list,
                "equipment_list": equipment,
                "gacha_info_list": [gacha_info],
                "encyclopedia_info": [],
                "mail_arrived": false,
            }),
            history_entries,
        )
    } else {
        let mut draws = Vec::with_capacity(pull_count as usize);
        let mut characters_by_id = Map::new();
        let mut item_list = payment.item_list;
        let mut encyclopedia_info = Map::new();
        let mut history_entries = Vec::with_capacity(pull_count as usize);
        for draw_number in 0..pull_count {
            let character_id = draw_character_id(
                gacha,
                draw_number,
                body.r#type == GUARANTEED_CHARACTER_SINGLE_TICKET_DRAW,
            )?;
            let reward = grant_character(root, body.viewer_id, character_id, server_time)?;
            if reward.joined {
                if record_character_encyclopedia_state(root, character_id)? {
                    encyclopedia_info.insert(format!("1{character_id}01"), json!({"read": false}));
                }
            }
            let movie_id = movie::draw_movie_id(gacha, character_id)?;
            history_entries.push(ReceiveHistoryEntry::reward(5, Some(character_id), 1));
            let mut draw = json!({
                "character_id": character_id,
                "movie_id": movie_id,
                "seed": movie::movie_seed(character_id, movie_id)?,
                "entry_count": 1,
            });
            apply_duplicate_item_to_draw(&mut draw, &mut item_list, reward.duplicate_item)?;
            draws.push(draw);
            merge_character_response(&mut characters_by_id, character_id, reward.character)?;
        }
        let characters = characters_by_id
            .into_iter()
            .map(|(_, character)| character)
            .collect::<Vec<_>>();
        let gacha_info = response_gacha_info(
            update_gacha_info(
                root,
                resolution.canonical,
                pull_count,
                availability,
                count_increment,
            )?,
            resolution,
            database,
        )?;
        (
            json!({
                "user_info": {
                    "free_vmoney": payment.free_vmoney,
                    "vmoney": payment.vmoney,
                },
                "draw": draws,
                "character_list": characters,
                "item_list": item_list,
                "gacha_campaign_list": payment.campaign_list,
                "gacha_info_list": [gacha_info],
                "encyclopedia_info": encyclopedia_info,
                "mail_arrived": false,
            }),
            history_entries,
        )
    };
    let encoded_player_data = encode_player_data(&player_data)?;
    let history_event_key = format!(
        "gacha:exec:{}:{}:{}:{}:{}",
        resolution.canonical, body.payment_type, body.r#type, body.number_of_exec, body.api_count
    );
    if is_daily_payment {
        let saved = database.save_player_snapshot_with_cn_daily_draw(
            snapshot.account_id,
            resolution.canonical,
            &encoded_player_data,
            &history_event_key,
            server_time,
            &history_entries,
        )?;
        if !saved {
            return Ok(error_response(
                "400 Bad Request",
                "daily_gacha_already_used",
            ));
        }
    } else {
        database.save_player_snapshot_with_receive_history(
            snapshot.account_id,
            &encoded_player_data,
            &history_event_key,
            server_time,
            &history_entries,
        )?;
    }
    msgpack_response_at(body.viewer_id, false, server_time, response)
}
// //// /执行 CN 角色或装备扭蛋 ////

// //// 兑换 CN 扭蛋角色并扣除积分 [@x380kkm 2026-07-24] ////
fn exchange_character(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ExchangeCharacterRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.character_id > 0 && body.api_count >= 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_gacha_request")),
    };
    let resolution = match region::resolve(body.gacha_id) {
        Ok(resolution) => resolution,
        Err(_) => return Ok(error_response("400 Bad Request", "invalid_gacha_request")),
    };
    let gacha = direct_gacha_definition(resolution.canonical)?;
    if gacha_type(gacha)? != CHARACTER_GACHA_TYPE {
        return Ok(error_response("400 Bad Request", "invalid_gacha_request"));
    }
    if !is_character_in_gacha(gacha, body.character_id) {
        return Ok(error_response("400 Bad Request", "character_not_in_gacha"));
    }
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    if let Some(response) = closed_gacha_response(database, root, resolution)? {
        return Ok(response);
    }
    let current_points = match gacha_exchange_points(root, resolution.canonical)? {
        Some(points) => points,
        None => return Ok(error_response("400 Bad Request", "no_gacha_info")),
    };
    if current_points < EXCHANGE_REQUIRED_POINTS {
        return Ok(error_response(
            "400 Bad Request",
            "not_enough_exchange_points",
        ));
    }
    let is_daily_first = gacha_supports_daily_discount(gacha)?
        && database.is_cn_gacha_daily_available(snapshot.account_id, resolution.canonical)?;
    let availability = GachaAvailability {
        is_daily_first,
        is_account_first: account_first_available(root, gacha, resolution.canonical)?,
    };
    let gacha_info = response_gacha_info(
        adjust_gacha_info(
            root,
            resolution.canonical,
            -EXCHANGE_REQUIRED_POINTS,
            availability,
            GachaCountIncrement::default(),
        )?,
        resolution,
        database,
    )?;
    let response_time = server_time(database)?;
    let reward = grant_character(root, body.viewer_id, body.character_id, response_time)?;
    let item_list = duplicate_item_list(reward.duplicate_item);
    let mut encyclopedia_info = Map::new();
    if reward.joined {
        if record_character_encyclopedia_state(root, body.character_id)? {
            encyclopedia_info.insert(format!("1{}01", body.character_id), json!({"read": false}));
        }
    }
    let response = json!({
        "character_list": [reward.character],
        "item_list": item_list,
        "gacha_info_list": [gacha_info],
        "encyclopedia_info": encyclopedia_info,
        "mail_arrived": false,
    });
    database.save_player_snapshot_with_receive_history(
        snapshot.account_id,
        &encode_player_data(&player_data)?,
        &format!(
            "gacha:exchange-character:{}:{}:{}:{}",
            resolution.canonical, body.character_id, body.api_count, body.viewer_id
        ),
        response_time,
        &[ReceiveHistoryEntry::reward(5, Some(body.character_id), 1)],
    )?;
    msgpack_response_at(body.viewer_id, false, response_time, response)
}
// //// /兑换 CN 扭蛋角色并扣除积分 ////

// //// 兑换 CN 扭蛋装备并扣除积分 [@x380kkm 2026-07-24] ////
fn exchange_equipment(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ExchangeEquipmentRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.equipment_id > 0 && body.api_count >= 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_gacha_request")),
    };
    let resolution = match region::resolve(body.gacha_id) {
        Ok(resolution) => resolution,
        Err(_) => return Ok(error_response("400 Bad Request", "invalid_gacha_request")),
    };
    let gacha = direct_gacha_definition(resolution.canonical)?;
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    if let Some(response) = closed_gacha_response(database, root, resolution)? {
        return Ok(response);
    }
    let current_points = match gacha_exchange_points(root, resolution.canonical)? {
        Some(points) => points,
        None => return Ok(error_response("400 Bad Request", "no_gacha_info")),
    };
    if current_points < EXCHANGE_REQUIRED_POINTS {
        return Ok(error_response(
            "400 Bad Request",
            "not_enough_exchange_points",
        ));
    }
    let is_daily_first = gacha_supports_daily_discount(gacha)?
        && database.is_cn_gacha_daily_available(snapshot.account_id, resolution.canonical)?;
    let availability = GachaAvailability {
        is_daily_first,
        is_account_first: account_first_available(root, gacha, resolution.canonical)?,
    };
    let gacha_info = response_gacha_info(
        adjust_gacha_info(
            root,
            resolution.canonical,
            -EXCHANGE_REQUIRED_POINTS,
            availability,
            GachaCountIncrement::default(),
        )?,
        resolution,
        database,
    )?;
    let equipment = add_equipment(root, body.equipment_id)?;
    let response_time = server_time(database)?;
    let response = json!({
        "equipment_list": [equipment],
        "gacha_info_list": [gacha_info],
        "encyclopedia_info": [],
        "mail_arrived": false,
    });
    database.save_player_snapshot_with_receive_history(
        snapshot.account_id,
        &encode_player_data(&player_data)?,
        &format!(
            "gacha:exchange-equipment:{}:{}:{}:{}",
            resolution.canonical, body.equipment_id, body.api_count, body.viewer_id
        ),
        response_time,
        &[ReceiveHistoryEntry::reward(6, Some(body.equipment_id), 1)],
    )?;
    msgpack_response_at(body.viewer_id, false, response_time, response)
}
// //// /兑换 CN 扭蛋装备并扣除积分 ////

// //// 按卡池页面和客户端抽取类型扣除扭蛋支付资源 [@x380kkm 2026-08-24] ////
fn apply_payment(
    root: &mut Map<String, Value>,
    gacha: &Value,
    body: &ExecuteRequest,
    canonical_gacha_id: i64,
    daily_available: bool,
    account_first_available: bool,
) -> Result<Result<PaymentResult, &'static str>, PersonalServiceError> {
    let (free_vmoney, vmoney) = {
        let user_info = require_object(root, "user_info")?;
        let free_vmoney = user_info
            .get("free_vmoney")
            .and_then(Value::as_i64)
            .ok_or_else(|| PersonalServiceError::new("stored CN free vmoney is missing"))?;
        let vmoney = user_info
            .get("vmoney")
            .and_then(Value::as_i64)
            .ok_or_else(|| PersonalServiceError::new("stored CN vmoney is missing"))?;
        (free_vmoney, vmoney)
    };
    let mut new_free_vmoney = free_vmoney;
    let mut new_vmoney = vmoney;
    let mut item_list = Map::new();
    let mut campaign_list = Vec::new();
    let mut is_daily_payment = false;
    let mut is_account_first_payment = false;
    let page_kind = gacha_page_kind(gacha)?;
    let pull_count = match body.payment_type {
        FREE_VMONEY_PAYMENT => {
            if !matches!(page_kind, 0 | 8) || body.number_of_exec != 1 {
                return Ok(Err("invalid_gacha_request"));
            }
            let cost_key = match body.r#type {
                SINGLE_DRAW => "singleCost",
                MULTI_DRAW => "multiCost",
                _ => return Ok(Err("invalid_gacha_request")),
            };
            let cost =
                required_positive_gacha_integer(gacha, cost_key, "CN gacha cost is invalid")?;
            if free_vmoney >= cost {
                new_free_vmoney = free_vmoney - cost;
            } else {
                let overflow = cost - free_vmoney;
                if vmoney < overflow {
                    return Ok(Err("not_enough_vmoney"));
                }
                new_free_vmoney = 0;
                new_vmoney = vmoney - overflow;
            }
            if body.r#type == MULTI_DRAW {
                10
            } else {
                1
            }
        }
        PAID_VMONEY_PAYMENT => {
            if body.number_of_exec != 1 {
                return Ok(Err("invalid_gacha_request"));
            }
            let (cost_key, error, pulls) = match body.r#type {
                DAILY_SINGLE_DRAW if page_kind == 0 => {
                    if !daily_available {
                        return Ok(Err("daily_gacha_already_used"));
                    }
                    is_daily_payment = true;
                    ("discountCost", "CN gacha daily cost is invalid", 1)
                }
                ACCOUNT_FIRST_MULTI_DRAW if page_kind == 1 => {
                    if !account_first_available {
                        return Ok(Err("account_first_gacha_already_used"));
                    }
                    is_account_first_payment = true;
                    (
                        "tenTimesPerAccountCost",
                        "CN gacha account-first cost is invalid",
                        10,
                    )
                }
                _ => return Ok(Err("invalid_gacha_request")),
            };
            let cost = required_positive_gacha_integer(gacha, cost_key, error)?;
            if vmoney < cost {
                return Ok(Err("not_enough_vmoney"));
            }
            new_vmoney = vmoney - cost;
            pulls
        }
        TICKET_PAYMENT => {
            let ticket = match ticket_payment(gacha, body.r#type)? {
                Ok(ticket) => ticket,
                Err(code) => return Ok(Err(code)),
            };
            if body.number_of_exec <= 0 || body.number_of_exec > 100 {
                return Ok(Err("invalid_gacha_request"));
            }
            let items = require_object(root, "item_list")?;
            let item_id = ticket.item_id.to_string();
            let current = stored_item_count(items, ticket.item_id)?;
            if current < body.number_of_exec {
                return Ok(Err("not_enough_tickets"));
            }
            let new_count = current - body.number_of_exec;
            items.insert(item_id.clone(), Value::from(new_count));
            item_list.insert(item_id, Value::from(new_count));
            body.number_of_exec * ticket.pulls_per_ticket
        }
        CAMPAIGN_PAYMENT => {
            if body.number_of_exec != 1
                || !matches!(
                    body.r#type,
                    LEGACY_CAMPAIGN_SINGLE_DRAW | CAMPAIGN_SINGLE_DRAW | CAMPAIGN_MULTI_DRAW
                )
            {
                return Ok(Err("invalid_gacha_request"));
            }
            let updated = match campaign::redeem(root, canonical_gacha_id)? {
                Ok(campaign) => campaign,
                Err(code) => return Ok(Err(code)),
            };
            campaign_list.push(updated);
            if body.r#type == CAMPAIGN_MULTI_DRAW {
                10
            } else {
                1
            }
        }
        _ => return Ok(Err("unsupported_gacha_payment")),
    };
    let user_info = require_object(root, "user_info")?;
    user_info.insert("free_vmoney".to_owned(), Value::from(new_free_vmoney));
    user_info.insert("vmoney".to_owned(), Value::from(new_vmoney));
    Ok(Ok(PaymentResult {
        pull_count,
        free_vmoney: new_free_vmoney,
        vmoney: new_vmoney,
        item_list,
        campaign_list,
        is_daily_payment,
        is_account_first_payment,
    }))
}
// //// /按卡池页面和客户端抽取类型扣除扭蛋支付资源 ////

// //// 将客户端票券抽取类型映射到卡池允许的真实道具 [@x380kkm 2026-08-24] ////
fn ticket_payment(
    gacha: &Value,
    draw_type: i64,
) -> Result<Result<TicketPayment, &'static str>, PersonalServiceError> {
    let page_kind = gacha_page_kind(gacha)?;
    let kind = gacha_type(gacha)?;
    let ticket = match draw_type {
        POOL_SINGLE_TICKET_DRAW if matches!(page_kind, 0 | 2 | 3) => TicketPayment {
            item_id: required_gacha_item_id(gacha, "onceTicketItemId")?,
            pulls_per_ticket: 1,
        },
        POOL_MULTI_TICKET_DRAW if matches!(page_kind, 0 | 2 | 4) => TicketPayment {
            item_id: required_gacha_item_id(gacha, "tenTimesTicketItemId")?,
            pulls_per_ticket: 10,
        },
        CRAZY_MULTI_TICKET_DRAW if page_kind == 5 => TicketPayment {
            item_id: required_gacha_item_id(gacha, "crazyTenTimesTicketItemId")?,
            pulls_per_ticket: 10,
        },
        CHARACTER_SINGLE_TICKET_DRAW
            if matches!(page_kind, 0 | 8)
                && kind == CHARACTER_GACHA_TYPE
                && required_gacha_boolean(gacha, "wildcardCharacterTicketAvailable")? =>
        {
            TicketPayment {
                item_id: 999_003,
                pulls_per_ticket: 1,
            }
        }
        CHARACTER_MULTI_TICKET_DRAW
            if matches!(page_kind, 0 | 8)
                && kind == CHARACTER_GACHA_TYPE
                && required_gacha_boolean(gacha, "wildcardCharacterTicketAvailable")? =>
        {
            TicketPayment {
                item_id: 999_001,
                pulls_per_ticket: 10,
            }
        }
        EQUIPMENT_SINGLE_TICKET_DRAW
            if page_kind == 0
                && kind == EQUIPMENT_GACHA_TYPE
                && required_gacha_boolean(gacha, "wildcardEquipmentTicketAvailable")? =>
        {
            TicketPayment {
                item_id: 999_005,
                pulls_per_ticket: 1,
            }
        }
        EQUIPMENT_MULTI_TICKET_DRAW
            if page_kind == 0
                && kind == EQUIPMENT_GACHA_TYPE
                && required_gacha_boolean(gacha, "wildcardEquipmentTicketAvailable")? =>
        {
            TicketPayment {
                item_id: 999_004,
                pulls_per_ticket: 10,
            }
        }
        GUARANTEED_CHARACTER_SINGLE_TICKET_DRAW
            if matches!(page_kind, 0 | 8)
                && kind == CHARACTER_GACHA_TYPE
                && required_gacha_boolean(gacha, "wildcardCharacterTicketAvailable")?
                && required_gacha_boolean(gacha, "freemiumGuaranteeAvailable")? =>
        {
            TicketPayment {
                item_id: 999_008,
                pulls_per_ticket: 1,
            }
        }
        POOL_SINGLE_TICKET_DRAW
        | POOL_MULTI_TICKET_DRAW
        | CHARACTER_MULTI_TICKET_DRAW
        | CHARACTER_SINGLE_TICKET_DRAW
        | EQUIPMENT_SINGLE_TICKET_DRAW
        | EQUIPMENT_MULTI_TICKET_DRAW
        | CRAZY_MULTI_TICKET_DRAW
        | GUARANTEED_CHARACTER_SINGLE_TICKET_DRAW => {
            return Ok(Err("unsupported_gacha_type"));
        }
        _ => return Ok(Err("invalid_gacha_request")),
    };
    Ok(Ok(ticket))
}
// //// /将客户端票券抽取类型映射到卡池允许的真实道具 ////

fn gacha_page_kind(gacha: &Value) -> Result<i64, PersonalServiceError> {
    gacha
        .get("pageKind")
        .and_then(Value::as_i64)
        .filter(|kind| matches!(*kind, 0 | 1 | 2 | 3 | 4 | 5 | 8))
        .ok_or_else(|| PersonalServiceError::new("CN gacha page kind is invalid"))
}

fn gacha_supports_daily_discount(gacha: &Value) -> Result<bool, PersonalServiceError> {
    let cost = gacha
        .get("discountCost")
        .and_then(Value::as_i64)
        .filter(|cost| *cost >= 0)
        .ok_or_else(|| PersonalServiceError::new("CN gacha daily cost is invalid"))?;
    Ok(gacha_page_kind(gacha)? == 0 && cost > 0)
}

fn required_positive_gacha_integer(
    gacha: &Value,
    key: &str,
    error: &'static str,
) -> Result<i64, PersonalServiceError> {
    gacha
        .get(key)
        .and_then(Value::as_i64)
        .filter(|value| *value > 0)
        .ok_or_else(|| PersonalServiceError::new(error))
}

fn required_gacha_boolean(gacha: &Value, key: &str) -> Result<bool, PersonalServiceError> {
    gacha
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| PersonalServiceError::new(format!("CN gacha {key} is missing")))
}

fn optional_gacha_item_id(gacha: &Value, key: &str) -> Result<Option<i64>, PersonalServiceError> {
    let value = gacha
        .get(key)
        .ok_or_else(|| PersonalServiceError::new(format!("CN gacha {key} is missing")))?;
    if value.is_null() {
        return Ok(None);
    }
    value
        .as_i64()
        .filter(|item_id| *item_id > 0)
        .map(Some)
        .ok_or_else(|| PersonalServiceError::new(format!("CN gacha {key} is invalid")))
}

fn required_gacha_item_id(gacha: &Value, key: &str) -> Result<i64, PersonalServiceError> {
    optional_gacha_item_id(gacha, key)?.ok_or_else(|| {
        PersonalServiceError::new(format!("CN gacha {key} is required for this draw type"))
    })
}

fn stored_item_count(
    items: &Map<String, Value>,
    item_id: i64,
) -> Result<i64, PersonalServiceError> {
    let Some(value) = items.get(&item_id.to_string()) else {
        return Ok(0);
    };
    value
        .as_i64()
        .filter(|count| *count >= 0)
        .ok_or_else(|| PersonalServiceError::new("stored CN gacha ticket count is invalid"))
}

fn direct_gacha_definition(gacha_id: i64) -> Result<&'static Value, PersonalServiceError> {
    gacha_document()?
        .get(&gacha_id.to_string())
        .ok_or_else(|| PersonalServiceError::new("CN gacha does not exist"))
}

fn gacha_definition(gacha_id: i64) -> Result<&'static Value, PersonalServiceError> {
    direct_gacha_definition(region::resolve(gacha_id)?.canonical)
}

fn gacha_document() -> Result<&'static Value, PersonalServiceError> {
    let document = CN_GACHA_DATA.get_or_init(|| {
        serde_json::from_str::<Value>(CN_GACHA_ASSET)
            .map_err(|error| format!("failed to decode CN gacha asset: {error}"))
    });
    let document = document
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    Ok(document)
}

fn gacha_type(gacha: &Value) -> Result<i64, PersonalServiceError> {
    gacha
        .get("type")
        .and_then(Value::as_i64)
        .filter(|kind| matches!(*kind, CHARACTER_GACHA_TYPE | EQUIPMENT_GACHA_TYPE))
        .ok_or_else(|| PersonalServiceError::new("CN gacha type is invalid"))
}

fn account_first_available(
    root: &Map<String, Value>,
    gacha: &Value,
    gacha_id: i64,
) -> Result<bool, PersonalServiceError> {
    if gacha_page_kind(gacha)? != 1 {
        return Ok(false);
    }
    let Some(list) = root.get("gacha_info_list") else {
        return Ok(true);
    };
    let list = list
        .as_array()
        .ok_or_else(|| PersonalServiceError::new("stored CN gacha info is invalid"))?;
    account_first_available_from_list(list, gacha, gacha_id)
}

fn account_first_available_from_list(
    list: &[Value],
    gacha: &Value,
    gacha_id: i64,
) -> Result<bool, PersonalServiceError> {
    if gacha_page_kind(gacha)? != 1 {
        return Ok(false);
    }
    let Some(info) = list
        .iter()
        .find(|info| info.get("gacha_id").and_then(Value::as_i64) == Some(gacha_id))
    else {
        return Ok(true);
    };
    info.get("is_account_first")
        .and_then(Value::as_bool)
        .ok_or_else(|| PersonalServiceError::new("stored CN account-first gacha state is invalid"))
}

// //// 按活动日历限制 CN 扭蛋操作 [@x380kkm 2026-08-22] ////
fn closed_gacha_response(
    database: &ServiceDatabase,
    _root: &Map<String, Value>,
    resolution: region::ResolvedGachaId,
) -> Result<Option<HttpResponse>, PersonalServiceError> {
    if resolution.is_temporary {
        if database
            .activity_temporary_open_window(&format!("gacha:{}", resolution.canonical))?
            .is_some()
        {
            return Ok(None);
        }
        return Ok(Some(error_response("400 Bad Request", "activity_ended")));
    }
    if resolution.is_coverage || resolution.is_regional {
        let is_selected_bridge = region::coverage_aliases(database.current_server_time_millis()?)?
            .iter()
            .any(|bridge| bridge.requested == resolution.requested);
        if is_selected_bridge {
            return Ok(None);
        }
        if resolution.is_coverage {
            return Ok(Some(error_response("400 Bad Request", "activity_ended")));
        }
    }
    let status = player_gacha_activity_status(database, resolution.canonical)?;
    let code = match status {
        ActivityWindowStatus::Unscheduled | ActivityWindowStatus::Open => return Ok(None),
        ActivityWindowStatus::Disabled => "activity_disabled",
        ActivityWindowStatus::NotStarted => "activity_not_started",
        ActivityWindowStatus::Ended => "activity_ended",
    };
    Ok(Some(error_response("400 Bad Request", code)))
}
// //// /按活动日历限制 CN 扭蛋操作 ////

// //// 返回显式 DB 卡池规则状态 [@x380kkm 2026-08-24] ////
fn player_gacha_activity_status(
    database: &ServiceDatabase,
    gacha_id: i64,
) -> Result<ActivityWindowStatus, PersonalServiceError> {
    let activity_id = format!("gacha:{gacha_id}");
    let now_ms = database.current_server_time_millis()?;
    Ok(database.activity_window_status(&activity_id, now_ms)?)
}
// //// /返回显式 DB 卡池规则状态 ////

// //// 返回 CN 扭蛋当前活动状态 [@x380kkm 2026-08-24] ////
fn gacha_activity_status(
    database: &ServiceDatabase,
    gacha_id: i64,
) -> Result<ActivityWindowStatus, PersonalServiceError> {
    let activity_id = format!("gacha:{gacha_id}");
    let now_ms = database.current_server_time_millis()?;
    let status = database.activity_window_status(&activity_id, now_ms)?;
    Ok(if status == ActivityWindowStatus::Unscheduled {
        gacha_master_window_status(gacha_id, now_ms)?
    } else {
        status
    })
}
// //// /返回 CN 扭蛋当前活动状态 ////

// //// 按客户端卡池 master 判断默认开放时间 [@x380kkm 2026-08-24] ////
fn gacha_master_window_status(
    gacha_id: i64,
    now_ms: i64,
) -> Result<ActivityWindowStatus, PersonalServiceError> {
    let resolution = region::resolve(gacha_id)?;
    gacha_master_window_status_from_definition(
        direct_gacha_definition(resolution.canonical)?,
        now_ms,
    )
}

fn gacha_master_window_status_from_definition(
    gacha: &Value,
    now_ms: i64,
) -> Result<ActivityWindowStatus, PersonalServiceError> {
    let (start_at_ms, end_at_ms) = gacha_master_times(gacha)?;
    Ok(if now_ms < start_at_ms {
        ActivityWindowStatus::NotStarted
    } else if now_ms >= end_at_ms {
        ActivityWindowStatus::Ended
    } else {
        ActivityWindowStatus::Open
    })
}

fn gacha_master_times(gacha: &Value) -> Result<(i64, i64), PersonalServiceError> {
    let start_at_ms = gacha
        .get("startAtMs")
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new("CN gacha start time is missing"))?;
    let end_at_ms = gacha
        .get("endAtMs")
        .and_then(Value::as_i64)
        .filter(|end_at_ms| *end_at_ms > start_at_ms)
        .ok_or_else(|| PersonalServiceError::new("CN gacha end time is invalid"))?;
    Ok((start_at_ms, end_at_ms))
}
// //// /按客户端卡池 master 判断默认开放时间 ////

// //// 判断当前抽取序号是否触发十连保底 [@x380kkm 2026-08-24] ////
fn is_ten_pull_guaranteed_draw(draw_index: i64) -> bool {
    draw_index % 10 == 9
}
// //// /判断当前抽取序号是否触发十连保底 ////

// //// 按卡池概率选择角色或装备星级 [@x380kkm 2026-08-24] ////
fn draw_rank(
    gacha: &Value,
    draw_index: i64,
    guarantee_rank_four_or_higher: bool,
) -> Result<usize, PersonalServiceError> {
    let is_guaranteed = guarantee_rank_four_or_higher || is_ten_pull_guaranteed_draw(draw_index);
    let rate_name = if is_guaranteed {
        "multiGuarantee"
    } else {
        "normal"
    };
    let rates = gacha
        .get("rankRates")
        .and_then(|rates| rates.get(rate_name))
        .and_then(Value::as_array)
        .ok_or_else(|| PersonalServiceError::new("CN gacha rank rates are missing"))?;
    let has_valid_rank_count = if is_guaranteed {
        matches!(rates.len(), 1 | 2)
    } else {
        rates.len() == 3
    };
    if !has_valid_rank_count {
        return Err(PersonalServiceError::new(
            "CN gacha rank rates have an invalid length",
        ));
    }
    let weights = rates
        .iter()
        .map(Value::as_f64)
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| PersonalServiceError::new("CN gacha rank rate is invalid"))?;
    Ok(select_weighted_index(&weights)? + 1)
}
// //// /按卡池概率选择角色或装备星级 ////

fn draw_character_id(
    gacha: &Value,
    draw_number: i64,
    guarantee_rank_four_or_higher: bool,
) -> Result<i64, PersonalServiceError> {
    let rank = draw_rank(gacha, draw_number, guarantee_rank_four_or_higher)?;
    let pool = gacha
        .get("pool")
        .and_then(Value::as_object)
        .and_then(|pool| pool.get(&rank.to_string()))
        .and_then(Value::as_array)
        .ok_or_else(|| PersonalServiceError::new("CN gacha character pool is missing"))?;
    let item_weights = pool
        .iter()
        .map(|item| item.get("rarity").and_then(Value::as_f64).unwrap_or(0.0))
        .collect::<Vec<_>>();
    let item = pool
        .get(select_weighted_index(&item_weights)?)
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("CN gacha pool item is invalid"))?;
    item.get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new("CN gacha character id is missing"))
}

// //// 从 CN 装备扭蛋池抽取装备 [@x380kkm 2026-08-22] ////
fn draw_equipment_id(gacha: &Value, draw_number: i64) -> Result<i64, PersonalServiceError> {
    let rank = draw_rank(gacha, draw_number, false)?;
    let pool = gacha
        .get("pool")
        .and_then(Value::as_object)
        .and_then(|pool| pool.get(&rank.to_string()))
        .and_then(Value::as_array)
        .ok_or_else(|| PersonalServiceError::new("CN gacha equipment pool is missing"))?;
    let item_weights = pool
        .iter()
        .map(|item| item.get("rarity").and_then(Value::as_f64).unwrap_or(0.0))
        .collect::<Vec<_>>();
    let item = pool
        .get(select_weighted_index(&item_weights)?)
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("CN gacha equipment pool item is invalid"))?;
    item.get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new("CN gacha equipment id is missing"))
}
// //// /从 CN 装备扭蛋池抽取装备 ////

fn is_character_in_gacha(gacha: &Value, character_id: i64) -> bool {
    gacha
        .get("pool")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|pool| pool.values())
        .filter_map(Value::as_array)
        .flatten()
        .any(|item| item.get("id").and_then(Value::as_i64) == Some(character_id))
}

fn select_weighted_index(weights: &[f64]) -> Result<usize, PersonalServiceError> {
    let total = weights
        .iter()
        .copied()
        .filter(|weight| weight.is_finite() && *weight > 0.0)
        .sum::<f64>();
    if total <= 0.0 || !total.is_finite() {
        return Err(PersonalServiceError::new("CN gacha weights are invalid"));
    }
    let mut bytes = [0_u8; 8];
    getrandom(&mut bytes)
        .map_err(|error| PersonalServiceError::new(format!("failed to draw CN gacha: {error}")))?;
    let roll = (u64::from_le_bytes(bytes) as f64 / (u64::MAX as f64 + 1.0)) * total;
    let mut offset = 0.0;
    for (index, weight) in weights.iter().enumerate() {
        if weight.is_finite() && *weight > 0.0 {
            offset += *weight;
            if roll < offset {
                return Ok(index);
            }
        }
    }
    weights
        .iter()
        .rposition(|weight| weight.is_finite() && *weight > 0.0)
        .ok_or_else(|| PersonalServiceError::new("CN gacha weights are empty"))
}

fn apply_duplicate_item_to_draw(
    draw: &mut Value,
    item_list: &mut Map<String, Value>,
    duplicate_item: Option<DuplicateCharacterItem>,
) -> Result<(), PersonalServiceError> {
    let Some(duplicate_item) = duplicate_item else {
        return Ok(());
    };
    draw.as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("CN gacha draw data is invalid"))?
        .insert(
            "ex_boost_item".to_owned(),
            json!({"id": duplicate_item.id, "count": duplicate_item.count}),
        );
    item_list.insert(
        duplicate_item.id.to_string(),
        Value::from(duplicate_item.total),
    );
    Ok(())
}

// //// 合并同一响应内重复角色的最新状态 [@x380kkm 2026-08-26] ////
fn merge_character_response(
    characters_by_id: &mut Map<String, Value>,
    character_id: i64,
    character: Value,
) -> Result<(), PersonalServiceError> {
    let key = character_id.to_string();
    let Some(existing) = characters_by_id.get_mut(&key) else {
        characters_by_id.insert(key, character);
        return Ok(());
    };
    let existing_object = existing
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("CN gacha character response is invalid"))?;
    let character_object = character
        .as_object()
        .ok_or_else(|| PersonalServiceError::new("CN gacha character response is invalid"))?;
    for (field, value) in character_object {
        existing_object.insert(field.clone(), value.clone());
    }
    Ok(())
}

fn duplicate_item_list(duplicate_item: Option<DuplicateCharacterItem>) -> Value {
    let Some(duplicate_item) = duplicate_item else {
        return Value::Array(Vec::new());
    };
    Value::Object(Map::from_iter([(
        duplicate_item.id.to_string(),
        Value::from(duplicate_item.total),
    )]))
}

// //// 构造新角色对应的百科增量 [@x380kkm 2026-08-28] ////
pub(crate) fn record_character_encyclopedia_state(
    root: &mut Map<String, Value>,
    character_id: i64,
) -> Result<bool, PersonalServiceError> {
    let key = format!("1{character_id}01");
    let stored = root
        .entry("encyclopedia_list".to_owned())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN encyclopedia data is invalid"))?;
    if stored.contains_key(&key) {
        return Ok(false);
    }
    let value = json!({"read": false});
    stored.insert(key, value);
    Ok(true)
}
// //// /构造新角色对应的百科增量 ////

fn add_equipment(
    root: &mut Map<String, Value>,
    equipment_id: i64,
) -> Result<Value, PersonalServiceError> {
    let equipment_list = require_object(root, "user_equipment_list")?;
    let key = equipment_id.to_string();
    let was_owned = equipment_list.contains_key(&key);
    let mut equipment = equipment_list.get(&key).cloned().unwrap_or_else(|| {
        json!({
            "enhancement_level": 0,
            "level": 1,
            "protection": false,
            "stack": 0,
        })
    });
    let (stack, protection, level, enhancement_level) = {
        let object = equipment
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN equipment data is invalid"))?;
        let current_stack = object
            .get("stack")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let stack = if was_owned {
            current_stack
                .checked_add(1)
                .ok_or_else(|| PersonalServiceError::new("CN equipment stack exceeds range"))?
        } else {
            current_stack
        };
        object.insert("stack".to_owned(), Value::from(stack));
        (
            stack,
            object
                .get("protection")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            object.get("level").and_then(Value::as_i64).unwrap_or(1),
            object
                .get("enhancement_level")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
        )
    };
    equipment_list.insert(key, equipment);
    Ok(json!({
        "equipment_id": equipment_id,
        "protection": protection,
        "level": level,
        "enhancement_level": enhancement_level,
        "stack": stack,
    }))
}

fn update_gacha_info(
    root: &mut Map<String, Value>,
    gacha_id: i64,
    pull_count: i64,
    availability: GachaAvailability,
    count_increment: GachaCountIncrement,
) -> Result<Value, PersonalServiceError> {
    adjust_gacha_info(root, gacha_id, pull_count, availability, count_increment)
}

fn gacha_exchange_points(
    root: &Map<String, Value>,
    gacha_id: i64,
) -> Result<Option<i64>, PersonalServiceError> {
    let Some(value) = root.get("gacha_info_list") else {
        return Ok(None);
    };
    let list = value
        .as_array()
        .ok_or_else(|| PersonalServiceError::new("stored CN gacha info is invalid"))?;
    let Some(info) = list.iter().find(|value| {
        value
            .get("gacha_id")
            .and_then(Value::as_i64)
            .is_some_and(|id| id == gacha_id)
    }) else {
        return Ok(None);
    };
    let object = info
        .as_object()
        .ok_or_else(|| PersonalServiceError::new("stored CN gacha info is invalid"))?;
    Ok(Some(
        object
            .get("gacha_exchange_point")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
    ))
}

fn adjust_gacha_info(
    root: &mut Map<String, Value>,
    gacha_id: i64,
    point_delta: i64,
    availability: GachaAvailability,
    count_increment: GachaCountIncrement,
) -> Result<Value, PersonalServiceError> {
    let list = root
        .entry("gacha_info_list".to_owned())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN gacha info is invalid"))?;
    if let Some(existing) = list.iter_mut().find(|value| {
        value
            .get("gacha_id")
            .and_then(Value::as_i64)
            .is_some_and(|id| id == gacha_id)
    }) {
        let object = existing
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN gacha info is invalid"))?;
        let current = object
            .get("gacha_exchange_point")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let updated = current
            .checked_add(point_delta)
            .ok_or_else(|| PersonalServiceError::new("CN gacha exchange points overflow"))?;
        object.insert(
            "is_account_first".to_owned(),
            Value::Bool(availability.is_account_first),
        );
        object.insert(
            "is_daily_first".to_owned(),
            Value::Bool(availability.is_daily_first),
        );
        apply_gacha_count_increment(object, count_increment)?;
        object.insert("gacha_exchange_point".to_owned(), Value::from(updated));
        return Ok(Value::Object(object.clone()));
    }
    let info = json!({
        "gacha_id": gacha_id,
        "is_account_first": availability.is_account_first,
        "is_daily_first": availability.is_daily_first,
        "gacha_exchange_point": point_delta,
        "daily_one_count": count_increment.daily_one,
        "daily_ten_count": count_increment.daily_ten,
        "crazy_draw_count": count_increment.crazy_draw,
    });
    list.push(info.clone());
    Ok(info)
}

// //// 更新客户端显示的卡池抽取次数 [@x380kkm 2026-08-24] ////
fn gacha_count_increment(body: &ExecuteRequest) -> GachaCountIncrement {
    match (body.payment_type, body.r#type) {
        (FREE_VMONEY_PAYMENT, SINGLE_DRAW) => GachaCountIncrement {
            daily_one: body.number_of_exec,
            ..GachaCountIncrement::default()
        },
        (FREE_VMONEY_PAYMENT, MULTI_DRAW) => GachaCountIncrement {
            daily_ten: body.number_of_exec,
            ..GachaCountIncrement::default()
        },
        (TICKET_PAYMENT, CRAZY_MULTI_TICKET_DRAW) => GachaCountIncrement {
            crazy_draw: body.number_of_exec,
            ..GachaCountIncrement::default()
        },
        _ => GachaCountIncrement::default(),
    }
}

fn apply_gacha_count_increment(
    info: &mut Map<String, Value>,
    increment: GachaCountIncrement,
) -> Result<(), PersonalServiceError> {
    for (field, delta) in [
        ("daily_one_count", increment.daily_one),
        ("daily_ten_count", increment.daily_ten),
        ("crazy_draw_count", increment.crazy_draw),
    ] {
        let current = match info.get(field) {
            Some(value) => value.as_i64().ok_or_else(|| {
                PersonalServiceError::new("stored CN gacha draw count is invalid")
            })?,
            None => 0,
        };
        let updated = current
            .checked_add(delta)
            .ok_or_else(|| PersonalServiceError::new("CN gacha draw count exceeds range"))?;
        info.insert(field.to_owned(), Value::from(updated));
    }
    Ok(())
}
// //// /更新客户端显示的卡池抽取次数 ////

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // //// 验证重复角色扭蛋同时返回素材并持久化余额 [@x380kkm 2026-08-23] ////
    #[test]
    fn adds_duplicate_character_item_to_gacha_draw() {
        let mut root = Map::from_iter([
            ("user_character_list".to_owned(), json!({})),
            ("item_list".to_owned(), json!({})),
        ]);
        let first = grant_character(&mut root, 7, 111_001, 100).unwrap();
        assert!(first.joined);
        let duplicate = grant_character(&mut root, 7, 111_001, 200).unwrap();
        let mut draw = json!({"character_id": 111_001});
        let mut item_list = Map::new();

        apply_duplicate_item_to_draw(&mut draw, &mut item_list, duplicate.duplicate_item).unwrap();

        assert_eq!(root["user_character_list"]["111001"]["entry_count"], 1);
        assert_eq!(root["user_character_list"]["111001"]["stack"], 1);
        assert_eq!(root["item_list"]["14003"], 1);
        assert_eq!(draw["ex_boost_item"], json!({"id": 14_003, "count": 1}));
        assert_eq!(item_list["14003"], 1);
    }
    // //// /验证重复角色扭蛋同时返回素材并持久化余额 ////

    // //// 验证重复角色状态合并保留首次建档字段 [@x380kkm 2026-08-26] ////
    #[test]
    fn merges_partial_duplicate_character_response() {
        let mut characters = Map::from_iter([(
            "111001".to_owned(),
            json!({
                "character_id": 111_001,
                "entry_count": 1,
                "exp": 0,
                "bond_token_list": [],
                "mana_board_index": 1,
                "stack": 0,
            }),
        )]);

        merge_character_response(
            &mut characters,
            111_001,
            json!({"character_id": 111_001, "stack": 2}),
        )
        .unwrap();

        assert_eq!(characters["111001"]["entry_count"], 1);
        assert_eq!(characters["111001"]["mana_board_index"], 1);
        assert_eq!(characters["111001"]["stack"], 2);
    }
    // //// /验证重复角色状态合并保留首次建档字段 ////
}
