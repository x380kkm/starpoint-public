// audience: internal
// # personal-service-cn-expod-inject
// 经验池按实际升级需要消耗, 达到上限后的剩余经验保留在池中, 请求重放保持同一次操作.

use super::settlement::ExpodSettlement;
use super::{
    character_exp_cap, character_level_from_experience, character_rarity, error_response,
    read_character, require_character, serialize_injected_character,
};
use crate::cn::{decode_request, server_time};
use crate::cn_request_receipt::RequestReceipt;
use crate::cn_tutorial::{decode_player_data, player_snapshot, require_object, require_root};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct InjectExpRequest {
    character_id: i64,
    viewer_id: i64,
    exp: i64,
    api_count: Option<i64>,
}

// //// 消耗 EXP 池提升角色经验 [@x380kkm 2026-07-24] ////
pub(super) fn apply(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<InjectExpRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.character_id > 0
                && body.api_count.map_or(true, |count| count >= 0) =>
        {
            body
        }
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let add_exp = match body.exp.checked_abs() {
        Some(exp) => exp,
        None => return Ok(error_response("400 Bad Request", "invalid_exp")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let receipt = body.api_count.map(|count| {
        RequestReceipt::new(
            "expod",
            "inject-exp",
            body.viewer_id,
            count,
            json!({"character_id":body.character_id,"exp":body.exp}),
        )
    });
    if let Some(response) = receipt
        .as_ref()
        .map(|receipt| receipt.replay(root))
        .transpose()?
        .flatten()
    {
        return Ok(response);
    }
    let rarity = match character_rarity(body.character_id) {
        Ok(rarity) => rarity,
        Err(_) => return Ok(error_response("400 Bad Request", "character_not_found")),
    };
    let character_data = match read_character(root, body.character_id) {
        Ok(character) => character,
        Err(_) => return Ok(error_response("400 Bad Request", "character_not_owned")),
    };
    let current_exp = character_data
        .get("exp")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let over_limit_step = character_data
        .get("over_limit_step")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let exp_cap = character_exp_cap(rarity, over_limit_step)
        .ok_or_else(|| PersonalServiceError::new("CN character rarity is invalid"))?;
    let server_time = server_time(database)?;
    crate::cn_exp_pool::settle(root, server_time)?;
    let user_info = require_object(root, "user_info")?;
    let exp_pool = user_info
        .get("exp_pool")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    if add_exp > exp_pool {
        return Ok(error_response("400 Bad Request", "not_enough_exp"));
    }
    let total_exp = current_exp
        .checked_add(add_exp)
        .ok_or_else(|| PersonalServiceError::new("CN character EXP exceeds the supported range"))?;
    let after_exp = total_exp.min(exp_cap);
    let overflow_exp = total_exp.saturating_sub(after_exp);
    let after_exp_pool = exp_pool
        .checked_sub(add_exp)
        .and_then(|value| value.checked_add(overflow_exp))
        .ok_or_else(|| PersonalServiceError::new("CN EXP pool exceeds the supported range"))?;
    let exp_pooled_time = user_info
        .get("exp_pooled_time")
        .cloned()
        .unwrap_or(Value::from(server_time));
    user_info.insert("exp_pool".to_owned(), Value::from(after_exp_pool));
    let character = require_character(root, body.character_id)?;
    character.insert("exp".to_owned(), Value::from(after_exp));
    let character_response =
        serialize_injected_character(body.character_id, character, server_time);
    let character_level = character_level_from_experience(rarity, after_exp, over_limit_step)
        .ok_or_else(|| PersonalServiceError::new("CN character progression is invalid"))?;
    let mut mission_delta = crate::cn_mission::record_character_level_action(
        root,
        database,
        snapshot.account_id,
        character_level,
    )?;
    if after_exp > current_exp {
        mission_delta
            .active_mission_list
            .extend(crate::cn_mission::record_active_counters(
                root,
                database,
                &[("injected_exp_first_time", 1)],
            )?);
    }
    let mut response = json!({
        "add_exp_list": [{
            "character_id": body.character_id,
            "add_exp": after_exp.saturating_sub(current_exp),
            "after_exp": after_exp,
            "add_exp_pool": overflow_exp,
        }],
        "character_list": [character_response],
        "user_info": {
            "exp_pool": after_exp_pool,
            "exp_pooled_time": exp_pooled_time,
        },
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
    ExpodSettlement {
        viewer_id: body.viewer_id,
        account_id: snapshot.account_id,
        response_time: server_time,
        receipt,
    }
    .save(database, &mut player_data, response, &[])
}
// //// /消耗 EXP 池提升角色经验 ////
