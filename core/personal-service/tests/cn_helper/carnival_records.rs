// audience: internal
// # personal-service-carnival-record-presentation-tests
// 土俑入口与小助手读取相同成绩, 展示队伍保留有效成员位置和玩家奖励记录.

use super::{api, request, save_snapshot, setup, snapshot};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

// //// 迁移和重启后的土俑成绩以空槽位呈现失效角色 [@x380kkm 2026-10-02] ////
#[test]
fn preserves_carnival_scores_and_rewards_with_invalid_record_members() {
    for legacy in [false, true] {
        let root = TempDir::new().unwrap();
        let (service, viewer) = setup(root.path());
        let mut player = snapshot(root.path());
        let mut original_records = json!([
            {"folder_id":1,"best_score":12345,"previous_score":12000,
                "previous_character_ids":[111001,0,999999999],
                "previous_unison_character_ids":[null,-1,111002]},
            {"folder_id":2,"best_score":9876,"previous_score":9876,
                "previous_character_ids":null}
        ]);
        if !legacy {
            original_records.as_array_mut().unwrap().extend([
                Value::Null, json!(7), json!([]), json!({}),
                json!({"folder_id":"1","best_score":8888}),
                json!({"folder_id":2147483648_i64,"best_score":8888}),
                json!({"folder_id":3,"best_score":"invalid","previous_score":{},
                    "previous_character_ids":[null,null,111003],"previous_unison_character_ids":[]}),
            ]);
        }
        player["cn_activity_state"]["event_families"]["carnival"]["5001"] =
            json!({"carnival_records": original_records});
        if legacy {
            player.as_object_mut().unwrap().remove("cn_activity_state");
            player["carnival_event_records"] = json!({
                "5001:1":{"score":12345,"character_ids":[111001,0,999999999]},
                "5001:2":{"score":9876,"character_ids":null}
            });
        }
        player["carnival_reward_receipts"] = json!({"69":{"rewards":[{"item_id":999005,"count":10}]}});
        save_snapshot(root.path(), &player);
        let before = snapshot(root.path());
        let mut expected = json!([
            {"folder_id":1,"best_score":12345,"previous_score":if legacy {12345} else {12000},
                "previous_character_ids":[111001,null,null],
                "previous_unison_character_ids":if legacy {json!([null,null,null])} else {json!([null,null,111002])}},
            {"folder_id":2,"best_score":9876,"previous_score":9876,
                "previous_character_ids":[null,null,null],"previous_unison_character_ids":[null,null,null]}
        ]);
        if !legacy {
            expected.as_array_mut().unwrap().push(json!({
                "folder_id":3,"best_score":null,"previous_score":null,
                "previous_character_ids":[null,null,111003],"previous_unison_character_ids":[]
            }));
            let helper = api(&service, "/assistant/get_assistant_list", request(viewer));
            assert_eq!(helper["records"], expected);
            assert_eq!(snapshot(root.path()), before);
        }
        let index = api(&service, "/carnival_event/index", json!({"viewer_id":viewer,"event_id":5001}));
        assert_eq!(index["records"], expected);
        let after = snapshot(root.path());
        for field in ["carnival_reward_receipts", "user_info", "item_list", "user_character_list", "user_equipment_list"] {
            assert_eq!(after[field], before[field], "{field}");
        }
        if legacy {
            assert_eq!(after["carnival_event_records"], before["carnival_event_records"]);
        } else {
            assert_eq!(after["cn_activity_state"]["event_families"]["carnival"]["5001"]["carnival_records"], original_records);
        }
        service.stop().unwrap();
        let restarted = PersonalService::start(root.path(), 0).unwrap();
        let helper = api(&restarted, "/assistant/get_assistant_list", request(viewer));
        assert_eq!(helper["records"], expected);
        assert_eq!(snapshot(root.path()), after);
        if !legacy {
            for invalid_records in [Value::Null, json!({}), json!(17)] {
                let mut player = after.clone();
                player["cn_activity_state"]["event_families"]["carnival"]["5001"]["carnival_records"] = invalid_records.clone();
                save_snapshot(root.path(), &player);
                for (path, body) in [
                    ("/assistant/get_assistant_list", request(viewer)),
                    ("/carnival_event/index", json!({"viewer_id":viewer,"event_id":5001})),
                ] {
                    assert_eq!(api(&restarted, path, body)["records"], json!([]));
                    assert_eq!(snapshot(root.path()), player);
                }
            }
        }
        restarted.stop().unwrap();
    }
}
