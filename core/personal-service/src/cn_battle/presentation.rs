// audience: internal
// # personal-service-cn-battle-presentation
// 活动结果的角色引用取自客户端主表, 展示修复保留战斗成绩和奖励回执.

use crate::cn_character_presentation::{is_valid_character, leader_character_id};
use serde_json::{Map, Value};

// //// 从实际队伍生成具有合法主角色的展示列表 [@x380kkm 2026-09-28] ////
pub(crate) fn main_character_ids(
    root: &Map<String, Value>,
    characters: &[Option<i64>],
) -> Map<String, Value> {
    let mut ids = characters
        .iter()
        .take(3)
        .enumerate()
        .filter_map(|(slot, id)| {
            id.filter(|id| is_valid_character(*id))
                .map(|id| (slot.to_string(), Value::from(id)))
        })
        .collect::<Map<_, _>>();
    ensure_main_character(root, &mut ids);
    ids
}

fn ensure_main_character(root: &Map<String, Value>, ids: &mut Map<String, Value>) {
    if ids.is_empty() {
        ids.insert("0".to_owned(), Value::from(leader_character_id(root, None)));
    }
}

// //// 整理历史结算中的角色展示并保留奖励结果 [@x380kkm 2026-09-28] ////
pub(crate) fn normalize_finish_response(root: &Map<String, Value>, response: &mut Value) {
    if let Some(event) = response
        .get_mut("carnival_event")
        .and_then(Value::as_object_mut)
    {
        let preferred = event.get("leader_character_id").and_then(Value::as_i64);
        event.insert(
            "leader_character_id".to_owned(),
            Value::from(leader_character_id(root, preferred)),
        );
    }
    for field in ["score_attack_event", "solo_time_attack_event"] {
        if let Some(event) = response.get_mut(field).and_then(Value::as_object_mut) {
            let mut ids = event
                .get("main_character_ids")
                .and_then(Value::as_object)
                .map(|ids| {
                    ids.iter()
                        .filter(|(slot, id)| {
                            matches!(slot.as_str(), "0" | "1" | "2")
                                && id.as_i64().is_some_and(is_valid_character)
                        })
                        .map(|(slot, id)| (slot.clone(), id.clone()))
                        .collect::<Map<_, _>>()
                })
                .unwrap_or_default();
            ensure_main_character(root, &mut ids);
            event.insert("main_character_ids".to_owned(), Value::Object(ids));
        }
    }
    if let Some(event) = response
        .get_mut("rush_event")
        .and_then(Value::as_object_mut)
    {
        for field in [
            "rush_battle_played_party_list",
            "endless_battle_played_party_list",
        ] {
            if let Some(parties) = event.get_mut(field) {
                crate::cn_activity::normalize_played_parties(parties);
            }
        }
    }
}
// //// /整理历史结算中的角色展示并保留奖励结果 ////

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // //// 无效和空编组使用合法展示角色而有效队伍保持槽位 [@x380kkm 2026-09-28] ////
    #[test]
    fn keeps_valid_slots_and_restores_empty_result_parties() {
        let root = json!({"user_character_list":{"111001":{}}})
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(
            main_character_ids(&root, &[Some(1), None, Some(111001)]),
            json!({"0":1,"2":111001}).as_object().unwrap().clone()
        );
        for slots in [
            vec![],
            vec![None, None, None],
            vec![Some(0), Some(999999999)],
        ] {
            assert_eq!(
                main_character_ids(&root, &slots),
                json!({"0":111001}).as_object().unwrap().clone()
            );
        }
        assert!(is_valid_character(
            main_character_ids(&Map::new(), &[])["0"].as_i64().unwrap()
        ));
    }

    // //// 旧回执只替换失效角色并保留货币成绩奖励与有效空槽 [@x380kkm 2026-09-28] ////
    #[test]
    fn normalizes_cached_event_characters_without_changing_rewards() {
        let root = json!({"user_character_list":{"111001":{}}})
            .as_object()
            .unwrap()
            .clone();
        let mut response = json!({
            "user_info":{"free_vmoney":1234}, "item_list":{"9":7},
            "carnival_event":{"leader_character_id":0,"score":{"time_bonus":45000},"reward_ids":[11]},
            "score_attack_event":{"main_character_ids":{"0":999999999,"2":1},"reward_ids":[12]},
            "solo_time_attack_event":{"main_character_ids":{},"elapsed_time_ms":70000,"reward_ids":[13]},
            "rush_event":{"high_score":6000,"endless_battle_played_party_list":{"1":{
                "character_id_1":999999999,"character_id_2":1,"character_id_3":null,
                "equipment_id_1":7,"unison_character_id_1":0
            }}}
        });
        let mut expected = response.clone();
        expected["carnival_event"]["leader_character_id"] = json!(111001);
        expected["score_attack_event"]["main_character_ids"] = json!({"2":1});
        expected["solo_time_attack_event"]["main_character_ids"] = json!({"0":111001});
        expected["rush_event"]["endless_battle_played_party_list"]["1"]["character_id_1"] =
            Value::Null;
        expected["rush_event"]["endless_battle_played_party_list"]["1"]["unison_character_id_1"] =
            Value::Null;
        normalize_finish_response(&root, &mut response);
        assert_eq!(response, expected);
        normalize_finish_response(&root, &mut response);
        assert_eq!(response, expected);
    }
}
