// audience: internal
// # personal-service-cn-ex-boost-tests
// EX 抽取的素材, 暂存结果和选择凭据随存档恢复, 请求重放返回原结算.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, LoadRequest, SignupData, SignupRequest};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

fn setup() -> (TempDir, PersonalService, i64) {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 72 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let database =
        rusqlite::Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let mut player = snapshot(&root);
    player["user_character_list"]["1"]["over_limit_step"] = json!(6);
    player["user_character_list"]["1"]["ex_boost"] = Value::Null;
    player["user_character_list"]["1"]["join_time"] = json!(1_700_000_000);
    player["item_list"]["10001"] = json!(30);
    player["item_list"]["10003"] = json!(2);
    database
        .execute(
            "UPDATE player_snapshots SET data_json=?1",
            [player.to_string()],
        )
        .unwrap();
    (root, service, viewer_id)
}

fn snapshot(root: &TempDir) -> Value {
    let database =
        rusqlite::Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let stored: String = database
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    serde_json::from_str(&stored).unwrap()
}

fn draw_request(viewer_id: i64, cost_item_id: i64, api_count: i64) -> Value {
    json!({"viewer_id":viewer_id,"character_id":1,"cost_item_id":cost_item_id,"api_count":api_count})
}

fn request(service: &PersonalService, route: &str, body: &Value) -> Value {
    decode_response::<Value>(&cn_support::send_request(
        service.port(),
        route,
        &encode_request(body),
    ))
    .data
}

fn load(service: &PersonalService, viewer_id: i64) -> Value {
    decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ))
    .data
}

// //// 验证首次抽取自动接受并在重试时保留素材与结果 [@x380kkm 2026-09-23] ////
#[test]
fn first_draw_accepts_once_and_preserves_character_dates() {
    let (root, service, viewer_id) = setup();
    let before = snapshot(&root);
    let invalid = cn_support::send_request(
        service.port(),
        "/api/index.php/ex_boost/draw",
        &encode_request(&draw_request(viewer_id, 10003, 7)),
    );
    assert!(invalid.starts_with("HTTP/1.1 400 Bad Request"));
    assert_eq!(snapshot(&root), before);

    let body = draw_request(viewer_id, 10003, 8);
    let drawn = request(&service, "/api/index.php/ex_boost/first_draw", &body);
    assert_eq!(drawn["item_list"]["10003"], 1);
    assert_eq!(
        drawn["character_list"][0]["ex_boost"]["ability_id_list"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        drawn["character_list"][0]["create_time"],
        "2023-11-14 22:13:20"
    );
    assert_eq!(
        drawn["character_list"][0]["join_time"],
        "2023-11-14 22:13:20"
    );
    assert!(drawn["character_list"][0]["update_time"].is_string());
    let committed = snapshot(&root);
    assert_eq!(
        committed["user_character_list"]["1"]["join_time"],
        1_700_000_000
    );
    assert!(committed["user_character_list"]["1"]["update_time"].is_i64());
    assert_eq!(
        request(&service, "/api/index.php/ex_boost/first_draw", &body),
        drawn
    );
    assert_eq!(snapshot(&root), committed);
    assert!(committed.get("pending_ex_boost").is_none());
    assert_eq!(
        committed["user_character_list"]["1"]["ex_boost"],
        drawn["character_list"][0]["ex_boost"]
    );

    let repeated_first = cn_support::send_request(
        service.port(),
        "/api/index.php/ex_boost/first_draw",
        &encode_request(&draw_request(viewer_id, 10003, 9)),
    );
    assert!(repeated_first.starts_with("HTTP/1.1 400 Bad Request"));
    assert_eq!(snapshot(&root), committed);
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let loaded = load(&service, viewer_id);
    assert_eq!(
        loaded["user_character_list"]["1"],
        committed["user_character_list"]["1"]
    );
    service.stop().unwrap();
}
// //// /验证首次抽取自动接受并在重试时保留素材与结果 ////

// //// 验证中断后恢复暂存选择以及接受和丢弃的请求重放 [@x380kkm 2026-09-23] ////
#[test]
fn restores_pending_draw_and_replays_accept_and_discard() {
    let (root, service, viewer_id) = setup();
    request(
        &service,
        "/api/index.php/ex_boost/first_draw",
        &draw_request(viewer_id, 10003, 20),
    );
    let original = snapshot(&root)["user_character_list"]["1"]["ex_boost"].clone();
    let body = draw_request(viewer_id, 10001, 21);
    let pending = request(&service, "/api/index.php/ex_boost/draw", &body);
    assert_eq!(pending["item_list"]["10001"], 25);
    let committed = snapshot(&root);
    assert_eq!(committed["user_character_list"]["1"]["ex_boost"], original);
    assert_eq!(
        request(&service, "/api/index.php/ex_boost/draw", &body),
        pending
    );
    assert_eq!(snapshot(&root), committed);
    let overwrite = cn_support::send_request(
        service.port(),
        "/api/index.php/ex_boost/draw",
        &encode_request(&draw_request(viewer_id, 10001, 22)),
    );
    assert!(overwrite.starts_with("HTTP/1.1 400 Bad Request"));
    assert_eq!(snapshot(&root), committed);
    service.stop().unwrap();

    let service = PersonalService::start(root.path(), 0).unwrap();
    let loaded = load(&service, viewer_id);
    assert_eq!(
        loaded["ex_boost_draw_result"],
        json!({"character_id":1,"draw_result":pending["draw_result"]})
    );
    assert_eq!(loaded["user_character_list"]["1"]["ex_boost"], original);
    assert!(loaded.get("pending_ex_boost").is_none());
    assert!(loaded.get("ex_boost_request_receipts").is_none());
    let select = json!({"viewer_id":viewer_id,"is_confirm":true,"api_count":23});
    let accepted = request(&service, "/api/index.php/ex_boost/select", &select);
    assert_eq!(
        accepted["character_list"][0]["ex_boost"],
        pending["draw_result"]
    );
    assert!(accepted["character_list"][0]["join_time"].is_string());
    assert!(accepted["character_list"][0]["update_time"].is_string());
    let committed = snapshot(&root);
    assert!(committed["user_character_list"]["1"]["update_time"].is_i64());
    assert_eq!(
        request(&service, "/api/index.php/ex_boost/select", &select),
        accepted
    );
    assert_eq!(snapshot(&root), committed);
    assert!(committed.get("pending_ex_boost").is_none());

    let rerolled = request(
        &service,
        "/api/index.php/ex_boost/draw",
        &draw_request(viewer_id, 10001, 24),
    );
    assert_eq!(rerolled["item_list"]["10001"], 20);
    let discard = json!({"viewer_id":viewer_id,"is_confirm":false,"api_count":25});
    let discarded = request(&service, "/api/index.php/ex_boost/select", &discard);
    let committed = snapshot(&root);
    assert_eq!(
        request(&service, "/api/index.php/ex_boost/select", &discard),
        discarded
    );
    assert_eq!(snapshot(&root), committed);
    assert_eq!(
        committed["user_character_list"]["1"]["ex_boost"],
        pending["draw_result"]
    );
    assert!(committed.get("pending_ex_boost").is_none());
    assert!(load(&service, viewer_id)["ex_boost_draw_result"].is_null());
    service.stop().unwrap();
}
// //// /验证中断后恢复暂存选择以及接受和丢弃的请求重放 ////

// //// 恢复 EX 存档的时间类型并保留结果和已结算资产 [@x380kkm 2026-09-27] ////
#[test]
fn restores_saved_character_times_and_replayed_response_dates() {
    let (root, service, viewer_id) = setup();
    let body = draw_request(viewer_id, 10003, 40);
    let drawn = request(&service, "/api/index.php/ex_boost/first_draw", &body);
    service.stop().unwrap();
    let mut stored = snapshot(&root);
    stored["user_character_list"]["1"]["join_time"] = json!("1700000000");
    stored["user_character_list"]["1"]["update_time"] = json!("2024-07-20 09:30:00");
    let receipt = stored["ex_boost_request_receipts"]
        .as_object_mut()
        .unwrap()
        .values_mut()
        .next()
        .unwrap();
    receipt["response"]["character_list"][0]["join_time"] = json!(1_700_000_000);
    let database =
        rusqlite::Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    database
        .execute(
            "UPDATE player_snapshots SET data_json=?1",
            [stored.to_string()],
        )
        .unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let loaded = load(&service, viewer_id);
    let character = &loaded["user_character_list"]["1"];
    assert_eq!(character["join_time"], 1_700_000_000);
    assert_eq!(character["update_time"], 1_721_467_800);
    assert_eq!(
        character["ex_boost"],
        drawn["character_list"][0]["ex_boost"]
    );
    assert_eq!(character["over_limit_step"], 6);
    assert_eq!(loaded["item_list"]["10001"], 30);
    assert_eq!(loaded["item_list"]["10003"], 1);
    let recovered = snapshot(&root);
    assert_eq!(
        recovered["user_character_list"]["1"]["join_time"],
        1_700_000_000
    );
    assert_eq!(
        recovered["user_character_list"]["1"]["update_time"],
        1_721_467_800
    );
    let replayed = request(&service, "/api/index.php/ex_boost/first_draw", &body);
    assert_eq!(
        replayed["character_list"][0]["join_time"],
        "2023-11-14 22:13:20"
    );
    assert_eq!(
        replayed["character_list"][0]["ex_boost"],
        character["ex_boost"]
    );
    assert!(replayed["character_list"][0]["update_time"].is_string());
    assert_eq!(snapshot(&root), recovered);
    request(&service, "/api/index.php/single_battle_quest/start", &json!({
        "viewer_id": viewer_id, "play_id": "ex-recovered-character",
        "quest_id": 1_001_002, "category": 1, "party_id": 1,
        "use_boss_boost_point": false, "use_boost_point": false,
        "is_auto_start_mode": false,
    }));
    let settled = request(&service, "/api/index.php/single_battle_quest/finish", &json!({
        "viewer_id": viewer_id, "play_id": "ex-recovered-character",
        "quest_id": 1_001_002, "category": 1, "elapsed_time_ms": 1000,
        "score": 2_147_483_648.0, "add_mana": 0, "is_accomplished": true,
        "statistics": {
            "party": {
                "characters": [{"id": 1}, null, null],
                "unison_characters": [null, null, null],
            },
        },
    }));
    assert_eq!(settled["clear_rank"], 5);
    let after_battle = snapshot(&root);
    assert_eq!(after_battle["user_character_list"]["1"]["ex_boost"], character["ex_boost"]);
    assert_eq!(after_battle["user_character_list"]["1"]["join_time"], 1_700_000_000);
    assert!(after_battle["user_character_list"]["1"]["update_time"].is_i64());
    service.stop().unwrap();
}
// //// /恢复 EX 存档的时间类型并保留结果和已结算资产 ////
