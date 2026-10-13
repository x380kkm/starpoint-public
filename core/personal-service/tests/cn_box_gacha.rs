// audience: internal
// # personal-service-cn-box-gacha-tests
//
// 该文件验证 CN 箱池目录、活动日历开关和抽取响应契约.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;
#[path = "cn_box_gacha/retry.rs"]
mod retry;
#[path = "cn_box_gacha/exhaustion.rs"]
mod exhaustion;
#[path = "cn_box_gacha/capacity.rs"]
mod capacity;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::collections::BTreeSet;
use std::fs;
use support::request_with_headers;
use tempfile::TempDir;

#[derive(Serialize)]
struct BoxListRequest {
    viewer_id: i64,
    box_gacha_id: i64,
}

#[derive(Serialize)]
struct BoxExecRequest {
    viewer_id: i64,
    box_gacha_id: i64,
    box_id: i64,
    number: i64,
    stop_on_featured_rewards: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_count: Option<i64>,
}

#[derive(Serialize)]
struct BoxResetRequest {
    viewer_id: i64,
    box_gacha_id: i64,
    box_id: i64,
}

// //// 设置 CN 箱池测试货币 [@x380kkm 2026-08-23] ////
fn set_box_currency(root: &TempDir, amount: i64) {
    update_player_snapshot(root, |player_data| {
        player_data["item_list"]["30101"] = Value::from(amount);
    });
}
// //// /设置 CN 箱池测试货币 ////

// //// 更新隔离服务中的箱池玩家快照 [@x380kkm 2026-09-26] ////
fn update_player_snapshot(root: &TempDir, update: impl FnOnce(&mut Value)) {
    let database = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("personal service database opens");
    let (account_id, serialized): (i64, String) = database
        .query_row(
            "SELECT account_id, data_json FROM player_snapshots",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("player snapshot is read");
    let mut player_data =
        serde_json::from_str::<Value>(&serialized).expect("player snapshot is JSON");
    update(&mut player_data);
    database
        .execute(
            "UPDATE player_snapshots SET data_json = ?1 WHERE account_id = ?2",
            params![
                serde_json::to_string(&player_data).expect("player snapshot is encoded"),
                account_id,
            ],
        )
        .expect("player snapshot is updated");
}
// //// /更新隔离服务中的箱池玩家快照 ////

// //// 读取隔离服务持久化的玩家状态 [@x380kkm 2026-09-26] ////
fn read_player_snapshot(root: &TempDir) -> Value {
    let database = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("personal service database opens");
    let serialized: String = database
        .query_row("SELECT data_json FROM player_snapshots", [], |row| row.get(0))
        .expect("player snapshot is read");
    serde_json::from_str(&serialized).expect("player snapshot is JSON")
}
// //// /读取隔离服务持久化的玩家状态 ////

// //// 设置仅剩指定奖励的真实尾箱状态 [@x380kkm 2026-09-26] ////
fn set_tail_box(root: &TempDir, reward_id: &str, currency: i64) {
    set_remaining_rewards(root, 1, 1, &[reward_id]);
    update_player_snapshot(root, |player_data| {
        player_data["item_list"]["30101"] = json!(currency);
        player_data["item_list"]["100000"] = json!(99);
        player_data["mission_window_progress"] = json!({"1:66":99});
    });
}
// //// /设置仅剩指定奖励的真实尾箱状态 ////

// //// 按实际目录保留指定箱池奖励 [@x380kkm 2026-09-26] ////
fn set_remaining_rewards(root: &TempDir, pool: i64, box_id: i64, remaining: &[&str]) {
    let catalog: Value = serde_json::from_str(include_str!("../../../assets/box_reward.json"))
        .expect("box reward catalog is JSON");
    let drawn = catalog[pool.to_string()][box_id.to_string()]
        .as_object().expect("box rewards are an object")
        .iter()
        .map(|(id, reward)| {
            let available = reward["available"].as_i64().expect("reward count exists");
            (id.clone(), json!(available - i64::from(remaining.contains(&id.as_str()))))
        })
        .collect::<serde_json::Map<_, _>>();
    update_player_snapshot(root, |player_data| {
        player_data["box_gacha_list"] = json!({pool.to_string():[{
            "box_id":box_id, "reset_times":0, "remaining_number":remaining.len(), "is_closed":false
        }]});
        player_data["box_gacha_drawn_rewards"] = json!({pool.to_string():{box_id.to_string():drawn}});
    });
}
// //// /按实际目录保留指定箱池奖励 ////

// //// 验证尾箱按实抽扣费并同步道具任务收益 [@x380kkm 2026-09-26] ////
#[test]
fn settles_actual_draw_cost_and_collection_mission_together() {
    for (reward_id, stop_on_featured_rewards, currency) in [
        ("1000101001", true, 100),
        ("1000101011", false, 100),
        ("1000101011", false, 10),
        ("1000101011", false, 9),
    ] {
        let root = TempDir::new().expect("temporary service directory is created");
        let cdn_root = TempDir::new().expect("CN asset root is created");
        let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
            .expect("service starts");
        let signup = decode_response::<SignupData>(&cn_support::send_request(
            service.port(), "/api/index.php/tool/signup",
            &encode_request(&SignupRequest { device_id: 71 }),
        ));
        let viewer_id = signup.data_headers.viewer_id;
        set_tail_box(&root, reward_id, currency);
        let before = read_player_snapshot(&root);
        let request = encode_request(&BoxExecRequest {
            viewer_id, box_gacha_id: 1, box_id: 1, number: 10, stop_on_featured_rewards,
            api_count: Some(431),
        });
        let response = cn_support::send_request(
            service.port(), "/api/index.php/box_gacha/exec", &request,
        );
        let saved = read_player_snapshot(&root);
        if currency < 10 {
            assert!(response.ends_with("{\"error\":\"not_enough_box_currency\"}"));
            assert_eq!(saved, before);
        } else {
            let draw = decode_response::<Value>(&response);
            assert_eq!(draw.data["drawn_reward_list"], json!([{
                "reward_id":reward_id.parse::<i64>().unwrap(), "number":1
            }]));
            assert_eq!(saved["item_list"]["30101"], currency - 10);
            assert_eq!(draw.data["item_list"]["30101"], currency - 10);
            assert_eq!(saved["box_gacha_list"]["1"][0]["remaining_number"], 0);
            assert_eq!(saved["box_gacha_list"]["1"][0]["is_closed"], true);
            assert_eq!(draw.data["all_box_info"].as_array().unwrap().last().unwrap()["is_closed"], true);
            if reward_id == "1000101011" {
                assert_eq!(saved["item_list"]["100000"], 100);
                assert_eq!(saved["mission_window_progress"]["1:66"], 100);
                assert_eq!(saved["user_info"]["free_vmoney"].as_i64().unwrap(),
                    before["user_info"]["free_vmoney"].as_i64().unwrap() + 5);
                assert_eq!(draw.data["user_info"]["free_vmoney"], saved["user_info"]["free_vmoney"]);
                assert!(draw.data["mission_info"].as_array().unwrap().iter().any(|mission|
                    mission["mission_category_id"] == 1 && mission["mission_id"] == 66));
            } else {
                assert!(saved["user_equipment_list"].get("5100001").is_some());
                assert_eq!(saved["mission_window_progress"]["1:66"], 99);
            }
            let repeated = cn_support::send_request(
                service.port(), "/api/index.php/box_gacha/exec", &request,
            );
            assert_eq!(repeated, response);
            assert_eq!(read_player_snapshot(&root), saved);
        }
        service.stop().expect("service stops cleanly");
    }
}
// //// /验证尾箱按实抽扣费并同步道具任务收益 ////

// //// 验证全部特奖入库后按实际抽数停止 [@x380kkm 2026-09-27] ////
#[test]
fn stops_after_drawing_all_featured_rewards() {
    let root = TempDir::new().expect("temporary service directory is created");
    let cdn_root = TempDir::new().expect("CN asset root is created");
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
        .expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(), "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 71 }),
    ));
    set_remaining_rewards(&root, 5, 3, &["1000503001", "1000503002"]);
    update_player_snapshot(&root, |player_data| {
        player_data["item_list"]["30105"] = json!(20);
    });
    let draw = decode_response::<Value>(&cn_support::send_request(
        service.port(), "/api/index.php/box_gacha/exec",
        &encode_request(&BoxExecRequest {
            viewer_id: signup.data_headers.viewer_id,
            box_gacha_id: 5, box_id: 3, number: 10, stop_on_featured_rewards: true,
            api_count: None,
        }),
    ));
    let saved = read_player_snapshot(&root);
    assert_eq!(draw.data["drawn_reward_list"].as_array().unwrap().len(), 2);
    assert_eq!(draw.data["drawn_reward_list"][0]["number"], 1);
    assert_eq!(draw.data["drawn_reward_list"][1]["number"], 1);
    assert_eq!(draw.data["item_list"]["30105"], 0);
    assert_eq!(saved["item_list"]["30105"], 0);
    assert_eq!(saved["box_gacha_list"]["5"][0]["remaining_number"], 0);
    assert_eq!(saved["box_gacha_list"]["5"][0]["is_closed"], true);
    service.stop().expect("service stops cleanly");
}
// //// /验证全部特奖入库后按实际抽数停止 ////

// //// 验证 CN 箱池目录和禁用状态 [@x380kkm 2026-08-22] ////
#[test]
fn returns_box_list_and_honors_disabled_activity() {
    let root = TempDir::new().expect("temporary service directory is created");
    let cdn_root = TempDir::new().expect("CN asset root is created");
    fs::write(
        cdn_root.path().join("activity-catalog.json"),
        r#"{
            "format_version": 1,
            "region": "cn",
            "client_version": "1.8.4",
            "asset_version": "1.4.54",
            "generated_at": "2030-01-01T00:00:00Z",
            "activities": [{
                "activity_id": "box-gacha:1",
                "name": "Box Gacha 1",
                "kind": "box-gacha",
                "tags": ["test"],
                "description": "box gacha activity",
                "default_start_at_ms": 1893456000000,
                "default_end_at_ms": 1893715200000
            }]
        }"#,
    )
    .expect("activity manifest is written");
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
        .expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 71 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let time = request_with_headers(
        service.port(),
        "PUT",
        "/v1/time",
        "application/json",
        &authorization,
        br#"{"enabled":true,"iso":"2030-01-02T12:00:00.000Z","rate":1.0}"#,
    );
    assert!(time.starts_with("HTTP/1.1 200 OK"));
    let request = BoxListRequest {
        viewer_id,
        box_gacha_id: 1,
    };
    let open = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/box_gacha/get_box_list",
        &encode_request(&request),
    ));
    assert!(!open.data["all_box_info"]
        .as_array()
        .expect("box list is an array")
        .is_empty());

    set_box_currency(&root, 100);
    let draw = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/box_gacha/exec",
        &encode_request(&BoxExecRequest {
            viewer_id,
            box_gacha_id: 1,
            box_id: 1,
            number: 1,
            stop_on_featured_rewards: false,
            api_count: None,
        }),
    ));
    let draw_keys = draw
        .data
        .as_object()
        .expect("box draw response data is an object")
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let required_keys = [
            "all_box_info",
            "character_list",
            "drawn_reward_list",
            "equipment_list",
            "item_list",
            "joined_character_id_list",
            "mail_arrived",
            "user_info",
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
    assert!(required_keys.is_subset(&draw_keys));
    if let Some(over_max) = draw.data.get("over_max") {
        assert!(over_max.as_array().expect("overflow notices are an array").is_empty());
    }
    assert_eq!(draw.data["item_list"]["30101"], 90);

    let reset = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/box_gacha/reset",
        &encode_request(&BoxResetRequest {
            viewer_id,
            box_gacha_id: 1,
            box_id: 1,
        }),
    ));
    assert_eq!(reset.data["all_box_info"][0]["reset_times"], 1);
    assert!(reset.data["all_box_info"][0]["all_drawn_reward_list"]
        .as_array()
        .expect("drawn reward list is an array")
        .is_empty());

    let another_pool = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/box_gacha/get_box_list",
        &encode_request(&BoxListRequest {
            viewer_id,
            box_gacha_id: 2,
        }),
    ));
    assert!(!another_pool.data["all_box_info"]
        .as_array()
        .expect("master box list is an array")
        .is_empty());

    let disabled = request_with_headers(
        service.port(),
        "PUT",
        "/v1/activities/calendar/box-gacha:1",
        "application/json",
        &authorization,
        br#"{"enabled":false,"start_at_ms":1893456000000,"end_at_ms":1893715200000}"#,
    );
    assert!(disabled.starts_with("HTTP/1.1 200 OK"));
    let blocked = cn_support::send_request(
        service.port(),
        "/api/index.php/box_gacha/get_box_list",
        &encode_request(&request),
    );
    assert!(blocked.ends_with("{\"error\":\"activity_disabled\"}"));
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 箱池目录和禁用状态 ////
