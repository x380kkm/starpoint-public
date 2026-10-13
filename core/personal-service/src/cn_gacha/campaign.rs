// audience: internal
// # personal-service-cn-gacha-campaign
// 活动次数按真实卡池和原始活动保存, 响应顺序与客户端选择活动的顺序一致.

use super::GachaLoadContext;
use crate::database::{evaluate_activity_schedule, ActivityWindowStatus, ServiceDatabase};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

const CAMPAIGN_ASSET: &str = include_str!("../../../../assets/gacha_campaign.json");
static CAMPAIGNS: OnceLock<Result<BTreeMap<i64, Campaign>, String>> = OnceLock::new();

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Campaign {
    kind: i64,
    start_time: i64,
    end_time: i64,
    gacha_ids: Vec<i64>,
    temporary_campaign_ids: BTreeMap<i64, i64>,
}

// //// 按活动和卡池的开放条件列出免费次数 [@x380kkm 2026-09-28] ////
pub(super) fn replace_available_for_load(
    root: &mut Map<String, Value>,
    context: &GachaLoadContext,
) -> Result<(), PersonalServiceError> {
    let existing = root.get("gacha_campaign_list").and_then(Value::as_array);
    let mut campaigns = Vec::new();
    for (&campaign_id, campaign) in document()? {
        for &gacha_id in &campaign.gacha_ids {
            if !super::region::is_enumerable(gacha_id)
                || context.activity_status(gacha_id)? != ActivityWindowStatus::Open
                || !is_available(campaign_id, campaign, gacha_id, context)
            {
                continue;
            }
            let count = stored_count(existing, gacha_id, campaign_id);
            let response_id =
                response_campaign_id(campaign_id, campaign, gacha_id, gacha_id, context.now_ms)?;
            campaigns
                .push(json!({"gacha_id": gacha_id, "campaign_id": response_id, "count": count}));
        }
    }
    root.insert("gacha_campaign_list".to_owned(), Value::Array(campaigns));
    Ok(())
}

// //// 按客户端类型与顺序选择免费活动 [@x380kkm 2026-09-28] ////
pub(super) fn select_available(
    database: &ServiceDatabase,
    root: &Map<String, Value>,
    gacha_id: i64,
    response_gacha_id: i64,
    kind: i64,
) -> Result<Option<i64>, PersonalServiceError> {
    let context = GachaLoadContext::from_database(database)?;
    let existing = root.get("gacha_campaign_list").and_then(Value::as_array);
    let mut exhausted = None;
    for (&campaign_id, campaign) in document()? {
        if campaign.kind != kind
            || !campaign.gacha_ids.contains(&gacha_id)
            || !is_available(campaign_id, campaign, gacha_id, &context)
        {
            continue;
        }
        exhausted.get_or_insert(campaign_id);
        let response_id = response_campaign_id(
            campaign_id,
            campaign,
            gacha_id,
            response_gacha_id,
            context.now_ms,
        )?;
        if response_id != campaign_id {
            if stored_count(existing, gacha_id, campaign_id) > 0 {
                return Ok(Some(campaign_id));
            }
            continue;
        }
        for &pool_id in &campaign.gacha_ids {
            if super::region::is_enumerable(pool_id)
                && context.activity_status(pool_id)? == ActivityWindowStatus::Open
                && stored_count(existing, pool_id, campaign_id) > 0
            {
                return Ok(Some(campaign_id));
            }
        }
    }
    Ok(exhausted)
}

fn is_available(
    campaign_id: i64,
    campaign: &Campaign,
    gacha_id: i64,
    context: &GachaLoadContext,
) -> bool {
    let activity_id = format!("gacha-campaign:{campaign_id}");
    if context.temporary_open_windows.contains_key(&activity_id) {
        return true;
    }
    if let Some(schedule) = context.schedules.get(&activity_id) {
        return evaluate_activity_schedule(schedule, context.now_ms).status
            == ActivityWindowStatus::Open;
    }
    if context
        .temporary_open_windows
        .contains_key(&format!("gacha:{gacha_id}"))
    {
        return true;
    }
    is_master_available(campaign, context.now_ms)
}

fn is_master_available(campaign: &Campaign, now_ms: i64) -> bool {
    let now = now_ms.div_euclid(1_000);
    now >= campaign.start_time && now <= campaign.end_time
}

fn response_campaign_id(
    campaign_id: i64,
    campaign: &Campaign,
    canonical_gacha_id: i64,
    response_gacha_id: i64,
    now_ms: i64,
) -> Result<i64, PersonalServiceError> {
    if canonical_gacha_id == response_gacha_id && is_master_available(campaign, now_ms) {
        return Ok(campaign_id);
    }
    campaign
        .temporary_campaign_ids
        .get(&canonical_gacha_id)
        .copied()
        .ok_or_else(|| PersonalServiceError::new("CN temporary gacha campaign is missing"))
}

// //// 将真实卡池的活动关系投影到临时入口 [@x380kkm 2026-09-28] ////
pub(super) fn project_alias_for_load(
    root: &mut Map<String, Value>,
    canonical_gacha_id: i64,
    response_gacha_id: i64,
) -> Result<(), PersonalServiceError> {
    if canonical_gacha_id == response_gacha_id {
        return Ok(());
    }
    let campaigns = require_campaigns(root)?;
    let mut projected = campaigns
        .iter()
        .filter(|entry| entry.get("gacha_id").and_then(Value::as_i64) == Some(canonical_gacha_id))
        .cloned()
        .collect::<Vec<_>>();
    project_response(&mut projected, response_gacha_id, 0)?;
    for entry in projected {
        let campaign_id = entry["campaign_id"].as_i64().unwrap_or_default();
        if let Some(existing) = campaigns
            .iter_mut()
            .find(|candidate| matches_entry(candidate, response_gacha_id, campaign_id))
        {
            *existing = entry;
        } else {
            campaigns.push(entry);
        }
    }
    Ok(())
}

pub(super) fn project_response(
    campaigns: &mut [Value],
    response_gacha_id: i64,
    now_ms: i64,
) -> Result<(), PersonalServiceError> {
    let catalog = document()?;
    for entry in campaigns {
        let campaign_id = entry
            .get("campaign_id")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let gacha_id = entry
            .get("gacha_id")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let (&canonical_campaign_id, campaign) = catalog
            .iter()
            .find(|(&id, campaign)| {
                id == campaign_id
                    || campaign.temporary_campaign_ids.get(&gacha_id) == Some(&campaign_id)
            })
            .ok_or_else(|| PersonalServiceError::new("CN temporary gacha campaign is missing"))?;
        let response_id = response_campaign_id(
            canonical_campaign_id,
            campaign,
            gacha_id,
            response_gacha_id,
            now_ms,
        )?;
        let entry = entry
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN gacha campaign is invalid"))?;
        entry.insert("campaign_id".to_owned(), Value::from(response_id));
        entry.insert("gacha_id".to_owned(), Value::from(response_gacha_id));
    }
    Ok(())
}
// //// /将真实卡池的活动关系投影到临时入口 ////

// //// 重置每日扭蛋活动次数 [@x380kkm 2026-08-23] ////
pub(super) fn reset_daily_counts(
    root: &mut Map<String, Value>,
) -> Result<(), PersonalServiceError> {
    for campaign in require_campaigns(root)? {
        let campaign = campaign
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN gacha campaign is invalid"))?;
        campaign.insert("count".to_owned(), Value::from(1));
    }
    Ok(())
}

// //// 扣减所选活动在真实卡池的次数 [@x380kkm 2026-09-28] ////
pub(super) fn redeem(
    root: &mut Map<String, Value>,
    gacha_id: i64,
    campaign_id: i64,
) -> Result<Result<Value, &'static str>, PersonalServiceError> {
    let campaigns = require_campaigns(root)?;
    if let Some(campaign) = campaigns
        .iter_mut()
        .find(|entry| matches_entry(entry, gacha_id, campaign_id))
    {
        let campaign = campaign
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN gacha campaign is invalid"))?;
        let count = campaign
            .get("count")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        if count <= 0 {
            return Ok(Err("gacha_campaign_already_used"));
        }
        campaign.insert("count".to_owned(), Value::from(count - 1));
        return Ok(Ok(Value::Object(campaign.clone())));
    }
    let campaign = json!({"gacha_id": gacha_id, "campaign_id": campaign_id, "count": 0});
    campaigns.push(campaign.clone());
    Ok(Ok(campaign))
}

fn matches_entry(entry: &Value, gacha_id: i64, campaign_id: i64) -> bool {
    entry.get("gacha_id").and_then(Value::as_i64) == Some(gacha_id)
        && entry.get("campaign_id").and_then(Value::as_i64) == Some(campaign_id)
}

fn stored_count(existing: Option<&Vec<Value>>, gacha_id: i64, campaign_id: i64) -> i64 {
    existing
        .into_iter()
        .flatten()
        .find(|entry| matches_entry(entry, gacha_id, campaign_id))
        .and_then(|entry| entry.get("count"))
        .and_then(Value::as_i64)
        .unwrap_or(1)
}

fn require_campaigns(
    root: &mut Map<String, Value>,
) -> Result<&mut Vec<Value>, PersonalServiceError> {
    root.entry("gacha_campaign_list".to_owned())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN gacha campaigns are invalid"))
}

fn document() -> Result<&'static BTreeMap<i64, Campaign>, PersonalServiceError> {
    CAMPAIGNS
        .get_or_init(|| {
            serde_json::from_str(CAMPAIGN_ASSET)
                .map_err(|error| format!("failed to decode CN gacha campaigns: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // //// 对齐客户端秒精度的连续活动边界 [@x380kkm 2026-09-28] ////
    #[test]
    fn campaign_window_includes_the_entire_final_second() {
        let catalog = document().unwrap();
        let first = &catalog[&1011];
        let next = &catalog[&1012];
        assert!(!is_master_available(first, first.start_time * 1_000 - 1));
        assert!(is_master_available(first, first.start_time * 1_000));
        assert!(is_master_available(first, first.end_time * 1_000 + 999));
        assert!(!is_master_available(first, next.start_time * 1_000));
        assert!(is_master_available(next, next.start_time * 1_000));
    }
}
