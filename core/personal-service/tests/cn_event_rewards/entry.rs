// audience: internal
// # personal-service-cn-event-entry-tests
// 活动专用入场在截止和服务重启后保留当前局任务资格与重放结果.

use super::*;

// //// 专用开战跨截止结算任务并保存重放结果 [@x380kkm 2026-09-26] ////
#[test]
fn preserves_missions_for_special_event_entries_after_closure() {
    for (family, event, category, quest, cost) in
        [("raid", 1, 23, 1001, 8), ("rush", 700006, 24, 700006008, 0)]
    {
        let root = TempDir::new().unwrap();
        let service = PersonalService::start(root.path(), 0).unwrap();
        manage(
            &service,
            "PUT",
            "/v1/time",
            json!({"enabled":true,"iso":"2025-01-02T12:00:00.000Z","rate":1.0}),
        );
        let viewer = signup(&service);
        let activity = format!("{family}:{event}");
        for id in [&activity, "active-mission:150"] {
            manage(
                &service,
                "PUT",
                &format!("/v1/activities/{id}/mode"),
                json!({"mode":"always"}),
            );
        }
        update_snapshot(root.path(), |player| {
            player["all_active_mission_list"] = json!({"25003":{"progress":2,"stages":{}}});
            player["user_info"]["stamina"] = json!(100);
            let heal_time = player["user_info"]["stamina_heal_time"].as_i64().unwrap();
            player["user_info"]["stamina_heal_time"] = json!(heal_time + 86400);
        });
        let route = format!("/api/index.php/event/{family}/battle/start");
        let body = json!({"viewer_id":viewer,"event_id":event,"quest_id":quest,"party_id":1,"party_group_id":1,"play_id":"special-entry","is_auto_start_mode":false});
        let started = decode_response::<Value>(&send(&service, &route, body.clone()));
        assert_eq!(started.data_headers.result_code, 1);
        assert_eq!(started.data["user_info"]["stamina"], 100 - cost);
        assert_eq!(
            snapshot(root.path())["user_info"]["stamina_heal_time"],
            started.data_headers.servertime
        );
        for id in [&activity, "active-mission:150"] {
            manage(
                &service,
                "POST",
                &format!("/v1/activities/{id}/close"),
                json!({}),
            );
        }
        manage(
            &service,
            "PUT",
            "/v1/time",
            json!({"enabled":true,"iso":"2025-01-03T12:00:00.000Z","rate":1.0}),
        );
        service.stop().unwrap();
        let service = PersonalService::start(root.path(), 0).unwrap();
        let before_resume = snapshot(root.path());
        let resumed = decode_response::<Value>(&send(&service, &route, body.clone()));
        assert_eq!(resumed.data, started.data);
        assert_eq!(
            resumed.data_headers.servertime,
            started.data_headers.servertime
        );
        assert_eq!(snapshot(root.path()), before_resume);
        let mut conflicting = body.clone();
        conflicting["quest_id"] = json!(quest - 1);
        assert!(send(&service, &route, conflicting).starts_with("HTTP/1.1 409"));
        let mut next = body.clone();
        next["play_id"] = json!("next-special-entry");
        let rejected = decode_response::<Value>(&send(&service, &route, next.clone()));
        assert_eq!(rejected.data_headers.result_code, 4050);
        assert_eq!(snapshot(root.path()), before_resume);
        let finish = finish_body(viewer, category, quest, "special-entry", true);
        let result = request(
            &service,
            "/api/index.php/single_battle_quest/finish",
            finish.clone(),
        );
        assert_eq!(
            snapshot(root.path())["all_active_mission_list"]["25003"]["progress"],
            3,
            "{family}: {result}"
        );
        let saved = snapshot(root.path());
        assert_eq!(
            request(
                &service,
                "/api/index.php/single_battle_quest/finish",
                finish
            ),
            result
        );
        assert_eq!(snapshot(root.path()), saved);
        manage(
            &service,
            "PUT",
            &format!("/v1/activities/{activity}/mode"),
            json!({"mode":"always"}),
        );
        request(&service, &route, next);
        request(
            &service,
            "/api/index.php/single_battle_quest/finish",
            finish_body(viewer, category, quest, "next-special-entry", true),
        );
        assert_eq!(
            snapshot(root.path())["all_active_mission_list"]["25003"]["progress"],
            3
        );
        service.stop().unwrap();
    }
}
