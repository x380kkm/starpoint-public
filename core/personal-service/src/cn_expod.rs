// audience: internal
// # personal-service-cn-expod
//
// 该模块实现 CN 角色副本转 EXP, 角色经验上限和等级映射, 以及 EXP 注入角色的离线协议.

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, format_client_time, player_snapshot, require_object,
    require_root,
};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;

const CHARACTER_ASSET: &str = include_str!("../../../assets/character.json");
const REWARD_ITEM_ID: &str = "990008";
const CHARACTER_EXP_CAPS: [&[i64]; 6] = [
    &[],
    &[
        11_416, 15_820, 21_477, 28_538, 37_241, 49_481, 66_600, 91_180, 125_223, 170_928, 216_633,
        262_338, 308_043,
    ],
    &[
        21_477, 28_538, 37_241, 49_481, 66_600, 91_180, 125_223, 170_928, 216_633, 262_338, 308_043,
    ],
    &[
        37_241, 49_481, 66_600, 91_180, 125_223, 170_928, 216_633, 262_338, 308_043,
    ],
    &[76_272, 102_829, 139_190, 189_995, 240_800, 291_605, 342_410],
    &[153_988, 210_488, 266_988, 323_488, 379_988],
];
const CHARACTER_LEVEL_EXPERIENCE: [i64; 100] = [
    0, 10, 20, 38, 64, 100, 146, 202, 269, 348, 440, 545, 663, 796, 944, 1_108, 1_288, 1_485,
    1_699, 1_932, 2_184, 2_455, 2_746, 3_058, 3_392, 3_748, 4_126, 4_528, 4_954, 5_404, 5_880,
    6_382, 6_910, 7_465, 8_048, 8_660, 9_301, 9_971, 10_672, 11_416, 12_204, 13_037, 13_917,
    14_844, 15_820, 16_846, 17_924, 19_054, 20_238, 21_477, 22_772, 24_124, 25_535, 27_006, 28_538,
    30_132, 31_789, 33_511, 35_299, 37_241, 39_343, 41_612, 44_054, 46_675, 49_481, 52_479, 55_675,
    59_075, 62_685, 66_600, 70_832, 75_394, 80_298, 85_556, 91_180, 97_183, 103_576, 110_372,
    117_584, 125_223, 134_364, 143_505, 152_646, 161_787, 170_928, 180_069, 189_210, 198_351,
    207_492, 216_633, 225_774, 234_915, 244_056, 253_197, 262_338, 271_479, 280_620, 289_761,
    298_902, 308_043,
];
static CHARACTER_DATA: OnceLock<Result<Value, String>> = OnceLock::new();

#[derive(Deserialize)]
struct StackToExpRequest {
    character_id: i64,
    #[serde(default)]
    api_count: i64,
    number: i64,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct InjectExpRequest {
    character_id: i64,
    viewer_id: i64,
    exp: i64,
    #[serde(default)]
    api_count: i64,
}

#[derive(Deserialize)]
struct BulkStackToExpRequest {
    viewer_id: i64,
    #[serde(default)]
    api_count: i64,
}

// //// 分派 CN 角色经验道具请求 [@x380kkm 2026-07-24] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    let response = match request.path() {
        "/api/index.php/expod/stack_to_exp" => stack_to_exp(request, database),
        "/api/index.php/expod/bulk_stack_to_exp" => bulk_stack_to_exp(request, database),
        "/api/index.php/expod/inject_exp" => inject_exp(request, database),
        _ => return None,
    };
    Some(response)
}

// //// 将已满突破角色的全部副本批量转换 [@x380kkm 2026-08-22] ////
fn bulk_stack_to_exp(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<BulkStackToExpRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.api_count >= 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let response_time = server_time(database)?;
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let character_snapshot = root
        .get("user_character_list")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| PersonalServiceError::new("stored CN character list is missing"))?;
    let mut conversions = Vec::new();
    let mut total_exp = 0_i64;
    let mut total_items = 0_i64;

    for (character_id, character) in character_snapshot {
        let Ok(character_id) = character_id.parse::<i64>() else {
            continue;
        };
        let Some(character) = character.as_object() else {
            continue;
        };
        let stack = character
            .get("stack")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        if stack <= 0 {
            continue;
        }
        let Ok(rarity) = character_rarity(character_id) else {
            continue;
        };
        let max_over_limit = CHARACTER_EXP_CAPS
            .get(rarity as usize)
            .map(|levels| levels.len().saturating_sub(1) as i64)
            .unwrap_or_default();
        let over_limit = character
            .get("over_limit_step")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        if over_limit < max_over_limit {
            continue;
        }
        let (exp_per_stack, item_per_stack) = stack_conversion(rarity)?;
        total_exp =
            total_exp
                .checked_add(exp_per_stack.checked_mul(stack).ok_or_else(|| {
                    PersonalServiceError::new("CN bulk converted EXP exceeds range")
                })?)
                .ok_or_else(|| PersonalServiceError::new("CN bulk converted EXP exceeds range"))?;
        total_items = total_items
            .checked_add(item_per_stack.checked_mul(stack).ok_or_else(|| {
                PersonalServiceError::new("CN bulk converted item count exceeds range")
            })?)
            .ok_or_else(|| {
                PersonalServiceError::new("CN bulk converted item count exceeds range")
            })?;
        conversions.push(character_id);
    }

    let user_info = require_object(root, "user_info")?;
    let current_exp_pool = user_info
        .get("exp_pool")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let exp_pooled_time = user_info
        .get("exp_pooled_time")
        .cloned()
        .unwrap_or(Value::from(response_time));
    let new_exp_pool = current_exp_pool
        .checked_add(total_exp)
        .ok_or_else(|| PersonalServiceError::new("CN EXP pool exceeds range"))?;
    user_info.insert("exp_pool".to_owned(), Value::from(new_exp_pool));

    let item_list = require_object(root, "item_list")?;
    let current_items = item_list
        .get(REWARD_ITEM_ID)
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let new_items = current_items
        .checked_add(total_items)
        .ok_or_else(|| PersonalServiceError::new("CN EXP item count exceeds range"))?;
    if total_items > 0 {
        item_list.insert(REWARD_ITEM_ID.to_owned(), Value::from(new_items));
    }

    let mut response_characters = Vec::with_capacity(conversions.len());
    for character_id in conversions {
        let character = require_character(root, character_id)?;
        character.insert("stack".to_owned(), Value::from(0));
        response_characters.push(serialize_character(
            body.viewer_id,
            character_id,
            character,
            response_time,
        ));
    }
    let response_items = root
        .get("item_list")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| PersonalServiceError::new("stored CN item list is missing"))?;
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        json!({
            "character_list": response_characters,
            "converted_exp_info": {"add_exp": total_exp},
            "item_list": response_items,
            "user_info": {
                "exp_pool": new_exp_pool,
                "exp_pooled_time": exp_pooled_time,
            },
            "mail_arrived": false,
        }),
    )
}
// //// /将已满突破角色的全部副本批量转换 ////
// //// /分派 CN 角色经验道具请求 ////

// //// 把角色副本转换为 EXP 和奖励道具 [@x380kkm 2026-07-24] ////
fn stack_to_exp(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<StackToExpRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.character_id > 0
                && body.api_count >= 0
                && body.number > 0 =>
        {
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
    let rarity = match character_rarity(body.character_id) {
        Ok(rarity) => rarity,
        Err(_) => return Ok(error_response("400 Bad Request", "character_not_found")),
    };
    let character_data = match read_character(root, body.character_id) {
        Ok(character) => character,
        Err(_) => return Ok(error_response("400 Bad Request", "character_not_owned")),
    };
    let stack = character_data
        .get("stack")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let after_stack = stack.checked_sub(body.number).ok_or_else(|| {
        PersonalServiceError::new("CN character stack exceeds the supported range")
    })?;
    if after_stack < 0 {
        return Ok(error_response("400 Bad Request", "not_enough_stack"));
    }
    let (exp_per_stack, item_per_stack) = stack_conversion(rarity)?;
    let increase_exp = exp_per_stack
        .checked_mul(body.number)
        .ok_or_else(|| PersonalServiceError::new("CN converted EXP exceeds the supported range"))?;
    let increase_items = item_per_stack.checked_mul(body.number).ok_or_else(|| {
        PersonalServiceError::new("CN converted item count exceeds the supported range")
    })?;
    let user_info = require_object(root, "user_info")?;
    let exp_pool = user_info
        .get("exp_pool")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let after_exp_pool = exp_pool
        .checked_add(increase_exp)
        .ok_or_else(|| PersonalServiceError::new("CN EXP pool exceeds the supported range"))?;
    let server_time = server_time(database)?;
    user_info.insert("exp_pool".to_owned(), Value::from(after_exp_pool));
    let exp_pooled_time = user_info
        .get("exp_pooled_time")
        .cloned()
        .unwrap_or(Value::from(server_time));
    let item_list = require_object(root, "item_list")?;
    let current_items = item_list
        .get(REWARD_ITEM_ID)
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let after_items = current_items.checked_add(increase_items).ok_or_else(|| {
        PersonalServiceError::new("CN EXP item count exceeds the supported range")
    })?;
    item_list.insert(REWARD_ITEM_ID.to_owned(), Value::from(after_items));
    let character = require_character(root, body.character_id)?;
    character.insert("stack".to_owned(), Value::from(after_stack));
    let character_response =
        serialize_character(body.viewer_id, body.character_id, character, server_time);
    let response = json!({
        "user_info": {
            "exp_pool": after_exp_pool,
            "exp_pooled_time": exp_pooled_time,
        },
        "character_list": [character_response],
        "converted_exp_info": {"add_exp": increase_exp},
        "item_list": {REWARD_ITEM_ID: after_items},
        "mail_arrived": false,
    });
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(body.viewer_id, false, server_time, response)
}
// //// /把角色副本转换为 EXP 和奖励道具 ////

// //// 消耗 EXP 池提升角色经验 [@x380kkm 2026-07-24] ////
fn inject_exp(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<InjectExpRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.character_id > 0 && body.api_count >= 0 => body,
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
    let server_time = server_time(database)?;
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
    let mission_delta = crate::cn_mission::record_character_level_action(
        root,
        database,
        snapshot.account_id,
        character_level,
    )?;
    let response = json!({
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
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(body.viewer_id, false, server_time, response)
}

pub(crate) fn character_level_from_experience(
    rarity: i64,
    experience: i64,
    over_limit_step: i64,
) -> Option<i64> {
    let level_cap = character_level_cap(rarity, over_limit_step)?;
    let experience = experience.max(0);
    Some(
        (1..=level_cap)
            .rev()
            .find(|level| {
                character_experience_for_level(rarity, *level)
                    .is_some_and(|required| required <= experience)
            })
            .unwrap_or(1),
    )
}

pub(crate) fn character_exp_cap(rarity: i64, over_limit_step: i64) -> Option<i64> {
    let (levels, over_limit_index) = character_progression(rarity, over_limit_step)?;
    levels.get(over_limit_index).copied()
}

fn character_level_cap(rarity: i64, over_limit_step: i64) -> Option<i64> {
    let (_, over_limit_index) = character_progression(rarity, over_limit_step)?;
    let first_cap = match rarity {
        1 => 40,
        2 => 50,
        3 => 60,
        4 => 70,
        5 => 80,
        _ => return None,
    };
    Some(first_cap + over_limit_index as i64 * 5)
}

fn character_progression(rarity: i64, over_limit_step: i64) -> Option<(&'static [i64], usize)> {
    let rarity_index = usize::try_from(rarity).ok()?;
    let levels = *CHARACTER_EXP_CAPS.get(rarity_index)?;
    if levels.is_empty() {
        return None;
    }
    let over_limit_index = usize::try_from(over_limit_step.max(0))
        .unwrap_or(usize::MAX)
        .min(levels.len() - 1);
    Some((levels, over_limit_index))
}

fn character_experience_for_level(rarity: i64, level: i64) -> Option<i64> {
    let level_index = usize::try_from(level.checked_sub(1)?).ok()?;
    let base_experience = *CHARACTER_LEVEL_EXPERIENCE.get(level_index)?;
    if rarity <= 3 {
        return matches!(rarity, 1..=3).then_some(base_experience);
    }

    let (caps, _) = character_progression(rarity, 0)?;
    let first_cap = character_level_cap(rarity, 0)?;
    let first_cap_index = usize::try_from(first_cap - 1).ok()?;
    let first_cap_experience = *CHARACTER_LEVEL_EXPERIENCE.get(first_cap_index)?;
    if level <= first_cap {
        return interpolate_character_experience(
            base_experience,
            0,
            first_cap_experience,
            0,
            *caps.first()?,
        );
    }

    let upper_cap_index = usize::try_from((level - first_cap + 4) / 5).ok()?;
    let lower_cap_index = upper_cap_index.checked_sub(1)?;
    let lower_level = first_cap + lower_cap_index as i64 * 5;
    let upper_level = first_cap + upper_cap_index as i64 * 5;
    interpolate_character_experience(
        base_experience,
        CHARACTER_LEVEL_EXPERIENCE[usize::try_from(lower_level - 1).ok()?],
        CHARACTER_LEVEL_EXPERIENCE[usize::try_from(upper_level - 1).ok()?],
        *caps.get(lower_cap_index)?,
        *caps.get(upper_cap_index)?,
    )
}

fn interpolate_character_experience(
    value: i64,
    source_start: i64,
    source_end: i64,
    target_start: i64,
    target_end: i64,
) -> Option<i64> {
    let source_span = source_end.checked_sub(source_start)?;
    let target_span = target_end.checked_sub(target_start)?;
    let source_offset = value.checked_sub(source_start)?;
    let scaled = i128::from(source_offset) * i128::from(target_span) / i128::from(source_span);
    i64::try_from(i128::from(target_start) + scaled).ok()
}
// //// /消耗 EXP 池提升角色经验 ////

fn character_data(character_id: i64) -> Result<&'static Value, PersonalServiceError> {
    let document = CHARACTER_DATA.get_or_init(|| {
        serde_json::from_str::<Value>(CHARACTER_ASSET)
            .map_err(|error| format!("failed to decode character asset: {error}"))
    });
    let document = document
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    document
        .get(&character_id.to_string())
        .ok_or_else(|| PersonalServiceError::new("CN character does not exist"))
}

fn character_rarity(character_id: i64) -> Result<i64, PersonalServiceError> {
    character_data(character_id)?
        .get("rarity")
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new("CN character rarity is missing"))
}

fn stack_conversion(rarity: i64) -> Result<(i64, i64), PersonalServiceError> {
    match rarity {
        1..=3 => Ok((500, 2)),
        4 => Ok((2_000, 10)),
        5 => Ok((10_000, 30)),
        _ => Err(PersonalServiceError::new("CN character rarity is invalid")),
    }
}

fn read_character(
    root: &Map<String, Value>,
    character_id: i64,
) -> Result<Map<String, Value>, PersonalServiceError> {
    root.get("user_character_list")
        .and_then(Value::as_object)
        .and_then(|characters| characters.get(&character_id.to_string()))
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| PersonalServiceError::new("CN player does not own character"))
}

fn require_character<'a>(
    root: &'a mut Map<String, Value>,
    character_id: i64,
) -> Result<&'a mut Map<String, Value>, PersonalServiceError> {
    require_object(root, "user_character_list")?
        .get_mut(&character_id.to_string())
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("CN player does not own character"))
}

fn serialize_character(
    viewer_id: i64,
    character_id: i64,
    character: &Map<String, Value>,
    server_time: i64,
) -> Value {
    let mut response = character_exp_response(character_id, character, server_time);
    response.insert("viewer_id".to_owned(), Value::from(viewer_id));
    response.insert(
        "stack".to_owned(),
        Value::from(
            character
                .get("stack")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
        ),
    );
    Value::Object(response)
}

fn serialize_injected_character(
    character_id: i64,
    character: &Map<String, Value>,
    server_time: i64,
) -> Value {
    Value::Object(character_exp_response(character_id, character, server_time))
}

fn character_exp_response(
    character_id: i64,
    character: &Map<String, Value>,
    server_time: i64,
) -> Map<String, Value> {
    let join_time = format_character_time(character.get("join_time"), server_time);
    let update_time = format_character_time(character.get("update_time"), server_time);
    let exp = character
        .get("exp")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    Map::from_iter([
        ("character_id".to_owned(), Value::from(character_id)),
        ("exp".to_owned(), Value::from(exp)),
        ("exp_total".to_owned(), Value::from(exp)),
        ("create_time".to_owned(), Value::from(join_time.clone())),
        ("update_time".to_owned(), Value::from(update_time)),
        ("join_time".to_owned(), Value::from(join_time)),
    ])
}

fn format_character_time(value: Option<&Value>, server_time: i64) -> String {
    value
        .and_then(Value::as_i64)
        .map(format_client_time)
        .or_else(|| value.and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_else(|| format_client_time(server_time))
}

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}

#[cfg(test)]
mod tests {
    use super::{character_exp_cap, character_level_from_experience};

    // //// 验证角色等级使用累计经验阈值 [@x380kkm 2026-08-28] ////
    #[test]
    fn character_level_uses_cumulative_experience_thresholds() {
        assert_eq!(character_level_from_experience(4, 0, 0), Some(1));
        assert_eq!(character_level_from_experience(3, 11_415, 0), Some(39));
        assert_eq!(character_level_from_experience(3, 11_416, 0), Some(40));
        assert_eq!(character_level_from_experience(4, 76_271, 0), Some(69));
        assert_eq!(character_level_from_experience(4, 76_272, 0), Some(70));
        assert_eq!(character_level_from_experience(4, i64::MAX, 0), Some(70));
        assert_eq!(character_level_from_experience(4, 102_828, 1), Some(74));
        assert_eq!(character_level_from_experience(4, 102_829, 1), Some(75));
        assert_eq!(character_level_from_experience(5, 153_987, 0), Some(79));
        assert_eq!(character_level_from_experience(5, 153_988, 0), Some(80));
        assert_eq!(character_level_from_experience(5, 379_988, 4), Some(100));
        assert_eq!(character_level_from_experience(0, 100, 0), None);
    }
    // //// /验证角色等级使用累计经验阈值 ////

    // //// 验证存档中的突破阶段收敛到角色上限 [@x380kkm 2026-08-28] ////
    #[test]
    fn character_progression_clamps_stored_over_limit_step() {
        assert_eq!(character_exp_cap(4, -1), Some(76_272));
        assert_eq!(character_exp_cap(4, 100), Some(342_410));
        assert_eq!(character_level_from_experience(4, 342_410, 100), Some(100));
    }
    // //// /验证存档中的突破阶段收敛到角色上限 ////
}
