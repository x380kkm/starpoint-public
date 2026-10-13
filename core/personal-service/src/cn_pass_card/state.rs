// audience: internal
// # personal-service-cn-pass-card-state
//
// 每期积分, 购买资格和双轨领取记录保存在同一玩家快照中.
// 已选中的客户端活动编号用于绑定通用修行商品.

use super::catalog::{catalog, PassCardEvent};
use crate::database::{ActivityWindowStatus, ServiceDatabase};
use crate::PersonalServiceError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

#[derive(Default, Deserialize, Serialize)]
pub(super) struct PassCardState {
    pub(super) point: i64,
    pub(super) is_buy: bool,
    pub(super) all_received_record: Vec<ReceivedRecord>,
    #[serde(default)]
    login_days: Vec<i64>,
}

#[derive(Deserialize, Serialize)]
pub(super) struct ReceivedRecord {
    pub(super) reward_id: i64,
    pub(super) is_received_1: i64,
    pub(super) is_received_2: i64,
}

// //// 读取指定修行期的快照记录 [@x380kkm 2026-09-22] ////
pub(super) fn read_state(
    root: &Map<String, Value>,
    event_id: i64,
) -> Result<PassCardState, PersonalServiceError> {
    match root
        .get("pass_card_state")
        .and_then(|state| state.get("events"))
        .and_then(|events| events.get(event_id.to_string()))
    {
        Some(value) => serde_json::from_value(value.clone()).map_err(|error| {
            PersonalServiceError::new(format!("invalid CN pass card state: {error}"))
        }),
        None => Ok(PassCardState::default()),
    }
}

// //// 保存修行期状态并保留其他活动记录 [@x380kkm 2026-09-22] ////
pub(super) fn write_state(
    root: &mut Map<String, Value>,
    event_id: i64,
    state: &PassCardState,
) -> Result<(), PersonalServiceError> {
    let data = root
        .entry("pass_card_state".to_owned())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN pass card data is invalid"))?;
    data.entry("events".to_owned())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN pass card events are invalid"))?
        .insert(
            event_id.to_string(),
            serde_json::to_value(state).map_err(|error| {
                PersonalServiceError::new(format!("failed to encode CN pass card state: {error}"))
            })?,
        );
    Ok(())
}

// //// 记录客户端选择的修行活动 [@x380kkm 2026-09-22] ////
pub(super) fn select_event(
    root: &mut Map<String, Value>,
    event_id: i64,
) -> Result<(), PersonalServiceError> {
    root.entry("pass_card_state".to_owned())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN pass card data is invalid"))?
        .insert("current_event_id".to_owned(), Value::from(event_id));
    Ok(())
}

// //// 将付费资格绑定到当前开放的修行期 [@x380kkm 2026-09-26] ////
pub(crate) fn purchase_event_id(
    root: &Map<String, Value>,
    database: &ServiceDatabase,
    now: i64,
) -> Result<Option<i64>, PersonalServiceError> {
    let catalog = catalog()?;
    if let Some(event) = root
        .get("pass_card_state")
        .and_then(|state| state.get("current_event_id"))
        .and_then(Value::as_i64)
        .and_then(|id| catalog.events.get(&id))
    {
        if !event.is_free && is_open_event(database, event, now)? {
            return Ok(Some(event.id));
        }
    }
    for event in catalog.events.values().rev().filter(|event| !event.is_free) {
        if is_open_event(database, event, now)? { return Ok(Some(event.id)); }
    }
    Ok(None)
}

// //// 从主表和管理日历选择当前开放的修行期 [@x380kkm 2026-09-25] ////
fn open_event(
    database: &ServiceDatabase,
    now: i64,
) -> Result<Option<&'static PassCardEvent>, PersonalServiceError> {
    for event in catalog()?.events.values().rev() {
        if is_open_event(database, event, now)? {
            return Ok(Some(event));
        }
    }
    Ok(None)
}
// //// /从主表和管理日历选择当前开放的修行期 ////

fn is_open_event(database: &ServiceDatabase, event: &PassCardEvent, now: i64) -> Result<bool, PersonalServiceError> {
    let status = database.activity_window_status(&format!("pass-card:{}", event.id), now.saturating_mul(1000))?;
    Ok(status == ActivityWindowStatus::Open
        || (status == ActivityWindowStatus::Unscheduled && now >= event.start_time && now <= event.end_time))
}

// //// 将修行商品绑定到所选活动的高阶路线 [@x380kkm 2026-09-22] ////
pub(crate) fn grant_purchase(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    now: i64,
) -> Result<i64, PersonalServiceError> {
    let event_id = purchase_event_id(root, database, now)?
        .ok_or_else(|| PersonalServiceError::new("CN pass card purchase period is closed"))?;
    let mut state = read_state(root, event_id)?;
    state.is_buy = true;
    write_state(root, event_id, &state)?;
    select_event(root, event_id)?;
    Ok(event_id)
}

// //// 增加指定修行活动的积分 [@x380kkm 2026-09-22] ////
pub(crate) fn add_points_for_event(
    root: &mut Map<String, Value>,
    event_id: i64,
    amount: i64,
) -> Result<(), PersonalServiceError> {
    let event = catalog()?
        .events
        .get(&event_id)
        .ok_or_else(|| PersonalServiceError::new("unknown CN pass card event"))?;
    if amount < 0 {
        return Err(PersonalServiceError::new(
            "CN pass card points must be nonnegative",
        ));
    }
    let mut state = read_state(root, event_id)?;
    state.point = state
        .point
        .checked_add(amount)
        .ok_or_else(|| PersonalServiceError::new("CN pass card point range exceeded"))?
        .min(event.point_cap);
    write_state(root, event_id, &state)
}

// //// 记录修行期内的登录日期并保留回拨后的已计数日期 [@x380kkm 2026-09-22] ////
pub(crate) fn record_login_day_for_event(
    root: &mut Map<String, Value>,
    event_id: i64,
    day: i64,
) -> Result<i64, PersonalServiceError> {
    let mut state = read_state(root, event_id)?;
    if !state.login_days.contains(&day) {
        state.login_days.push(day);
    }
    let count = state.login_days.len() as i64;
    write_state(root, event_id, &state)?;
    Ok(count)
}

pub(crate) struct PointPurchaseTarget {
    pub(crate) event_id: i64,
    pub(crate) capacity: i64,
}

// //// 将积分商品绑定到仍可领奖的所选期或当前开放期 [@x380kkm 2026-09-25] ////
pub(crate) fn point_purchase_target(
    root: &Map<String, Value>,
    database: &ServiceDatabase,
    now: i64,
) -> Result<Option<PointPurchaseTarget>, PersonalServiceError> {
    let catalog = catalog()?;
    if let Some(event) = root
        .get("pass_card_state")
        .and_then(|state| state.get("current_event_id"))
        .and_then(Value::as_i64)
        .and_then(|id| catalog.events.get(&id))
    {
        if super::claim::is_claim_period(database, event, now)? {
            return point_target(root, event).map(Some);
        }
    }
    open_event(database, now)?.map(|event| point_target(root, event)).transpose()
}
// //// /将积分商品绑定到仍可领奖的所选期或当前开放期 ////

fn point_target(
    root: &Map<String, Value>,
    event: &PassCardEvent,
) -> Result<PointPurchaseTarget, PersonalServiceError> {
    Ok(PointPurchaseTarget {
        event_id: event.id,
        capacity: event.point_cap.saturating_sub(read_state(root, event.id)?.point).max(0),
    })
}

// //// 增加通用商品指定的当期修行积分 [@x380kkm 2026-09-22] ////
pub(crate) fn add_points(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    amount: i64,
    now: i64,
) -> Result<(), PersonalServiceError> {
    let event_id = point_purchase_target(root, database, now)?
        .ok_or_else(|| PersonalServiceError::new("CN pass card point purchase period is closed"))?.event_id;
    add_points_for_event(root, event_id, amount)
}
