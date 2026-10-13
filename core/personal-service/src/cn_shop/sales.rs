// audience: internal
// # personal-service-cn-shop-sales
//
// 该模块按玩家库存、虚拟时间和活动日历生成 CN 商店销售目录.

use super::activity::event_shop_activity_state;
use super::catalog::{
    boss_coin_shop_catalog, boss_coin_shop_document, event_activity_id, event_shop_catalog,
    event_shop_document, shop_catalog, shop_item_is_active, shop_item_is_in_client_master,
    SHOP_TYPE_BOSS_COIN, SHOP_TYPE_EQUIPMENT_ENHANCEMENT, SHOP_TYPE_EVENT, SHOP_TYPE_GENERAL,
    SHOP_TYPE_SPECIAL_PACK, SHOP_TYPE_STAR_GRAIN, SHOP_TYPE_TREASURE,
};
use super::enhancement::equipment_enhancement_sale_entries;
use super::error_response;
use super::period;
use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{decode_player_data, encode_player_data, player_snapshot};
use crate::database::{ActivityWindowStatus, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Default)]
pub(crate) struct ShopSources {
    pub(crate) available: Vec<Value>,
    pub(crate) unselected: Vec<Value>,
}

#[derive(Deserialize)]
struct SalesListRequest {
    viewer_id: i64,
    #[serde(flatten)]
    query: SalesQuery,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct SalesQuery {
    shop_types: Vec<i64>,
    boss_coin_shop_category_ids: Vec<i64>,
    #[serde(default)]
    equipment_enhancement_shop_category_ids: Vec<i64>,
    event_list: Vec<EventShopRequest>,
}

#[derive(Deserialize)]
struct EventShopRequest {
    event_type: i64,
    event_ids: Vec<i64>,
}

// //// 返回当前账号可浏览的 CN 商店目录 [@x380kkm 2026-08-22] ////
pub(super) fn get_sales_list(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<SalesListRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let response_time = server_time(database)?;
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = player_data
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN player data is not an object"))?;
    let (sales_list, state_changed) =
        prepare_sales_list(root, database, asset_root, &body.query, response_time)?;
    if state_changed {
        database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    }
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        json!({"sales_list": sales_list}),
    )
}
// //// /返回当前账号可浏览的 CN 商店目录 ////

// //// 从同一玩家快照投影请求的商店销售状态 [@x380kkm 2026-09-23] ////
pub(crate) fn prepare_sales_list(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    asset_root: &Path,
    body: &SalesQuery,
    response_time: i64,
) -> Result<(Vec<Value>, bool), PersonalServiceError> {
    let mut sales_list = Vec::new();
    let mut seen = BTreeSet::new();
    let mut state_changed = false;
    let mut equipment_enhancement_groups: BTreeMap<(i64, i64), BTreeMap<i64, &Map<String, Value>>> =
        BTreeMap::new();

    for &shop_type in &body.shop_types {
        let Some(catalog) = shop_catalog(shop_type)? else {
            continue;
        };
        for (shop_item_id, item) in catalog {
            let (Ok(shop_item_id), Some(item)) = (shop_item_id.parse::<i64>(), item.as_object())
            else {
                continue;
            };
            if !shop_item_is_in_client_master(shop_type, shop_item_id)? {
                continue;
            }
            if !shop_item_is_active(item, response_time) {
                continue;
            }
            let grants_pass_points = item.get("rewards").and_then(Value::as_array)
                .is_some_and(|rewards| rewards.iter().any(|reward| reward["type"] == 6));
            if grants_pass_points
                && crate::cn_pass_card::point_purchase_target(root, database, response_time)?.is_none()
            {
                continue;
            }
            if !seen.insert((shop_type, shop_item_id)) {
                continue;
            }
            if shop_type == SHOP_TYPE_EQUIPMENT_ENHANCEMENT
                && !body.equipment_enhancement_shop_category_ids.is_empty()
                && !item
                    .get("shopCategoryId")
                    .and_then(Value::as_i64)
                    .is_some_and(|category_id| {
                        body.equipment_enhancement_shop_category_ids
                            .contains(&category_id)
                    })
            {
                continue;
            }
            if shop_type == SHOP_TYPE_EQUIPMENT_ENHANCEMENT {
                let group_id = item
                    .get("groupId")
                    .and_then(Value::as_i64)
                    .unwrap_or_default();
                let category_id = item
                    .get("shopCategoryId")
                    .and_then(Value::as_i64)
                    .unwrap_or_default();
                equipment_enhancement_groups
                    .entry((category_id, group_id))
                    .or_default()
                    .insert(shop_item_id, item);
                continue;
            }
            let (sale, changed) = shop_sale_entry(
                root,
                database,
                asset_root,
                response_time,
                shop_type,
                shop_item_id,
            )?;
            state_changed |= changed;
            sales_list.push(sale);
        }
    }
    let boss_categories = if body.shop_types.contains(&SHOP_TYPE_BOSS_COIN)
        && body.boss_coin_shop_category_ids.is_empty()
    {
        boss_coin_shop_document()?
            .keys()
            .filter_map(|id| id.parse::<i64>().ok())
            .collect::<Vec<_>>()
    } else {
        body.boss_coin_shop_category_ids.clone()
    };
    for category_id in boss_categories {
        let Some(catalog) = boss_coin_shop_catalog(category_id)? else {
            continue;
        };
        for (shop_item_id, item) in catalog {
            let (Ok(shop_item_id), Some(item)) = (shop_item_id.parse::<i64>(), item.as_object())
            else {
                continue;
            };
            let availability = super::selection::availability(
                root,
                database,
                SHOP_TYPE_BOSS_COIN,
                shop_item_id,
                response_time,
            )?;
            if !matches!(
                availability,
                super::selection::Availability::Unavailable
                    | super::selection::Availability::Unselected
            ) && shop_item_is_in_client_master(SHOP_TYPE_BOSS_COIN, shop_item_id)?
                && (availability == super::selection::Availability::Available
                    || shop_item_is_active(item, response_time))
            {
                if !seen.insert((SHOP_TYPE_BOSS_COIN, shop_item_id)) {
                    continue;
                }
                let (sale, changed) = shop_sale_entry(
                    root,
                    database,
                    asset_root,
                    response_time,
                    SHOP_TYPE_BOSS_COIN,
                    shop_item_id,
                )?;
                state_changed |= changed;
                sales_list.push(sale);
            }
        }
    }
    for event in &body.event_list {
        for &event_id in &event.event_ids {
            let Some(activity_id) = event_activity_id(event.event_type, event_id) else {
                continue;
            };
            let activity_state = event_shop_activity_state(database, &activity_id)?;
            if activity_state.status != ActivityWindowStatus::Open {
                continue;
            }
            let Some(catalog) = event_shop_catalog(event.event_type, event_id)? else {
                continue;
            };
            for (shop_item_id, item) in catalog {
                let (Ok(shop_item_id), Some(item)) =
                    (shop_item_id.parse::<i64>(), item.as_object())
                else {
                    continue;
                };
                let availability = super::selection::availability(
                    root,
                    database,
                    SHOP_TYPE_EVENT,
                    shop_item_id,
                    response_time,
                )?;
                if !matches!(
                    availability,
                    super::selection::Availability::Unavailable
                        | super::selection::Availability::Unselected
                ) && shop_item_is_in_client_master(SHOP_TYPE_EVENT, shop_item_id)?
                    && (availability == super::selection::Availability::Available
                        || activity_state.item_is_active(shop_item_id, item, response_time))
                {
                    if !seen.insert((SHOP_TYPE_EVENT, shop_item_id)) {
                        continue;
                    }
                    let (sale, changed) = shop_sale_entry(
                        root,
                        database,
                        asset_root,
                        response_time,
                        SHOP_TYPE_EVENT,
                        shop_item_id,
                    )?;
                    state_changed |= changed;
                    sales_list.push(sale);
                }
            }
        }
    }
    sales_list.extend(equipment_enhancement_sale_entries(
        root,
        equipment_enhancement_groups,
    ));
    Ok((sales_list, state_changed))
}
// //// /从同一玩家快照投影请求的商店销售状态 ////

pub(super) fn shop_sale_entry(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    asset_root: &Path,
    response_time: i64,
    shop_type: i64,
    shop_item_id: i64,
) -> Result<(Value, bool), PersonalServiceError> {
    let state = period::synchronize_purchase_state(
        root,
        database,
        asset_root,
        shop_type,
        shop_item_id,
        response_time,
    )?;
    Ok((
        sales_list_sale_value(shop_type, shop_item_id, state),
        state.changed,
    ))
}

fn projected_shop_sale_entry(
    root: &Map<String, Value>,
    database: &ServiceDatabase,
    asset_root: &Path,
    response_time: i64,
    shop_type: i64,
    shop_item_id: i64,
) -> Result<Value, PersonalServiceError> {
    let state = period::purchase_state(
        root,
        database,
        asset_root,
        shop_type,
        shop_item_id,
        response_time,
    )?;
    Ok(how_to_get_sale_value(shop_type, shop_item_id, state))
}

// //// 返回商店目录商品状态 [@x380kkm 2026-08-29] ////
fn sales_list_sale_value(
    shop_type: i64,
    shop_item_id: i64,
    state: period::ShopPurchaseState,
) -> Value {
    sale_value(
        shop_type,
        shop_item_id,
        state,
        json!({
            "group_total_stock_quantity": state.stock_quantity,
            "group_total_purchase_num": state.total_purchase_num,
            "multi_stage": false,
            "other_group_items": null,
        }),
    )
}
// //// /返回商店目录商品状态 ////

// //// 返回获取途径中的商品状态 [@x380kkm 2026-08-29] ////
fn how_to_get_sale_value(
    shop_type: i64,
    shop_item_id: i64,
    state: period::ShopPurchaseState,
) -> Value {
    sale_value(
        shop_type,
        shop_item_id,
        state,
        json!({
            "group_total_stock_quantity": state.stock_quantity,
            "group_total_purchase_num": state.total_purchase_num,
            "multi_stage": false,
            "other_group_items": null,
        }),
    )
}
// //// /返回获取途径中的商品状态 ////

// //// 生成客户端商店商品字段 [@x380kkm 2026-08-29] ////
fn sale_value(
    shop_type: i64,
    shop_item_id: i64,
    state: period::ShopPurchaseState,
    group_info: Value,
) -> Value {
    json!({
        "shop_item_id": shop_item_id,
        "stock_quantity": state.stock_quantity,
        "today_purchase_num": state.today_purchase_num,
        "this_month_purchase_num": state.this_month_purchase_num,
        "total_purchase_num": state.total_purchase_num,
        "discount_id": null,
        "discount_rate": null,
        "discounted_price": null,
        "group_info": group_info,
        "shop_type": shop_type,
    })
}
// //// /生成客户端商店商品字段 ////

// //// 查询出售指定奖励的 CN 商店商品 [@x380kkm 2026-08-24] ////
pub(super) fn how_to_get_sales(
    root: &Map<String, Value>,
    item_id: Option<i64>,
    equipment_id: Option<i64>,
    _now: &str,
    database: &ServiceDatabase,
    asset_root: &Path,
) -> Result<ShopSources, PersonalServiceError> {
    let response_time = crate::cn::server_time(database)?;
    let mut sales = ShopSources::default();
    for shop_type in [
        SHOP_TYPE_TREASURE,
        SHOP_TYPE_SPECIAL_PACK,
        SHOP_TYPE_GENERAL,
        SHOP_TYPE_STAR_GRAIN,
        SHOP_TYPE_EQUIPMENT_ENHANCEMENT,
    ] {
        if let Some(catalog) = shop_catalog(shop_type)? {
            append_matching_sales(
                &mut sales,
                root,
                database,
                asset_root,
                response_time,
                shop_type,
                catalog,
                item_id,
                equipment_id,
                None,
            )?;
        }
    }
    for catalog in boss_coin_shop_document()?
        .values()
        .filter_map(Value::as_object)
    {
        append_matching_sales(
            &mut sales,
            root,
            database,
            asset_root,
            response_time,
            SHOP_TYPE_BOSS_COIN,
            catalog,
            item_id,
            equipment_id,
            None,
        )?;
    }
    for (event_type, events) in event_shop_document()? {
        let (Ok(event_type), Some(events)) = (event_type.parse::<i64>(), events.as_object()) else {
            continue;
        };
        for (event_id, catalog) in events {
            let (Ok(event_id), Some(catalog)) = (event_id.parse::<i64>(), catalog.as_object())
            else {
                continue;
            };
            let Some(activity_id) = event_activity_id(event_type, event_id) else {
                continue;
            };
            let activity_state = event_shop_activity_state(database, &activity_id)?;
            if activity_state.status != ActivityWindowStatus::Open {
                continue;
            }
            append_matching_sales(
                &mut sales,
                root,
                database,
                asset_root,
                response_time,
                SHOP_TYPE_EVENT,
                catalog,
                item_id,
                equipment_id,
                Some(&activity_state),
            )?;
        }
    }
    for entries in [&mut sales.available, &mut sales.unselected] {
        entries.sort_by_key(|sale| {
            (
                sale.get("shop_type")
                    .and_then(Value::as_i64)
                    .unwrap_or_default(),
                sale.get("shop_item_id")
                    .and_then(Value::as_i64)
                    .unwrap_or_default(),
            )
        });
        entries.dedup_by(|left, right| {
            left.get("shop_type") == right.get("shop_type")
                && left.get("shop_item_id") == right.get("shop_item_id")
        });
    }
    Ok(sales)
}

// //// 将匹配奖励的可用商品加入来源列表 [@x380kkm 2026-08-24] ////
fn append_matching_sales(
    sales: &mut ShopSources,
    root: &Map<String, Value>,
    database: &ServiceDatabase,
    asset_root: &Path,
    response_time: i64,
    shop_type: i64,
    catalog: &Map<String, Value>,
    item_id: Option<i64>,
    equipment_id: Option<i64>,
    event_shop_state: Option<&super::activity::EventShopActivityState>,
) -> Result<(), PersonalServiceError> {
    for (shop_item_id, item) in catalog {
        let (Ok(shop_item_id), Some(item)) = (shop_item_id.parse::<i64>(), item.as_object()) else {
            continue;
        };
        let availability =
            super::selection::availability(root, database, shop_type, shop_item_id, response_time)?;
        if !shop_item_is_in_client_master(shop_type, shop_item_id)?
            || availability == super::selection::Availability::Unavailable
            || (availability == super::selection::Availability::Ordinary
                && !event_shop_state.map_or_else(
                    || shop_item_is_active(item, response_time),
                    |state| state.item_is_active(shop_item_id, item, response_time),
                ))
            || !shop_item_contains_reward(item, item_id, equipment_id)
        {
            continue;
        }
        let entries = if availability == super::selection::Availability::Unselected {
            &mut sales.unselected
        } else {
            &mut sales.available
        };
        entries.push(projected_shop_sale_entry(
            root,
            database,
            asset_root,
            response_time,
            shop_type,
            shop_item_id,
        )?);
    }
    Ok(())
}
// //// /将匹配奖励的可用商品加入来源列表 ////

// //// 判断商店商品是否包含指定奖励 [@x380kkm 2026-08-24] ////
fn shop_item_contains_reward(
    item: &Map<String, Value>,
    item_id: Option<i64>,
    equipment_id: Option<i64>,
) -> bool {
    if equipment_id.is_some_and(|equipment_id| {
        item.get("equipmentId").and_then(Value::as_i64) == Some(equipment_id)
    }) {
        return true;
    }
    item.get("rewards")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|reward| match (item_id, equipment_id) {
            (Some(item_id), None) => {
                reward.get("type").and_then(Value::as_i64) == Some(0)
                    && reward.get("id").and_then(Value::as_i64) == Some(item_id)
            }
            (None, Some(equipment_id)) => {
                reward.get("type").and_then(Value::as_i64) == Some(4)
                    && reward.get("id").and_then(Value::as_i64) == Some(equipment_id)
            }
            _ => false,
        })
}
// //// /判断商店商品是否包含指定奖励 ////
// //// /查询出售指定奖励的 CN 商店商品 ////
