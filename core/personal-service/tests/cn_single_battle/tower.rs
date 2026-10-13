// audience: internal
// # personal-service-cn-tower-battle-tests
// 幽玄域按父活动和当期期别开放, 已入场战斗跨关闭和换期保留结算资格.

use super::*;

// //// 修改幽玄域管理规则 [@x380kkm 2026-09-25] ////
fn manage_tower(service: &PersonalService, method: &str, route: &str, body: Value) {
    let authorization = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        method,
        route,
        "application/json",
        &[("Authorization", authorization.as_str())],
        body.to_string().as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
}

// //// 约束幽玄域新入场并保留跨期战斗的进度与奖励 [@x380kkm 2026-09-25] ////
#[test]
fn closes_new_tower_entries_and_finishes_started_quests_across_periods() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    set_virtual_time(&service, "2022-05-20T12:00:00.000Z");
    let viewer_id = decode_response::<SignupData>(&send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 209 }),
    ))
    .data_headers
    .viewer_id;
    let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let snapshot = || {
        let stored: String = database
            .query_row("SELECT data_json FROM player_snapshots", [], |row| {
                row.get(0)
            })
            .unwrap();
        serde_json::from_str::<Value>(&stored).unwrap()
    };
    let mut player = snapshot();
    player["user_info"]["stamina"] = json!(99);
    database
        .execute(
            "UPDATE player_snapshots SET data_json=?1",
            [player.to_string()],
        )
        .unwrap();
    manage_tower(
        &service,
        "POST",
        "/v1/activities/tower-dungeon:1/close",
        json!({}),
    );
    let before = snapshot();
    let closed = decode_response::<Value>(&start_battle_for(
        service.port(),
        viewer_id,
        20,
        1001,
        "tower-closed",
        false,
    ));
    assert_eq!(closed.data_headers.result_code, 4050);
    assert_eq!(snapshot(), before);
    manage_tower(
        &service,
        "PUT",
        "/v1/activities/calendar/tower-dungeon:1",
        json!({
            "enabled":true, "start_at":"2022-05-21T00:00:00.000Z", "end_at":"2022-05-22T00:00:00.000Z"
        }),
    );
    let future = decode_response::<Value>(&start_battle_for(
        service.port(),
        viewer_id,
        20,
        1001,
        "tower-future",
        false,
    ));
    assert_eq!(future.data_headers.result_code, 4050);
    set_virtual_time(&service, "2022-05-21T12:00:00.000Z");
    let started = decode_response::<Value>(&start_battle_for(
        service.port(),
        viewer_id,
        20,
        1001,
        "tower-active",
        false,
    ));
    assert_eq!(started.data_headers.result_code, 1);
    manage_tower(
        &service,
        "POST",
        "/v1/activities/tower-dungeon:1/close",
        json!({}),
    );
    set_virtual_time(&service, "2022-06-20T12:00:00.000Z");
    let resumed = decode_response::<Value>(&start_battle_for(
        service.port(),
        viewer_id,
        20,
        1001,
        "tower-active",
        false,
    ));
    assert_eq!(resumed.data, started.data);
    let finished = decode_response::<Value>(&finish_battle_for(
        service.port(),
        viewer_id,
        20,
        1001,
        "tower-active",
        100_000,
        1_000,
    ));
    assert_eq!(finished.data_headers.result_code, 1);
    let settled = snapshot();
    assert!(settled["quest_progress"]["20"]
        .as_array()
        .unwrap()
        .iter()
        .any(|quest| quest["quest_id"] == 1001 && quest["finished"] == true));
    let replay = decode_response::<Value>(&finish_battle_for(
        service.port(),
        viewer_id,
        20,
        1001,
        "tower-active",
        100_000,
        1_000,
    ));
    assert_eq!(replay.data, finished.data);
    assert_eq!(snapshot(), settled);
    set_activity_always(&service, "tower-dungeon:1");
    let old_period = decode_response::<Value>(&start_battle_for(
        service.port(),
        viewer_id,
        20,
        1001,
        "tower-old-period",
        false,
    ));
    assert_eq!(old_period.data_headers.result_code, 4050);
    let next = decode_response::<Value>(&start_battle_for(
        service.port(),
        viewer_id,
        20,
        1011,
        "tower-next-period",
        false,
    ));
    assert_eq!(next.data_headers.result_code, 1);
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let finished = decode_response::<Value>(&finish_battle_for(
        service.port(),
        viewer_id,
        20,
        1011,
        "tower-next-period",
        100_000,
        1_000,
    ));
    assert_eq!(finished.data_headers.result_code, 1);
    let stored = snapshot();
    for id in [1001, 1011] {
        assert!(stored["quest_progress"]["20"]
            .as_array()
            .unwrap()
            .iter()
            .any(|quest| quest["quest_id"] == id && quest["finished"] == true));
    }
    service.stop().unwrap();
}
// //// /约束幽玄域新入场并保留跨期战斗的进度与奖励 ////
