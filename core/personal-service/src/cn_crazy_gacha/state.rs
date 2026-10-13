// audience: internal
// # personal-service-cn-crazy-gacha-state
// 槽位保存角色 ID, 中断恢复保存带动画字段的完整候选.

use super::GACHA_ID;
use crate::PersonalServiceError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

const STATE_FIELD: &str = "crazy_gacha_state";
const SLOTS_FIELD: &str = "crazy_gacha_result_list";
const LAST_DRAW_FIELD: &str = "last_crazy_gacha_draw_result";
const MAX_DRAW_COUNT: i64 = 50;

#[derive(Default, Deserialize, Serialize)]
struct Session {
    draw_count: i64,
    selected_index: Option<i64>,
}

// //// 读取候选会话并确定当前抽取所需券数 [@x380kkm 2026-09-23] ////
fn session(root: &Map<String, Value>) -> Result<Option<Session>, PersonalServiceError> {
    root.get(STATE_FIELD)
        .map(|value| {
            serde_json::from_value(value.clone())
                .map_err(|_| PersonalServiceError::new("stored CN crazy gacha session is invalid"))
        })
        .transpose()
}

pub(crate) fn ticket_cost(
    root: &Map<String, Value>,
) -> Result<Result<i64, &'static str>, PersonalServiceError> {
    let Some(session) = session(root)?.filter(|session| session.selected_index.is_none()) else {
        return Ok(Ok(1));
    };
    if !(1..=MAX_DRAW_COUNT).contains(&session.draw_count) {
        return Err(PersonalServiceError::new(
            "stored CN crazy gacha draw count is invalid",
        ));
    }
    Ok(if session.draw_count == MAX_DRAW_COUNT {
        Err("crazy_gacha_draw_limit")
    } else {
        Ok(0)
    })
}
// //// /读取候选会话并确定当前抽取所需券数 ////

// //// 为载入响应补齐三组候选槽位和中断恢复数组 [@x380kkm 2026-09-23] ////
pub(crate) fn prepare_load(root: &mut Map<String, Value>) -> Result<(), PersonalServiceError> {
    let slots = root
        .entry(SLOTS_FIELD.to_owned())
        .or_insert_with(|| json!({}));
    if slots.as_array().is_some_and(Vec::is_empty) {
        *slots = json!({});
    }
    let slots = slots
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN crazy gacha slots are invalid"))?;
    for index in 0..=2 {
        slots.entry(index.to_string()).or_insert_with(|| json!([]));
    }
    root.entry(LAST_DRAW_FIELD.to_owned())
        .or_insert_with(|| json!([]));
    Ok(())
}
// //// /为载入响应补齐三组候选槽位和中断恢复数组 ////

// //// 保留当前十连候选并递增同一张券的抽取次数 [@x380kkm 2026-09-23] ////
pub(crate) fn store_draw(
    root: &mut Map<String, Value>,
    draws: &[Value],
) -> Result<(), PersonalServiceError> {
    let mut session = session(root)?
        .filter(|session| session.selected_index.is_none())
        .unwrap_or_default();
    if session.draw_count == 0 {
        clear_candidates(root);
        reset_draw_count(root)?;
    }
    prepare_load(root)?;
    let character_ids = draws
        .iter()
        .map(|draw| draw["character_id"].clone())
        .collect::<Vec<_>>();
    root.get_mut(SLOTS_FIELD).unwrap()["0"] = json!(character_ids);
    root.insert(LAST_DRAW_FIELD.to_owned(), json!(draws));
    session.draw_count += 1;
    root.insert(STATE_FIELD.to_owned(), json!(session));
    Ok(())
}
// //// /保留当前十连候选并递增同一张券的抽取次数 ////

// //// 保存当前候选且保留已保存组 [@x380kkm 2026-09-23] ////
pub(super) fn save_slot(
    root: &mut Map<String, Value>,
    index: i64,
) -> Result<Result<(), &'static str>, PersonalServiceError> {
    if !matches!(index, 1 | 2) {
        return Ok(Err("invalid_crazy_gacha_slot"));
    }
    let current = match selection(root, 0)? {
        Ok(Some(current)) => current,
        _ => return Ok(Err("crazy_gacha_result_missing")),
    };
    prepare_load(root)?;
    let slots = root.get_mut(SLOTS_FIELD).unwrap().as_object_mut().unwrap();
    let saved = slots
        .get(&index.to_string())
        .and_then(Value::as_array)
        .ok_or_else(|| PersonalServiceError::new("stored CN crazy gacha slot is invalid"))?;
    if !saved.is_empty() {
        return Ok(Err("crazy_gacha_slot_occupied"));
    }
    slots.insert(index.to_string(), json!(current));
    Ok(Ok(()))
}
// //// /保存当前候选且保留已保存组 ////

// //// 读取选中候选并识别已完成的重复选定 [@x380kkm 2026-09-23] ////
pub(super) fn selection(
    root: &Map<String, Value>,
    index: i64,
) -> Result<Result<Option<Vec<i64>>, &'static str>, PersonalServiceError> {
    if !(0..=2).contains(&index) {
        return Ok(Err("invalid_crazy_gacha_slot"));
    }
    let Some(session) = session(root)? else {
        return Ok(Err("crazy_gacha_result_missing"));
    };
    if let Some(selected) = session.selected_index {
        return Ok(if selected == index {
            Ok(None)
        } else {
            Err("crazy_gacha_already_selected")
        });
    }
    let Some(slot) = root
        .get(SLOTS_FIELD)
        .and_then(|slots| slots.get(index.to_string()))
        .and_then(Value::as_array)
        .filter(|slot| slot.len() == 10)
    else {
        return Ok(Err("crazy_gacha_result_missing"));
    };
    let ids = slot
        .iter()
        .map(|value| {
            value.as_i64().filter(|id| *id > 0).ok_or_else(|| {
                PersonalServiceError::new("stored CN crazy gacha character is invalid")
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Ok(Some(ids)))
}
// //// /读取选中候选并识别已完成的重复选定 ////

// //// 完成候选会话并清除客户端的选择入口 [@x380kkm 2026-09-23] ////
pub(super) fn finish(
    root: &mut Map<String, Value>,
    index: i64,
) -> Result<(), PersonalServiceError> {
    let mut session = session(root)?
        .ok_or_else(|| PersonalServiceError::new("stored CN crazy gacha session is missing"))?;
    session.selected_index = Some(index);
    root.insert(STATE_FIELD.to_owned(), json!(session));
    clear_candidates(root);
    reset_draw_count(root)
}

fn clear_candidates(root: &mut Map<String, Value>) {
    root.insert(SLOTS_FIELD.to_owned(), json!({"0": [], "1": [], "2": []}));
    root.insert(LAST_DRAW_FIELD.to_owned(), json!([]));
}

fn reset_draw_count(root: &mut Map<String, Value>) -> Result<(), PersonalServiceError> {
    if let Some(info) = root
        .get_mut("gacha_info_list")
        .and_then(Value::as_array_mut)
        .and_then(|list| list.iter_mut().find(|info| info["gacha_id"] == GACHA_ID))
    {
        let info = info
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN gacha info is invalid"))?;
        info.insert("crazy_draw_count".to_owned(), json!(0));
    }
    Ok(())
}
// //// /完成候选会话并清除客户端的选择入口 ////

// //// 返回保存槽位和中断恢复候选及当前抽取次数 [@x380kkm 2026-09-23] ////
pub(super) fn response(root: &Map<String, Value>) -> Result<Value, PersonalServiceError> {
    let info = root
        .get("gacha_info_list")
        .and_then(Value::as_array)
        .and_then(|list| list.iter().find(|info| info["gacha_id"] == GACHA_ID))
        .ok_or_else(|| PersonalServiceError::new("stored CN crazy gacha info is missing"))?;
    Ok(json!({
        "crazy_gacha_result_list": root[SLOTS_FIELD],
        "last_crazy_gacha_draw_result": root[LAST_DRAW_FIELD],
        "gacha_info_list": [info],
    }))
}
// //// /返回保存槽位和中断恢复候选及当前抽取次数 ////
