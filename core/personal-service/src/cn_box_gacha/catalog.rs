// audience: internal
// # personal-service-cn-box-gacha-catalog
//
// 该模块加载 CN 箱池目录并读写箱体和已抽奖励状态.

use crate::PersonalServiceError;
use getrandom::getrandom;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

const BOX_GACHA_ASSET: &str = include_str!("../../../../assets/box_gacha.json");
const BOX_REWARD_ASSET: &str = include_str!("../../../../assets/box_reward.json");
static BOX_GACHA_DATA: OnceLock<Result<Value, String>> = OnceLock::new();
static BOX_REWARD_DATA: OnceLock<Result<Value, String>> = OnceLock::new();

pub(super) fn all_box_info(
    root: &Map<String, Value>,
    box_gacha_id: i64,
    gacha: &Map<String, Value>,
    rewards: &Map<String, Value>,
) -> Result<Vec<Value>, PersonalServiceError> {
    let mut box_ids = rewards
        .keys()
        .filter_map(|box_id| box_id.parse::<i64>().ok())
        .collect::<Vec<_>>();
    box_ids.sort_unstable();
    box_ids
        .into_iter()
        .map(|box_id| {
            let status = find_box_status(root, box_gacha_id, box_id);
            let drawn = drawn_reward_counts(root, box_gacha_id, box_id)?;
            Ok(json!({
                "box_id": box_id,
                "reset_times": status.and_then(|status| status.get("reset_times")).and_then(Value::as_i64).unwrap_or_default(),
                "all_drawn_reward_list": drawn.into_iter().map(|(reward_id, number)| json!({"reward_id": reward_id, "number": number})).collect::<Vec<_>>(),
                "coming_next_reward_list": [],
                "is_closed": status.and_then(|status| status.get("is_closed")).and_then(Value::as_bool).unwrap_or(false)
                    || status.and_then(|status| status.get("remaining_number")).and_then(Value::as_i64).unwrap_or_else(|| box_available_count(gacha, box_id)) == 0,
            }))
        })
        .collect()
}

pub(super) fn choose_reward(
    rewards: &Map<String, Value>,
    drawn: &BTreeMap<i64, i64>,
) -> Result<Option<i64>, PersonalServiceError> {
    let mut candidates = Vec::new();
    let mut total = 0_i64;
    for (reward_id, reward) in rewards {
        let reward_id = reward_id
            .parse::<i64>()
            .map_err(|_| PersonalServiceError::new("CN box reward id is invalid"))?;
        let available = reward
            .get("available")
            .and_then(Value::as_i64)
            .unwrap_or_default()
            .saturating_sub(drawn.get(&reward_id).copied().unwrap_or_default());
        if available > 0 {
            total = total.saturating_add(available);
            candidates.push((reward_id, available));
        }
    }
    if total <= 0 {
        return Ok(None);
    }
    let mut bytes = [0_u8; 8];
    getrandom(&mut bytes)
        .map_err(|error| PersonalServiceError::new(format!("failed to draw CN box: {error}")))?;
    let mut roll = (u64::from_le_bytes(bytes) % total as u64) as i64;
    for (reward_id, weight) in candidates {
        if roll < weight {
            return Ok(Some(reward_id));
        }
        roll -= weight;
    }
    Ok(None)
}

pub(super) fn ensure_box_status<'a>(
    root: &'a mut Map<String, Value>,
    box_gacha_id: i64,
    box_id: i64,
    available: i64,
) -> Result<&'a mut Map<String, Value>, PersonalServiceError> {
    let lists = root
        .entry("box_gacha_list".to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN box list is invalid"))?;
    let list = lists
        .entry(box_gacha_id.to_string())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN box status list is invalid"))?;
    if let Some(index) = list
        .iter()
        .position(|status| status.get("box_id").and_then(Value::as_i64) == Some(box_id))
    {
        return list[index]
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN box status is invalid"));
    }
    list.push(json!({
        "box_id": box_id,
        "reset_times": 0,
        "remaining_number": available,
        "is_closed": false,
    }));
    list.last_mut()
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN box status is invalid"))
}

pub(super) fn drawn_reward_counts(
    root: &Map<String, Value>,
    box_gacha_id: i64,
    box_id: i64,
) -> Result<BTreeMap<i64, i64>, PersonalServiceError> {
    root.get("box_gacha_drawn_rewards")
        .and_then(Value::as_object)
        .and_then(|gachas| gachas.get(&box_gacha_id.to_string()))
        .and_then(Value::as_object)
        .and_then(|boxes| boxes.get(&box_id.to_string()))
        .and_then(Value::as_object)
        .map(|drawn| {
            drawn
                .iter()
                .map(|(reward_id, number)| {
                    Ok((
                        reward_id.parse::<i64>().map_err(|_| {
                            PersonalServiceError::new("stored CN box reward id is invalid")
                        })?,
                        number.as_i64().ok_or_else(|| {
                            PersonalServiceError::new("stored CN box reward count is invalid")
                        })?,
                    ))
                })
                .collect()
        })
        .unwrap_or_else(|| Ok(BTreeMap::new()))
}

pub(super) fn save_drawn_reward_counts(
    root: &mut Map<String, Value>,
    box_gacha_id: i64,
    box_id: i64,
    drawn: &BTreeMap<i64, i64>,
) -> Result<(), PersonalServiceError> {
    let gachas = root
        .entry("box_gacha_drawn_rewards".to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN box reward state is invalid"))?;
    let boxes = gachas
        .entry(box_gacha_id.to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN box reward state is invalid"))?;
    boxes.insert(
        box_id.to_string(),
        Value::Object(
            drawn
                .iter()
                .map(|(reward_id, number)| (reward_id.to_string(), Value::from(*number)))
                .collect(),
        ),
    );
    Ok(())
}

pub(super) fn box_available_count(gacha: &Map<String, Value>, box_id: i64) -> i64 {
    gacha
        .get("availableCounts")
        .and_then(Value::as_object)
        .and_then(|counts| counts.get(&box_id.to_string()))
        .and_then(Value::as_i64)
        .unwrap_or_default()
}

pub(super) fn box_gacha(box_gacha_id: i64) -> Result<Map<String, Value>, PersonalServiceError> {
    parsed_asset(&BOX_GACHA_DATA, BOX_GACHA_ASSET)?
        .get(box_gacha_id.to_string())
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| PersonalServiceError::new("CN box gacha does not exist"))
}

pub(super) fn box_rewards(box_gacha_id: i64) -> Result<Map<String, Value>, PersonalServiceError> {
    parsed_asset(&BOX_REWARD_DATA, BOX_REWARD_ASSET)?
        .get(box_gacha_id.to_string())
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| PersonalServiceError::new("CN box rewards do not exist"))
}

// //// 查询包含指定奖励的 CN 箱池 [@x380kkm 2026-08-24] ////
pub(super) fn box_gacha_ids_for_reward(
    item_id: Option<i64>,
    equipment_id: Option<i64>,
) -> Result<Vec<i64>, PersonalServiceError> {
    let (reward_type, target_id) = match (item_id, equipment_id) {
        (Some(item_id), None) if item_id > 0 => (0, item_id),
        (None, Some(equipment_id)) if equipment_id > 0 => (1, equipment_id),
        _ => return Ok(Vec::new()),
    };
    let document = parsed_asset(&BOX_REWARD_DATA, BOX_REWARD_ASSET)?
        .as_object()
        .ok_or_else(|| PersonalServiceError::new("CN box reward asset is not an object"))?;
    let mut box_gacha_ids = document
        .iter()
        .filter_map(|(box_gacha_id, boxes)| {
            boxes
                .as_object()
                .into_iter()
                .flat_map(Map::values)
                .filter_map(Value::as_object)
                .flat_map(Map::values)
                .filter_map(Value::as_object)
                .any(|reward| {
                    reward.get("type").and_then(Value::as_i64) == Some(reward_type)
                        && reward.get("id").and_then(Value::as_i64) == Some(target_id)
                })
                .then(|| box_gacha_id.parse::<i64>().ok())
                .flatten()
        })
        .collect::<Vec<_>>();
    box_gacha_ids.sort_unstable();
    box_gacha_ids.dedup();
    Ok(box_gacha_ids)
}
// //// /查询包含指定奖励的 CN 箱池 ////

fn find_box_status(
    root: &Map<String, Value>,
    box_gacha_id: i64,
    box_id: i64,
) -> Option<&Map<String, Value>> {
    root.get("box_gacha_list")
        .and_then(Value::as_object)
        .and_then(|lists| lists.get(&box_gacha_id.to_string()))
        .and_then(Value::as_array)
        .and_then(|list| {
            list.iter()
                .find(|status| status.get("box_id").and_then(Value::as_i64) == Some(box_id))
        })
        .and_then(Value::as_object)
}

fn parsed_asset(
    cache: &'static OnceLock<Result<Value, String>>,
    asset: &'static str,
) -> Result<&'static Value, PersonalServiceError> {
    cache
        .get_or_init(|| {
            serde_json::from_str(asset)
                .map_err(|error| format!("failed to decode CN box asset: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}
