// audience: internal
// # personal-service-cn-shop-transactions
// 商店购买把费用、实发奖励、任务计数和请求收据共同保存到玩家快照.

use super::*;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct BuyRequest {
    viewer_id: i64,
    api_count: Option<i64>,
    shop_type: i64,
    shop_item_id: i64,
    number: i64,
}

#[derive(Deserialize)]
struct BulkBuyRequest {
    viewer_id: i64,
    api_count: Option<i64>,
    shop_type: Option<i64>,
    #[serde(default, alias = "shop_item_list")]
    buy_item_list: BulkBuyItems,
}

#[derive(Default, Deserialize)]
#[serde(untagged)]
enum BulkBuyItems {
    Items(BTreeMap<String, i64>),
    EmptyList(Vec<Value>),
    #[default]
    Missing,
}

// //// 验证客户端整型购买数量 [@x380kkm 2026-09-20] ////
fn is_valid_purchase_count(count: i64) -> bool {
    (1..=i64::from(i32::MAX)).contains(&count)
}
// //// /验证客户端整型购买数量 ////

// //// 将商品开放和数量变化转换为客户端可处理的购买结果 [@x380kkm 2026-09-24] ////
fn purchase_failure_response(
    viewer_id: i64,
    response_time: i64,
    code: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    match code {
        selection::UNAVAILABLE_ERROR
        | "shop_item_unavailable"
        | "activity_disabled"
        | "activity_not_started"
        | "activity_ended"
        | "activity_not_found" => msgpack_result_code_response_at(viewer_id, response_time, 2053),
        "shop_stock_exceeded"
        | "shop_user_balance_insufficient"
        | "shop_item_balance_insufficient"
        | "shop_reward_capacity_exceeded"
        | "equipment_not_owned"
        | "equipment_level_shortage"
        | "equipment_enhancement_stage_mismatch" => {
            msgpack_result_code_response_at(viewer_id, response_time, 3010)
        }
        _ => Ok(error_response("400 Bad Request", code)),
    }
}
// //// /将商品开放和数量变化转换为客户端可处理的购买结果 ////

// //// 购买 CN 商店商品并保存费用、奖励和库存 [@x380kkm 2026-08-22] ////
pub(super) fn buy(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<BuyRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.shop_item_id > 0
                && body.api_count.map_or(true, |count| count >= 0)
                && is_valid_purchase_count(body.number) =>
        {
            body
        }
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let response_time = server_time(database)?;
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let receipt = receipt::PurchaseReceipt::new(
        root,
        database,
        "buy",
        body.viewer_id,
        body.api_count,
        body.shop_type,
        &BTreeMap::from([(body.shop_item_id, body.number)]),
    )?;
    if let Some(response) = receipt.replay(root)? {
        return Ok(response);
    }
    let mana_before = crate::cn_mission::total_mana(root)?;
    let rewards = match purchase_shop_item(
        root,
        database,
        asset_root,
        body.viewer_id,
        body.shop_type,
        body.shop_item_id,
        body.number,
        response_time,
    )? {
        Ok(rewards) => rewards,
        Err(code) => return purchase_failure_response(body.viewer_id, response_time, code),
    };
    let purchase_count = root
        .get("shop_purchase_counts")
        .and_then(Value::as_object)
        .and_then(|counts| counts.get(&shop_purchase_key(body.shop_type, body.shop_item_id)))
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let (mission_info, mut active_mission_list) = record_shop_actions(
        root,
        database,
        snapshot.account_id,
        body.shop_type,
        body.number,
        &rewards,
        mana_before,
        response_time,
    )?;
    active_mission_list.extend(crate::cn_mission::refresh_active_missions(root, database)?);
    let mut response = json!({
        "user_info": rewards.user_info,
        "joined_character_id_list": rewards.joined_character_id_list,
        "character_list": rewards.character_list,
        "equipment_list": rewards.equipment_list,
        "item_list": rewards.item_list,
        "active_mission_list": active_mission_list,
        "mission_info": mission_info,
        "mail_arrived": false,
    });
    let completed = response["mission_info"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    crate::cn_mission::apply_mission_reward_response(
        &mut response,
        root,
        &completed,
        body.viewer_id,
        response_time,
    )?;
    receipt.record(root, &response, &rewards, response_time)?;
    database.save_player_snapshot_with_receive_history(
        snapshot.account_id,
        &encode_player_data(&player_data)?,
        &format!(
            "shop:buy:{}:{}:{purchase_count}",
            body.shop_type, body.shop_item_id
        ),
        response_time,
        &rewards.history_entries,
    )?;
    msgpack_response_at(body.viewer_id, false, response_time, response)
}
// //// /购买 CN 商店商品并保存费用、奖励和库存 ////

// //// 批量购买 CN 商店商品并一次保存完整结果 [@x380kkm 2026-08-24] ////
pub(super) fn bulk_buy(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<BulkBuyRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.api_count.map_or(true, |count| count >= 0) => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let items = match body.buy_item_list {
        BulkBuyItems::Items(items) => {
            let mut parsed = BTreeMap::new();
            for (shop_item_id, count) in items {
                let Ok(shop_item_id) = shop_item_id.parse::<i64>() else {
                    return Ok(error_response("400 Bad Request", "invalid_request_body"));
                };
                if parsed.insert(shop_item_id, count).is_some() {
                    return Ok(error_response("400 Bad Request", "invalid_request_body"));
                }
            }
            parsed
        }
        BulkBuyItems::EmptyList(items) if items.is_empty() => BTreeMap::new(),
        BulkBuyItems::EmptyList(_) => {
            return Ok(error_response("400 Bad Request", "invalid_request_body"))
        }
        BulkBuyItems::Missing => BTreeMap::new(),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let response_time = server_time(database)?;
    if items.is_empty() {
        return msgpack_response_at(body.viewer_id, false, response_time, json!({}));
    }
    let Some(shop_type) = body.shop_type.filter(|shop_type| *shop_type > 0) else {
        return Ok(error_response("400 Bad Request", "invalid_request_body"));
    };
    if items
        .iter()
        .any(|(shop_item_id, count)| *shop_item_id <= 0 || !is_valid_purchase_count(*count))
    {
        return Ok(error_response("400 Bad Request", "invalid_request_body"));
    }

    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let receipt = receipt::PurchaseReceipt::new(
        root,
        database,
        "bulk_buy",
        body.viewer_id,
        body.api_count,
        shop_type,
        &items,
    )?;
    if let Some(response) = receipt.replay(root)? {
        return Ok(response);
    }
    let mana_before = crate::cn_mission::total_mana(root)?;
    let mut combined_rewards: Option<purchase::ShopRewards> = None;
    for (shop_item_id, count) in &items {
        let rewards = match purchase_shop_item(
            root,
            database,
            asset_root,
            body.viewer_id,
            shop_type,
            *shop_item_id,
            *count,
            response_time,
        )? {
            Ok(rewards) => rewards,
            Err(code) => return purchase_failure_response(body.viewer_id, response_time, code),
        };
        if let Some(combined) = &mut combined_rewards {
            combined.merge(rewards);
        } else {
            combined_rewards = Some(rewards);
        }
    }
    let rewards = combined_rewards
        .ok_or_else(|| PersonalServiceError::new("CN bulk shop purchase is empty"))?;
    let purchased_count = items.values().try_fold(0_i64, |total, count| {
        total
            .checked_add(*count)
            .ok_or_else(|| PersonalServiceError::new("CN bulk shop purchase count exceeds range"))
    })?;
    let (mission_info, mut active_mission_list) = record_shop_actions(
        root,
        database,
        snapshot.account_id,
        shop_type,
        purchased_count,
        &rewards,
        mana_before,
        response_time,
    )?;
    active_mission_list.extend(crate::cn_mission::refresh_active_missions(root, database)?);
    let mut response = json!({
        "user_info": rewards.user_info,
        "joined_character_id_list": rewards.joined_character_id_list,
        "character_list": rewards.character_list,
        "equipment_list": rewards.equipment_list,
        "item_list": rewards.item_list,
        "active_mission_list": active_mission_list,
        "mission_info": mission_info,
        "mail_arrived": false,
    });
    let completed = response["mission_info"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    crate::cn_mission::apply_mission_reward_response(
        &mut response,
        root,
        &completed,
        body.viewer_id,
        response_time,
    )?;
    receipt.record(root, &response, &rewards, response_time)?;
    let total_purchased = receipt::total_purchased(root)?;
    database.save_player_snapshot_with_receive_history(
        snapshot.account_id,
        &encode_player_data(&player_data)?,
        &format!("shop:bulk_buy:total:{total_purchased}"),
        response_time,
        &rewards.history_entries,
    )?;
    msgpack_response_at(body.viewer_id, false, response_time, response)
}
// //// /批量购买 CN 商店商品并一次保存完整结果 ////

// //// 按商品实际奖励数量更新收集任务 [@x380kkm 2026-09-21] ////
fn record_shop_item_acquisitions(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    rewards: &[crate::database::ReceiveHistoryEntry],
    response_time: i64,
) -> Result<Vec<Value>, PersonalServiceError> {
    let mut acquired = BTreeMap::<i64, i64>::new();
    for reward in rewards.iter().filter(|reward| reward.kind == 1) {
        if let Some(item_id) = reward.type_id {
            let count = acquired.entry(item_id).or_default();
            *count = count.checked_add(reward.number).ok_or_else(|| {
                PersonalServiceError::new("CN shop item acquisition exceeds range")
            })?;
        }
    }
    crate::cn_mission::record_collected_item_rewards(
        root,
        &crate::cn_mission::plan_collect_item_missions(database)?,
        &acquired,
        response_time,
    )
}
// //// /按商品实际奖励数量更新收集任务 ////

// //// 按商店类型, 购买份数和实际玛纳收支累计任务 [@x380kkm 2026-09-22] ////
fn record_shop_actions(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    account_id: i64,
    shop_type: i64,
    count: i64,
    rewards: &purchase::ShopRewards,
    mana_before: i64,
    response_time: i64,
) -> Result<(Vec<Value>, Vec<Value>), PersonalServiceError> {
    let equipment_count = rewards
        .history_entries
        .iter()
        .filter(|reward| reward.kind == 6)
        .try_fold(0_i64, |total, reward| {
            total
                .checked_add(reward.number)
                .ok_or_else(|| PersonalServiceError::new("CN shop equipment count exceeds range"))
        })?;
    let spent_mana = rewards
        .cost_entries
        .iter()
        .filter(|cost| cost["kind"] == "free_mana" || cost["kind"] == "paid_mana")
        .filter_map(|cost| cost["count"].as_i64())
        .try_fold(0_i64, |total, cost| {
            total
                .checked_add(cost)
                .ok_or_else(|| PersonalServiceError::new("CN shop mana cost exceeds range"))
        })?;
    let active_missions = crate::cn_mission::record_active_counters(
        root,
        database,
        &[
            (
                "treasure_shop_bought_item_count",
                if shop_type == catalog::SHOP_TYPE_TREASURE {
                    count
                } else {
                    0
                },
            ),
            (
                "traded_count_by_boss_coin_shop",
                if shop_type == catalog::SHOP_TYPE_BOSS_COIN {
                    count
                } else {
                    0
                },
            ),
            (
                "traded_count_to_equipment_by_boss_coin",
                if shop_type == catalog::SHOP_TYPE_BOSS_COIN {
                    equipment_count
                } else {
                    0
                },
            ),
            ("total_used_mana_count", spent_mana),
        ],
    )?;
    let mut missions =
        record_shop_item_acquisitions(root, database, &rewards.history_entries, response_time)?;
    let acquired_mana = crate::cn_mission::total_mana(root)?
        .checked_sub(mana_before)
        .and_then(|difference| difference.checked_add(spent_mana))
        .ok_or_else(|| PersonalServiceError::new("CN shop mana income exceeds range"))?;
    missions.extend(crate::cn_mission::record_economic_actions(
        root,
        &crate::cn_mission::plan_economic_missions(database, account_id)?,
        crate::cn_mission::EconomicActions {
            mana_acquired: acquired_mana,
            mana_spent: spent_mana,
            treasure_purchases: if shop_type == catalog::SHOP_TYPE_TREASURE {
                count
            } else {
                0
            },
            treasure_mana_spent: if shop_type == catalog::SHOP_TYPE_TREASURE {
                spent_mana
            } else {
                0
            },
            ..crate::cn_mission::EconomicActions::default()
        },
        response_time,
    )?);
    Ok((missions, active_missions))
}
// //// /按商店类型, 购买份数和实际玛纳收支累计任务 ////
