// audience: internal
// # personal-service-cn-gacha-campaign
//
// 该模块从扭蛋活动主数据生成可用次数并保存活动支付状态.

use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;

const CAMPAIGN_ASSET: &str = include_str!("../../../../assets/gacha_campaign.json");
static CAMPAIGNS: OnceLock<Result<Value, String>> = OnceLock::new();

pub(super) fn replace_available_for_load<F>(
    root: &mut Map<String, Value>,
    mut is_available: F,
) -> Result<(), PersonalServiceError>
where
    F: FnMut(i64) -> Result<bool, PersonalServiceError>,
{
    let document = document()?;
    let existing = root
        .get("gacha_campaign_list")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut campaign_list = Vec::new();
    for (gacha_id, campaign_id) in document
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(gacha_id, campaign_id)| {
            Some((gacha_id.parse::<i64>().ok()?, campaign_id.as_i64()?))
        })
        .filter(|(gacha_id, _)| super::region::is_enumerable(*gacha_id))
    {
        if !is_available(gacha_id)? {
            continue;
        }
        let count = existing
            .iter()
            .find(|campaign| {
                campaign.get("gacha_id").and_then(Value::as_i64) == Some(gacha_id)
                    && campaign.get("campaign_id").and_then(Value::as_i64) == Some(campaign_id)
            })
            .and_then(|campaign| campaign.get("count"))
            .and_then(Value::as_i64)
            .unwrap_or(1);
        campaign_list.push(json!({
            "gacha_id": gacha_id,
            "campaign_id": campaign_id,
            "count": count,
        }));
    }
    root.insert(
        "gacha_campaign_list".to_owned(),
        Value::Array(campaign_list),
    );
    Ok(())
}

// //// 将真实卡池的免费活动次数投影到响应入口 [@x380kkm 2026-08-24] ////
pub(super) fn project_alias_for_load(
    root: &mut Map<String, Value>,
    canonical_gacha_id: i64,
    response_gacha_id: i64,
) -> Result<(), PersonalServiceError> {
    if canonical_gacha_id == response_gacha_id {
        return Ok(());
    }
    let campaigns = root
        .entry("gacha_campaign_list".to_owned())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN gacha campaigns are invalid"))?;
    let Some(mut projected) = campaigns
        .iter()
        .find(|campaign| {
            campaign.get("gacha_id").and_then(Value::as_i64) == Some(canonical_gacha_id)
        })
        .cloned()
    else {
        return Ok(());
    };
    let projected = projected
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN gacha campaign is invalid"))?;
    projected.insert("campaign_id".to_owned(), Value::from(response_gacha_id));
    projected.insert("gacha_id".to_owned(), Value::from(response_gacha_id));
    let projected = Value::Object(projected.clone());
    if let Some(existing) = campaigns.iter_mut().find(|campaign| {
        campaign.get("gacha_id").and_then(Value::as_i64) == Some(response_gacha_id)
            && campaign.get("campaign_id") == projected.get("campaign_id")
    }) {
        *existing = projected;
    } else {
        campaigns.push(projected);
    }
    Ok(())
}

pub(super) fn project_temporary_response(
    campaigns: &mut [Value],
    response_gacha_id: i64,
) -> Result<(), PersonalServiceError> {
    for campaign in campaigns {
        let campaign = campaign
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN gacha campaign is invalid"))?;
        campaign.insert("campaign_id".to_owned(), Value::from(response_gacha_id));
        campaign.insert("gacha_id".to_owned(), Value::from(response_gacha_id));
    }
    Ok(())
}
// //// /将真实卡池的免费活动次数投影到响应入口 ////

// //// 重置每日扭蛋活动次数 [@x380kkm 2026-08-23] ////
pub(super) fn reset_daily_counts(
    root: &mut Map<String, Value>,
) -> Result<(), PersonalServiceError> {
    let campaigns = root
        .get_mut("gacha_campaign_list")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN gacha campaigns are invalid"))?;
    for campaign in campaigns {
        let campaign = campaign
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN gacha campaign is invalid"))?;
        campaign.insert("count".to_owned(), Value::from(1));
    }
    Ok(())
}
// //// /重置每日扭蛋活动次数 ////

pub(super) fn redeem(
    root: &mut Map<String, Value>,
    gacha_id: i64,
) -> Result<Result<Value, &'static str>, PersonalServiceError> {
    let Some(campaign_id) = document()?
        .get(gacha_id.to_string())
        .and_then(Value::as_i64)
    else {
        return Ok(Err("gacha_campaign_not_found"));
    };
    let campaigns = root
        .entry("gacha_campaign_list".to_owned())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN gacha campaigns are invalid"))?;
    let campaign = campaigns.iter_mut().find(|campaign| {
        campaign.get("gacha_id").and_then(Value::as_i64) == Some(gacha_id)
            && campaign.get("campaign_id").and_then(Value::as_i64) == Some(campaign_id)
    });
    if let Some(campaign) = campaign {
        let campaign = campaign
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN gacha campaign is invalid"))?;
        if campaign
            .get("count")
            .and_then(Value::as_i64)
            .unwrap_or_default()
            <= 0
        {
            return Ok(Err("gacha_campaign_already_used"));
        }
        campaign.insert("count".to_owned(), Value::from(0));
        return Ok(Ok(Value::Object(campaign.clone())));
    }
    let campaign = json!({
        "gacha_id": gacha_id,
        "campaign_id": campaign_id,
        "count": 0,
    });
    campaigns.push(campaign.clone());
    Ok(Ok(campaign))
}

fn document() -> Result<&'static Value, PersonalServiceError> {
    CAMPAIGNS
        .get_or_init(|| {
            serde_json::from_str::<Value>(CAMPAIGN_ASSET)
                .map_err(|error| format!("failed to decode CN gacha campaigns: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}
