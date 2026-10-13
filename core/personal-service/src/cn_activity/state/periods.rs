// audience: internal
// # personal-service-activity-period-state
// 原始 CN 进度保存在活动根记录, 区域期别按开始时间分别保存成绩与领取状态.
// 当前战斗使用入场时捕获的期别, 管理重置保留其他期别和已投递资产.

use super::{event_container, event_container_mut, event_state_mut};
use crate::database::{ActiveSingleQuest, ServiceDatabase};
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};

// //// 按有效活动时间选择区域期别 [@x380kkm 2026-09-27] ////
pub(crate) fn select_event_period(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    family: &str,
    event_id: i64,
) -> Result<(), PersonalServiceError> {
    let period = crate::activity_projection::effective_regional_activity_period(
        database,
        &format!("{family}:{event_id}"),
        database.current_server_time_millis()?,
    )?;
    select_period(root, family, event_id, period.map(|period| period.start_at_ms))
}

pub(crate) fn select_period(
    root: &mut Map<String, Value>,
    family: &str,
    event_id: i64,
    start_at_ms: Option<i64>,
) -> Result<(), PersonalServiceError> {
    if !matches!(family, "ranking" | "raid" | "rush") {
        return Ok(());
    }
    if family == "ranking" && start_at_ms.is_some() {
        super::super::ranking::preserve_original_record(root, event_id)?;
    }
    if start_at_ms.is_some() {
        preserve_original_receipt_round(root, family, event_id)?;
    }
    let event = event_container_mut(root, family, event_id)?;
    if let Some(start) = start_at_ms {
        event.insert("active_period_start_ms".to_owned(), json!(start));
        event_state_mut(root, family, event_id)?;
    } else {
        event.remove("active_period_start_ms");
    }
    if family == "rush" {
        super::prepare_event_state(root, family, event_id)?;
    }
    Ok(())
}

fn preserve_original_receipt_round(
    root: &mut Map<String, Value>,
    family: &str,
    event_id: i64,
) -> Result<(), PersonalServiceError> {
    let round = crate::activity_history::participation_round(root, &format!("{family}:{event_id}"));
    if family == "ranking" {
        if let Some(receipt) = root.get_mut("ranking_reward_receipts")
            .and_then(|receipts| receipts.get_mut(event_id.to_string())).and_then(Value::as_object_mut)
        {
            receipt.entry("participation_round".to_owned()).or_insert(json!(round));
        }
    }
    if family == "rush" {
        if let Some(receipt) = event_container_mut(root, family, event_id)?
            .get_mut("ranking_reward_receipt").and_then(Value::as_object_mut)
        {
            receipt.entry("participation_round".to_owned()).or_insert(json!(round));
        }
    }
    if let Some(receipts) = event_container_mut(root, family, event_id)?
        .get_mut("kill_reward_receipts").and_then(Value::as_object_mut)
    {
        for receipt in receipts.values_mut().filter_map(Value::as_object_mut) {
            receipt.entry("participation_round".to_owned()).or_insert(json!(round));
        }
    }
    Ok(())
}

pub(crate) fn event_period_start(
    root: &Map<String, Value>,
    family: &str,
    event_id: i64,
) -> Option<i64> {
    event_container(root, family, event_id)?
        .get("active_period_start_ms")?
        .as_i64()
}

pub(crate) fn reward_scope(root: &Map<String, Value>, family: &str, event_id: i64) -> String {
    let activity_id = format!("{family}:{event_id}");
    match event_period_start(root, family, event_id) {
        Some(start) => format!("{activity_id}:period:{start}"),
        None => activity_id,
    }
}
// //// /按有效活动时间选择区域期别 ////

// //// 读取入场期别并用于跨时间结算 [@x380kkm 2026-09-27] ////
pub(crate) fn activate_battle_period(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    active: &ActiveSingleQuest,
    activity_id: Option<&str>,
    completed_at: i64,
) -> Result<(), PersonalServiceError> {
    let Some(activity_id) = activity_id else { return Ok(()); };
    let Some((family, event_id)) = activity_id.split_once(':') else {
        return Ok(());
    };
    if !matches!(family, "ranking" | "raid" | "rush") {
        return Ok(());
    }
    let event_id = event_id.parse::<i64>()
        .map_err(|_| PersonalServiceError::new("invalid battle activity identity"))?;
    let captured = root.get("battle_mission_window").filter(|window| {
        window.get("play_id").and_then(Value::as_str) == Some(active.play_id.as_str())
    });
    let period = if let Some(periods) = captured.and_then(|window| window.get("event_periods")).and_then(Value::as_object) {
        periods.get(activity_id).and_then(Value::as_i64)
    } else {
        let started_at = captured.and_then(|window| window.get("started_at")).and_then(Value::as_i64)
            .unwrap_or(completed_at);
        crate::activity_projection::effective_regional_activity_period(
            database, activity_id, started_at.saturating_mul(1_000),
        )?.map(|period| period.start_at_ms)
    };
    select_period(root, family, event_id, period)
}
// //// /读取入场期别并用于跨时间结算 ////

// //// 清除选中期别并保留其他期别记录 [@x380kkm 2026-09-27] ////
pub(crate) fn reset_event_state(
    root: &mut Map<String, Value>,
    family: &str,
    event_id: i64,
) -> Result<(), PersonalServiceError> {
    let Some(event) = event_container(root, family, event_id) else { return Ok(()); };
    let original = event.get("active_period_start_ms").and_then(Value::as_i64).is_none();
    let retained_periods = original.then(|| event.get("periods").cloned()).flatten();
    let event = event_state_mut(root, family, event_id)?;
    event.clear();
    event.insert("legacy_state_imported".to_owned(), Value::Bool(true));
    if family == "ranking" {
        event.insert("quest_progress_imported".to_owned(), Value::Bool(true));
    }
    if let Some(periods) = retained_periods {
        event.insert("periods".to_owned(), periods);
    }
    Ok(())
}
