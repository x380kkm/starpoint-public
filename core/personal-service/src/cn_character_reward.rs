// audience: internal
// # personal-service-cn-character-reward
//
// 该模块统一发放 CN 角色, 重复角色素材和主表定义的角色百科记录. 邮件角色使用独立的计数契约.

use crate::cn_character::{character_asset_data, character_time};
use crate::cn_tutorial::{
    create_character_response, create_stored_character, require_object,
};
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;

pub(crate) mod overflow;

const CHARACTER_ENCYCLOPEDIA_ASSET: &str = include_str!("../assets/cn-character-encyclopedia.json");
static CHARACTER_ENCYCLOPEDIA_DATA: OnceLock<Result<Map<String, Value>, String>> = OnceLock::new();

// //// 按角色主百科映射保存已读状态并返回客户端增量 [@x380kkm 2026-09-21] ////
pub(crate) fn record_character_encyclopedia_state(
    root: &mut Map<String, Value>,
    character_id: i64,
) -> Result<Map<String, Value>, PersonalServiceError> {
    let document = CHARACTER_ENCYCLOPEDIA_DATA.get_or_init(|| {
        serde_json::from_str(CHARACTER_ENCYCLOPEDIA_ASSET)
            .map_err(|error| format!("failed to decode character encyclopedia asset: {error}"))
    });
    let mapping = document
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    let Some(encyclopedia_id) = mapping.get(&character_id.to_string()).and_then(Value::as_i64) else {
        return Ok(Map::new());
    };
    let key = encyclopedia_id.to_string();
    let stored = root
        .entry("encyclopedia_list".to_owned())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN encyclopedia data is invalid"))?;
    let alias = format!("1{character_id}01");
    let alias_value = (alias != key).then(|| stored.remove(&alias)).flatten();
    let current = stored.get(&key);
    let current_read = current.and_then(|value| value.get("read")).and_then(Value::as_bool);
    let alias_read = alias_value
        .as_ref()
        .and_then(|value| value.get("read"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let read = current_read.unwrap_or(false) || alias_read;
    if current_read == Some(read) {
        return Ok(Map::new());
    }
    let value = json!({"read": read});
    stored.insert(key.clone(), value.clone());
    Ok(Map::from_iter([(key, value)]))
}
// //// /按角色主百科映射保存已读状态并返回客户端增量 ////

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DuplicateCharacterItem {
    pub(crate) id: i64,
    pub(crate) count: i64,
    pub(crate) total: i64,
}

pub(crate) struct CharacterReward {
    pub(crate) character: Value,
    pub(crate) joined: bool,
    pub(crate) duplicate_item: Option<DuplicateCharacterItem>,
}

// //// 发放一个 CN 角色并结算重复角色素材 [@x380kkm 2026-08-23] ////
pub(crate) fn grant_character(
    root: &mut Map<String, Value>,
    viewer_id: i64,
    character_id: i64,
    server_time: i64,
) -> Result<CharacterReward, PersonalServiceError> {
    let asset = character_asset_data(character_id)?;
    grant_character_with_duplicate_item(
        root,
        viewer_id,
        character_id,
        server_time,
        duplicate_item(asset.rarity, asset.element),
    )
}
// //// /发放一个 CN 角色并结算重复角色素材 ////

// //// 发放一个不产生重复素材的 CN 角色 [@x380kkm 2026-08-28] ////
pub(crate) fn grant_character_without_duplicate_item(
    root: &mut Map<String, Value>,
    viewer_id: i64,
    character_id: i64,
    server_time: i64,
) -> Result<CharacterReward, PersonalServiceError> {
    character_asset_data(character_id)?;
    grant_character_with_duplicate_item(root, viewer_id, character_id, server_time, None)
}
// //// /发放一个不产生重复素材的 CN 角色 ////

fn grant_character_with_duplicate_item(
    root: &mut Map<String, Value>,
    viewer_id: i64,
    character_id: i64,
    server_time: i64,
    duplicate_item: Option<(i64, i64)>,
) -> Result<CharacterReward, PersonalServiceError> {
    let key = character_id.to_string();
    let is_owned = require_object(root, "user_character_list")?.contains_key(&key);
    if !is_owned {
        let character = insert_character(root, viewer_id, character_id, server_time)?;
        return Ok(CharacterReward {
            character,
            joined: true,
            duplicate_item: None,
        });
    }

    let character = increment_character_stack(root, viewer_id, character_id, server_time)?;
    let duplicate_item = duplicate_item
        .map(|(item_id, count)| add_duplicate_item(root, item_id, count))
        .transpose()?;
    Ok(CharacterReward {
        character,
        joined: false,
        duplicate_item,
    })
}

// //// 按邮件契约发放一个 CN 角色 [@x380kkm 2026-08-23] ////
pub(crate) fn grant_mailed_character(
    root: &mut Map<String, Value>,
    viewer_id: i64,
    character_id: i64,
    server_time: i64,
) -> Result<Value, PersonalServiceError> {
    let key = character_id.to_string();
    let is_owned = require_object(root, "user_character_list")?.contains_key(&key);
    if !is_owned {
        return insert_character(root, viewer_id, character_id, server_time);
    }

    let characters = require_object(root, "user_character_list")?;
    let character = characters
        .get_mut(&key)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN character data is invalid"))?;
    let entry_count = character
        .get("entry_count")
        .and_then(Value::as_i64)
        .unwrap_or(1)
        .checked_add(1)
        .ok_or_else(|| {
            PersonalServiceError::new("CN character entry count exceeds the supported range")
        })?;
    character.insert("entry_count".to_owned(), Value::from(entry_count));
    if server_time > 0 {
        character.insert("update_time".to_owned(), Value::from(server_time));
    }
    Ok(create_existing_character_response(
        character_id,
        &Value::Object(character.clone()),
        server_time,
    ))
}
// //// /按邮件契约发放一个 CN 角色 ////

fn insert_character(
    root: &mut Map<String, Value>,
    _viewer_id: i64,
    character_id: i64,
    server_time: i64,
) -> Result<Value, PersonalServiceError> {
    let stored = create_stored_character(character_id, server_time)?;
    let bond_token_list = stored
        .get("bond_token_list")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let mana_board_index = stored
        .get("mana_board_index")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    let entry_count = stored
        .get("entry_count")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    let exp = stored
        .get("exp")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let created_at = character_time(None, server_time);
    let response = json!({
        "viewer_id": 0,
        "character_id": character_id,
        "entry_count": entry_count,
        "exp": exp,
        "exp_total": exp,
        "bond_token_list": bond_token_list,
        "mana_board_index": mana_board_index,
        "create_time": created_at,
        "update_time": created_at,
        "join_time": created_at,
    });
    require_object(root, "user_character_list")?.insert(character_id.to_string(), stored);
    Ok(response)
}

fn increment_character_stack(
    root: &mut Map<String, Value>,
    _viewer_id: i64,
    character_id: i64,
    server_time: i64,
) -> Result<Value, PersonalServiceError> {
    let stored = root.get("user_character_list")
        .and_then(|characters| characters.get(character_id.to_string()))
        .ok_or_else(|| PersonalServiceError::new("stored CN character data is invalid"))?;
    if stored.get("stack").and_then(Value::as_i64).unwrap_or_default()
        >= crate::cn_reward_capacity::MAX_CHARACTER_STACK
    {
        overflow::convert(root, character_id, server_time)?;
        return Ok(create_existing_character_response(
            character_id, &root["user_character_list"][character_id.to_string()], server_time,
        ));
    }
    let character = require_object(root, "user_character_list")?
        .get_mut(&character_id.to_string())
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN character data is invalid"))?;
    let stack = character
        .get("stack")
        .and_then(Value::as_i64)
        .unwrap_or_default()
        .checked_add(1)
        .ok_or_else(|| {
            PersonalServiceError::new("CN character stack exceeds the supported range")
        })?;
    character.insert("stack".to_owned(), Value::from(stack));
    if server_time > 0 {
        character.insert("update_time".to_owned(), Value::from(server_time));
    }
    let stored = Value::Object(character.clone());
    let response = create_existing_character_response(character_id, &stored, server_time);
    Ok(response)
}

// //// 构造已有角色的客户端状态增量 [@x380kkm 2026-08-28] ////
fn create_existing_character_response(
    character_id: i64,
    stored: &Value,
    server_time: i64,
) -> Value {
    let mut response = create_character_response(0, character_id, stored, server_time);
    if let Some(response_object) = response.as_object_mut() {
        for field in ["evolution_level", "over_limit_step", "protection", "stack"] {
            if let Some(value) = stored.get(field) {
                response_object.insert(field.to_owned(), value.clone());
            }
        }
    }
    response
}
// //// /构造已有角色的客户端状态增量 ////

fn add_duplicate_item(
    root: &mut Map<String, Value>,
    item_id: i64,
    count: i64,
) -> Result<DuplicateCharacterItem, PersonalServiceError> {
    let item_list = require_object(root, "item_list")?;
    let key = item_id.to_string();
    let total = item_list
        .get(&key)
        .and_then(Value::as_i64)
        .unwrap_or_default()
        .checked_add(count)
        .ok_or_else(|| {
            PersonalServiceError::new("CN duplicate character item count exceeds range")
        })?;
    item_list.insert(key, Value::from(total));
    Ok(DuplicateCharacterItem {
        id: item_id,
        count,
        total,
    })
}

// //// 按 ex_boost_item_acquisition 主表选择重复角色的 EX 强化道具和数量 [@x380kkm 2026-10-08] ////
fn duplicate_item(rarity: i64, element: i64) -> Option<(i64, i64)> {
    let (rarity_offset, count) = match rarity {
        3 => (0, 1),
        4 => (1, 1),
        5 => (2, 3),
        _ => return None,
    };
    let element_offset = match element {
        0 => 0,
        1 => 3,
        2 => 6,
        3 => 9,
        4 => 15,
        5 => 12,
        _ => return None,
    };
    Some((14_001 + rarity_offset + element_offset, count))
}
// //// /按 ex_boost_item_acquisition 主表选择重复角色的 EX 强化道具和数量 ////

#[cfg(test)]
mod tests {
    use crate::cn_tutorial::format_client_time;
    use super::*;

    // //// 验证短编号角色的百科键归一化保留已读状态 [@x380kkm 2026-09-21] ////
    #[test]
    fn normalizes_short_character_encyclopedia_keys_without_losing_read_state() {
        for (alias_read, current_read) in [(true, false), (false, true), (false, false)] {
            let mut root = Map::from_iter([("encyclopedia_list".to_owned(), json!({
                "11001": {"read": alias_read},
                "100001001": {"read": current_read},
                "151200101": {"read": true},
            }))]);
            let delta = record_character_encyclopedia_state(&mut root, 10).unwrap();
            assert!(root["encyclopedia_list"].get("11001").is_none());
            assert_eq!(root["encyclopedia_list"]["100001001"]["read"], alias_read || current_read);
            assert_eq!(root["encyclopedia_list"]["151200101"]["read"], true);
            assert_eq!(!delta.is_empty(), alias_read && !current_read);
            assert!(record_character_encyclopedia_state(&mut root, 10).unwrap().is_empty());
        }
    }
    // //// /验证短编号角色的百科键归一化保留已读状态 ////

    // //// 验证角色奖励沿用实际百科主表的记录范围 [@x380kkm 2026-09-21] ////
    #[test]
    fn records_only_character_encyclopedia_entries_present_in_the_client() {
        let mut root = Map::new();
        assert!(record_character_encyclopedia_state(&mut root, 412_004).unwrap().is_empty());
        assert!(root.is_empty());
        let delta = record_character_encyclopedia_state(&mut root, 111_069).unwrap();
        assert_eq!(delta["111106901"], json!({"read": false}));
        assert_eq!(root["encyclopedia_list"]["111106901"]["read"], false);
    }
    // //// /验证角色奖励沿用实际百科主表的记录范围 ////

    // //// 验证邮件重复角色只增加领取次数 [@x380kkm 2026-08-23] ////
    #[test]
    fn increments_entry_count_for_mailed_duplicate_without_material() {
        let mut root = Map::from_iter([
            ("user_character_list".to_owned(), json!({})),
            ("item_list".to_owned(), json!({})),
        ]);

        grant_mailed_character(&mut root, 7, 111_001, 100).unwrap();
        let duplicate = grant_mailed_character(&mut root, 7, 111_001, 200).unwrap();

        assert_eq!(root["user_character_list"]["111001"]["entry_count"], 2);
        assert_eq!(root["user_character_list"]["111001"]["stack"], 0);
        assert_eq!(root["user_character_list"]["111001"]["update_time"], 200);
        assert_eq!(root["item_list"], json!({}));
        assert_eq!(duplicate["entry_count"], 2);
        assert_eq!(duplicate["stack"], 0);
        assert_eq!(duplicate["join_time"], format_client_time(100));
        assert_eq!(duplicate["update_time"], format_client_time(200));
    }
    // //// /验证邮件重复角色只增加领取次数 ////

    // //// 验证同一响应内重复角色保留客户端建档字段 [@x380kkm 2026-08-26] ////
    #[test]
    fn duplicate_character_response_keeps_creation_fields() {
        let mut root = Map::from_iter([
            ("user_character_list".to_owned(), json!({})),
            ("item_list".to_owned(), json!({})),
        ]);

        grant_character(&mut root, 7, 111_001, 100).unwrap();
        let duplicate = grant_character(&mut root, 7, 111_001, 200).unwrap();

        assert_eq!(duplicate.character["entry_count"], 1);
        assert_eq!(duplicate.character["stack"], 1);
        assert!(duplicate.character["bond_token_list"].is_array());
        assert_eq!(duplicate.character["mana_board_index"], 1);
        assert_eq!(duplicate.character["exp"], 0);
        assert_eq!(duplicate.character["update_time"], format_client_time(200));
        assert_eq!(duplicate.character["join_time"], format_client_time(100));
        assert_eq!(root["user_character_list"]["111001"]["update_time"], 200);
    }
    // //// /验证同一响应内重复角色保留客户端建档字段 ////
}
