// audience: internal
// # personal-service-cn-raid-party-members
// 团队战固定展示三个队长. 角色充足时同组角色唯一, 空队使用持有角色补齐.

use crate::cn_party::PartyInventory;
use serde_json::{json, Value};
use std::collections::BTreeSet;

const PARTY_COUNT: usize = 3;
const SLOT_FIELDS: [&str; 4] = [
    "character_ids", "unison_character_ids", "equipment_ids", "ability_soul_ids",
];

// //// 整理团队战队长和跨队角色占用 [@x380kkm 2026-09-28] ////
pub(crate) fn normalize_raid_party_groups(
    groups: &mut Vec<Value>,
    inventory: &PartyInventory,
    edited_party_ids: &BTreeSet<i64>,
) {
    complete_groups(groups);
    for group in groups {
        let Some(parties) = group.get_mut("party_list").and_then(Value::as_array_mut) else {
            continue;
        };
        let mut order = (0..parties.len()).collect::<Vec<_>>();
        order.sort_by_key(|index| {
            parties[*index]
                .get("party_id")
                .and_then(Value::as_i64)
                .is_some_and(|id| edited_party_ids.contains(&id))
        });
        let mut used = BTreeSet::new();
        for &index in &order {
            let Some(party) = parties[index].as_object_mut() else {
                continue;
            };
            inventory.normalize(party);
            for key in ["character_ids", "unison_character_ids"] {
                for slot in party
                    .get_mut(key)
                    .and_then(Value::as_array_mut)
                    .into_iter()
                    .flatten()
                {
                    if slot.as_i64().is_some_and(|id| id > 0 && !used.insert(id)) {
                        *slot = Value::Null;
                    }
                }
            }
            inventory.normalize(party);
        }
        for &index in &order {
            if parties[index]["character_ids"][0].as_i64().is_none() {
                fill_leader(parties, index, inventory, &mut used);
            }
        }
    }
}
// //// /整理团队战队长和跨队角色占用 ////

// //// 保留三个团队战槽位的元数据并补齐缺少的队伍 [@x380kkm 2026-09-28] ////
fn complete_groups(groups: &mut Vec<Value>) {
    if groups.is_empty() {
        groups.push(json!({"party_group_id":1,"party_group_color_id":15,"party_list":[]}));
    }
    groups.truncate(1);
    for group in groups {
        group["party_group_id"] = json!(1);
        let Some(parties) = group.get_mut("party_list").and_then(Value::as_array_mut) else {
            continue;
        };
        parties.truncate(PARTY_COUNT);
        while parties.len() < PARTY_COUNT {
            let id = parties.len() as i64 + 1;
            parties.push(json!({
                "party_id":id,"party_name":format!("Party {id}"),"party_edited":false,
                "character_ids":[null,null,null],"unison_character_ids":[null,null,null],
                "equipment_ids":[null,null,null],"ability_soul_ids":[null,null,null],
                "options":{"allow_other_players_to_heal_me":true},
            }));
        }
        for (index, party) in parties.iter_mut().enumerate() {
            party["party_id"] = json!(index + 1);
        }
    }
}

// //// 从本队和未占用持有角色补齐真实队长 [@x380kkm 2026-09-28] ////
fn fill_leader(
    parties: &mut [Value],
    index: usize,
    inventory: &PartyInventory,
    used: &mut BTreeSet<i64>,
) {
    if let Some(slot) = (1..3).find(|slot| parties[index]["character_ids"][*slot].as_i64().is_some()) {
        for field in SLOT_FIELDS {
            parties[index][field].as_array_mut().unwrap().swap(0, slot);
        }
        return;
    }
    if let Some(slot) = (0..3).find(|slot| parties[index]["unison_character_ids"][*slot].as_i64().is_some()) {
        let character = parties[index]["unison_character_ids"][slot].take();
        parties[index]["character_ids"][0] = character;
        return;
    }
    if let Some(character) = inventory.character_ids().find(|id| !used.contains(id)) {
        parties[index]["character_ids"][0] = json!(character);
        used.insert(character);
        return;
    }
    if move_support_character(parties, index) {
        return;
    }
    if let Some(character) = inventory.character_ids().next() {
        parties[index]["character_ids"][0] = json!(character);
    }
}

// //// 将其他队伍的非队长角色与装配移入空队 [@x380kkm 2026-09-28] ////
fn move_support_character(parties: &mut [Value], target: usize) -> bool {
    for donor in (0..parties.len()).rev().filter(|donor| *donor != target) {
        if let Some(slot) = (1..3).rev().find(|slot| parties[donor]["character_ids"][*slot].as_i64().is_some()) {
            for field in SLOT_FIELDS {
                let value = parties[donor][field][slot].take();
                parties[target][field][0] = value;
            }
            return true;
        }
        if let Some(slot) = (0..3).rev().find(|slot| parties[donor]["unison_character_ids"][*slot].as_i64().is_some()) {
            let character = parties[donor]["unison_character_ids"][slot].take();
            parties[target]["character_ids"][0] = character;
            return true;
        }
    }
    false
}
