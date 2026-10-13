// audience: internal
// # personal-service-cn-helper-tests
// 小助手汇总与玩法入口读取同一玩家状态, 浏览操作保留资产与奖励记录.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

#[path = "cn_helper/carnival_records.rs"]
mod carnival_records;

use cn_support::{decode_response, encode_request, send_request};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::collections::BTreeSet;
use std::path::Path;
use support::request_with_headers;
use tempfile::TempDir;

// //// 准备具有已存成绩和可消费资源的独立账号 [@x380kkm 2026-09-23] ////
fn setup(root: &Path) -> (PersonalService, i64) {
    let service = PersonalService::start(root, 0).unwrap();
    admin(
        &service,
        "/v1/time",
        json!({"enabled":true,"iso":"2023-09-22T12:00:00.000Z","rate":1.0}),
    );
    let signup = decode_response::<Value>(&send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&json!({"device_id": 887})),
    ));
    let viewer = signup.data_headers.viewer_id;
    for activity in ["box-gacha:1", "carnival:5001", "advent:1"] {
        set_activity(&service, activity, true);
    }
    let mut player = snapshot(root);
    player["user_info"]["free_mana"] = json!(5000);
    player["item_list"]["30101"] = json!(30);
    player["user_notice_list"] = json!([{"kind":1,"status":1},{"kind":2,"status":1}]);
    player["cn_activity_state"] = json!({"event_families":{"carnival":{"5001":{
        "carnival_records":[{
            "folder_id":1,"best_score":12345,"previous_score":12000,
            "previous_character_ids":[111001,null,null],"previous_unison_character_ids":[null,null,null]
        }]
    }}}});
    save_snapshot(root, &player);
    (service, viewer)
}

fn admin(service: &PersonalService, path: &str, body: Value) {
    let bearer = format!("Bearer {}", service.management_token());
    let response = request_with_headers(
        service.port(),
        "PUT",
        path,
        "application/json",
        &[("Authorization", &bearer)],
        body.to_string().as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
}

fn set_activity(service: &PersonalService, id: &str, enabled: bool) {
    admin(
        service,
        &format!("/v1/activities/calendar/{}", id.replace(':', "%3A")),
        json!({"enabled":enabled,"start_at_ms":0,"end_at_ms":253402300799000_i64}),
    );
}

fn snapshot(root: &Path) -> Value {
    let database = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    let data: String = database
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    serde_json::from_str(&data).unwrap()
}

fn save_snapshot(root: &Path, player: &Value) {
    Connection::open(root.join("personal-service.sqlite3")).unwrap()
        .execute("UPDATE player_snapshots SET data_json = ?1", params![player.to_string()])
        .unwrap();
}

fn api(service: &PersonalService, path: &str, body: Value) -> Value {
    decode_response::<Value>(&send_request(
        service.port(),
        &format!("/api/index.php{path}"),
        &encode_request(&body),
    ))
    .data
}

fn request(viewer: i64) -> Value {
    json!({
        "viewer_id":viewer,
        "shop_types":[2,2,4,7],
        "event_list":[{"event_type":0,"event_ids":[1,1]}],
        "boss_coin_shop_category_ids":[9,9],
        "browse_treasure_flag":false,
        "box_gacha_ids":[1],
        "category_list":[{"category":1},{"category":2}],
        "carnival_event_id":5001
    })
}

fn mission(data: &Value, category: i64, id: i64) -> &Value {
    data["mission_progress_list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["mission_category"] == category && entry["mission_id"] == id)
        .unwrap()
}
// //// /准备具有已存成绩和可消费资源的独立账号 ////

// //// 实际购买和抽箱后汇总保持与原功能入口一致 [@x380kkm 2026-09-23] ////
#[test]
fn aggregates_real_purchases_drawn_rewards_mission_progress_and_scores() {
    let root = TempDir::new().unwrap();
    let (service, viewer) = setup(root.path());
    api(
        &service,
        "/shop/buy",
        json!({"viewer_id":viewer,"api_count":1,"shop_type":2,"shop_item_id":200031,"number":1}),
    );
    let drawn = api(
        &service,
        "/box_gacha/exec",
        json!({"viewer_id":viewer,"box_gacha_id":1,"box_id":1,"number":3,"stop_on_featured_rewards":false}),
    );
    let before = snapshot(root.path());
    let helper = api(&service, "/assistant/get_assistant_list", request(viewer));
    let sales = helper["sales_list"].as_array().unwrap();
    let identities = sales
        .iter()
        .map(|sale| {
            (
                sale["shop_type"].as_i64().unwrap(),
                sale["shop_item_id"].as_i64().unwrap(),
            )
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(identities.len(), sales.len());
    assert_eq!(snapshot(root.path()), before);
    assert_eq!(mission(&helper, 1, 41)["progress_value"], 200);
    assert!(helper["sales_list"]
        .as_array()
        .unwrap()
        .iter()
        .any(|sale| sale["shop_item_id"] == 200031 && sale["today_purchase_num"] == 1));
    let box_list = api(
        &service,
        "/box_gacha/get_box_list",
        json!({"viewer_id":viewer,"box_gacha_id":1}),
    );
    assert_eq!(helper["all_box_info"], box_list["all_box_info"]);
    let total_drawn: i64 = helper["all_box_info"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|entry| entry["all_drawn_reward_list"].as_array().unwrap())
        .map(|reward| reward["number"].as_i64().unwrap())
        .sum();
    assert_eq!(total_drawn, 3);
    assert_eq!(
        drawn["drawn_reward_list"]
            .as_array()
            .unwrap()
            .iter()
            .map(|reward| reward["number"].as_i64().unwrap())
            .sum::<i64>(),
        3
    );
    let sales = api(&service, "/shop/get_sales_list", request(viewer));
    assert_eq!(helper["sales_list"], sales["sales_list"]);
    let progress = api(
        &service,
        "/mission/get_mission_progress",
        json!({
            "viewer_id":viewer,"api_count":2,"category_list":[{"category":1},{"category":2}]
        }),
    );
    assert_eq!(mission(&helper, 1, 41), mission(&progress, 1, 41));
    let carnival = api(
        &service,
        "/carnival_event/index",
        json!({"viewer_id":viewer,"event_id":5001}),
    );
    assert_eq!(helper["records"], carnival["records"]);
    assert_eq!(helper["records"][0]["best_score"], 12345);
    assert_eq!(helper["user_notice_list"], before["user_notice_list"]);
    service.stop().unwrap();
}
// //// /实际购买和抽箱后汇总保持与原功能入口一致 ////

// //// 活动关闭与请求筛选只影响对应汇总集合 [@x380kkm 2026-09-23] ////
#[test]
fn filters_closed_activities_unknown_boxes_and_unrequested_domains() {
    let root = TempDir::new().unwrap();
    let (service, viewer) = setup(root.path());
    for activity in ["box-gacha:1", "carnival:5001", "advent:1"] {
        set_activity(&service, activity, false);
    }
    let mut body = request(viewer);
    body["box_gacha_ids"] = json!([1, 999999, 1]);
    let before = snapshot(root.path());
    let helper = api(&service, "/assistant/get_assistant_list", body);
    assert_eq!(helper["all_box_info"], json!([]));
    assert_eq!(helper["records"], json!([]));
    assert!(!helper["sales_list"]
        .as_array()
        .unwrap()
        .iter()
        .any(|sale| sale["shop_type"] == 4));
    assert!(helper["sales_list"]
        .as_array()
        .unwrap()
        .iter()
        .any(|sale| sale["shop_type"] == 2));
    assert_eq!(snapshot(root.path()), before);
    let empty = api(
        &service,
        "/assistant/get_assistant_list",
        json!({"viewer_id":viewer}),
    );
    for field in [
        "all_box_info",
        "records",
        "mission_progress_list",
        "sales_list",
    ] {
        assert_eq!(empty[field], json!([]));
    }
    assert_eq!(empty["user_notice_list"], before["user_notice_list"]);
    service.stop().unwrap();
}
// //// /活动关闭与请求筛选只影响对应汇总集合 ////

// //// 显式浏览仅确认指定通知且无效请求保留存档 [@x380kkm 2026-09-23] ////
#[test]
fn acknowledges_treasure_notice_without_changing_other_state() {
    let root = TempDir::new().unwrap();
    let (service, viewer) = setup(root.path());
    let before = snapshot(root.path());
    let invalid = send_request(
        service.port(),
        "/api/index.php/assistant/get_assistant_list",
        &encode_request(&json!({"viewer_id":viewer,"category_list":[{"category":4}]})),
    );
    assert!(invalid.starts_with("HTTP/1.1 400"), "{invalid}");
    assert_eq!(snapshot(root.path()), before);
    let response = api(
        &service,
        "/assistant/get_assistant_list",
        json!({"viewer_id":viewer,"browse_treasure_flag":true}),
    );
    assert_eq!(
        response["user_notice_list"],
        json!([{"kind":1,"status":0},{"kind":2,"status":1}])
    );
    let mut expected = before;
    expected["user_notice_list"] = response["user_notice_list"].clone();
    assert_eq!(snapshot(root.path()), expected);
    let repeated = api(
        &service,
        "/assistant/get_assistant_list",
        json!({"viewer_id":viewer,"browse_treasure_flag":true}),
    );
    assert_eq!(repeated["user_notice_list"], response["user_notice_list"]);
    assert_eq!(snapshot(root.path()), expected);
    service.stop().unwrap();
}
// //// /显式浏览仅确认指定通知且无效请求保留存档 ////
