// audience: internal
// # personal-service-cn-mission-battle-conditions
//
// 战力使用本场开战统计, 缺少统计时读取匹配参战编队的保存值. 技能槽条件使用首个战斗区域的开场统计.

use crate::cn_battle_state::FinishBattleInput;
use serde_json::{Map, Value};

fn slots_equal(party: &Value, field: &str, actual: &[Option<i64>]) -> bool {
    (0..3).all(|index| {
        party
            .get(field)
            .and_then(|slots| slots.get(index))
            .and_then(Value::as_i64)
            == actual.get(index).copied().flatten()
    })
}

// //// 读取参战编队战力与开场技能槽统计 [@x380kkm 2026-09-21] ////
pub(crate) fn battle_power(
    root: &Map<String, Value>,
    input: &FinishBattleInput<'_>,
    statistics: &Value,
) -> Option<i64> {
    if let Some(power) = statistics.get("max_power") {
        return power.as_i64().filter(|power| *power >= 0);
    }
    if input.party_id <= 0 {
        return None;
    }
    let group_id = (input.party_id - 1) / 10 + 1;
    let slot = (input.party_id - 1) % 10 + 1;
    let party = root
        .get("user_party_group_list")?
        .get(group_id.to_string())?
        .get("list")?
        .get(slot.to_string())?;
    if !slots_equal(party, "character_ids", input.main_character_ids)
        || !slots_equal(party, "unison_character_ids", input.unison_character_ids)
        || !slots_equal(party, "equipment_ids", input.equipment_ids)
        || !slots_equal(party, "ability_soul_ids", input.ability_soul_ids)
    {
        return None;
    }
    party
        .get("current_battle_power")?
        .as_i64()
        .filter(|power| *power >= 0)
}

pub(crate) fn has_complete_skill_gauges(statistics: &Value) -> bool {
    statistics
        .get("zones")
        .and_then(Value::as_array)
        .and_then(|zones| zones.first())
        .and_then(|zone| zone.get("skill_point_over_on_start"))
        .and_then(Value::as_i64)
        .is_some_and(|count| count >= 3)
}
// //// /读取参战编队战力与开场技能槽统计 ////
