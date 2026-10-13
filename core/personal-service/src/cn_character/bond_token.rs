// audience: internal
// # personal-service-cn-character-bond-token
// 每块玛纳板的羁绊之证按角色与板块记录一次领取, 资产, 任务和历史共享角色操作事务.

use super::{
    character_mut, character_time, character_value, error_response, CharacterBoardRequest,
};
use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{decode_player_data, encode_player_data, player_snapshot, require_root};
use crate::database::{ReceiveHistoryEntry, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};

// //// 领取羁绊之证并保存资产和历史 [@x380kkm 2026-09-23] ////
pub(super) fn receive(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<CharacterBoardRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.character_id > 0 && body.mana_board_index > 0 => {
            body
        }
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let mut bond_tokens = match character_value(root, body.character_id) {
        Ok(character) => character,
        Err(response) => return Ok(response),
    }
    .get("bond_token_list")
    .and_then(Value::as_array)
    .cloned()
    .ok_or_else(|| PersonalServiceError::new("stored CN bond token list is missing"))?;
    let token_index = usize::try_from(body.mana_board_index - 1)
        .map_err(|_| PersonalServiceError::new("CN bond token index is out of range"))?;
    let token = match bond_tokens.get_mut(token_index) {
        Some(token) => token,
        None => {
            return Ok(error_response(
                "400 Bad Request",
                "invalid_mana_board_index",
            ))
        }
    };
    let bond_token = crate::cn_tutorial::user_info_value(root, "bond_token")?;
    let (new_bond_token, claimed) = match token.get("status").and_then(Value::as_i64) {
        Some(1) => {
            token["status"] = Value::from(2);
            let updated = bond_token.checked_add(1).ok_or_else(|| {
                PersonalServiceError::new("CN bond token exceeds supported range")
            })?;
            (updated, true)
        }
        Some(2) => (bond_token, false),
        _ => return Ok(error_response("400 Bad Request", "bond_token_not_ready")),
    };
    if claimed {
        crate::cn_tutorial::set_user_info_value(root, "bond_token", new_bond_token)?;
        let character = match character_mut(root, body.character_id) {
            Ok(character) => character,
            Err(response) => return Ok(response),
        };
        character.insert(
            "bond_token_list".to_owned(),
            Value::Array(bond_tokens.clone()),
        );
    }
    let server_time = server_time(database)?;
    let character = character_value(root, body.character_id)
        .map_err(|_| PersonalServiceError::new("stored CN character disappeared"))?;
    let response_character =
        bond_token_response_character(body.character_id, character, bond_tokens, server_time);
    let mission_delta =
        crate::cn_mission::record_bond_token_action(root, database, snapshot.account_id, claimed)?;
    let mut response = json!({
        "user_info": {"bond_token": new_bond_token},
        "character_list": [response_character],
        "user_character_mana_node_list": {},
        "item_list": {},
        "evolution": [],
        "mission_info": mission_delta.mission_info,
        "active_mission_list": mission_delta.active_mission_list,
        "mail_arrived": false,
    });
    crate::cn_mission::apply_mission_reward_response(
        &mut response,
        root,
        &mission_delta.mission_info,
        body.viewer_id,
        server_time,
    )?;
    if claimed {
        database.save_player_snapshot_with_receive_history(
            snapshot.account_id,
            &encode_player_data(&player_data)?,
            &format!("bond-token:{}:{}", body.character_id, body.mana_board_index),
            server_time,
            &[ReceiveHistoryEntry::reward(10, None, 1)],
        )?;
    }
    msgpack_response_at(body.viewer_id, false, server_time, response)
}
// //// /领取羁绊之证并保存资产和历史 ////

// //// 构造 CN 羁绊代币响应角色 [@x380kkm 2026-08-23] ////
fn bond_token_response_character(
    character_id: i64,
    character: &Map<String, Value>,
    bond_tokens: Vec<Value>,
    server_time: i64,
) -> Value {
    let evolution_level = character
        .get("evolution_level")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    json!({
        "character_id": character_id,
        "evolution_level": evolution_level,
        "evolution_img_level": evolution_level,
        "create_time": character_time(character.get("join_time"), server_time),
        "update_time": character_time(character.get("update_time"), server_time),
        "join_time": character_time(character.get("join_time"), server_time),
        "bond_token_list": bond_tokens,
    })
}
// //// /构造 CN 羁绊代币响应角色 ////
