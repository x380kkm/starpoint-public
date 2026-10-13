// audience: internal
// # personal-service-cn-growth-fund
//
// 基金奖励同时要求对应购买资格和等级或真实关卡通关记录.
// 每个奖励编号的领取状态与资产投递使用同一事务.

use crate::cn::{
    decode_request, msgpack_response_at, msgpack_result_code_response_at, server_time,
};
use crate::cn_mission::{apply_reward_response, player_rank, settle_rewards, MissionReward};
use crate::cn_payment::catalog::GrowthFundKind;
use crate::cn_tutorial::{decode_player_data, encode_player_data, player_snapshot, require_root};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::sync::OnceLock;

#[derive(Deserialize)]
struct GrowthFundRequest {
    viewer_id: i64,
    bonus_ids: Vec<i64>,
}

#[derive(Deserialize)]
struct GrowthFundCatalog {
    rows: Vec<GrowthFundReward>,
}

#[derive(Deserialize)]
struct GrowthFundReward {
    bonus_id: i64,
    fund_kind: GrowthFundKind,
    required_rank: Option<i64>,
    quest_category: Option<i64>,
    quest_id: Option<i64>,
    rewards: Vec<MissionReward>,
}

static CATALOG: OnceLock<Result<GrowthFundCatalog, String>> = OnceLock::new();

// //// 读取基金的等级和关卡奖励目录 [@x380kkm 2026-09-22] ////
fn catalog() -> Result<&'static GrowthFundCatalog, PersonalServiceError> {
    CATALOG
        .get_or_init(|| {
            serde_json::from_str(include_str!("../../assets/cn-growth-fund.json"))
                .map_err(|error| format!("failed to decode CN growth fund catalog: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

// //// 判断基金购买资格和实际成长条件 [@x380kkm 2026-09-22] ////
fn is_eligible(
    root: &Map<String, Value>,
    reward: &GrowthFundReward,
) -> Result<bool, PersonalServiceError> {
    if !root
        .get("user_info")
        .and_then(|user| user.get(reward.fund_kind.user_flag()))
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(false);
    }
    if let Some(rank) = reward.required_rank {
        return Ok(player_rank(root)? >= rank);
    }
    let Some((category, quest_id)) = reward.quest_category.zip(reward.quest_id) else {
        return Ok(false);
    };
    Ok(root
        .get("quest_progress")
        .and_then(|progress| progress.get(category.to_string()))
        .and_then(Value::as_array)
        .is_some_and(|quests| {
            quests.iter().any(|quest| {
                quest.get("quest_id").and_then(Value::as_i64) == Some(quest_id)
                    && quest.get("finished").and_then(Value::as_bool) == Some(true)
            })
        }))
}

// //// 原子领取符合资格的基金奖励并保留累计领取记录 [@x380kkm 2026-09-22] ////
pub(super) fn receive(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let now = server_time(database)?;
    let body = match decode_request::<GrowthFundRequest>(request) {
        Ok(body)
            if body.viewer_id > 0 && !body.bonus_ids.is_empty() && body.bonus_ids.len() <= 100 =>
        {
            body
        }
        _ => return msgpack_result_code_response_at(0, now, 20001),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut data)?;
    let definitions = catalog()?;
    let requested = body.bonus_ids.into_iter().collect::<BTreeSet<_>>();
    let mut received = root
        .get("fund_receive_list")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_i64)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let mut selected = Vec::new();
    let mut rewards = Vec::new();
    for id in &requested {
        let Some(reward) = definitions.rows.iter().find(|row| row.bonus_id == *id) else {
            return msgpack_result_code_response_at(body.viewer_id, now, 20001);
        };
        rewards.extend(reward.rewards.iter().cloned());
        if received.contains(id) {
            continue;
        }
        if !is_eligible(root, reward)? {
            return msgpack_result_code_response_at(body.viewer_id, now, 20001);
        }
        selected.push(reward);
    }
    let mut history = Vec::new();
    for reward in selected {
        if let Some(entries) = settle_rewards(
            root,
            "growth_fund_reward_receipts",
            &reward.bonus_id.to_string(),
            &reward.rewards,
            &[],
            now,
        )? {
            history.extend(entries);
        }
        received.insert(reward.bonus_id);
    }
    root.insert("fund_receive_list".to_owned(), json!(received));
    let mut response = json!({"fund_receive_list": root.get("fund_receive_list"), "mail_arrived": database.has_unreceived_mail(snapshot.account_id, now)?});
    apply_reward_response(&mut response, root, &rewards, body.viewer_id, now)?;
    let event_key = requested
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    database.save_player_snapshot_with_receive_history(
        snapshot.account_id,
        &encode_player_data(&data)?,
        &format!("growth-fund:{event_key}"),
        now,
        &history,
    )?;
    msgpack_response_at(body.viewer_id, false, now, response)
}
