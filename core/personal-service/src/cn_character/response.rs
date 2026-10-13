// audience: internal
// # personal-service-cn-character-response
// 客户端角色增量使用日期字符串, 载入使用数字秒. 时间投影只修改响应, 角色资产和持久化时间保持原值.

use crate::cn_tutorial::format_client_time;
use crate::database::parse_iso_timestamp;
use serde_json::{Map, Value};

const MAX_CHARACTER_TIME_SECONDS: i64 = 7_289_654_399;

// //// 将角色秒数或日期转换为客户端支持的日期字符串 [@x380kkm 2026-09-30] ////
pub(crate) fn character_time(value: Option<&Value>, response_time: i64) -> String {
    let seconds = value
        .and_then(character_timestamp)
        .unwrap_or_else(|| response_time.clamp(0, MAX_CHARACTER_TIME_SECONDS));
    format_client_time(seconds)
}

// //// 解析客户端 1970 到 2200 年范围内的角色时间 [@x380kkm 2026-09-30] ////
fn character_timestamp(value: &Value) -> Option<i64> {
    let seconds = value.as_i64().or_else(|| {
        let text = value.as_str()?.trim();
        text.parse::<i64>().ok().or_else(|| {
            let iso = format!("{}Z", text.trim_end_matches('Z').replace(' ', "T"));
            parse_iso_timestamp(&iso).map(|millis| millis.div_euclid(1000))
        })
    })?;
    (0..=MAX_CHARACTER_TIME_SECONDS)
        .contains(&seconds)
        .then_some(seconds)
}

// //// 将载入响应中的角色时间投影为数字秒 [@x380kkm 2026-09-30] ////
pub(crate) fn project_load_character_times(response: &mut Value, response_time: i64) {
    let Some(characters) = response
        .get_mut("user_character_list")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    for character in characters.values_mut().filter_map(Value::as_object_mut) {
        for field in ["join_time", "update_time"] {
            let seconds = character
                .get(field)
                .and_then(character_timestamp)
                .unwrap_or_else(|| response_time.clamp(0, MAX_CHARACTER_TIME_SECONDS));
            character.insert(field.to_owned(), Value::from(seconds));
        }
    }
}

// //// 按响应和存档时间补齐角色增量的必填日期 [@x380kkm 2026-09-30] ////
pub(crate) fn complete_character_response_times(
    response: &mut Value,
    root: &Map<String, Value>,
    response_time: i64,
) {
    let Some(characters) = response
        .get_mut("character_list")
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    for character in characters.iter_mut().filter_map(Value::as_object_mut) {
        let stored = character
            .get("character_id")
            .and_then(Value::as_i64)
            .and_then(|id| root.get("user_character_list")?.get(id.to_string()));
        for field in ["join_time", "update_time"] {
            let seconds = character
                .get(field)
                .and_then(character_timestamp)
                .or_else(|| stored?.get(field).and_then(character_timestamp))
                .unwrap_or_else(|| response_time.clamp(0, MAX_CHARACTER_TIME_SECONDS));
            let value = format_client_time(seconds);
            character.insert(field.to_owned(), Value::from(value));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // //// 混合角色增量补齐必填时间并保留缓存奖励和角色属性 [@x380kkm 2026-09-30] ////
    #[test]
    fn completes_missing_times_and_preserves_receipt_values() {
        let root = json!({"user_character_list":{
            "1":{"join_time":100,"update_time":200},
            "2":{"join_time":null,"update_time":null},
            "3":{"join_time":"300","update_time":"1970-01-01 00:06:40"},
        }})
        .as_object()
        .unwrap()
        .clone();
        let mut response = json!({"item_list":{"9":20},"character_list":[
            {"character_id":1,"mana_board_awake":{"1":1},"update_time":"1970-01-01 00:01:41"},
            {"character_id":2,"join_time":null,"update_time":null,"exp":900},
            {"character_id":3,"mana_board_awake":{"1":1}},
            {"character_id":4,"join_time":100,"update_time":200,"stack":3},
            {"character_id":5,"mana_board_awake":{"1":1}},
        ]});
        let mut expected = response.clone();
        for (index, join, update) in [
            (0, "1970-01-01 00:01:40", "1970-01-01 00:01:41"),
            (1, "1970-01-01 00:08:20", "1970-01-01 00:08:20"),
            (2, "1970-01-01 00:05:00", "1970-01-01 00:06:40"),
            (3, "1970-01-01 00:01:40", "1970-01-01 00:03:20"),
            (4, "1970-01-01 00:08:20", "1970-01-01 00:08:20"),
        ] {
            expected["character_list"][index]["join_time"] = json!(join);
            expected["character_list"][index]["update_time"] = json!(update);
        }
        complete_character_response_times(&mut response, &root, 500);
        assert_eq!(response, expected);
        complete_character_response_times(&mut response, &root, 600);
        assert_eq!(response, expected);
    }

    // //// 缓存空日期和越界年份采用存档时刻并接受 ISO 日期 [@x380kkm 2026-09-30] ////
    #[test]
    fn restores_parseable_dates_from_saved_values() {
        let root = json!({"user_character_list":{"1":{
            "join_time":"2023-11-14T22:13:20.000Z", "update_time":1_700_000_060,
        }}})
        .as_object()
        .unwrap()
        .clone();
        let mut response = json!({"character_list":[{
            "character_id":1, "join_time":"0000-00-00 00:00:00", "update_time":"2201-01-01 00:00:00",
        }]});
        complete_character_response_times(&mut response, &root, 500);
        assert_eq!(
            response["character_list"][0]["join_time"],
            "2023-11-14 22:13:20"
        );
        assert_eq!(
            response["character_list"][0]["update_time"],
            "2023-11-14 22:14:20"
        );
    }
}
