// audience: internal
// # personal-service-regional-activity-api-tests
// 隔离存档按区域期别验证剧情赠角, 商店兑换和跨期任务结算.

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

fn send(service: &PersonalService, path: &str, body: Value) -> Value {
    let response = cn_support::send_request(service.port(), path, &encode_request(&body));
    let decoded = decode_response::<Value>(&response);
    assert_eq!(decoded.data_headers.result_code, 1, "{response}");
    decoded.data
}

fn set_time(service: &PersonalService, iso: &str) {
    let token = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        "PUT",
        "/v1/time",
        "application/json",
        &[("Authorization", token.as_str())],
        json!({"enabled":true,"iso":iso,"rate":1})
            .to_string()
            .as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

fn sales(service: &PersonalService, viewer: i64) -> Vec<Value> {
    event_sales(service, viewer, 100003)
}

fn event_sales(service: &PersonalService, viewer: i64, event_id: i64) -> Vec<Value> {
    send(
        service,
        "/api/index.php/shop/get_sales_list",
        json!({
            "viewer_id":viewer,"shop_types":[4],"boss_coin_shop_category_ids":[],
            "equipment_enhancement_shop_category_ids":[],"browse_treasure_flag":false,
            "event_list":[{"event_type":0,"event_ids":[event_id]}]
        }),
    )["sales_list"]
        .as_array()
        .unwrap()
        .clone()
}

// //// 在原始复刻日期结算赠角与批量兑换并保留国服库存 [@x380kkm 2026-09-27] ////
#[test]
fn saga_story_and_shop_work_during_original_period() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let signup = cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest {
            device_id: 927100003,
        }),
    );
    let viewer = decode_response::<SignupData>(&signup)
        .data_headers
        .viewer_id;
    set_time(&service, "2021-04-24T03:59:00Z");
    assert!(sales(&service, viewer).is_empty());
    set_time(&service, "2021-04-25T04:00:00Z");
    assert_eq!(sales(&service, viewer).len(), 57);

    let story_request = json!({"viewer_id":viewer,"party_id":1,"category":7,"quest_id":100003001});
    let story = send(
        &service,
        "/api/index.php/story_quest/finish_with_skip",
        story_request.clone(),
    );
    assert_eq!(story["joined_character_id_list"], json!([263003]));
    let character = story["character_list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|character| character["character_id"] == 263003)
        .unwrap();
    assert_eq!(character["entry_count"], 1);
    send(&service, "/api/index.php/story_quest/finish", story_request);

    let connection = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let stored: String = connection
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    let mut player: Value = serde_json::from_str(&stored).unwrap();
    assert_eq!(player["user_character_list"]["263003"]["entry_count"], 1);
    let before = player["item_list"]["102"].as_i64().unwrap_or_default();
    player["item_list"]["50210"] = json!(100);
    connection
        .execute(
            "UPDATE player_snapshots SET data_json=?1",
            params![player.to_string()],
        )
        .unwrap();
    let purchase = json!({"viewer_id":viewer,"shop_type":4,"shop_item_id":330149,"number":3});
    let bought = send(&service, "/api/index.php/shop/buy", purchase);
    assert_eq!(bought["item_list"]["50210"], 70);
    assert_eq!(bought["item_list"]["102"], before + 3);

    set_time(&service, "2021-05-01T04:00:00Z");
    assert_eq!(sales(&service, viewer).len(), 57);
    set_time(&service, "2021-05-10T04:00:00Z");
    assert!(sales(&service, viewer).is_empty());
    set_time(&service, "2023-03-25T04:00:00Z");
    assert_eq!(sales(&service, viewer).len(), 57);
    let restored: String = connection
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    let restored: Value = serde_json::from_str(&restored).unwrap();
    assert_eq!(restored["user_character_list"]["263003"]["entry_count"], 1);
    assert_eq!(restored["item_list"]["102"], before + 3);
    service.stop().unwrap();
}
// //// /在原始复刻日期结算赠角与批量兑换并保留国服库存 ////

// //// 团队战跨活动结束结算时沿用入场任务阶段并发奖 [@x380kkm 2026-09-28] ////
#[test]
fn regional_raid_battle_keeps_mission_eligibility_at_entry() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let signup = cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest {
            device_id: 928400001,
        }),
    );
    let viewer = decode_response::<SignupData>(&signup)
        .data_headers
        .viewer_id;
    set_time(&service, "2021-12-16T12:59:00Z");
    let loaded = send(
        &service,
        "/api/index.php/load",
        json!({"viewer_id":viewer,"keychain":viewer}),
    );
    let before = loaded["item_list"]["999003"].as_i64().unwrap_or_default();
    send(
        &service,
        "/api/index.php/single_battle_quest/start",
        json!({
            "viewer_id":viewer,"api_count":1,"category":23,"quest_id":1001,
            "party_id":1,"play_id":"regional-raid-mission","is_auto_start_mode":false,
            "use_boss_boost_point":false,"use_boost_point":false
        }),
    );
    set_time(&service, "2021-12-16T13:01:00Z");
    let finish = json!({
        "viewer_id":viewer,"api_count":1,"category":23,"quest_id":1001,
        "play_id":"regional-raid-mission","is_restored":false,"continue_count":0,
        "elapsed_time_ms":60000,"score":1000,"add_mana":0,"is_accomplished":true,
        "statistics":{"clear_phase":1,"zones":[],"party":{
            "characters":[{"id":1},null,null],"unison_characters":[null,null,null],
            "equipments":[null,null,null],"ability_soul_ids":[null,null,null]
        }}
    });
    let finished = send(
        &service,
        "/api/index.php/single_battle_quest/finish",
        finish.clone(),
    );
    assert!(finished["mission_info"]
        .as_array()
        .unwrap()
        .iter()
        .any(|mission| mission["mission_id"] == 400001));
    let reloaded = send(
        &service,
        "/api/index.php/load",
        json!({"viewer_id":viewer,"keychain":viewer}),
    );
    assert_eq!(reloaded["item_list"]["999003"], before + 1);
    send(
        &service,
        "/api/index.php/single_battle_quest/finish",
        finish,
    );
    let replayed = send(
        &service,
        "/api/index.php/load",
        json!({"viewer_id":viewer,"keychain":viewer}),
    );
    assert_eq!(replayed["item_list"]["999003"], before + 1);
    service.stop().unwrap();
}
// //// /团队战跨活动结束结算时沿用入场任务阶段并发奖 ////

// //// 区域商店使用 CN 代币和奖励结算改号商品及扩充商品 [@x380kkm 2026-09-28] ////
#[test]
fn regional_shop_variants_deliver_cn_rewards_at_cn_costs() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let signup = cn_support::send_request(service.port(), "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 9285389 }));
    let viewer = decode_response::<SignupData>(&signup).data_headers.viewer_id;
    let connection = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    for (event_id, at, item_id, currency, cost, reward, amount, number) in [
        (200004, "2022-08-24T04:00:00Z", 10000159, 40302, 500, 999001, 1, 1),
        (100002, "2021-01-17T04:00:00Z", 340056, 50300, 80, 54, 4, 3),
        (19, "2022-03-01T04:00:00Z", 1215819, 49303, 50, 54, 4, 3),
    ] {
        set_time(&service, at);
        assert!(event_sales(&service, viewer, event_id).iter().any(|item| item["shop_item_id"] == item_id));
        let stored: String = connection.query_row("SELECT data_json FROM player_snapshots", [], |row| row.get(0)).unwrap();
        let mut player: Value = serde_json::from_str(&stored).unwrap();
        let before = player["item_list"][reward.to_string()].as_i64().unwrap_or_default();
        player["item_list"][currency.to_string()] = json!(10_000);
        connection.execute("UPDATE player_snapshots SET data_json=?1", params![player.to_string()]).unwrap();
        let bought = send(&service, "/api/index.php/shop/buy", json!({
            "viewer_id":viewer,"shop_type":4,"shop_item_id":item_id,"number":number,
        }));
        assert_eq!(bought["item_list"][currency.to_string()], 10_000 - cost * number);
        assert_eq!(bought["item_list"][reward.to_string()], before + amount * number);
    }
    service.stop().unwrap();
}
// //// /区域商店使用 CN 代币和奖励结算改号商品及扩充商品 ////
