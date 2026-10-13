// audience: internal
// # personal-service-ranking-presentation
// 客户端以空 best_record 表示尚未参赛, 非空记录的队长引用必须存在于角色主表.

use crate::cn_character_presentation::leader_character_id;
use serde_json::{Map, Value};

// //// 整理成绩摘要的角色引用并保留成绩与奖励字段 [@x380kkm 2026-09-28] ////
pub(super) fn normalize(root: &Map<String, Value>, response: &mut Value) {
    if response.get("best_record").map_or(true, Value::is_null) {
        return;
    }
    let preferred = response.get("leader_character_id").and_then(Value::as_i64);
    let leader = leader_character_id(root, preferred);
    let evolution = root.get("user_character_list").and_then(|characters| characters.get(leader.to_string()))
        .and_then(|character| character.get("evolution_img_level")).and_then(Value::as_i64)
        .or_else(|| (preferred == Some(leader)).then(|| response.get("leader_character_evolution_img_level").and_then(Value::as_i64)).flatten())
        .unwrap_or_default();
    response["leader_character_id"] = Value::from(leader);
    response["leader_character_evolution_img_level"] = Value::from(evolution);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // //// 存档缺少队长或队伍时保留成绩并恢复合法角色引用 [@x380kkm 2026-09-28] ////
    #[test]
    fn repairs_missing_zero_and_unknown_record_leaders() {
        for preferred in [Value::Null, json!(0), json!(999999999)] {
            let root = json!({"user_info":{},"user_party_group_list":{},"user_character_list":{"111001":{"evolution_img_level":1}}})
                .as_object().unwrap().clone();
            let record = json!({"elapsed_time_ms":120000,"is_accomplished":true,"score":37});
            let mut response = json!({"best_record":record,"leader_character_id":preferred,"rank_percentage":50.0});
            normalize(&root, &mut response);
            assert_eq!(response["leader_character_id"], 111001);
            assert_eq!(response["leader_character_evolution_img_level"], 1);
            assert_eq!(response["best_record"], record);
            assert_eq!(response["rank_percentage"], 50.0);
        }
        let mut response = json!({"best_record":{"elapsed_time_ms":10,"is_accomplished":true,"score":1},"leader_character_id":0});
        normalize(&Map::new(), &mut response);
        assert!(crate::cn_character::character_asset_data(response["leader_character_id"].as_i64().unwrap()).is_ok());
    }
}
