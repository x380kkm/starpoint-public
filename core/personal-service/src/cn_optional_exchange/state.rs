// audience: internal
// # personal-service-cn-optional-exchange-state
// 活动资格和原结算响应随玩家快照保存, 同一请求计数保留原结果.

use super::{catalog, Map, PersonalServiceError, ServiceDatabase, Value};
use crate::cn::{msgpack_response_at, msgpack_result_code_response_at};
use crate::cn_tutorial::require_object;
use crate::http::HttpResponse;
use serde_json::json;

pub(super) fn list_key(family: &str) -> &'static str {
    if family == "special" {
        "special_exchange_campaign_list"
    } else {
        "start_dash_exchange_campaign_list"
    }
}

pub(super) fn campaign_state<'a>(
    root: &'a Map<String, Value>,
    family: &str,
    id: i64,
) -> Option<&'a Value> {
    root.get(list_key(family))
        .and_then(Value::as_array)?
        .iter()
        .find(|entry| entry["campaign_id"].as_i64() == Some(id))
}

pub(super) fn status(root: &Map<String, Value>, family: &str, id: i64) -> i64 {
    campaign_state(root, family, id)
        .and_then(|state| state["status"].as_i64())
        .unwrap_or(1)
}

pub(super) fn set_status(
    root: &mut Map<String, Value>,
    family: &str,
    id: i64,
    status: i64,
) -> Result<(), PersonalServiceError> {
    let entries = root
        .entry(list_key(family).to_owned())
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("invalid CN exchange campaign list"))?;
    if let Some(entry) = entries
        .iter_mut()
        .find(|entry| entry["campaign_id"].as_i64() == Some(id))
    {
        entry["status"] = json!(status);
    } else {
        entries.push(json!({"campaign_id":id,"status":status}));
    }
    Ok(())
}

// //// 保留普通自选进度并按初始角色入队时间固定新手资格 [@x380kkm 2026-09-23] ////
pub(crate) fn refresh_campaigns(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
) -> Result<(), PersonalServiceError> {
    let catalog = catalog::definitions()?;
    for (id, campaign) in &catalog.special {
        if campaign_state(root, "special", *id).is_none()
            && catalog::is_open(
                database,
                "special",
                *id,
                campaign.start_at,
                campaign.end_at,
                campaign.end_at,
            )?
        {
            set_status(root, "special", *id, 1)?;
        }
    }
    let Some(created_at) = root
        .get("user_character_list")
        .and_then(|characters| characters.get("1"))
        .and_then(|character| character.get("join_time"))
        .and_then(Value::as_i64)
        .filter(|time| *time > 0)
    else {
        return Ok(());
    };
    for (id, campaign) in &catalog.start_dash {
        if campaign_state(root, "start_dash", *id).is_some()
            || created_at < campaign.start_at
            || campaign.end_at.is_some_and(|end| created_at > end)
        {
            continue;
        }
        let end = created_at.saturating_add(campaign.period_days.saturating_mul(86400));
        let record = start_dash_state(
            catalog,
            *id,
            campaign,
            created_at,
            campaign.end_at.map_or(end, |limit| limit.min(end)),
        )?;
        let entries = root
            .entry(list_key("start_dash").to_owned())
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| PersonalServiceError::new("invalid CN start dash campaign list"))?;
        entries.push(record);
    }
    Ok(())
}
// //// /保留普通自选进度并按初始角色入队时间固定新手资格 ////

// //// 按资格时刻固定新手档期和可兑换卡池 [@x380kkm 2026-09-23] ////
pub(super) fn start_dash_state(
    catalog: &catalog::Catalog,
    id: i64,
    campaign: &catalog::StartDash,
    start: i64,
    end: i64,
) -> Result<Value, PersonalServiceError> {
    let (term, _) = campaign
        .terms
        .iter()
        .filter(|(_, time)| **time <= start)
        .max_by_key(|(_, time)| *time)
        .or_else(|| campaign.terms.iter().min_by_key(|(_, time)| *time))
        .ok_or_else(|| PersonalServiceError::new("missing CN start dash term"))?;
    let (gacha, _) = catalog
        .gachas
        .iter()
        .filter(|(_, gacha)| gacha.start_at <= start)
        .max_by_key(|(id, gacha)| {
            (
                gacha.end_at >= start,
                gacha.start_at,
                -gacha.list_order,
                -**id,
            )
        })
        .or_else(|| {
            catalog
                .gachas
                .iter()
                .min_by_key(|(id, gacha)| (gacha.start_at, gacha.list_order, **id))
        })
        .ok_or_else(|| PersonalServiceError::new("missing CN start dash gacha"))?;
    Ok(
        json!({"campaign_id":id,"status":1,"gacha_id":gacha,"term_index":term,"period_start_time":start,"period_end_time":end}),
    )
}
// //// /按资格时刻固定新手档期和可兑换卡池 ////

pub(super) fn item_count(root: &Map<String, Value>, id: i64) -> Result<i64, PersonalServiceError> {
    root.get("item_list")
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("missing CN item inventory"))?
        .get(&id.to_string())
        .map_or(Ok(0), |count| {
            count
                .as_i64()
                .filter(|count| *count >= 0)
                .ok_or_else(|| PersonalServiceError::new("invalid CN exchange ticket count"))
        })
}

pub(super) fn adjust_item(
    root: &mut Map<String, Value>,
    id: i64,
    change: i64,
) -> Result<i64, PersonalServiceError> {
    let count = item_count(root, id)?
        .checked_add(change)
        .filter(|value| *value >= 0 && *value <= i64::from(i32::MAX))
        .ok_or_else(|| PersonalServiceError::new("CN exchange item count exceeds range"))?;
    require_object(root, "item_list")?.insert(id.to_string(), json!(count));
    Ok(count)
}

// //// 用会话和请求计数恢复响应并拒绝改变兑换目标的重放 [@x380kkm 2026-09-23] ////
pub(super) fn replay(
    root: &Map<String, Value>,
    identity: &Value,
    now: i64,
) -> Result<Option<HttpResponse>, PersonalServiceError> {
    let Some(key) = request_key(identity) else {
        return Ok(None);
    };
    let Some(receipt) = root
        .get("optional_exchange_receipts")
        .and_then(|entries| entries.get(key))
    else {
        return Ok(None);
    };
    let viewer = identity["viewer_id"].as_i64().unwrap_or_default();
    if receipt["request"] != *identity {
        return msgpack_result_code_response_at(viewer, now, 20001).map(Some);
    }
    msgpack_response_at(
        viewer,
        false,
        receipt["received_at"].as_i64().unwrap_or(now),
        receipt["response"].clone(),
    )
    .map(Some)
}
// //// /用会话和请求计数恢复响应并拒绝改变兑换目标的重放 ////

fn request_key(identity: &Value) -> Option<String> {
    Some(format!(
        "{}:{}:{}:{}:{}",
        identity["viewer_id"].as_i64()?,
        identity["api_count"].as_i64()?,
        identity["family"].as_str()?,
        identity["action"].as_str()?,
        identity["participation_round"].as_i64()?
    ))
}

pub(super) fn record(
    root: &mut Map<String, Value>,
    identity: Value,
    response: &Value,
    costs: Value,
    rewards: &[crate::database::ReceiveHistoryEntry],
    now: i64,
) -> Result<String, PersonalServiceError> {
    let key = request_key(&identity).unwrap_or_else(|| {
        format!(
            "{}:{}:{}:{}",
            identity["family"],
            identity["campaign_id"],
            identity["action"],
            identity["participation_round"]
        )
    });
    let activity_id = identity["activity_id"].clone();
    let round = identity["participation_round"].clone();
    let rewards = rewards
        .iter()
        .map(|reward| json!({"kind":reward.kind,"type_id":reward.type_id,"number":reward.number}))
        .collect::<Vec<_>>();
    root.entry("optional_exchange_receipts".to_owned()).or_insert_with(|| json!({})).as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("invalid CN optional exchange receipts"))?.insert(
        key.clone(),
        json!({"activity_id":activity_id,"participation_round":round,"request":identity,"response":response,"costs":costs,"rewards":rewards,"received_at":now}),
    );
    Ok(format!("optional-exchange:{key}"))
}
