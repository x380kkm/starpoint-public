// audience: internal
// # personal-service-cn-login-bonus
// 登录奖励在载入快照时到账, 展示确认只清除提示.
// 每日领取使用设备现实本地日期, 奖励组的开放窗口使用游戏时间.

mod catalog;
#[cfg(test)]
mod tests;

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_battle_rewards::apply_reward_at;
use crate::cn_tutorial::{decode_player_data, encode_player_data, player_snapshot, require_root};
use crate::database::{ReceiveHistoryEntry, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default, Deserialize, Serialize)]
struct LoginBonusState {
    groups: BTreeMap<String, GroupState>,
    #[serde(default)]
    last_login_at: Option<i64>,
}

#[derive(Default, Deserialize, Serialize)]
struct GroupState {
    #[serde(default)]
    received_days: BTreeSet<i64>,
    #[serde(default)]
    received_dates: BTreeSet<String>,
    index: i64,
}

#[derive(Default)]
pub(crate) struct LoginBonusDelivery {
    pub(crate) event_key: String,
    pub(crate) entries: Vec<ReceiveHistoryEntry>,
}

#[derive(Deserialize)]
struct ShownRequest {
    viewer_id: i64,
}

// //// 按有效分组发放当天奖励并保存展示位置 [@x380kkm 2026-09-22] ////
pub(crate) fn prepare(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    now: i64,
) -> Result<LoginBonusDelivery, PersonalServiceError> {
    prepare_for_date(root, database, now, &database.current_wall_date()?)
}

fn prepare_for_date(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    now: i64,
    date: &str,
) -> Result<LoginBonusDelivery, PersonalServiceError> {
    let definitions = catalog::definitions()?;
    let mut state: LoginBonusState = serde_json::from_value(
        root.get("login_bonus_state")
            .cloned()
            .unwrap_or_else(|| json!({"groups":{}})),
    )
    .map_err(|error| PersonalServiceError::new(format!("invalid CN login bonus state: {error}")))?;
    recover_pending_receipts(root, definitions, &mut state, date);
    let previous_login = state
        .last_login_at
        .or_else(|| catalog::stored_login_time(root));
    let eligible = definitions.eligible_groups(root, &state.groups, previous_login, now);
    state.last_login_at = Some(state.last_login_at.unwrap_or(now).max(now));
    let mut pending = root
        .get("bonus_index_list")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut delivery = LoginBonusDelivery::default();
    let mut acquired = BTreeMap::<i64, i64>::new();
    let mut keys = Vec::new();
    for id in eligible {
        let rows = &definitions.groups[&id];
        let group = state.groups.entry(id.clone()).or_default();
        if group.received_dates.contains(date) {
            continue;
        }
        let next = rows
            .iter()
            .find(|row| row.index > group.index)
            .or_else(|| (rows[0].group_type == 0).then_some(&rows[0]));
        let Some(row) = next else {
            continue;
        };
        for reward in &row.rewards {
            let count = reward.count.unwrap_or_default();
            let repeats = if reward.kind == 2 { count } else { 1 };
            for _ in 0..repeats {
                let result = apply_reward_at(root, reward, now)?;
                for character_id in result.joined_character_ids {
                    crate::cn_character_reward::record_character_encyclopedia_state(
                        root,
                        character_id,
                    )?;
                }
            }
            let kind = match reward.kind {
                0 => 1,
                2 => 5,
                3 => 4,
                4 => 8,
                5 => 9,
                _ => unreachable!(),
            };
            let mut entry = ReceiveHistoryEntry::reward(kind, reward.id, count);
            entry.reason_id = row.reason_id;
            entry.subject = row.name.clone();
            delivery.entries.push(entry);
            if reward.kind == 0 {
                *acquired.entry(reward.id.unwrap()).or_default() += count;
            }
        }
        group.index = row.index;
        group.received_dates.insert(date.to_owned());
        pending.retain(|entry| entry["bonus_group_id"] != id);
        pending.push(
            json!({"bonus_group_id":id,"bonus_group_type":row.group_type_name(),"index":row.index}),
        );
        keys.push(format!("{id}:{date}:{}", row.index));
    }
    if !keys.is_empty() {
        crate::cn_mission::record_collected_item_rewards(
            root,
            &crate::cn_mission::plan_collect_item_missions(database)?,
            &acquired,
            now,
        )?;
        root.insert("login_bonus_received_at".to_owned(), Value::from(now));
        root.insert("login_bonus_received_date".to_owned(), Value::from(date));
        root.insert("bonus_index_list".to_owned(), Value::Array(pending));
        delivery.event_key = format!("login-bonus:{}", keys.join(";"));
    }
    root.insert(
        "login_bonus_state".to_owned(),
        serde_json::to_value(state)
            .map_err(|error| PersonalServiceError::new(error.to_string()))?,
    );
    Ok(delivery)
}
// //// /按有效分组发放当天奖励并保存展示位置 ////

// //// 将已到账的展示位置合并到分组领取状态 [@x380kkm 2026-09-22] ////
fn recover_pending_receipts(
    root: &Map<String, Value>,
    definitions: &catalog::Catalog,
    state: &mut LoginBonusState,
    date: &str,
) {
    if root.get("login_bonus_received_at").and_then(Value::as_i64).is_none() {
        return;
    }
    let received_date = root.get("login_bonus_received_date")
        .and_then(Value::as_str)
        .unwrap_or(date);
    let Some(pending) = root.get("bonus_index_list").and_then(Value::as_array) else {
        return;
    };
    for entry in pending {
        let Some(id) = entry.get("bonus_group_id").and_then(Value::as_str) else {
            continue;
        };
        let Some(index) = entry.get("index").and_then(Value::as_i64) else {
            continue;
        };
        let Some(rows) = definitions.groups.get(id) else {
            continue;
        };
        if !rows.iter().any(|row| row.index == index) {
            continue;
        }
        let group = state.groups.entry(id.to_owned()).or_insert_with(|| GroupState {
            index,
            ..GroupState::default()
        });
        if group.received_dates.is_empty() {
            group.received_dates.insert(received_date.to_owned());
        }
    }
}
// //// /将已到账的展示位置合并到分组领取状态 ////

// //// 保存登录奖励展示确认 [@x380kkm 2026-09-22] ////
pub(crate) fn shown(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ShownRequest>(request) {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player)?;
    let mut state: LoginBonusState = serde_json::from_value(
        root.get("login_bonus_state")
            .cloned()
            .unwrap_or_else(|| json!({"groups":{}})),
    )
    .map_err(|error| PersonalServiceError::new(format!("invalid CN login bonus state: {error}")))?;
    recover_pending_receipts(root, catalog::definitions()?, &mut state, &database.current_wall_date()?);
    root.insert(
        "login_bonus_state".to_owned(),
        serde_json::to_value(state)
            .map_err(|error| PersonalServiceError::new(error.to_string()))?,
    );
    for field in [
        "bonus_index_list",
        "login_bonus_received_at",
        "login_bonus_received_date",
        "premium_bonus_index_list",
        "premium_bonus_mailed_item_list",
    ] {
        root.insert(field.to_owned(), Value::Null);
    }
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player)?)?;
    msgpack_response_at(body.viewer_id, false, server_time(database)?, json!([]))
}
// //// /保存登录奖励展示确认 ////
