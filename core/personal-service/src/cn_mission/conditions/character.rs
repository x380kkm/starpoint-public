// audience: internal
// # personal-service-cn-mission-character-conditions
//
// 玛纳节点条件区分能力学会与整板完成. 技能进化编号按已解锁的技能进化节点计算.

use crate::PersonalServiceError;
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::sync::OnceLock;

static MANA_NODES: OnceLock<Result<Value, String>> = OnceLock::new();

fn nodes() -> Result<&'static Value, PersonalServiceError> {
    MANA_NODES
        .get_or_init(|| {
            serde_json::from_str(include_str!("../../../../../assets/mana_node.json"))
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

fn unlocked_nodes(root: &Map<String, Value>, character_id: i64) -> BTreeSet<i64> {
    root.get("user_character_mana_node_list")
        .and_then(|characters| characters.get(character_id.to_string()))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|node| {
            node.as_i64()
                .or_else(|| node.get("mana_node_multiplied_id").and_then(Value::as_i64))
                .or_else(|| node.get("multiplied_id").and_then(Value::as_i64))
        })
        .collect()
}

pub(crate) fn character_level(root: &Map<String, Value>, character_id: i64) -> i64 {
    let Some(character) = root
        .get("user_character_list")
        .and_then(|characters| characters.get(character_id.to_string()))
    else {
        return 0;
    };
    let rarity = super::battle_fixture()
        .ok()
        .and_then(|fixture| fixture.characters.get(&character_id.to_string()))
        .map(|character| character.rarity);
    rarity
        .and_then(|rarity| {
            crate::cn_expod::character_level_from_experience(
                rarity,
                character
                    .get("exp")
                    .and_then(Value::as_i64)
                    .unwrap_or_default(),
                character
                    .get("over_limit_step")
                    .and_then(Value::as_i64)
                    .unwrap_or_default(),
            )
        })
        .or_else(|| character.get("level").and_then(Value::as_i64))
        .unwrap_or_default()
}

// //// 分别核对第二板完整节点与能力四至六 [@x380kkm 2026-09-21] ////
pub(crate) fn has_complete_second_board(
    root: &Map<String, Value>,
    character_id: i64,
) -> Result<bool, PersonalServiceError> {
    let Some(expected) = nodes()?
        .get(character_id.to_string())
        .and_then(|boards| boards.get("2"))
        .and_then(Value::as_object)
    else {
        return Ok(false);
    };
    if expected.is_empty() {
        return Ok(false);
    }
    let unlocked = unlocked_nodes(root, character_id);
    Ok(expected.keys().all(|node| {
        node.parse::<i64>()
            .ok()
            .is_some_and(|node| unlocked.contains(&node))
    }))
}

pub(crate) fn has_second_board_abilities(
    root: &Map<String, Value>,
    character_id: i64,
) -> Result<bool, PersonalServiceError> {
    let Some(expected) = nodes()?
        .get(character_id.to_string())
        .and_then(|boards| boards.get("2"))
        .and_then(Value::as_object)
    else {
        return Ok(false);
    };
    let unlocked = unlocked_nodes(root, character_id);
    Ok(["4", "5", "6"].iter().all(|slot| {
        expected.iter().any(|(id, node)| {
            node.get("field5").and_then(Value::as_str) == Some("0")
                && node.get("field6").and_then(Value::as_str) == Some(*slot)
                && id
                    .parse::<i64>()
                    .ok()
                    .is_some_and(|id| unlocked.contains(&id))
        })
    }))
}
// //// /分别核对第二板完整节点与能力四至六 ////

pub(super) fn skill_evolution(
    root: &Map<String, Value>,
    character_id: i64,
) -> Result<i64, PersonalServiceError> {
    let unlocked = unlocked_nodes(root, character_id);
    let count = nodes()?
        .get(character_id.to_string())
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|boards| boards.values())
        .filter_map(Value::as_object)
        .flat_map(|nodes| nodes.iter())
        .filter(|(id, node)| {
            node.get("field5").and_then(Value::as_str) == Some("2")
                && id
                    .parse::<i64>()
                    .ok()
                    .is_some_and(|id| unlocked.contains(&id))
        })
        .count() as i64;
    Ok(1 + count)
}
