// audience: internal
// # personal-service-cn-activity-party
//
// 该模块将玩家快照中的队伍转换为 CN 活动接口使用的数组结构.

use super::state;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};

#[derive(Clone, Copy)]
enum PartyLayout {
    Carnival,
    Raid,
    Rush,
}

// //// 读取并保存活动队伍副本 [@x380kkm 2026-08-22] ////
pub(super) fn carnival_party_groups(
    root: &mut Map<String, Value>,
    family: &str,
    event_id: i64,
) -> Result<Value, PersonalServiceError> {
    persisted_party_groups(root, family, event_id, PartyLayout::Carnival)
}

pub(super) fn raid_party_groups(
    root: &mut Map<String, Value>,
    family: &str,
    event_id: i64,
) -> Result<Value, PersonalServiceError> {
    persisted_party_groups(root, family, event_id, PartyLayout::Raid)
}

pub(super) fn rush_party_groups(
    root: &mut Map<String, Value>,
    family: &str,
    event_id: i64,
) -> Result<Value, PersonalServiceError> {
    persisted_party_groups(root, family, event_id, PartyLayout::Rush)
}

fn persisted_party_groups(
    root: &mut Map<String, Value>,
    family: &str,
    event_id: i64,
    layout: PartyLayout,
) -> Result<Value, PersonalServiceError> {
    let mut groups = if let Some(groups) = state::event_state(root, family, event_id)
        .and_then(|event| event.get("party_group_list"))
        .filter(|groups| !groups.is_null())
    {
        groups.clone()
    } else {
        project_party_groups(root, layout)?
    };
    normalize_party_ids(&mut groups, layout)?;
    state::event_state_mut(root, family, event_id)?
        .insert("party_group_list".to_owned(), groups.clone());
    Ok(groups)
}
// //// /读取并保存活动队伍副本 ////

fn project_party_groups(
    root: &Map<String, Value>,
    layout: PartyLayout,
) -> Result<Value, PersonalServiceError> {
    let groups = root
        .get("user_party_group_list")
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("stored CN party groups are missing"))?;
    let mut indexed_groups = groups.iter().collect::<Vec<_>>();
    indexed_groups.sort_by_key(|(id, _)| id.parse::<i64>().unwrap_or(i64::MAX));
    if matches!(layout, PartyLayout::Raid) {
        indexed_groups.truncate(1);
    }
    let mut response_groups = Vec::with_capacity(indexed_groups.len());
    for (group_id, group) in indexed_groups {
        let group = group
            .as_object()
            .ok_or_else(|| PersonalServiceError::new("stored CN party group is invalid"))?;
        let parties = group
            .get("list")
            .and_then(Value::as_object)
            .ok_or_else(|| PersonalServiceError::new("stored CN party list is invalid"))?;
        let mut indexed_parties = parties.iter().collect::<Vec<_>>();
        indexed_parties.sort_by_key(|(id, _)| id.parse::<i64>().unwrap_or(i64::MAX));
        if matches!(layout, PartyLayout::Raid) {
            indexed_parties.truncate(3);
        }
        let mut party_list = Vec::with_capacity(indexed_parties.len());
        for (party_id, party) in indexed_parties {
            party_list.push(project_party(
                party_id.parse::<i64>().unwrap_or_default(),
                party,
            )?);
        }
        let color_id = if matches!(layout, PartyLayout::Raid) {
            15
        } else {
            group
                .get("color_id")
                .and_then(Value::as_i64)
                .unwrap_or_default()
        };
        response_groups.push(json!({
            "party_group_color_id": color_id,
            "party_group_id": group_id.parse::<i64>().unwrap_or_default(),
            "party_list": party_list,
        }));
    }
    Ok(Value::Array(response_groups))
}

fn normalize_party_ids(
    groups: &mut Value,
    layout: PartyLayout,
) -> Result<(), PersonalServiceError> {
    let groups = groups
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN activity party groups are invalid"))?;
    for group in groups {
        let group = group.as_object_mut().ok_or_else(|| {
            PersonalServiceError::new("stored CN activity party group is invalid")
        })?;
        let group_id = group
            .get("party_group_id")
            .and_then(Value::as_i64)
            .unwrap_or(1)
            .max(1);
        let parties = group
            .get_mut("party_list")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| PersonalServiceError::new("stored CN activity party list is invalid"))?;
        for (party_index, party) in parties.iter_mut().enumerate() {
            let party = party
                .as_object_mut()
                .ok_or_else(|| PersonalServiceError::new("stored CN activity party is invalid"))?;
            let slot_id = i64::try_from(party_index + 1)
                .map_err(|_| PersonalServiceError::new("CN activity party index exceeds range"))?;
            let party_id = match layout {
                PartyLayout::Carnival => (group_id - 1)
                    .checked_mul(10)
                    .and_then(|offset| offset.checked_add(slot_id))
                    .ok_or_else(|| {
                        PersonalServiceError::new("CN activity party id exceeds range")
                    })?,
                PartyLayout::Raid | PartyLayout::Rush => slot_id,
            };
            party.insert("party_id".to_owned(), Value::from(party_id));
        }
    }
    Ok(())
}

fn project_party(party_id: i64, party: &Value) -> Result<Value, PersonalServiceError> {
    let party = party
        .as_object()
        .ok_or_else(|| PersonalServiceError::new("stored CN party is invalid"))?;
    Ok(json!({
        "ability_soul_ids": party_slots(party, "ability_soul_ids"),
        "character_ids": party_slots(party, "character_ids"),
        "equipment_ids": party_slots(party, "equipment_ids"),
        "unison_character_ids": party_slots(party, "unison_character_ids"),
        "options": {
            "allow_other_players_to_heal_me": party
                .get("options")
                .and_then(Value::as_object)
                .and_then(|options| options.get("allow_other_players_to_heal_me"))
                .and_then(Value::as_bool)
                .unwrap_or(true),
        },
        "party_edited": party
            .get("edited")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        "party_id": party_id,
        "party_name": party
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Party"),
    }))
}

fn party_slots(party: &Map<String, Value>, key: &str) -> Value {
    party
        .get(key)
        .filter(|value| value.as_array().is_some_and(|slots| slots.len() == 3))
        .cloned()
        .unwrap_or_else(|| json!([null, null, null]))
}
