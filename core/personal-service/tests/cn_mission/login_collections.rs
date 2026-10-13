// audience: internal
// # personal-service-cn-mission-login-collection-api-tests
// 隔离服务验证活动登录任务的游戏日去重, 活动重置, 任务奖励入库和邮件领取对应的收集数量.

use super::*;

fn mission_progress(service: &PersonalService, viewer: i64, category: i64, id: i64) -> i64 {
    let response = decode_response::<Value>(&send(service, "/api/index.php/mission/get_mission_progress",
        json!({"viewer_id":viewer,"api_count":1,"category_list":[{"category":category}]}))).data;
    response["mission_progress_list"].as_array().unwrap().iter().find(|entry| entry["mission_id"] == id)
        .unwrap()["progress_value"].as_i64().unwrap()
}

fn prepare_active_reward(root: &Path, held: i64) {
    update_player(root, |player| {
        player["item_list"]["100000"] = json!(held);
        player["mission_window_progress"] = json!({});
        player["active_mission_receipts"] = json!({});
        player["all_active_mission_list"]["11020"] = json!({"progress":1,"stages":{"1":false}});
    });
}

// //// 验证活动登录天数在有效期内累计并随管理重置重新记录 [@x380kkm 2026-09-28] ////
#[test]
fn counts_activity_login_days_across_clock_changes_and_reset() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    std::fs::write(cdn.path().join("activity-catalog.json"), json!({"format_version":1,"activities":[{
        "activity_id":"collect-item:10016","name":"特别活动任务","kind":"collect-item",
        "default_start_at_ms":1708574400000_i64,"default_end_at_ms":1709827199000_i64
    }]}).to_string()).unwrap();
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn.path()).unwrap();
    let viewer = signup(&service, 928201);
    set_time(&service, "2024-02-21T06:00:00.000Z");
    load(&service, viewer);
    assert_eq!(collect_progress(&service, viewer, 10016, 2089), 0);
    let tickets = read_player(root.path())["item_list"]["999003"].as_i64().unwrap_or_default();
    for day in 22..=26 {
        set_time(&service, &format!("2024-02-{day}T06:00:00.000Z"));
        load(&service, viewer);
        assert_eq!(collect_progress(&service, viewer, 10016, 2089), day - 21);
        assert_eq!(collect_progress(&service, viewer, 10016, 2089), day - 21);
    }
    assert_eq!(read_player(root.path())["item_list"]["999003"], tickets + 5);
    set_time(&service, "2024-02-23T06:00:00.000Z");
    load(&service, viewer);
    assert_eq!(collect_progress(&service, viewer, 10016, 2089), 5);
    service.stop().unwrap();
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn.path()).unwrap();
    assert_eq!(collect_progress(&service, viewer, 10016, 2089), 5);
    set_time(&service, "2024-03-08T06:00:00.000Z");
    load(&service, viewer);
    assert_eq!(collect_progress(&service, viewer, 10016, 2089), 0);
    set_time(&service, "2024-02-26T06:00:00.000Z");
    let saves = manage(&service, "GET", "/v1/local-saves", json!({}));
    let reset = format!("/v1/local-saves/{}/activities/collect-item:10016/reset", saves["slots"][0]["id"]);
    manage(&service, "POST", &reset, json!({"expected_round":0}));
    assert_eq!(collect_progress(&service, viewer, 10016, 2089), 1);
    assert_eq!(read_player(root.path())["item_list"]["999003"], tickets + 5);

    set_time(&service, "2019-11-27T06:00:00.000Z");
    assert_eq!(mission_progress(&service, viewer, 3, 1225), 1);
    assert_eq!(mission_progress(&service, viewer, 3, 1225), 1);
    set_time(&service, "2019-11-28T06:00:00.000Z");
    assert_eq!(mission_progress(&service, viewer, 3, 1225), 2);
    service.stop().unwrap();
}
// //// /验证活动登录天数在有效期内累计并随管理重置重新记录 ////

// //// 验证主动领奖和任务页自动奖励各计入实际物品收集量 [@x380kkm 2026-09-28] ////
#[test]
fn counts_active_and_automatic_mission_rewards_once() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 928202);
    set_time(&service, "2026-09-28T06:00:00.000Z");
    prepare_active_reward(root.path(), 0);
    let response = active_claim(&service, viewer, 11020);
    assert_eq!(response["item_list"]["100000"], 150);
    assert!(response["mission_info"].as_array().unwrap().iter().any(|entry| entry["mission_id"] == 66));
    assert_eq!(mission_progress(&service, viewer, 1, 66), 150);
    active_claim(&service, viewer, 11020);
    assert_eq!(mission_progress(&service, viewer, 1, 66), 150);

    manage(&service, "POST", "/v1/mails", json!({"viewer_id":viewer,"title":"装备","body":"装备奖励.","sender":"Starpoint","rewards":{"equipmentList":{"5030037":1}}}));
    decode_response::<Value>(&send(&service, "/api/index.php/mail/receive_all", json!({"viewer_id":viewer})));
    assert_eq!(mission_progress(&service, viewer, 1, 66), 450);
    assert_eq!(read_player(root.path())["item_list"]["100000"], 450);
    assert_eq!(mission_progress(&service, viewer, 1, 66), 450);
    service.stop().unwrap();
}
// //// /验证主动领奖和任务页自动奖励各计入实际物品收集量 ////

// //// 验证任务奖励余量领取后才进入收集进度与到账历史 [@x380kkm 2026-09-28] ////
#[test]
fn counts_mission_overflow_only_when_mail_is_received() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 928203);
    set_time(&service, "2026-09-28T06:00:00.000Z");
    let capacity: Value = serde_json::from_str(include_str!("../../assets/cn-shop-capacities.json")).unwrap();
    let limit = capacity["items"]["100000"].as_i64().unwrap();
    prepare_active_reward(root.path(), limit - 50);
    let response = active_claim(&service, viewer, 11020);
    assert_eq!(response["item_list"]["100000"], limit);
    assert_eq!(response["mail_arrived"], true);
    assert_eq!(mission_progress(&service, viewer, 1, 66), 50);
    let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let mail_id: i64 = database.query_row("SELECT id FROM player_mails WHERE received_at IS NULL", [], |row| row.get(0)).unwrap();
    let received = || database.query_row::<i64, _, _>("SELECT COALESCE(SUM(number),0) FROM player_receive_history WHERE kind=1 AND type_id=100000", [], |row| row.get(0)).unwrap();
    assert_eq!(received(), 50);
    active_claim(&service, viewer, 11020);
    assert_eq!(received(), 50);
    update_player(root.path(), |player| player["item_list"]["100000"] = json!(limit - 100));
    let request = json!({"viewer_id":viewer,"mail_id":mail_id});
    decode_response::<Value>(&send(&service, "/api/index.php/mail/receive", request.clone()));
    assert_eq!(mission_progress(&service, viewer, 1, 66), 150);
    assert_eq!(received(), 150);
    let replay = decode_response::<Value>(&send(&service, "/api/index.php/mail/receive", request));
    assert_eq!(replay.data_headers.result_code, 2002);
    assert_eq!(mission_progress(&service, viewer, 1, 66), 150);
    service.stop().unwrap();
}
// //// /验证任务奖励余量领取后才进入收集进度与到账历史 ////
