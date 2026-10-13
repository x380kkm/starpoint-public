// audience: internal
// # personal-service-cn-mana-state
// 节点读取限定角色主表中的有效 ID 和觉醒档位. 存档中的其他记录由原字段保留.

use super::mana_node_with_board;
use crate::cn_tutorial::require_object;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

// //// 读取兼容格式中的节点 ID [@x380kkm 2026-09-24] ////
pub(super) fn node_id(node: &Value) -> Option<i64> {
    node.get("mana_node_multiplied_id")
        .and_then(Value::as_i64)
        .or_else(|| node.get("multiplied_id").and_then(Value::as_i64))
        .or_else(|| node.as_i64())
}

// //// 按角色主表归一化已学习节点的觉醒等级 [@x380kkm 2026-09-24] ////
pub(super) fn node_awake_levels(
    root: &Map<String, Value>,
    character_id: i64,
) -> Result<BTreeMap<i64, i64>, PersonalServiceError> {
    validated_node_awake_levels(
        character_id,
        root.get("user_character_mana_node_list")
            .and_then(|characters| characters.get(character_id.to_string()))
            .unwrap_or(&Value::Null),
    )
}

// //// 读取属于当前角色的有效节点与觉醒等级 [@x380kkm 2026-09-24] ////
pub(crate) fn validated_node_awake_levels(
    character_id: i64,
    records: &Value,
) -> Result<BTreeMap<i64, i64>, PersonalServiceError> {
    super::mana_node_document()?;
    let records = records
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(std::slice::from_ref(records));
    let mut levels = BTreeMap::<i64, i64>::new();
    for record in records {
        let Some(id) = node_id(record) else { continue };
        let Ok((board, node)) = mana_node_with_board(character_id, id) else {
            continue;
        };
        let maximum =
            i64::from(board == 1 && node.get("field1").and_then(Value::as_str) == Some("0"));
        let level = record
            .get("awake_level")
            .and_then(Value::as_i64)
            .unwrap_or_default()
            .clamp(0, maximum);
        levels
            .entry(id)
            .and_modify(|stored| *stored = (*stored).max(level))
            .or_insert(level);
    }
    Ok(levels)
}
// //// /读取属于当前角色的有效节点与觉醒等级 ////

// //// 按角色板索引和入场时间筛选有效节点及觉醒等级 [@x380kkm 2026-09-25] ////
pub(crate) fn visible_node_awake_levels(
    character_id: i64,
    stored_board_index: i64,
    records: &Value,
    response_time: i64,
) -> Result<BTreeMap<i64, i64>, PersonalServiceError> {
    let board = super::visible_mana_board_index(character_id, stored_board_index, response_time)?;
    let visible = super::visible_mana_node_ids(character_id, board)?;
    Ok(validated_node_awake_levels(character_id, records)?.into_iter()
        .filter(|(id, _)| visible.contains(id)).collect())
}
// //// /按角色板索引和入场时间筛选有效节点及觉醒等级 ////

// //// 统计已拥有角色的有效学习节点 [@x380kkm 2026-09-24] ////
pub(super) fn total_unlocked_node_count(
    root: &Map<String, Value>,
) -> Result<i64, PersonalServiceError> {
    let mut count = 0_usize;
    for character_id in root
        .get("user_character_list")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|characters| characters.keys())
        .filter_map(|id| id.parse::<i64>().ok())
    {
        count = count
            .checked_add(node_awake_levels(root, character_id)?.len())
            .ok_or_else(|| {
                PersonalServiceError::new("CN unlocked Mana node count exceeds range")
            })?;
    }
    i64::try_from(count)
        .map_err(|_| PersonalServiceError::new("CN unlocked Mana node count exceeds range"))
}
// //// /统计已拥有角色的有效学习节点 ////

// //// 为节点写入保留原记录及兼容格式 [@x380kkm 2026-09-24] ////
pub(super) fn stored_nodes_mut(
    root: &mut Map<String, Value>,
    character_id: i64,
) -> Result<&mut Vec<Value>, PersonalServiceError> {
    let entry = require_object(root, "user_character_mana_node_list")?
        .entry(character_id.to_string())
        .or_insert_with(|| json!([]));
    if !entry.is_array() {
        let original = entry.take();
        *entry = Value::Array(vec![original]);
    }
    Ok(entry
        .as_array_mut()
        .expect("CN Mana node records are an array"))
}

// //// 仅更新本次觉醒节点并保留其余存档内容 [@x380kkm 2026-09-24] ////
pub(super) fn save_awake_nodes(
    root: &mut Map<String, Value>,
    character_id: i64,
    updates: &BTreeMap<i64, i64>,
) -> Result<(), PersonalServiceError> {
    for record in stored_nodes_mut(root, character_id)? {
        let Some(id) = node_id(record) else { continue };
        let Some(level) = updates.get(&id) else {
            continue;
        };
        if let Some(fields) = record.as_object_mut() {
            fields.insert("awake_level".to_owned(), json!(level));
        } else {
            *record = json!({"mana_node_multiplied_id": id, "awake_level": level});
        }
    }
    Ok(())
}
// //// /仅更新本次觉醒节点并保留其余存档内容 ////
