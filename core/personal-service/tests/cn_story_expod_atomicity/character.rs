// audience: internal
// # personal-service-cn-character-settlement-tests
// 角色突破的副本, 奖励, 任务和请求凭据共同提交, 相同请求在重启后仍保持同一结果.

use super::*;

// //// 建立可用副本和角色突破请求 [@x380kkm 2026-09-23] ////
fn prepare_character() -> (TempDir, PersonalService, i64) {
    let (root, service, viewer) = setup();
    update_player(root.path(), |player| {
        player["user_character_list"]["1"]["over_limit_step"] = json!(0);
        player["user_character_list"]["1"]["stack"] = json!(2);
    });
    (root, service, viewer)
}

fn cases() -> [(&'static str, Value, i64); 2] {
    [
        (
            "character/over_limit",
            json!({"character_id":1,"over_limit_count":1,"use_stack":true}),
            1,
        ),
        ("character/bulk_over_limit", json!({}), 2),
    ]
}
// //// /建立可用副本和角色突破请求 ////

// //// 角色突破最终保存失败时保留副本和全部任务状态 [@x380kkm 2026-09-23] ////
#[test]
fn rolls_back_uncapping_assets_missions_and_receipts_on_final_save_failure() {
    for (route, mut body, steps) in cases() {
        let (root, service, viewer) = prepare_character();
        body["viewer_id"] = json!(viewer);
        body["api_count"] = json!(71);
        let before = player_state(root.path());
        reject_snapshot_when(
            root.path(),
            r#"COALESCE(json_extract(NEW.data_json, '$.all_active_mission_list."13040".progress'), 0) > 0"#,
        );
        let failed = request(&service, route, body.clone());
        assert!(failed.starts_with("HTTP/1.1 500"), "{route}: {failed}");
        assert!(
            player_state(root.path()) == before,
            "{route} preserves the complete state"
        );
        allow_snapshot(root.path());

        let received = decode_response::<Value>(&request(&service, route, body));
        let after = player_state(root.path());
        assert_eq!(
            after.snapshot["user_character_list"]["1"]["over_limit_step"],
            steps
        );
        assert_eq!(
            after.snapshot["user_character_list"]["1"]["stack"],
            2 - steps
        );
        assert_eq!(
            after.snapshot["user_info"]["free_vmoney"].as_i64().unwrap(),
            before.snapshot["user_info"]["free_vmoney"]
                .as_i64()
                .unwrap()
                + 100
        );
        assert!(has_mission_reward(&received.data, 38, 38001));
        assert_eq!(
            after.snapshot["all_active_mission_list"]["13040"]["progress"],
            steps
        );
        service.stop().unwrap();
    }
}
// //// /角色突破最终保存失败时保留副本和全部任务状态 ////

// //// 角色突破重放保留同一结果并接受新的操作计数 [@x380kkm 2026-09-23] ////
#[test]
fn replays_uncapping_after_restart_and_accepts_a_new_request() {
    for (route, mut body, steps) in cases() {
        let (root, service, viewer) = prepare_character();
        body["viewer_id"] = json!(viewer);
        body["api_count"] = json!(71);
        let first = decode_response::<Value>(&request(&service, route, body.clone()));
        let after_first = player_state(root.path());
        let replay = decode_response::<Value>(&request(&service, route, body.clone()));
        assert_eq!(replay.data, first.data);
        assert!(player_state(root.path()) == after_first);
        if route == "character/over_limit" {
            let mut changed = body.clone();
            changed["over_limit_count"] = json!(2);
            let conflict = request(&service, route, changed);
            assert!(
                conflict.starts_with("HTTP/1.1 400 Bad Request"),
                "{conflict}"
            );
            assert!(conflict.ends_with("{\"error\":\"character_request_conflict\"}"));
            assert!(player_state(root.path()) == after_first);
        }
        service.stop().unwrap();
        let service = PersonalService::start(root.path(), 0).unwrap();
        let replay = decode_response::<Value>(&request(&service, route, body.clone()));
        assert_eq!(replay.data, first.data);
        assert!(player_state(root.path()) == after_first);

        update_player(root.path(), |player| {
            let stack = player["user_character_list"]["1"]["stack"]
                .as_i64()
                .unwrap();
            player["user_character_list"]["1"]["stack"] = json!(stack + 1);
        });
        body["api_count"] = json!(72);
        decode_response::<Value>(&request(&service, route, body));
        let after_second = player_state(root.path());
        assert_eq!(
            after_second.snapshot["user_character_list"]["1"]["over_limit_step"],
            steps + 1
        );
        assert_eq!(
            after_second.snapshot["user_character_list"]["1"]["stack"],
            2 - steps
        );
        let loaded = decode_response::<Value>(&request(
            &service,
            "load",
            json!({"viewer_id":viewer,"keychain":viewer}),
        ));
        assert!(loaded.data.get("character_request_receipts").is_none());
        service.stop().unwrap();
    }
}
// //// /角色突破重放保留同一结果并接受新的操作计数 ////
