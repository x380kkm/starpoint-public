// audience: internal
// # personal-service-cn-shop-catalog
//
// 该模块加载 CN 商店静态目录并按虚拟时间筛选可用商品.

use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

const TREASURE_SHOP_ASSET: &str = include_str!("../../../../assets/treasure_shop.json");
const GENERAL_SHOP_ASSET: &str = include_str!("../../../../assets/general_shop.json");
const STAR_GRAIN_SHOP_ASSET: &str = include_str!("../../../../assets/star_grain_shop.json");
const EQUIPMENT_ENHANCEMENT_SHOP_ASSET: &str =
    include_str!("../../../../assets/equipment_enhancement_shop.json");
const EVENT_SHOP_ASSET: &str = include_str!("../../../../assets/event_item_shop.json");
const EVENT_SHOP_ID_MAP_ASSET: &str =
    include_str!("../../../../assets/event_item_shop_id_map.json");
const BOSS_COIN_SHOP_ASSET: &str = include_str!("../../../../assets/boss_coin_shop.json");
const BOSS_COIN_SHOP_ITEM_CATEGORY_MAP_ASSET: &str =
    include_str!("../../../../assets/boss_coin_shop_item_category_map.json");
const SHOP_MASTER_WHITELISTS_ASSET: &str =
    include_str!("../../../../assets/cdn_shop_master_whitelists.json");
const SHOP_LIMITS_ASSET: &str = include_str!("../../../../assets/cn-shop-limits.json");
pub(super) const SHOP_TYPE_TREASURE: i64 = 2;
pub(super) const SHOP_TYPE_EVENT: i64 = 4;
pub(super) const SHOP_TYPE_BOSS_COIN: i64 = 7;
pub(super) const SHOP_TYPE_GENERAL: i64 = 8;
pub(super) const SHOP_TYPE_STAR_GRAIN: i64 = 9;
pub(super) const SHOP_TYPE_EQUIPMENT_ENHANCEMENT: i64 = 10;
static TREASURE_SHOP: OnceLock<Result<Value, String>> = OnceLock::new();
static GENERAL_SHOP: OnceLock<Result<Value, String>> = OnceLock::new();
static STAR_GRAIN_SHOP: OnceLock<Result<Value, String>> = OnceLock::new();
static EQUIPMENT_ENHANCEMENT_SHOP: OnceLock<Result<Value, String>> = OnceLock::new();
static EVENT_SHOP: OnceLock<Result<Value, String>> = OnceLock::new();
static EVENT_SHOP_ID_MAP: OnceLock<Result<Value, String>> = OnceLock::new();
static BOSS_COIN_SHOP: OnceLock<Result<Value, String>> = OnceLock::new();
static BOSS_COIN_SHOP_ITEM_CATEGORY_MAP: OnceLock<Result<Value, String>> = OnceLock::new();
static SHOP_MASTER_WHITELISTS: OnceLock<Result<BTreeMap<String, BTreeSet<i64>>, String>> =
    OnceLock::new();
static SHOP_LIMITS: OnceLock<
    Result<BTreeMap<String, BTreeMap<String, ShopPurchaseLimits>>, String>,
> = OnceLock::new();

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
pub(super) struct ShopPurchaseLimits {
    #[serde(rename = "buyMaxCount")]
    pub(super) buy_max_count: Option<i64>,
    #[serde(rename = "maxFrequency")]
    pub(super) max_frequency: Option<i64>,
    #[serde(rename = "dailyStock")]
    pub(super) daily_stock: Option<i64>,
    #[serde(rename = "monthlyStock")]
    pub(super) monthly_stock: Option<i64>,
}

impl ShopPurchaseLimits {
    pub(super) fn has_period_limit(self) -> bool {
        self.max_frequency.is_some() || self.daily_stock.is_some() || self.monthly_stock.is_some()
    }
}

// //// 返回指定类型的 CN 商店静态目录 [@x380kkm 2026-08-22] ////
pub(super) fn shop_catalog(
    shop_type: i64,
) -> Result<Option<&'static Map<String, Value>>, PersonalServiceError> {
    let catalog = match shop_type {
        SHOP_TYPE_TREASURE => Some(parse_shop_asset(TREASURE_SHOP_ASSET, &TREASURE_SHOP)?),
        SHOP_TYPE_GENERAL => Some(parse_shop_asset(GENERAL_SHOP_ASSET, &GENERAL_SHOP)?),
        SHOP_TYPE_STAR_GRAIN => Some(parse_shop_asset(STAR_GRAIN_SHOP_ASSET, &STAR_GRAIN_SHOP)?),
        SHOP_TYPE_EQUIPMENT_ENHANCEMENT => Some(parse_shop_asset(
            EQUIPMENT_ENHANCEMENT_SHOP_ASSET,
            &EQUIPMENT_ENHANCEMENT_SHOP,
        )?),
        _ => None,
    };
    Ok(catalog)
}
// //// /返回指定类型的 CN 商店静态目录 ////

// //// 查询客户端 master 中存在的商店商品 [@x380kkm 2026-08-23] ////
pub(super) fn shop_item_is_in_client_master(
    shop_type: i64,
    shop_item_id: i64,
) -> Result<bool, PersonalServiceError> {
    let whitelists = SHOP_MASTER_WHITELISTS
        .get_or_init(|| {
            serde_json::from_str::<BTreeMap<String, Vec<i64>>>(SHOP_MASTER_WHITELISTS_ASSET)
                .map(|whitelists| {
                    whitelists
                        .into_iter()
                        .map(|(shop_type, items)| (shop_type, items.into_iter().collect()))
                        .collect()
                })
                .map_err(|error| format!("failed to decode CN shop master whitelists: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    Ok(whitelists
        .get(&shop_type.to_string())
        .is_some_and(|items| items.contains(&shop_item_id)))
}
// //// /查询客户端 master 中存在的商店商品 ////

pub(super) fn shop_item(
    shop_type: i64,
    shop_item_id: i64,
) -> Result<Option<Map<String, Value>>, PersonalServiceError> {
    if !shop_item_is_in_client_master(shop_type, shop_item_id)? {
        return Ok(None);
    }
    if shop_type == SHOP_TYPE_BOSS_COIN {
        let category_map = parse_shop_asset(
            BOSS_COIN_SHOP_ITEM_CATEGORY_MAP_ASSET,
            &BOSS_COIN_SHOP_ITEM_CATEGORY_MAP,
        )?;
        let Some(category_id) = category_map
            .get(&shop_item_id.to_string())
            .and_then(Value::as_i64)
        else {
            return Ok(None);
        };
        return Ok(boss_coin_shop_catalog(category_id)?
            .and_then(|catalog| catalog.get(&shop_item_id.to_string()))
            .and_then(Value::as_object)
            .cloned());
    }
    if shop_type == SHOP_TYPE_EVENT {
        let Some((event_type, event_id)) = event_shop_identity(shop_item_id)? else {
            return Ok(None);
        };
        return Ok(event_shop_catalog(event_type, event_id)?
            .and_then(|catalog| catalog.get(&shop_item_id.to_string()))
            .and_then(Value::as_object)
            .cloned());
    }
    Ok(shop_catalog(shop_type)?
        .and_then(|catalog| catalog.get(&shop_item_id.to_string()))
        .and_then(Value::as_object)
        .cloned())
}

// //// 读取客户端商店购买限制 ////
pub(super) fn shop_purchase_limits(
    shop_type: i64,
    shop_item_id: i64,
) -> Result<ShopPurchaseLimits, PersonalServiceError> {
    let limits = SHOP_LIMITS
        .get_or_init(|| {
            serde_json::from_str::<BTreeMap<String, BTreeMap<String, ShopPurchaseLimits>>>(
                SHOP_LIMITS_ASSET,
            )
            .map_err(|error| format!("failed to decode CN shop limits: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    Ok(limits
        .get(&shop_type.to_string())
        .and_then(|items| items.get(&shop_item_id.to_string()))
        .copied()
        .unwrap_or_default())
}
// //// /读取客户端商店购买限制 ////

pub(super) fn boss_coin_shop_catalog(
    category_id: i64,
) -> Result<Option<&'static Map<String, Value>>, PersonalServiceError> {
    let document = parse_shop_asset(BOSS_COIN_SHOP_ASSET, &BOSS_COIN_SHOP)?;
    Ok(document
        .get(&category_id.to_string())
        .and_then(Value::as_object))
}

// //// 返回全部 CN 讨伐币商店目录 [@x380kkm 2026-08-24] ////
pub(super) fn boss_coin_shop_document() -> Result<&'static Map<String, Value>, PersonalServiceError>
{
    parse_shop_asset(BOSS_COIN_SHOP_ASSET, &BOSS_COIN_SHOP)
}
// //// /返回全部 CN 讨伐币商店目录 ////

// //// 将复刻活动映射到同系列活动商店目录 [@x380kkm 2026-08-24] ////
pub(super) fn event_shop_catalog(
    event_type: i64,
    event_id: i64,
) -> Result<Option<&'static Map<String, Value>>, PersonalServiceError> {
    let document = parse_shop_asset(EVENT_SHOP_ASSET, &EVENT_SHOP)?;
    let Some(events) = document
        .get(&event_type.to_string())
        .and_then(Value::as_object)
    else {
        return Ok(None);
    };
    if let Some(catalog) = events.get(&event_id.to_string()).and_then(Value::as_object) {
        return Ok(Some(catalog));
    }
    if (700_010..=700_019).contains(&event_id) {
        return Ok(events
            .get(&(event_id - 10).to_string())
            .and_then(Value::as_object));
    }
    Ok(None)
}
// //// /将复刻活动映射到同系列活动商店目录 ////

// //// 返回全部 CN 活动商店目录 [@x380kkm 2026-08-24] ////
pub(super) fn event_shop_document() -> Result<&'static Map<String, Value>, PersonalServiceError> {
    parse_shop_asset(EVENT_SHOP_ASSET, &EVENT_SHOP)
}
// //// /返回全部 CN 活动商店目录 ////

pub(super) fn event_shop_identity(
    shop_item_id: i64,
) -> Result<Option<(i64, i64)>, PersonalServiceError> {
    let document = parse_shop_asset(EVENT_SHOP_ID_MAP_ASSET, &EVENT_SHOP_ID_MAP)?;
    Ok(document
        .get(&shop_item_id.to_string())
        .and_then(Value::as_object)
        .and_then(|entry| {
            Some((
                entry.get("eventType")?.as_i64()?,
                entry.get("eventId")?.as_i64()?,
            ))
        }))
}

pub(super) fn event_activity_id(event_type: i64, event_id: i64) -> Option<String> {
    let prefix = match event_type {
        0 => "advent",
        1 => "ranking",
        2 => "story",
        3 => "daily-week",
        4 => "challenge-dungeon",
        5 => "daily-exp-mana",
        6 => "world-story",
        7 => "tower-dungeon",
        8 => "expert-single",
        9 => "collect-item",
        10 => "carnival",
        11 => "rush",
        12 => "score-attack",
        _ => return None,
    };
    Some(format!("{prefix}:{event_id}"))
}

// //// 返回同一轮突进活动的原版或复刻活动编号 [@x380kkm 2026-08-29] ////
pub(super) fn event_shop_activity_alias(activity_id: &str) -> Option<String> {
    let event_id = activity_id.strip_prefix("rush:")?.parse::<i64>().ok()?;
    let alias_id = match event_id {
        700_000..=700_009 => event_id + 10,
        700_010..=700_019 => event_id - 10,
        _ => return None,
    };
    Some(format!("rush:{alias_id}"))
}
// //// /返回同一轮突进活动的原版或复刻活动编号 ////

pub(super) fn shop_item_is_active(item: &Map<String, Value>, now: &str) -> bool {
    let starts_before_now = item
        .get("availableFrom")
        .and_then(Value::as_str)
        .map_or(true, |starts_at| starts_at <= now);
    let ends_after_now = item
        .get("availableUntil")
        .and_then(Value::as_str)
        .map_or(true, |ends_at| ends_at > now);
    starts_before_now && ends_after_now
}

pub(super) fn shop_purchase_key(shop_type: i64, shop_item_id: i64) -> String {
    format!("{shop_type}:{shop_item_id}")
}

fn parse_shop_asset(
    asset: &'static str,
    cache: &'static OnceLock<Result<Value, String>>,
) -> Result<&'static Map<String, Value>, PersonalServiceError> {
    cache
        .get_or_init(|| {
            serde_json::from_str::<Value>(asset)
                .map_err(|error| format!("failed to decode CN shop asset: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?
        .as_object()
        .ok_or_else(|| PersonalServiceError::new("CN shop asset is not an object"))
}
