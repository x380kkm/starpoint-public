// audience: internal
// # personal-service-cn-optional-exchange-management
// 活动重置保留资产和旧领取凭据; 新手重置的个人窗口以管理操作的游戏时间为起点.

use super::{catalog, state, Map, PersonalServiceError, ServiceDatabase, Value};
use serde_json::json;

pub(super) fn activity_id(family: &str, id: i64) -> String {
    let prefix = match family {
        "special" => "special-exchange",
        "start_dash" => "start-dash-exchange",
        _ => "treasure-vault-exchange",
    };
    format!("{prefix}:{id}")
}

pub(super) fn parse_activity(activity: &str) -> Option<(&'static str, i64)> {
    let (prefix, id) = activity.split_once(':')?;
    let family = match prefix {
        "special-exchange" => "special",
        "start-dash-exchange" => "start_dash",
        "treasure-vault-exchange" => "treasure",
        _ => return None,
    };
    Some((family, id.parse::<i64>().ok().filter(|id| *id > 0)?))
}

pub(crate) fn has_campaign(activity: &str) -> Result<bool, PersonalServiceError> {
    let Some((family, id)) = parse_activity(activity) else {
        return Ok(false);
    };
    let catalog = catalog::definitions()?;
    Ok(match family {
        "special" => catalog.special.contains_key(&id),
        "start_dash" => catalog.start_dash.contains_key(&id),
        _ => catalog.treasure.contains_key(&id),
    })
}

pub(crate) fn reward_period_millis(activity: &str) -> Result<Option<i64>, PersonalServiceError> {
    let Some((family, id)) = parse_activity(activity) else {
        return Ok(None);
    };
    let catalog = catalog::definitions()?;
    let campaign = match family {
        "special" => catalog.special.get(&id),
        "treasure" => catalog.treasure.get(&id),
        _ => return Ok(catalog.start_dash.get(&id).map(|_| 0)),
    };
    Ok(campaign.map(|campaign| {
        campaign
            .exchange_end_at
            .saturating_sub(campaign.end_at)
            .saturating_mul(1000)
    }))
}

pub(super) fn is_start_dash_open(
    database: &ServiceDatabase,
    id: i64,
) -> Result<bool, PersonalServiceError> {
    let Some(campaign) = catalog::definitions()?.start_dash.get(&id) else {
        return Ok(false);
    };
    let end = campaign.end_at.unwrap_or(i64::MAX / 1000);
    catalog::is_open(database, "start_dash", id, campaign.start_at, end, end)
}

// //// 将新手个人资格投影为当前全局活动窗口中的客户端入口 [@x380kkm 2026-09-23] ////
pub(crate) fn project_campaigns(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
) -> Result<(), PersonalServiceError> {
    let mut visible = Vec::new();
    if let Some(entries) = root
        .get(state::list_key("start_dash"))
        .and_then(Value::as_array)
    {
        for entry in entries {
            let Some(id) = entry["campaign_id"].as_i64() else {
                continue;
            };
            if is_start_dash_open(database, id)? {
                visible.push(entry.clone());
            }
        }
    }
    root.insert(state::list_key("start_dash").to_owned(), json!(visible));
    Ok(())
}
// //// /将新手个人资格投影为当前全局活动窗口中的客户端入口 ////

// //// 恢复活动资格并按管理时间重建新手个人期限 [@x380kkm 2026-09-23] ////
pub(crate) fn reset_campaign(
    root: &mut Map<String, Value>,
    activity: &str,
    reset_at: i64,
) -> Result<Value, PersonalServiceError> {
    let Some((family, id)) = parse_activity(activity) else {
        return Ok(Value::Null);
    };
    if !has_campaign(activity)? {
        return Ok(Value::Null);
    }
    let previous = if family == "treasure" {
        None
    } else {
        state::campaign_state(root, family, id).cloned()
    };
    if family == "special" {
        state::set_status(root, family, id, 1)?;
    } else if family == "start_dash" {
        let catalog = catalog::definitions()?;
        let campaign = &catalog.start_dash[&id];
        let end = reset_at.saturating_add(campaign.period_days.saturating_mul(86400));
        let record = state::start_dash_state(catalog, id, campaign, reset_at, end)?;
        let entries = root
            .entry(state::list_key(family).to_owned())
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| PersonalServiceError::new("invalid CN start dash campaign list"))?;
        entries.retain(|entry| entry["campaign_id"].as_i64() != Some(id));
        entries.push(record);
    }
    let current = if family == "treasure" {
        None
    } else {
        state::campaign_state(root, family, id)
    };
    Ok(json!({"previous_campaign":previous,"campaign":current,"reset_at":reset_at}))
}
// //// /恢复活动资格并按管理时间重建新手个人期限 ////
