// audience: internal
// # personal-service-cn-collect-currency-tests
// 隔离账号通过关卡结算获得收集币, 商店按当前活动币扣除成本并保存兑换奖励.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

// //// 发送隔离账号的游戏请求 [@x380kkm 2026-09-29] ////
fn request(service: &PersonalService, path: &str, body: Value) -> Value {
    let response = cn_support::send_request(service.port(), path, &encode_request(&body));
    decode_response::<Value>(&response).data
}

// //// 设置活动使用的虚拟时间 [@x380kkm 2026-09-29] ////
fn set_time(service: &PersonalService, iso: &str) {
    let authorization = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        "PUT",
        "/v1/time",
        "application/json",
        &[("Authorization", &authorization)],
        json!({"enabled": true, "iso": iso, "rate": 1})
            .to_string()
            .as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

// //// 读取隔离账号的持久化奖励 [@x380kkm 2026-09-29] ////
fn snapshot(database: &Connection) -> Value {
    let stored: String = database
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    serde_json::from_str(&stored).unwrap()
}

// //// 验证收集币跨活动结束结算后继续兑换 [@x380kkm 2026-09-29] ////
#[test]
fn collects_current_golden_jelly_and_exchanges_after_battle_window() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let signup = cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 782 }),
    );
    let viewer = decode_response::<SignupData>(&signup)
        .data_headers
        .viewer_id;
    let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let mut player = snapshot(&database);
    player["quest_progress"]["1"] =
        json!([{ "quest_id": 1004002, "finished": true, "clear_rank": 5 }]);
    player["item_list"]["70048"] = json!(0);
    player["item_list"]["70014"] = json!(17);
    database
        .execute(
            "UPDATE player_snapshots SET data_json=?1",
            params![player.to_string()],
        )
        .unwrap();
    set_time(&service, "2024-08-14T15:59:50.000Z");
    request(
        &service,
        "/api/index.php/single_battle_quest/start",
        json!({
            "viewer_id": viewer, "quest_id": 1005002, "category": 1, "play_id": "collect-currency", "party_id": 1,
            "use_boss_boost_point": false, "use_boost_point": false, "is_auto_start_mode": false,
        }),
    );
    set_time(&service, "2024-08-15T04:00:00.000Z");
    let finished = request(
        &service,
        "/api/index.php/single_battle_quest/finish",
        json!({
            "viewer_id": viewer, "quest_id": 1005002, "category": 1, "play_id": "collect-currency",
            "elapsed_time_ms": 100_000, "score": 5000, "add_mana": 0, "is_accomplished": true,
            "statistics": {"zones": [], "party": {"characters": [{"id": 111001}], "unison_characters": [], "equipments": [], "ability_soul_ids": []}},
        }),
    );
    assert!(finished["drop_additional_reward_ids"]
        .as_array()
        .unwrap()
        .iter()
        .any(|drop| drop["group_id"] == 300001 && drop["number"] == 100));
    let earned = snapshot(&database);
    assert_eq!(earned["item_list"]["70048"], 100);
    assert_eq!(earned["item_list"]["70014"], 17);
    let material_before = earned["item_list"]["1"].as_i64().unwrap_or_default();
    let bought = request(
        &service,
        "/api/index.php/shop/buy",
        json!({
            "viewer_id": viewer, "shop_type": 4, "shop_item_id": 10471, "number": 3,
        }),
    );
    assert_eq!(bought["item_list"]["70048"], 70);
    let stored = snapshot(&database);
    assert_eq!(stored["item_list"]["70048"], 70);
    assert_eq!(stored["item_list"]["70014"], 17);
    assert_eq!(
        stored["item_list"]["1"].as_i64().unwrap(),
        material_before + 3
    );
    service.stop().unwrap();
}
