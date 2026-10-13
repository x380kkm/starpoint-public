// audience: internal
// # personal-service-cn-party-slots
// 魂珠按单队装配数量核对库存, 装卸保留物品持有量. 四种槽位均保存三个位置.

use super::catalog::{normalize_group_color, rules, PartyRules};
use crate::PersonalServiceError;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct PartyInventory {
    characters: BTreeSet<i64>,
    equipment: BTreeSet<i64>,
    souls: BTreeMap<i64, i64>,
    rules: &'static PartyRules,
}

// //// 读取编队引用的持有记录 [@x380kkm 2026-09-23] ////
impl PartyInventory {
    pub(crate) fn from_player(root: &Map<String, Value>) -> Result<Self, PersonalServiceError> {
        let rules = rules()?;
        Ok(Self {
            characters: owned_ids(root, "user_character_list")?.into_iter()
                .filter(|id| rules.character_elements.contains_key(id)).collect(),
            equipment: owned_ids(root, "user_equipment_list")?,
            souls: root
                .get("item_list")
                .and_then(Value::as_object)
                .into_iter()
                .flat_map(|items| items.iter())
                .filter_map(|(id, count)| Some((id.parse().ok()?, count.as_i64()?)))
                .filter(|(_, count)| *count > 0)
                .collect(),
            rules,
        })
    }

    // //// 枚举玩家持有且属于角色主表的角色 [@x380kkm 2026-09-28] ////
    pub(crate) fn character_ids(&self) -> impl Iterator<Item = i64> + '_ {
        self.characters.iter().copied()
    }

    // //// 归一编队槽位并保留有效的角色和物品引用 [@x380kkm 2026-09-23] ////
    pub(crate) fn normalize(&self, party: &mut Map<String, Value>) {
        let mut characters = slots(party, "character_ids");
        let mut unisons = slots(party, "unison_character_ids");
        let mut used_characters = BTreeSet::new();
        for slot in characters.iter_mut().chain(unisons.iter_mut()) {
            *slot = slot.filter(|id| self.characters.contains(id) && used_characters.insert(*id));
        }
        let mut equipment = slots(party, "equipment_ids");
        let mut souls = slots(party, "ability_soul_ids");
        let mut used_equipment = BTreeSet::new();
        let mut used_souls = BTreeMap::<i64, i64>::new();
        for index in 0..3 {
            equipment[index] = equipment[index].filter(|id| {
                characters[index].is_some()
                    && self.equipment.contains(id)
                    && used_equipment.insert(*id)
            });
            souls[index] = souls[index].filter(|id| {
                let element = characters[index]
                    .and_then(|character| self.rules.character_elements.get(&character));
                let compatible = element.is_some_and(|element| {
                    self.rules
                        .ability_soul_elements
                        .get(id)
                        .is_some_and(|elements| elements.contains(element))
                });
                if !compatible {
                    return false;
                }
                let used = used_souls.entry(*id).or_default();
                if *used >= self.souls.get(id).copied().unwrap_or_default() {
                    return false;
                }
                *used += 1;
                true
            });
        }
        for (key, values) in [
            ("character_ids", characters),
            ("unison_character_ids", unisons),
            ("equipment_ids", equipment),
            ("ability_soul_ids", souls),
        ] {
            party.insert(key.to_owned(), serde_json::json!(values));
        }
    }
}
// //// /读取编队引用的持有记录 ////

fn owned_ids(root: &Map<String, Value>, key: &str) -> Result<BTreeSet<i64>, PersonalServiceError> {
    let values = root
        .get(key)
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new(format!("stored CN {key} is missing")))?;
    Ok(values
        .keys()
        .filter_map(|id| id.parse::<i64>().ok())
        .filter(|id| *id > 0)
        .collect())
}

fn slots(party: &Map<String, Value>, key: &str) -> [Option<i64>; 3] {
    let values = party.get(key).and_then(Value::as_array);
    std::array::from_fn(|index| {
        values
            .and_then(|values| values.get(index))
            .and_then(Value::as_i64)
    })
}

// //// 归一普通编队存档中的槽位与颜色 [@x380kkm 2026-09-23] ////
pub(crate) fn normalize_normal_party_groups(
    root: &mut Map<String, Value>,
) -> Result<(), PersonalServiceError> {
    let inventory = PartyInventory::from_player(root)?;
    let groups = root
        .get_mut("user_party_group_list")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN party groups are missing"))?;
    for group in groups.values_mut() {
        let group = group
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN party group is invalid"))?;
        let color = group
            .get("color_id")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        group.insert(
            "color_id".to_owned(),
            Value::from(normalize_group_color(color)?),
        );
        if let Some(parties) = group.get_mut("list").and_then(Value::as_object_mut) {
            for party in parties.values_mut().filter_map(Value::as_object_mut) {
                inventory.normalize(party);
            }
        }
    }
    Ok(())
}
