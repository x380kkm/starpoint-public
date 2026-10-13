// audience: internal
// # personal-service-cn-mission-party-conditions
//
// 参战角色的技能使用当前进化档. 装备属性来自当前等级生效的普通能力与强化能力.

use crate::cn_battle_state::FinishBattleInput;
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Default)]
pub(crate) struct PartyConditions<'a> {
    pub(crate) character_id: Option<i64>,
    pub(crate) character_element: Option<i64>,
    pub(crate) equipment_element: Option<i64>,
    pub(crate) action_effects: &'a [String],
    pub(crate) effects_ignore_characters: &'a [String],
}

#[derive(Deserialize)]
struct PartyMetadata {
    characters: BTreeMap<i64, CharacterMetadata>,
    equipment: BTreeMap<i64, EquipmentMetadata>,
}

#[derive(Deserialize)]
struct CharacterMetadata {
    string_id: String,
    element: i64,
    skills: BTreeMap<i64, SkillMetadata>,
}

#[derive(Deserialize)]
struct SkillMetadata {
    effects: Vec<String>,
    unisonable: bool,
}

#[derive(Deserialize)]
struct EquipmentMetadata {
    max_level: i64,
    abilities: Vec<EquipmentAbility>,
    enhancement_abilities: Vec<EquipmentAbility>,
}

#[derive(Deserialize)]
struct EquipmentAbility {
    slot: i64,
    learn_level: i64,
    elements: Vec<i64>,
}

static METADATA: OnceLock<Result<PartyMetadata, String>> = OnceLock::new();

fn metadata() -> Result<&'static PartyMetadata, PersonalServiceError> {
    METADATA
        .get_or_init(|| {
            serde_json::from_str(include_str!(
                "../../../assets/cn-mission-party-metadata.json"
            ))
            .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

fn has_active_element(abilities: &[EquipmentAbility], level: i64, element: i64) -> bool {
    let mut slots = BTreeMap::<i64, &EquipmentAbility>::new();
    for ability in abilities
        .iter()
        .filter(|ability| ability.learn_level <= level)
    {
        if slots
            .get(&ability.slot)
            .map_or(true, |previous| ability.learn_level >= previous.learn_level)
        {
            slots.insert(ability.slot, ability);
        }
    }
    slots
        .values()
        .any(|ability| ability.elements.contains(&element))
}

fn equipment_has_element(
    root: &Map<String, Value>,
    id: i64,
    element: i64,
) -> Result<bool, PersonalServiceError> {
    let Some(item) = root
        .get("user_equipment_list")
        .and_then(|equipment| equipment.get(id.to_string()))
    else {
        return Ok(false);
    };
    let Some(definition) = metadata()?.equipment.get(&id) else {
        return Ok(false);
    };
    let level = item.get("level").and_then(Value::as_i64).unwrap_or(1);
    let enhancement_level = item
        .get("enhancement_level")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    Ok(has_active_element(&definition.abilities, level, element)
        || has_active_element(
            &definition.enhancement_abilities,
            enhancement_level,
            element,
        ))
}

// //// 匹配参战角色属性和当前技能效果 [@x380kkm 2026-09-21] ////
impl PartyConditions<'_> {
    pub(crate) fn has_character(
        &self,
        root: &Map<String, Value>,
        input: &FinishBattleInput<'_>,
    ) -> Result<bool, PersonalServiceError> {
        let metadata = metadata()?;
        let matches = input
            .main_character_ids
            .iter()
            .chain(input.unison_character_ids)
            .flatten()
            .any(|id| {
                self.character_id.map_or(true, |expected| *id == expected)
                    && self.character_element.map_or(true, |element| {
                        metadata
                            .characters
                            .get(id)
                            .is_some_and(|character| character.element == element)
                    })
            });
        if !matches {
            return Ok(false);
        }
        if let Some(element) = self.equipment_element {
            for id in input.equipment_ids.iter().flatten() {
                if equipment_has_element(root, *id, element)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        Ok(true)
    }

    pub(crate) fn has_skill(
        &self,
        root: &Map<String, Value>,
        input: &FinishBattleInput<'_>,
    ) -> Result<bool, PersonalServiceError> {
        let metadata = metadata()?;
        let slots = input
            .main_character_ids
            .iter()
            .map(|id| (id, false))
            .chain(input.unison_character_ids.iter().map(|id| (id, true)));
        for (id, is_unison) in slots {
            let Some(id) = id else {
                continue;
            };
            let Some(character) = metadata.characters.get(id) else {
                continue;
            };
            if self
                .effects_ignore_characters
                .contains(&character.string_id)
            {
                continue;
            }
            let evolution = super::character::skill_evolution(root, *id)?;
            let Some((_, skill)) = character.skills.range(..=evolution).next_back() else {
                continue;
            };
            if is_unison && !skill.unisonable {
                continue;
            }
            if skill
                .effects
                .iter()
                .any(|effect| self.action_effects.contains(effect))
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
// //// /匹配参战角色属性和当前技能效果 ////

pub(crate) fn fully_awakened_equipment_count(
    root: &Map<String, Value>,
) -> Result<i64, PersonalServiceError> {
    let metadata = metadata()?;
    Ok(root
        .get("user_equipment_list")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|items| items.iter())
        .filter(|(id, item)| {
            id.parse::<i64>()
                .ok()
                .and_then(|id| metadata.equipment.get(&id))
                .is_some_and(|definition| {
                    definition.max_level > 1
                        && item.get("level").and_then(Value::as_i64).unwrap_or(1)
                            >= definition.max_level
                })
        })
        .count() as i64)
}
