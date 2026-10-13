// audience: internal
// # personal-service-cn-pass-card-claim
//
// 领取批次先校验活动, 等级和双轨资格, 再原子保存奖励与领取记录.
// 主表标记的高阶星导石奖励使用付费货币字段.

use super::catalog::{catalog, PassCardEvent, PassCardReward};
use super::state::{read_state, write_state, PassCardState, ReceivedRecord};
use crate::cn::{
    decode_request, msgpack_response_at, msgpack_result_code_response_at, server_time,
};
use crate::cn_mission::{apply_reward_response, settle_rewards, MissionReward};
use crate::cn_tutorial::{decode_player_data, encode_player_data, player_snapshot, require_root};
use crate::database::{
    evaluate_activity_schedule, ActivityWindowStatus, ReceiveHistoryEntry, ServiceDatabase,
};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct ReceiveRequest {
    viewer_id: i64,
    pass_card_id: i64,
    #[serde(default)]
    all_receive: Vec<i64>,
    #[serde(default)]
    reward1_receive: Vec<i64>,
    #[serde(default)]
    reward2_receive: Vec<i64>,
}

// //// 按主表和活动管理时窗判断领奖期 [@x380kkm 2026-09-22] ////
pub(super) fn is_claim_period(
    database: &ServiceDatabase,
    event: &PassCardEvent,
    now: i64,
) -> Result<bool, PersonalServiceError> {
    let activity_id = format!("pass-card:{}", event.id);
    let reward_delay_ms = event.force_end_time.saturating_sub(event.end_time).max(0).saturating_mul(1000);
    if database
        .activity_temporary_reward_window(&activity_id, reward_delay_ms)?
        .is_some()
    {
        return Ok(true);
    }
    if let Some(mut schedule) = database.get_activity_schedule(&activity_id)? {
        schedule.end_at_ms = schedule.end_at_ms.saturating_add(reward_delay_ms);
        return Ok(
            evaluate_activity_schedule(&schedule, now.saturating_mul(1000)).status
                == ActivityWindowStatus::Open,
        );
    }
    Ok(now >= event.start_time && now <= event.force_end_time)
}

// //// 合并同一奖励在批量和单轨参数中的请求 [@x380kkm 2026-09-22] ////
fn requested_tracks(body: &ReceiveRequest) -> BTreeMap<i64, u8> {
    let mut result = BTreeMap::new();
    for (ids, tracks) in [
        (&body.all_receive, 3),
        (&body.reward1_receive, 1),
        (&body.reward2_receive, 2),
    ] {
        for id in ids {
            *result.entry(*id).or_default() |= tracks;
        }
    }
    result
}

fn is_received(state: &PassCardState, id: i64, track: u8) -> bool {
    state
        .all_received_record
        .iter()
        .find(|record| record.reward_id == id)
        .is_some_and(|record| {
            if track == 1 {
                record.is_received_1 != 0
            } else {
                record.is_received_2 != 0
            }
        })
}

// //// 标记已投递的奖励轨道 [@x380kkm 2026-09-22] ////
fn record_received(state: &mut PassCardState, id: i64, track: u8) {
    if let Some(record) = state
        .all_received_record
        .iter_mut()
        .find(|record| record.reward_id == id)
    {
        if track == 1 {
            record.is_received_1 = 1;
        } else {
            record.is_received_2 = 1;
        }
    } else {
        state.all_received_record.push(ReceivedRecord {
            reward_id: id,
            is_received_1: i64::from(track == 1),
            is_received_2: i64::from(track == 2),
        });
    }
}

// //// 投递单轨奖励并保存可重放的领取凭据 [@x380kkm 2026-09-22] ////
fn grant_track(
    root: &mut Map<String, Value>,
    definition: &PassCardReward,
    track: u8,
    receipt_key: &str,
    now: i64,
) -> Result<Vec<ReceiveHistoryEntry>, PersonalServiceError> {
    let reward = if track == 1 {
        &definition.reward1
    } else {
        &definition.reward2
    };
    if track == 2 && definition.is_payment_money && reward.kind == 0 {
        let user = root
            .get_mut("user_info")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| PersonalServiceError::new("stored CN user info is invalid"))?;
        let amount = user
            .get("vmoney")
            .and_then(Value::as_i64)
            .unwrap_or_default()
            .checked_add(reward.amount)
            .ok_or_else(|| PersonalServiceError::new("CN paid currency range exceeded"))?;
        user.insert("vmoney".to_owned(), Value::from(amount));
        root.entry("pass_card_reward_receipts".to_owned())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN pass card receipts are invalid"))?
            .insert(
                receipt_key.to_owned(),
                json!({"rewards": [], "paid_vmoney": reward.amount, "received_at": now}),
            );
        return Ok(vec![ReceiveHistoryEntry::reward(3, None, reward.amount)]);
    }
    Ok(settle_rewards(
        root,
        "pass_card_reward_receipts",
        receipt_key,
        std::slice::from_ref(reward),
        &[],
        now,
    )?
    .unwrap_or_default())
}

// //// 校验并领取修行等级奖励 [@x380kkm 2026-09-22] ////
pub(super) fn receive(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let now = server_time(database)?;
    let body = match decode_request::<ReceiveRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.all_receive.len()
                    + body.reward1_receive.len()
                    + body.reward2_receive.len()
                    <= 240 =>
        {
            body
        }
        _ => return msgpack_result_code_response_at(0, now, 20001),
    };
    let definitions = catalog()?;
    let Some(event) = definitions.events.get(&body.pass_card_id) else {
        return msgpack_result_code_response_at(body.viewer_id, now, 20001);
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    if !is_claim_period(database, event, now)? {
        return msgpack_result_code_response_at(body.viewer_id, now, 20001);
    }
    let mut data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut data)?;
    let previous_items = crate::cn_mission::capture_collect_item_counts(root)?;
    let previous_mana = crate::cn_mission::total_mana(root)?;
    let mut state = read_state(root, event.id)?;
    let requested = requested_tracks(&body);
    let mut plan = Vec::new();
    let mut response_rewards: Vec<MissionReward> = Vec::new();
    for (id, tracks) in requested {
        let Some(reward) = definitions
            .rewards
            .get(&id)
            .filter(|reward| reward.event_id == event.id)
        else {
            return msgpack_result_code_response_at(body.viewer_id, now, 20001);
        };
        if state.point < reward.level * event.level_threshold
            || (tracks & 2 != 0 && !event.is_free && !state.is_buy)
        {
            return msgpack_result_code_response_at(body.viewer_id, now, 20001);
        }
        for track in [1, 2] {
            if tracks & track == 0 {
                continue;
            }
            response_rewards.push(if track == 1 {
                reward.reward1.clone()
            } else {
                reward.reward2.clone()
            });
            if !is_received(&state, id, track) {
                plan.push((reward, track));
            }
        }
    }
    let round =
        crate::activity_history::participation_round(root, &format!("pass-card:{}", event.id));
    let mut history = Vec::new();
    let mut receipt_keys = Vec::new();
    for (reward, track) in plan {
        let receipt_key = format!("{}:{}:{track}:round:{round}", event.id, reward.id);
        history.extend(grant_track(root, reward, track, &receipt_key, now)?);
        record_received(&mut state, reward.id, track);
        receipt_keys.push(receipt_key);
    }
    state
        .all_received_record
        .sort_by_key(|record| record.reward_id);
    write_state(root, event.id, &state)?;
    let mut mission_info = crate::cn_mission::record_collected_items(
        root,
        database,
        snapshot.account_id,
        &previous_items,
    )?;
    let acquired_mana = crate::cn_mission::total_mana(root)?.saturating_sub(previous_mana);
    mission_info.extend(crate::cn_mission::record_acquired_mana(
        root,
        database,
        snapshot.account_id,
        acquired_mana,
        now,
    )?);
    let mut response = json!({"all_received_record":state.all_received_record,"point":state.point,"is_buy":event.is_free || state.is_buy,"mail_arrived":database.has_unreceived_mail(snapshot.account_id, now)?});
    apply_reward_response(&mut response, root, &response_rewards, body.viewer_id, now)?;
    response["mission_info"] = json!(mission_info);
    crate::cn_mission::apply_mission_reward_response(
        &mut response,
        root,
        &mission_info,
        body.viewer_id,
        now,
    )?;
    if let Some(user) = root.get("user_info") {
        response["user_info"] = json!({"vmoney":user.get("vmoney"),"free_vmoney":user.get("free_vmoney"),"free_mana":user.get("free_mana"),"exp_pool":user.get("exp_pool"),"exp_pooled_time":user.get("exp_pooled_time")});
    }
    let digest = Sha256::digest(receipt_keys.join(",").as_bytes());
    database.save_player_snapshot_with_receive_history(
        snapshot.account_id,
        &encode_player_data(&data)?,
        &format!("pass-card:{digest:x}"),
        now,
        &history,
    )?;
    msgpack_response_at(body.viewer_id, false, now, response)
}
