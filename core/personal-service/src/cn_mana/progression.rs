// audience: internal
// # personal-service-cn-mana-progression
//
// 角色进化使用一板各能力槽和技能进化节点的解锁状态. 每块板的羁绊状态使用该板全部节点的解锁状态.

use super::{mana_node_document, mana_nodes, node_awake_levels};
use crate::PersonalServiceError;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

// //// 按节点所属板计算整板完成状态 [@x380kkm 2026-09-23] ////
pub(super) fn completed_boards(
    character_id: i64,
    board_indices: &BTreeSet<i64>,
    unlocked: &BTreeSet<i64>,
) -> Result<Vec<i64>, PersonalServiceError> {
    let mut completed = Vec::new();
    for board_index in board_indices {
        let nodes = mana_nodes(character_id, *board_index)?;
        if nodes.keys().all(|node_id| {
            node_id
                .parse::<i64>()
                .ok()
                .is_some_and(|id| unlocked.contains(&id))
        }) {
            completed.push(*board_index);
        }
    }
    Ok(completed)
}
// //// /按节点所属板计算整板完成状态 ////

// //// 按一板能力槽和技能进化节点计算角色进化等级 [@x380kkm 2026-09-23] ////
fn evolution_level(
    character_id: i64,
    awake_levels: &BTreeMap<i64, i64>,
) -> Result<i64, PersonalServiceError> {
    let mut abilities = BTreeMap::<&str, bool>::new();
    let mut skill_evolution = None;
    let mut skill_awake_level = 0;
    for (node_id, node) in mana_nodes(character_id, 1)? {
        let awake_level = node_id
            .parse::<i64>()
            .ok()
            .and_then(|id| awake_levels.get(&id));
        let learned = awake_level.is_some();
        match node.get("field5").and_then(Value::as_str) {
            Some("0") => {
                let slot = node
                    .get("field6")
                    .and_then(Value::as_str)
                    .ok_or_else(|| PersonalServiceError::new("CN Mana ability slot is missing"))?;
                let has_learned = abilities.entry(slot).or_default();
                *has_learned |= learned;
            }
            Some("2") => {
                *skill_evolution.get_or_insert(false) |= learned;
                skill_awake_level = awake_level.copied().unwrap_or_default();
            }
            _ => {}
        }
    }
    if abilities.values().all(|learned| *learned) && skill_evolution.unwrap_or(true) {
        skill_awake_level
            .checked_add(1)
            .ok_or_else(|| PersonalServiceError::new("CN character evolution level exceeds range"))
    } else {
        Ok(0)
    }
}
// //// /按一板能力槽和技能进化节点计算角色进化等级 ////

// //// 将立绘进化等级限制在角色资源档位内 [@x380kkm 2026-09-23] ////
pub(super) fn illustration_evolution_level(evolution_level: i64) -> i64 {
    evolution_level.min(1)
}
// //// /将立绘进化等级限制在角色资源档位内 ////

// //// 根据已保存节点更新全部角色的进化状态 [@x380kkm 2026-09-23] ////
pub(crate) fn reconcile_player_evolution(
    root: &mut Map<String, Value>,
) -> Result<(), PersonalServiceError> {
    let character_ids = root
        .get("user_character_list")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|characters| characters.keys())
        .filter_map(|id| id.parse::<i64>().ok())
        .collect::<Vec<_>>();
    for character_id in character_ids {
        reconcile_character_evolution(root, character_id)?;
    }
    Ok(())
}
// //// /根据已保存节点更新全部角色的进化状态 ////

// //// 更新角色节点对应的进化等级并返回发生的提升 [@x380kkm 2026-09-23] ////
pub(super) fn reconcile_character_evolution(
    root: &mut Map<String, Value>,
    character_id: i64,
) -> Result<Option<i64>, PersonalServiceError> {
    if mana_node_document()?
        .get(character_id.to_string())
        .and_then(|boards| boards.get("1"))
        .and_then(Value::as_object)
        .is_none()
    {
        return Ok(None);
    }
    let awake_levels = node_awake_levels(root, character_id)?;
    let derived_level = evolution_level(character_id, &awake_levels)?;
    let Some(character) = root
        .get_mut("user_character_list")
        .and_then(Value::as_object_mut)
        .and_then(|characters| characters.get_mut(&character_id.to_string()))
        .and_then(Value::as_object_mut)
    else {
        return Ok(None);
    };
    let current_level = character
        .get("evolution_level")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    if (0..=2).contains(&current_level) && derived_level <= current_level {
        return Ok(None);
    }
    character.insert("evolution_level".to_owned(), Value::from(derived_level));
    Ok(Some(derived_level))
}
// //// /更新角色节点对应的进化等级并返回发生的提升 ////
