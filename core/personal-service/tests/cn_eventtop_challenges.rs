// audience: internal
// # personal-service-cn-eventtop-challenges
// 此文件通过真实 load 响应检查 EventTop 在日期筛选前访问的挑战活动引用.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[path = "support/local_saves.rs"]
#[allow(dead_code)]
mod local_save_support;
#[allow(dead_code)]
mod support;

use local_save_support::{
    activate_local_save, assert_status, authorized_request, export_local_save, list_local_saves,
    load, response_body, signup,
};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::collections::{BTreeMap, BTreeSet};
use tempfile::TempDir;

// //// 执行客户端活动角标的强制 master 引用 [@x380kkm 2026-09-27] ////
fn assert_eventtop_challenges(player: &Value) {
    let campaigns: BTreeMap<i64, i64> =
        serde_json::from_str(include_str!("../assets/cn-daily-challenge-campaigns.json"))
            .expect("campaign master identities are decoded");
    let points = player["user_daily_challenge_point_list"]
        .as_array()
        .expect("daily challenge points are a client array");
    let mut point_ids = BTreeSet::new();
    for point in points {
        let point_id = point["id"].as_i64().expect("challenge id is an integer");
        assert!(
            point_ids.insert(point_id),
            "client C2830: duplicate challenge {point_id}"
        );
        let mut campaign_ids = BTreeSet::new();
        for campaign in point["campaign_list"]
            .as_array()
            .expect("campaign list is a client array")
        {
            let campaign_id = campaign["campaign_id"]
                .as_i64()
                .expect("campaign id is an integer");
            assert_eq!(
                campaigns.get(&campaign_id),
                Some(&point_id),
                "EventTop -> getTotalPoint -> getCampaigns -> master.get({campaign_id})"
            );
            assert!(
                campaign_ids.insert(campaign_id),
                "client C2830: duplicate campaign {campaign_id}"
            );
        }
    }
}

// //// 取出导入存档原有的挑战点条目 [@x380kkm 2026-10-04] ////
fn imported_points(player: &Value) -> Value {
    let imported = [1, 251, 5001, 10008];
    Value::Array(
        player["user_daily_challenge_point_list"]
            .as_array()
            .expect("daily challenge points are a client array")
            .iter()
            .filter(|entry| imported.contains(&entry["id"].as_i64().unwrap_or_default()))
            .cloned()
            .collect(),
    )
}

// //// 验证 7 月活动入口读取新玩家每日挑战活动 [@x380kkm 2026-09-27] ////
#[test]
fn july_eventtop_resolves_challenge_campaigns_after_load() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let response = authorized_request(
        &service,
        "PUT",
        "/v1/time",
        Some(&json!({"enabled":true, "iso":"2024-07-12T04:00:00.000Z", "rate":1.0})),
    );
    assert_status(&response, "200 OK");
    let account = signup(service.port(), 7412);
    let player = load(service.port(), account.data_headers.viewer_id, "1.4.75").data;
    println!(
        "EventTop challenge input: {}",
        player["user_daily_challenge_point_list"]
    );
    assert_eventtop_challenges(&player);
    for id in 401..=418 {
        let entry = player["user_daily_challenge_point_list"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["id"] == id)
            .unwrap_or_else(|| panic!("anniversary challenge {id} is missing"));
        assert_eq!(entry["point"], 1, "anniversary challenge {id}");
    }
    service.stop().expect("service stops");
}

// //// 验证导入挑战数据在重启和跨日后保持合法点数 [@x380kkm 2026-09-27] ////
#[test]
fn repairs_imported_challenge_references_and_preserves_remaining_points() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let device_id = 7413;
    assert_status(
        &authorized_request(
            &service,
            "PUT",
            "/v1/time",
            Some(&json!({"enabled":true, "iso":"2024-07-12T04:00:00.000Z", "rate":1.0})),
        ),
        "200 OK",
    );
    let account = signup(service.port(), device_id);
    load(service.port(), account.data_headers.viewer_id, "1.4.63");
    let slot_id = list_local_saves(&service)["slots"][0]["id"]
        .as_i64()
        .expect("slot id exists");
    let mut imported = export_local_save(&service, slot_id)["data"].clone();
    imported["user_daily_challenge_point_list"] = json!([
        {"id":1, "point":7, "campaign_list":[
            {"campaign_id":2022112801_i64, "additional_point":1},
            {"campaign_id":2022112801_i64, "additional_point":2},
            {"campaign_id":999999999_i64, "additional_point":80}
        ]},
        {"id":1, "point":5, "campaign_list":[{"campaign_id":2022112801_i64, "additional_point":1}]},
        {"id":251, "point":19, "campaign_list":[
            {"campaign_id":2023013102_i64, "additional_point":2},
            {"campaign_id":2022112801_i64, "additional_point":99}
        ]},
        {"id":5001, "point":10, "campaign_list":null},
        {"id":10008, "point":1, "campaign_list":[]}
    ]);
    let response = authorized_request(
        &service,
        "POST",
        "/v1/local-saves/import",
        Some(&json!({
            "format":"starpoint-local-save", "version":1, "name":"挑战次数恢复", "data":imported
        })),
    );
    assert_status(&response, "201 Created");
    let imported_slot_id = response_body(&response)["id"]
        .as_i64()
        .expect("imported slot exists");
    activate_local_save(&service, imported_slot_id, device_id);
    let viewer_id = signup(service.port(), device_id).data_headers.viewer_id;
    let player = load(service.port(), viewer_id, "1.4.63").data;
    assert_eventtop_challenges(&player);
    let expected = json!([
        {"id":1, "point":7, "campaign_list":[{"campaign_id":2022112801_i64, "additional_point":2}]},
        {"id":251, "point":19, "campaign_list":[]},
        {"id":5001, "point":10, "campaign_list":[]},
        {"id":10008, "point":1, "campaign_list":[]}
    ]);
    assert_eq!(imported_points(&player), expected);
    assert_eq!(
        imported_points(&export_local_save(&service, imported_slot_id)["data"]),
        expected
    );
    service.stop().expect("service stops");

    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let player = load(service.port(), viewer_id, "1.4.75").data;
    assert_eventtop_challenges(&player);
    assert_eq!(imported_points(&player), expected);
    assert_status(
        &authorized_request(
            &service,
            "PUT",
            "/v1/time",
            Some(&json!({"enabled":true, "iso":"2024-07-13T04:00:00.000Z", "rate":1.0})),
        ),
        "200 OK",
    );
    let next_day = load(service.port(), viewer_id, "1.4.75").data;
    assert_eventtop_challenges(&next_day);
    let restored_daily_point = next_day["user_daily_challenge_point_list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == 251)
        .expect("trial challenge exists");
    assert_eq!(restored_daily_point["point"], 2);
    service.stop().expect("service stops");
}
