// audience: internal
// # personal-service-cn-payment-rebate
// 返利资格和奖品来自玩家存档. 资产, 资格消耗, 领取凭据和领取历史在同一保存点提交.

mod rewards;

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{decode_player_data, encode_player_data, player_snapshot, require_root};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use rewards::RebateReward;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub(crate) const RECEIPTS_FIELD: &str = "payment_rebate_receipts";

#[derive(Deserialize)]
struct RebateRequest {
    viewer_id: i64,
}

struct RebateOffer {
    start_time: i64,
    expired_time: i64,
    rewards: Vec<RebateReward>,
    receipt_key: String,
}

enum RebateEligibility {
    Available(RebateOffer),
    Inactive,
    Invalid,
}

// //// 领取当前存档中的有效返利资格 [@x380kkm 2026-09-23] ////
pub(crate) fn receive(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<RebateRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => {
            return Ok(HttpResponse::json(
                "400 Bad Request",
                json!({"error": "invalid_request_body"}).to_string(),
            ));
        }
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let now = server_time(database)?;
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let unchanged = response(root)?;
    let offer = match read_offer(root, now)? {
        RebateEligibility::Available(offer) => offer,
        RebateEligibility::Inactive => {
            return msgpack_response_at(body.viewer_id, false, now, unchanged)
        }
        RebateEligibility::Invalid => return Ok(invalid_reward_response("invalid_rebate_rewards")),
    };
    let before_items = crate::cn_mission::capture_collect_item_counts(root)?;
    let before_mana = crate::cn_mission::total_mana(root)?;
    let granted = rewards::grant(root, &offer.rewards, body.viewer_id, now)?;
    if !granted.fits_client(root) {
        return Ok(invalid_reward_response("rebate_reward_capacity_exceeded"));
    }

    database
        .with_player_settlement(|database| {
            let root = require_root(&mut player_data)?;
            let mut mission_info = crate::cn_mission::record_collected_items(
                root,
                database,
                snapshot.account_id,
                &before_items,
            )?;
            let acquired_mana = crate::cn_mission::total_mana(root)?.saturating_sub(before_mana);
            mission_info.extend(crate::cn_mission::record_acquired_mana(
                root,
                database,
                snapshot.account_id,
                acquired_mana,
                now,
            )?);
            let mut data = response(root)?;
            granted.project(&mut data, root, body.viewer_id, now);
            crate::cn_mission::apply_mission_reward_response(
                &mut data,
                root,
                &mission_info,
                body.viewer_id,
                now,
            )?;
            if !mission_info.is_empty() {
                data["mission_info"] = Value::Array(mission_info);
            }
            consume_offer(root, &offer, now)?;
            let response = msgpack_response_at(body.viewer_id, false, now, data)?;
            database.save_player_snapshot_with_receive_history(
                snapshot.account_id,
                &encode_player_data(&player_data)?,
                &format!("payment-rebate:{}", offer.receipt_key),
                now,
                &granted.history,
            )?;
            Ok(Some(response))
        })?
        .ok_or_else(|| PersonalServiceError::new("CN rebate settlement returned no response"))
}
// //// /领取当前存档中的有效返利资格 ////

// //// 读取原有资格并按奖励内容识别同一次返利 [@x380kkm 2026-09-23] ////
fn read_offer(
    root: &Map<String, Value>,
    now: i64,
) -> Result<RebateEligibility, PersonalServiceError> {
    let Some(info) = root.get("payment_rebate_info").and_then(Value::as_object) else {
        return Ok(RebateEligibility::Inactive);
    };
    if info.get("status").and_then(Value::as_i64) != Some(1) {
        return Ok(RebateEligibility::Inactive);
    }
    let Some(expired_time) = info.get("expired_time").and_then(Value::as_i64) else {
        return Ok(RebateEligibility::Invalid);
    };
    let start_time = info
        .get("start_time")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    if expired_time <= now {
        return Ok(RebateEligibility::Inactive);
    }
    if expired_time > i32::MAX as i64 {
        return Ok(RebateEligibility::Invalid);
    }
    let Some(list) = info
        .get("present_list")
        .and_then(Value::as_array)
        .filter(|list| !list.is_empty())
    else {
        return Ok(RebateEligibility::Invalid);
    };
    let mut totals = BTreeMap::<(i64, Option<i64>), i64>::new();
    for entry in list {
        let Ok(reward) = serde_json::from_value::<RebateReward>(entry.clone()) else {
            return Ok(RebateEligibility::Invalid);
        };
        if !reward.is_valid() {
            return Ok(RebateEligibility::Invalid);
        }
        let total = totals.entry((reward.kind, reward.type_id)).or_default();
        let Some(value) = total
            .checked_add(reward.number)
            .filter(|value| *value <= i32::MAX as i64)
        else {
            return Ok(RebateEligibility::Invalid);
        };
        *total = value;
    }
    let rewards = totals
        .into_iter()
        .map(|((kind, type_id), number)| RebateReward {
            kind,
            type_id,
            number,
        })
        .collect::<Vec<_>>();
    let identity = serde_json::to_vec(&(start_time, expired_time, &rewards)).map_err(|error| {
        PersonalServiceError::new(format!("failed to encode rebate identity: {error}"))
    })?;
    let receipt_key = format!("{:x}", Sha256::digest(identity));
    if let Some(receipts) = root.get(RECEIPTS_FIELD).filter(|value| !value.is_null()) {
        let Some(receipts) = receipts.as_object() else {
            return Ok(RebateEligibility::Invalid);
        };
        if receipts.contains_key(&receipt_key) {
            return Ok(RebateEligibility::Inactive);
        }
    }
    Ok(RebateEligibility::Available(RebateOffer {
        start_time,
        expired_time,
        rewards,
        receipt_key,
    }))
}
// //// /读取原有资格并按奖励内容识别同一次返利 ////

// //// 保存资格消耗及其实际奖励凭据 [@x380kkm 2026-09-23] ////
fn consume_offer(
    root: &mut Map<String, Value>,
    offer: &RebateOffer,
    now: i64,
) -> Result<(), PersonalServiceError> {
    root.get_mut("payment_rebate_info")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("CN rebate eligibility disappeared"))?
        .insert("status".to_owned(), Value::from(0));
    if root.get(RECEIPTS_FIELD).is_some_and(Value::is_null) {
        root.remove(RECEIPTS_FIELD);
    }
    root.entry(RECEIPTS_FIELD.to_owned())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN rebate receipts are invalid"))?
        .insert(
            offer.receipt_key.clone(),
            json!({
                "start_time": offer.start_time,
                "expired_time": offer.expired_time,
                "rewards": offer.rewards,
                "received_at": now,
            }),
        );
    Ok(())
}
// //// /保存资格消耗及其实际奖励凭据 ////

fn response(root: &Map<String, Value>) -> Result<Value, PersonalServiceError> {
    let paid = root
        .get("user_info")
        .and_then(|user| user.get("vmoney"))
        .and_then(Value::as_i64)
        .unwrap_or_default();
    if !(0..=i32::MAX as i64).contains(&paid) {
        return Err(PersonalServiceError::new(
            "stored CN paid stones exceed the client range",
        ));
    }
    Ok(json!({"status": 0, "after_vmoney": paid}))
}

fn invalid_reward_response(code: &str) -> HttpResponse {
    HttpResponse::json("400 Bad Request", json!({"error":code}).to_string())
}
