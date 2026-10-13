// audience: internal
// # personal-service-cn-mana
//
// 该模块按 CN Mana node 资产执行节点解锁, 觉醒和载入投影, 扣除资源并保存玩家快照.

mod awake;
mod learn;
mod progression;
mod state;

pub(crate) use progression::reconcile_player_evolution;
pub(crate) use state::{validated_node_awake_levels, visible_node_awake_levels};
use state::{node_awake_levels, total_unlocked_node_count};

use crate::cn_character::character_time;
use crate::cn_tutorial::require_object;
use crate::database::{parse_iso_timestamp, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

const MANA_NODE_ASSET: &str = include_str!("../../../assets/mana_node.json");
const MANA_BOARD2_OPEN_CONDITION_ASSET: &str =
    include_str!("../assets/cn-mana-board2-open-condition.json");
const CN_TIMEZONE_OFFSET_MILLISECONDS: i64 = 8 * 60 * 60 * 1_000;
static MANA_NODE_DATA: OnceLock<Result<Value, String>> = OnceLock::new();
static MANA_BOARD2_OPEN_CONDITION_DATA: OnceLock<Result<Value, String>> = OnceLock::new();

// //// 分派 CN Mana node 请求 [@x380kkm 2026-07-24] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    let response = match request.path() {
        "/api/index.php/character/learn_mana_node" => learn::learn_mana_node(request, database),
        "/api/index.php/character/awake_mana_node" => awake::awake_mana_node(request, database),
        _ => return None,
    };
    Some(response)
}
// //// /分派 CN Mana node 请求 ////

fn mana_nodes(
    character_id: i64,
    board_index: i64,
) -> Result<&'static Map<String, Value>, PersonalServiceError> {
    mana_node_document()?
        .get(&character_id.to_string())
        .and_then(Value::as_object)
        .and_then(|boards| boards.get(&board_index.to_string()))
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("CN Mana node board is missing"))
}

pub(super) fn mana_node(
    character_id: i64,
    node_id: i64,
) -> Result<&'static Map<String, Value>, PersonalServiceError> {
    mana_node_with_board(character_id, node_id).map(|(_, node)| node)
}

// //// 读取节点定义及其所属玛纳板 [@x380kkm 2026-09-23] ////
fn mana_node_with_board(
    character_id: i64,
    node_id: i64,
) -> Result<(i64, &'static Map<String, Value>), PersonalServiceError> {
    mana_node_document()?
        .get(&character_id.to_string())
        .and_then(Value::as_object)
        .and_then(|boards| {
            boards.iter().find_map(|(board_index, nodes)| {
                let node = nodes.get(node_id.to_string())?.as_object()?;
                Some((board_index.parse().ok()?, node))
            })
        })
        .ok_or_else(|| PersonalServiceError::new("CN Mana node is missing"))
}
// //// /读取节点定义及其所属玛纳板 ////

// //// 读取 CN Mana node 资产 [@x380kkm 2026-08-28] ////
fn mana_node_document() -> Result<&'static Value, PersonalServiceError> {
    MANA_NODE_DATA
        .get_or_init(|| {
            serde_json::from_str::<Value>(MANA_NODE_ASSET)
                .map_err(|error| format!("failed to decode Mana node asset: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}
// //// /读取 CN Mana node 资产 ////

// //// 按客户端时间投影 CN load 的 Mana board 状态 [@x380kkm 2026-08-28] ////
pub(crate) fn project_load_mana_board_state(
    player_data: &mut Value,
    response_time: i64,
) -> Result<(), PersonalServiceError> {
    let root = player_data
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("CN load player data is not an object"))?;
    let visible_board_indexes = {
        let characters = root
            .get("user_character_list")
            .and_then(Value::as_object)
            .ok_or_else(|| PersonalServiceError::new("stored CN character list is missing"))?;
        let mut visible_board_indexes = BTreeMap::new();
        for (character_id, character) in characters {
            let Some(character) = character.as_object() else {
                continue;
            };
            let Ok(character_id_number) = character_id.parse::<i64>() else {
                continue;
            };
            let stored_board_index = character
                .get("mana_board_index")
                .and_then(Value::as_i64)
                .filter(|index| *index > 0)
                .unwrap_or(1);
            visible_board_indexes.insert(
                character_id.clone(),
                visible_mana_board_index(character_id_number, stored_board_index, response_time)?,
            );
        }
        visible_board_indexes
    };

    if let Some(characters) = root
        .get_mut("user_character_list")
        .and_then(Value::as_object_mut)
    {
        for (character_id, visible_board_index) in &visible_board_indexes {
            if let Some(character) = characters
                .get_mut(character_id)
                .and_then(Value::as_object_mut)
            {
                character.insert(
                    "mana_board_index".to_owned(),
                    Value::from(*visible_board_index),
                );
            }
        }
    }

    let mut projected_character_nodes = Map::new();
    for (character_id, visible_board_index) in &visible_board_indexes {
        let Ok(character_id_number) = character_id.parse::<i64>() else {
            continue;
        };
        let visible_node_ids = visible_mana_node_ids(character_id_number, *visible_board_index)?;
        let projected_nodes = node_awake_levels(root, character_id_number)?.into_iter()
            .filter(|(node_id, _)| visible_node_ids.contains(node_id))
            .map(|(node_id, awake_level)| json!({"multiplied_id": node_id, "awake_level": awake_level}))
            .collect();
        projected_character_nodes.insert(character_id.clone(), Value::Array(projected_nodes));
    }
    root.insert(
        "user_character_mana_node_list".to_owned(),
        Value::Object(projected_character_nodes),
    );
    Ok(())
}
// //// /按客户端时间投影 CN load 的 Mana board 状态 ////

// //// 计算角色当前可见的 Mana board 索引 [@x380kkm 2026-08-28] ////
fn visible_mana_board_index(
    character_id: i64,
    stored_board_index: i64,
    response_time: i64,
) -> Result<i64, PersonalServiceError> {
    let has_board_two = mana_node_document()?
        .get(&character_id.to_string())
        .and_then(Value::as_object)
        .is_some_and(|boards| boards.contains_key("2"));
    let highest_visible_board =
        if has_board_two && mana_board_two_is_open(character_id, response_time)? {
            2
        } else {
            1
        };
    Ok(stored_board_index.clamp(1, highest_visible_board))
}
// //// /计算角色当前可见的 Mana board 索引 ////

// //// 按 CN 客户端时区检查 Mana board 2 开放窗口 [@x380kkm 2026-08-28] ////
fn mana_board_two_is_open(
    character_id: i64,
    response_time: i64,
) -> Result<bool, PersonalServiceError> {
    let document = MANA_BOARD2_OPEN_CONDITION_DATA.get_or_init(|| {
        serde_json::from_str::<Value>(MANA_BOARD2_OPEN_CONDITION_ASSET)
            .map_err(|error| format!("failed to decode Mana board 2 open conditions: {error}"))
    });
    let document = document
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    let Some(window) = document
        .get(&character_id.to_string())
        .and_then(Value::as_array)
    else {
        return Ok(false);
    };
    let start_time = window
        .first()
        .and_then(Value::as_str)
        .map(parse_condition_timestamp)
        .transpose()?
        .flatten();
    let end_time = window
        .get(1)
        .and_then(Value::as_str)
        .map(parse_condition_timestamp)
        .transpose()?
        .flatten();
    let response_time = response_time.saturating_mul(1_000);
    Ok(
        start_time.map_or(true, |start_time| response_time >= start_time)
            && end_time.map_or(true, |end_time| response_time <= end_time),
    )
}
// //// /按 CN 客户端时区检查 Mana board 2 开放窗口 ////

// //// 解析 CN Mana board 条件的本地时间 [@x380kkm 2026-08-28] ////
fn parse_condition_timestamp(value: &str) -> Result<Option<i64>, PersonalServiceError> {
    if value == "(None)" {
        return Ok(None);
    }
    let normalized = format!("{}.000Z", value.replace(' ', "T"));
    let timestamp = parse_iso_timestamp(&normalized)
        .ok_or_else(|| PersonalServiceError::new("CN Mana board condition time is invalid"))?;
    timestamp
        .checked_sub(CN_TIMEZONE_OFFSET_MILLISECONDS)
        .map(Some)
        .ok_or_else(|| PersonalServiceError::new("CN Mana board condition time is out of range"))
}
// //// /解析 CN Mana board 条件的本地时间 ////

// //// 读取角色已开放板的 Mana node ID [@x380kkm 2026-08-28] ////
fn visible_mana_node_ids(
    character_id: i64,
    mana_board_index: i64,
) -> Result<BTreeSet<i64>, PersonalServiceError> {
    let Some(boards) = mana_node_document()?
        .get(&character_id.to_string())
        .and_then(Value::as_object)
    else {
        return Ok(BTreeSet::new());
    };
    let mut visible_node_ids = BTreeSet::new();
    for (board_index, nodes) in boards {
        let board_index = board_index
            .parse::<i64>()
            .map_err(|_| PersonalServiceError::new("CN Mana board index is invalid"))?;
        if board_index > mana_board_index {
            continue;
        }
        let nodes = nodes
            .as_object()
            .ok_or_else(|| PersonalServiceError::new("CN Mana node board is invalid"))?;
        for node_id in nodes.keys() {
            visible_node_ids.insert(
                node_id
                    .parse::<i64>()
                    .map_err(|_| PersonalServiceError::new("CN Mana node id is invalid"))?,
            );
        }
    }
    Ok(visible_node_ids)
}
// //// /读取角色已开放板的 Mana node ID ////

fn deduct_mana(free_mana: i64, paid_mana: i64, cost: i64) -> Option<(i64, i64)> {
    if cost <= free_mana {
        return Some((free_mana - cost, paid_mana));
    }
    let paid_cost = cost.checked_sub(free_mana)?;
    let remaining_paid = paid_mana.checked_sub(paid_cost)?;
    (remaining_paid >= 0).then_some((0, remaining_paid))
}

pub(super) fn user_info_value(
    root: &Map<String, Value>,
    key: &str,
) -> Result<i64, PersonalServiceError> {
    root.get("user_info")
        .and_then(Value::as_object)
        .and_then(|info| info.get(key))
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new(format!("stored CN {key} value is missing")))
}

pub(super) fn set_user_info_value(
    root: &mut Map<String, Value>,
    key: &str,
    value: i64,
) -> Result<(), PersonalServiceError> {
    require_object(root, "user_info")?.insert(key.to_owned(), Value::from(value));
    Ok(())
}

pub(super) fn character_value<'a>(
    root: &'a Map<String, Value>,
    character_id: i64,
) -> Result<&'a Map<String, Value>, HttpResponse> {
    root.get("user_character_list")
        .and_then(Value::as_object)
        .and_then(|characters| characters.get(&character_id.to_string()))
        .and_then(Value::as_object)
        .ok_or_else(|| error_response("400 Bad Request", "character_not_owned"))
}

pub(super) fn character_mut<'a>(
    root: &'a mut Map<String, Value>,
    character_id: i64,
) -> Result<&'a mut Map<String, Value>, PersonalServiceError> {
    root.get_mut("user_character_list")
        .and_then(Value::as_object_mut)
        .and_then(|characters| characters.get_mut(&character_id.to_string()))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("CN character is not owned"))
}

pub(super) fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}

#[cfg(test)]
mod tests {
    use super::visible_mana_board_index;
    use crate::database::parse_iso_timestamp;

    // //// 按角色开放时刻投影 Mana board 2 [@x380kkm 2026-08-28] ////
    #[test]
    fn projects_character_specific_mana_board_time_window() {
        let before_open = parse_iso_timestamp("2025-02-13T03:59:59.000Z")
            .expect("Mana board time is valid")
            / 1_000;
        let at_open = parse_iso_timestamp("2025-02-13T04:00:00.000Z")
            .expect("Mana board time is valid")
            / 1_000;

        assert_eq!(
            visible_mana_board_index(111_147, 2, before_open)
                .expect("Mana board index is projected"),
            1
        );
        assert_eq!(
            visible_mana_board_index(111_147, 2, at_open).expect("Mana board index is projected"),
            2
        );
    }
    // //// /按角色开放时刻投影 Mana board 2 ////
}
