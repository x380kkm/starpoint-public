// audience: internal
// # personal-service-cn-shop-sales
//
// 该模块按玩家库存、虚拟时间和活动日历生成 CN 商店销售目录.

use super::catalog::{
    boss_coin_shop_catalog, boss_coin_shop_document, event_activity_id, event_shop_activity_alias,
    event_shop_catalog, event_shop_document, shop_catalog, shop_item_is_active,
    shop_item_is_in_client_master, SHOP_TYPE_BOSS_COIN, SHOP_TYPE_EQUIPMENT_ENHANCEMENT,
    SHOP_TYPE_EVENT, SHOP_TYPE_GENERAL, SHOP_TYPE_STAR_GRAIN, SHOP_TYPE_TREASURE,
};
use super::error_response;
use super::period;
use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, format_client_time, player_snapshot,
};
use crate::database::{evaluate_activity_schedule, ActivityWindowStatus, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Deserialize)]
struct SalesListRequest {
    viewer_id: i64,
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
    let now = format_client_time(response_time);
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = player_data
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN player data is not an object"))?;
    let mut sales_list = Vec::new();
    let mut state_changed = false;
    let mut equipment_enhancement_groups: BTreeMap<i64, BTreeMap<i64, &Map<String, Value>>> =
        BTreeMap::new();

    for shop_type in body.shop_types {
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
            if !shop_item_is_active(item, &now) {
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
                equipment_enhancement_groups
                    .entry(group_id)
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
                item,
            )?;
            state_changed |= changed;
            sales_list.push(sale);
        }
    }
    for category_id in body.boss_coin_shop_category_ids {
        let Some(catalog) = boss_coin_shop_catalog(category_id)? else {
            continue;
        };
        for (shop_item_id, item) in catalog {
            let (Ok(shop_item_id), Some(item)) = (shop_item_id.parse::<i64>(), item.as_object())
            else {
                continue;
            };
            if shop_item_is_in_client_master(SHOP_TYPE_BOSS_COIN, shop_item_id)?
                && shop_item_is_active(item, &now)
            {
                let (sale, changed) = shop_sale_entry(
                    root,
                    database,
                    asset_root,
                    response_time,
                    SHOP_TYPE_BOSS_COIN,
                    shop_item_id,
                    item,
                )?;
                state_changed |= changed;
                sales_list.push(sale);
            }
        }
    }
    for event in body.event_list {
        for event_id in event.event_ids {
            let Some(activity_id) = event_activity_id(event.event_type, event_id) else {
                continue;
            };
            let activity_state = event_shop_activity_state(database, asset_root, &activity_id)?;
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
                if shop_item_is_in_client_master(SHOP_TYPE_EVENT, shop_item_id)?
                    && (activity_state.overrides_item_period || shop_item_is_active(item, &now))
                {
                    let (sale, changed) = shop_sale_entry(
                        root,
                        database,
                        asset_root,
                        response_time,
                        SHOP_TYPE_EVENT,
                        shop_item_id,
                        item,
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

// //// 按当前强化等级选择每组装备的可购买阶段 [@x380kkm 2026-08-24] ////
fn equipment_enhancement_sale_entries<'a>(
    root: &Map<String, Value>,
    groups: BTreeMap<i64, BTreeMap<i64, &'a Map<String, Value>>>,
) -> Vec<Value> {
    let equipment_list = root.get("user_equipment_list").and_then(Value::as_object);
    let mut sales = Vec::with_capacity(groups.len());

    for items in groups.into_values() {
        let mut items = items.into_iter().collect::<Vec<_>>();
        items.sort_by_key(|(shop_item_id, item)| {
            (
                item.get("stage")
                    .and_then(Value::as_i64)
                    .unwrap_or_default(),
                *shop_item_id,
            )
        });
        let Some((_, first_item)) = items.first() else {
            continue;
        };
        let Some(equipment_id) = first_item.get("equipmentId").and_then(Value::as_i64) else {
            continue;
        };
        let enhancement_level = equipment_list
            .and_then(|equipment_list| equipment_list.get(&equipment_id.to_string()))
            .and_then(Value::as_object)
            .map(|equipment| {
                equipment
                    .get("enhancement_level")
                    .and_then(Value::as_i64)
                    .unwrap_or_default()
            })
            .unwrap_or(-1);
        let target_index = if enhancement_level < 0 {
            0
        } else {
            items
                .iter()
                .position(|(_, item)| {
                    item.get("enhancementMaxLevel")
                        .and_then(Value::as_i64)
                        .unwrap_or_default()
                        > enhancement_level
                })
                .unwrap_or(items.len() - 1)
        };
        let (shop_item_id, target_item) = items[target_index];
        let target_level = target_item
            .get("enhancementMaxLevel")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let total_purchase_num = enhancement_level.max(0);
        let stock_quantity = if enhancement_level < 0 {
            target_level
        } else {
            target_level.saturating_sub(enhancement_level).max(0)
        };
        let max_level = items
            .iter()
            .filter_map(|(_, item)| item.get("enhancementMaxLevel").and_then(Value::as_i64))
            .max()
            .unwrap_or_default();
        let other_group_items =
            equipment_enhancement_other_group_items(&items, target_index, enhancement_level.max(0));
        sales.push(json!({
            "shop_item_id": shop_item_id,
            "stock_quantity": stock_quantity,
            "today_purchase_num": 0,
            "this_month_purchase_num": null,
            "total_purchase_num": total_purchase_num,
            "discount_id": null,
            "discount_rate": null,
            "discounted_price": null,
            "group_info": {
                "group_total_stock_quantity": max_level.saturating_sub(total_purchase_num).max(0),
                "group_total_purchase_num": total_purchase_num,
                "multi_stage": items.len() > 1,
                "other_group_items": other_group_items,
            },
            "shop_type": SHOP_TYPE_EQUIPMENT_ENHANCEMENT,
        }));
    }

    sales
}
// //// /按当前强化等级选择每组装备的可购买阶段 ////

// //// 返回装备强化组中其余阶段的可购买状态 [@x380kkm 2026-08-29] ////
fn equipment_enhancement_other_group_items(
    items: &[(i64, &Map<String, Value>)],
    target_index: usize,
    enhancement_level: i64,
) -> Vec<Value> {
    let mut previous_max_level = 0;
    items
        .iter()
        .enumerate()
        .filter_map(|(index, (shop_item_id, item))| {
            let max_level = item
                .get("enhancementMaxLevel")
                .and_then(Value::as_i64)
                .unwrap_or(previous_max_level)
                .max(previous_max_level);
            let stage_capacity = max_level.saturating_sub(previous_max_level);
            let stage_purchase_num = enhancement_level
                .saturating_sub(previous_max_level)
                .clamp(0, stage_capacity);
            previous_max_level = max_level;
            (index != target_index).then(|| {
                json!({
                    "shop_item_id": shop_item_id,
                    "stock_quantity": stage_capacity.saturating_sub(stage_purchase_num),
                    "today_purchase_num": 0,
                    "this_month_purchase_num": null,
                    "total_purchase_num": stage_purchase_num,
                    "discount_id": null,
                    "discount_rate": null,
                    "discounted_price": null,
                })
            })
        })
        .collect()
}
// //// /返回装备强化组中其余阶段的可购买状态 ////

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct EventShopActivityState {
    pub(super) status: ActivityWindowStatus,
    pub(super) overrides_item_period: bool,
    configured: bool,
}

// //// 统一活动商店状态并标记管理窗口是否覆盖商品日期 [@x380kkm 2026-08-29] ////
pub(super) fn event_shop_activity_state(
    database: &ServiceDatabase,
    _asset_root: &Path,
    activity_id: &str,
) -> Result<EventShopActivityState, PersonalServiceError> {
    let state = activity_state(database, activity_id)?;
    let Some(alias) = event_shop_activity_alias(activity_id) else {
        return Ok(state);
    };
    if state.configured {
        return Ok(state);
    }
    let alias_state = activity_state(database, &alias)?;
    if alias_state.configured {
        return Ok(alias_state);
    }
    Ok(state)
}
// //// /统一活动商店状态并标记管理窗口是否覆盖商品日期 ////

pub(super) fn shop_sale_entry(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    asset_root: &Path,
    response_time: i64,
    shop_type: i64,
    shop_item_id: i64,
    item: &Map<String, Value>,
) -> Result<(Value, bool), PersonalServiceError> {
    let state = period::synchronize_purchase_state(
        root,
        database,
        asset_root,
        shop_type,
        shop_item_id,
        item,
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
    item: &Map<String, Value>,
) -> Result<Value, PersonalServiceError> {
    let state = period::purchase_state(
        root,
        database,
        asset_root,
        shop_type,
        shop_item_id,
        item,
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
    now: &str,
    database: &ServiceDatabase,
    asset_root: &Path,
) -> Result<Vec<Value>, PersonalServiceError> {
    let response_time = crate::cn::server_time(database)?;
    let mut sales = Vec::new();
    for shop_type in [
        SHOP_TYPE_TREASURE,
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
                now,
                false,
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
            now,
            false,
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
            let activity_state = event_shop_activity_state(database, asset_root, &activity_id)?;
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
                now,
                activity_state.overrides_item_period,
            )?;
        }
    }
    sales.sort_by_key(|sale| {
        (
            sale.get("shop_type")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            sale.get("shop_item_id")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
        )
    });
    sales.dedup_by(|left, right| {
        left.get("shop_type") == right.get("shop_type")
            && left.get("shop_item_id") == right.get("shop_item_id")
    });
    Ok(sales)
}

// //// 将匹配奖励的可用商品加入来源列表 [@x380kkm 2026-08-24] ////
fn append_matching_sales(
    sales: &mut Vec<Value>,
    root: &Map<String, Value>,
    database: &ServiceDatabase,
    asset_root: &Path,
    response_time: i64,
    shop_type: i64,
    catalog: &Map<String, Value>,
    item_id: Option<i64>,
    equipment_id: Option<i64>,
    now: &str,
    overrides_item_period: bool,
) -> Result<(), PersonalServiceError> {
    for (shop_item_id, item) in catalog {
        let (Ok(shop_item_id), Some(item)) = (shop_item_id.parse::<i64>(), item.as_object()) else {
            continue;
        };
        if !shop_item_is_in_client_master(shop_type, shop_item_id)?
            || (!overrides_item_period && !shop_item_is_active(item, now))
            || !shop_item_contains_reward(item, item_id, equipment_id)
        {
            continue;
        }
        sales.push(projected_shop_sale_entry(
            root,
            database,
            asset_root,
            response_time,
            shop_type,
            shop_item_id,
            item,
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

fn activity_state(
    database: &ServiceDatabase,
    activity_id: &str,
) -> Result<EventShopActivityState, PersonalServiceError> {
    if database
        .activity_temporary_open_until(activity_id)?
        .is_some()
    {
        return Ok(EventShopActivityState {
            status: ActivityWindowStatus::Open,
            overrides_item_period: true,
            configured: true,
        });
    }
    let Some(schedule) = database.get_activity_schedule(activity_id)? else {
        return Ok(EventShopActivityState {
            status: ActivityWindowStatus::Open,
            overrides_item_period: false,
            configured: false,
        });
    };
    let status =
        evaluate_activity_schedule(&schedule, database.current_server_time_millis()?).status;
    Ok(EventShopActivityState {
        status,
        overrides_item_period: status == ActivityWindowStatus::Open,
        configured: true,
    })
}
