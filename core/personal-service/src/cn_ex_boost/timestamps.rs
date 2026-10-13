// audience: internal
// # cn-ex-boost-timestamps
// EX 角色的存储和载入使用数字秒, 角色增量响应使用日期字符串.

use super::RECEIPTS_FIELD;
use crate::cn_tutorial::format_client_time;
use crate::database::parse_iso_timestamp;
use serde_json::{Map, Value};

// //// 保留 EX 角色的实际时刻并恢复存储时间类型 [@x380kkm 2026-09-27] ////
pub(crate) fn normalize_saved_character_times(root: &mut Map<String, Value>) {
    if let Some(characters) = root
        .get_mut("user_character_list")
        .and_then(Value::as_object_mut)
    {
        for character in characters.values_mut().filter_map(Value::as_object_mut) {
            if !character.get("ex_boost").is_some_and(Value::is_object) {
                continue;
            }
            for field in ["join_time", "update_time"] {
                if let Some(time) = character
                    .get(field)
                    .and_then(Value::as_str)
                    .and_then(character_timestamp)
                {
                    character.insert(field.to_owned(), Value::from(time));
                }
            }
        }
    }
    normalize_receipt_times(root);
}

fn character_timestamp(value: &str) -> Option<i64> {
    if let Ok(seconds) = value.parse() {
        return Some(seconds);
    }
    let iso = format!("{}Z", value.trim_end_matches('Z').replace(' ', "T"));
    let millis = parse_iso_timestamp(&iso)?;
    (millis % 1_000 == 0).then_some(millis / 1_000)
}
// //// /保留 EX 角色的实际时刻并恢复存储时间类型 ////

// //// 将 EX 凭据中的角色时间转换为原响应时刻的日期字符串 [@x380kkm 2026-09-27] ////
fn normalize_receipt_times(root: &mut Map<String, Value>) {
    let Some(receipts) = root.get_mut(RECEIPTS_FIELD).and_then(Value::as_object_mut) else {
        return;
    };
    for receipt in receipts.values_mut() {
        let Some(characters) = receipt
            .get_mut("response")
            .and_then(|response| response.get_mut("character_list"))
            .and_then(Value::as_array_mut)
        else {
            continue;
        };
        for character in characters.iter_mut().filter_map(Value::as_object_mut) {
            for field in ["create_time", "join_time", "update_time"] {
                let seconds = character
                    .get(field)
                    .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()));
                if let Some(seconds) = seconds {
                    character.insert(field.to_owned(), Value::from(format_client_time(seconds)));
                }
            }
        }
    }
}
// //// /将 EX 凭据中的角色时间转换为原响应时刻的日期字符串 ////
