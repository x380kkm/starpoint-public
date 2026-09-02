// audience: internal
// # personal-service-cn-character-reward
//
// 该模块统一发放 CN 角色和重复角色素材. 邮件角色使用独立的计数契约.

use crate::cn_character::character_asset_data;
use crate::cn_tutorial::{
    create_character_response, create_stored_character, format_client_time, require_object,
};
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};

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
        duplicate_item_id(asset.rarity, asset.element),
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
    duplicate_item_id: Option<i64>,
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
    let duplicate_item = duplicate_item_id
        .map(|item_id| add_duplicate_item(root, item_id))
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
    let created_at = format_client_time(server_time);
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
        let join_time = character_time(stored.get("join_time"), server_time);
        let update_time = character_time(stored.get("update_time"), server_time);
        response_object.insert("create_time".to_owned(), Value::from(join_time.clone()));
        response_object.insert("join_time".to_owned(), Value::from(join_time));
        response_object.insert("update_time".to_owned(), Value::from(update_time));
        for field in ["evolution_level", "over_limit_step", "protection", "stack"] {
            if let Some(value) = stored.get(field) {
                response_object.insert(field.to_owned(), value.clone());
            }
        }
    }
    response
}
// //// /构造已有角色的客户端状态增量 ////

fn character_time(value: Option<&Value>, fallback: i64) -> String {
    value
        .and_then(Value::as_i64)
        .map(format_client_time)
        .or_else(|| value.and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_else(|| format_client_time(fallback))
}

fn add_duplicate_item(
    root: &mut Map<String, Value>,
    item_id: i64,
) -> Result<DuplicateCharacterItem, PersonalServiceError> {
    let item_list = require_object(root, "item_list")?;
    let key = item_id.to_string();
    let total = item_list
        .get(&key)
        .and_then(Value::as_i64)
        .unwrap_or_default()
        .checked_add(1)
        .ok_or_else(|| {
            PersonalServiceError::new("CN duplicate character item count exceeds range")
        })?;
    item_list.insert(key, Value::from(total));
    Ok(DuplicateCharacterItem {
        id: item_id,
        count: 1,
        total,
    })
}

fn duplicate_item_id(rarity: i64, element: i64) -> Option<i64> {
    let rarity_offset = match rarity {
        3 => 0,
        4 => 1,
        5 => 2,
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
    Some(14_001 + rarity_offset + element_offset)
}

#[cfg(test)]
mod tests {
    use super::*;

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
