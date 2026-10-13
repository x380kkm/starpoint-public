// audience: internal
// # personal-service-cn-rush-presentation
// 旧成绩保留轮数, 时间和队伍槽位, 失效角色引用使用客户端空成员表示.

use crate::cn_character_presentation::normalize_optional_character_reference;
use serde_json::{Map, Value};

// //// 按角色主表整理历史最佳队伍及逐轮参战成员 [@x380kkm 2026-09-28] ////
pub(crate) fn normalize_character_references(event: &mut Map<String, Value>) {
    if let Some(ids) = event.get_mut("endless_battle_max_round_character_ids").and_then(Value::as_array_mut) {
        normalize_ids(ids);
    }
    for field in ["folder_played_party_list", "endless_played_party_list"] {
        if let Some(parties) = event.get_mut(field) {
            normalize_played_parties(parties);
        }
    }
}

// //// 将逐轮参战详情中的缺失角色表示为原槽位空值 [@x380kkm 2026-09-28] ////
pub(crate) fn normalize_played_parties(value: &mut Value) {
    match value {
        Value::Object(parties) => parties.values_mut().for_each(normalize_party),
        Value::Array(parties) => parties.iter_mut().for_each(normalize_party),
        _ => {}
    }
}

fn normalize_party(value: &mut Value) {
    let Some(party) = value.as_object_mut() else { return; };
    for field in ["character_ids", "unison_character_ids"] {
        if let Some(ids) = party.get_mut(field).and_then(Value::as_array_mut) {
            normalize_ids(ids);
        }
    }
    for slot in 1..=3 {
        for prefix in ["character_id_", "unison_character_id_"] {
            if let Some(value) = party.get_mut(&format!("{prefix}{slot}")) {
                normalize_optional_character_reference(value);
            }
        }
    }
}

// //// 保留数组槽位并清除主表中不存在的角色编号 [@x380kkm 2026-09-28] ////
fn normalize_ids(ids: &mut [Value]) {
    for id in ids {
        normalize_optional_character_reference(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // //// 旧排行榜和参战详情保留成绩并清除无效角色引用 [@x380kkm 2026-09-28] ////
    #[test]
    fn preserves_rush_scores_with_empty_and_unknown_members() {
        let mut root = json!({"user_info":{"name":"Player","rank_point":0},
            "cn_activity_state":{"event_families":{"rush":{"700006":{
                "endless_battle_max_round":12,"endless_battle_max_round_time":12345,
                "endless_battle_max_round_character_ids":[0,99999999,111001],
                "endless_battle_max_round_character_evolution_img_lvls":[0,0,2],
                "endless_played_party_list":{"12":{"character_id_1":0,"character_id_2":99999999,
                    "character_id_3":111001,"unison_character_id_1":0,"equipment_id_3":5100001}}
            }}}}}).as_object().unwrap().clone();
        let ranking = super::super::player_ranking(&root, "rush", 700006, 1).unwrap();
        assert_eq!(ranking["best_round"], 12);
        assert_eq!(ranking["elapsed_time_ms"], 12345);
        assert_eq!(ranking["party_member_list"], json!([{"character_id":111001,"evolution_img_level":2}]));
        let event = root["cn_activity_state"]["event_families"]["rush"]["700006"].as_object_mut().unwrap();
        normalize_character_references(event);
        assert_eq!(event["endless_battle_max_round_character_ids"], json!([null,null,111001]));
        assert_eq!(event["endless_battle_max_round"], 12);
        let party = &event["endless_played_party_list"]["12"];
        assert_eq!(party["character_id_1"], Value::Null);
        assert_eq!(party["character_id_2"], Value::Null);
        assert_eq!(party["character_id_3"], 111001);
        assert_eq!(party["unison_character_id_1"], Value::Null);
        assert_eq!(party["equipment_id_3"], 5100001);
    }
}
