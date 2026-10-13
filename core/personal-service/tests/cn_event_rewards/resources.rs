// audience: internal
// # personal-service-cn-event-entry-resources-tests
// 专用入场的资源, 任务进度和战斗记录使用同一持久化结果.

use super::*;

// //// 核对真实关卡费用与失败和重试的持久化结果 [@x380kkm 2026-09-26] ////
#[test]
fn charges_special_event_entries_once_and_rolls_back_failures() {
    for (family, event, quest, cost) in [("raid", 1, 1001, 8), ("rush", 700001, 700001001, 4)] {
        let root = TempDir::new().unwrap();
        let service = PersonalService::start(root.path(), 0).unwrap();
        manage(
            &service,
            "PUT",
            "/v1/time",
            json!({"enabled":true,"iso":"2025-01-02T12:00:00.000Z","rate":1.0}),
        );
        let viewer = signup(&service);
        for activity in [format!("{family}:{event}"), "active-mission:150".to_owned()] {
            manage(
                &service,
                "PUT",
                &format!("/v1/activities/{activity}/mode"),
                json!({"mode":"always"}),
            );
        }
        update_snapshot(root.path(), |player| {
            player["user_info"]["stamina"] = json!(3);
            player["user_info"]["total_stamina_used"] = json!(10);
            player["all_active_mission_list"] = json!({"25008":{"progress":5,"stages":{}}});
        });
        let route = format!("/api/index.php/event/{family}/battle/start");
        let body = json!({"viewer_id":viewer,"quest_id":quest,"party_id":1,"party_group_id":1,"play_id":"paid-entry","is_auto_start_mode":false});
        let before = snapshot(root.path());
        let mut unknown = body.clone();
        unknown["quest_id"] = json!(999999999);
        let rejected = send(&service, &route, unknown);
        assert!(
            rejected.starts_with("HTTP/1.1 400") && rejected.contains("quest_not_found"),
            "{rejected}"
        );
        let mut mismatch = body.clone();
        mismatch["event_id"] = json!(event + 1);
        let rejected = send(&service, &route, mismatch);
        assert!(
            rejected.starts_with("HTTP/1.1 400") && rejected.contains("invalid_event_id"),
            "{rejected}"
        );
        let rejected = send(&service, &route, body.clone());
        assert!(
            rejected.starts_with("HTTP/1.1 400") && rejected.contains("insufficient_stamina"),
            "{rejected}"
        );
        assert_eq!(snapshot(root.path()), before);
        update_snapshot(root.path(), |player| {
            player["user_info"]["stamina"] = json!(100);
            let heal_time = player["user_info"]["stamina_heal_time"].as_i64().unwrap();
            player["user_info"]["stamina_heal_time"] = json!(heal_time + 86400);
        });
        let before = snapshot(root.path());
        let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
        db.execute_batch("CREATE TRIGGER reject_entry_receipt BEFORE INSERT ON single_battle_action_receipts BEGIN SELECT RAISE(ABORT, 'entry receipt unavailable'); END;").unwrap();
        let rejected = send(&service, &route, body.clone());
        assert!(rejected.starts_with("HTTP/1.1 500"), "{rejected}");
        assert_eq!(snapshot(root.path()), before);
        let active_count: i64 = db
            .query_row("SELECT COUNT(*) FROM active_single_quests", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(active_count, 0);
        db.execute_batch("DROP TRIGGER reject_entry_receipt")
            .unwrap();
        drop(db);
        let started = decode_response::<Value>(&send(&service, &route, body.clone()));
        assert_eq!(started.data_headers.result_code, 1);
        assert_eq!(started.data["user_info"]["stamina"], 100 - cost);
        assert_eq!(
            started.data["user_info"]["stamina_heal_time"],
            started.data_headers.servertime
        );
        let saved = snapshot(root.path());
        assert_eq!(saved["user_info"]["stamina"], 100 - cost);
        assert_eq!(saved["user_info"]["total_stamina_used"], 10 + cost);
        assert_eq!(
            saved["all_active_mission_list"]["25008"]["progress"],
            5 + cost
        );
        assert!(started.data["active_mission_list"]
            .as_array()
            .is_some_and(|updates| !updates.is_empty()));
        service.stop().unwrap();
        let service = PersonalService::start(root.path(), 0).unwrap();
        let resumed = decode_response::<Value>(&send(&service, &route, body));
        assert_eq!(resumed.data, started.data);
        assert_eq!(
            resumed.data_headers.servertime,
            started.data_headers.servertime
        );
        assert_eq!(snapshot(root.path()), saved);
        update_snapshot(root.path(), |player| {
            player["user_info"]["stamina_heal_time"] =
                json!(started.data_headers.servertime + 86400);
        });
        let loaded = decode_response::<Value>(&send(
            &service,
            "/api/index.php/load",
            json!({"viewer_id":viewer,"keychain":viewer}),
        ));
        assert_eq!(loaded.data["user_info"]["stamina"], 100 - cost);
        assert_eq!(
            loaded.data["user_info"]["stamina_heal_time"],
            loaded.data_headers.servertime
        );
        assert_eq!(snapshot(root.path())["user_info"]["stamina"], 100 - cost);
        update_snapshot(root.path(), |player| {
            player["user_info"]["stamina_heal_time"] =
                json!(loaded.data_headers.servertime + 86400);
            player["item_list"]["106"] = json!(1);
        });
        let recovered = decode_response::<Value>(&send(
            &service,
            "/api/index.php/item/use_item",
            json!({"viewer_id":viewer,"items":[{"id":106,"number":1}]}),
        ));
        assert_eq!(recovered.data["user_info"]["stamina"], 200 - cost);
        assert_eq!(recovered.data["item_list"]["106"], 0);
        let saved = snapshot(root.path());
        assert_eq!(saved["user_info"]["stamina"], 200 - cost);
        assert_eq!(
            saved["user_info"]["stamina_heal_time"],
            recovered.data_headers.servertime
        );
        service.stop().unwrap();
    }
}
