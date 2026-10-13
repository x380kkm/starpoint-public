// audience: internal
// # personal-service-cn-entry-resource-tests
// 此文件验证入场消耗的实际资产变化, 资源不足回滚和重复请求复用相同入场结果.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

// //// 构造隔离玩家和战斗请求 [@x380kkm 2026-09-22] ////
fn raw(service: &PersonalService, route: &str, body: Value) -> String {
    cn_support::send_request(
        service.port(),
        &format!("/api/index.php/{route}"),
        &encode_request(&body),
    )
}

fn request(service: &PersonalService, route: &str, body: Value) -> Value {
    decode_response::<Value>(&raw(service, route, body)).data
}

fn signup(service: &PersonalService, device: i64) -> i64 {
    decode_response::<SignupData>(&raw(
        service,
        "tool/signup",
        serde_json::to_value(SignupRequest { device_id: device }).unwrap(),
    ))
    .data_headers
    .viewer_id
}

fn snapshot(root: &Path, viewer: i64) -> Value {
    let db = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    let stored: String = db.query_row("SELECT data_json FROM player_snapshots JOIN sessions USING(account_id) WHERE sessions.token=?1",
        params![viewer.to_string()], |row| row.get(0)).unwrap();
    serde_json::from_str(&stored).unwrap()
}

fn update_snapshot(root: &Path, viewer: i64, change: impl FnOnce(&mut Value)) {
    let mut data = snapshot(root, viewer);
    change(&mut data);
    Connection::open(root.join("personal-service.sqlite3")).unwrap()
        .execute("UPDATE player_snapshots SET data_json=?1 WHERE account_id=(SELECT account_id FROM sessions WHERE token=?2)",
            params![data.to_string(), viewer.to_string()]).unwrap();
}

fn start_body(viewer: i64, category: i64, quest: i64, play: &str) -> Value {
    json!({"viewer_id":viewer,"category":category,"quest_id":quest,"play_id":play,"party_id":1,
        "is_auto_start_mode":false,"use_boost_point":false,"use_boss_boost_point":false,"api_count":1})
}

fn create_room(service: &PersonalService, viewer: i64) -> String {
    request(
        service,
        "multi_battle_quest/create_room",
        json!({"viewer_id":viewer,"category":2,"quest_id":1002001,"party_id":1}),
    )["room_number"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn multi_start(viewer: i64, room: &str, play: &str) -> Value {
    let mut body = start_body(viewer, 2, 1002001, play);
    body["room_number"] = room.into();
    body
}

fn finish_body(viewer: i64, category: i64, quest: i64, play: &str) -> Value {
    json!({"viewer_id":viewer,"category":category,"quest_id":quest,"play_id":play,"api_count":2,
        "elapsed_time_ms":1000,"score":1000,"add_mana":0,"is_accomplished":true,
        "statistics":{"zones":[],"party":{"characters":[],"unison_characters":[],"equipments":[],"ability_soul_ids":[]}}})
}
// //// /构造隔离玩家和战斗请求 ////

// //// 每个消耗道具关卡原子扣费并重放入场 [@x380kkm 2026-09-22] ////
#[test]
fn deducts_every_master_entry_item_once_and_rolls_back_insufficient_stamina() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 9301);
    let cases = [
        (7, 200021009, 40323, 30),
        (7, 200013009, 40314, 30),
        (7, 200076009, 10000072, 30),
        (7, 200071009, 10000049, 30),
        (7, 200050009, 40335, 30),
        (13, 1038, 30029, 0),
        (13, 1039, 30029, 0),
        (13, 1040, 30029, 0),
        (13, 2001, 500000, 10),
        (13, 2002, 500000, 10),
        (13, 2003, 500000, 10),
        (13, 2004, 500000, 10),
        (13, 2005, 500000, 10),
        (13, 2006, 500000, 10),
    ];
    for (category, quest, item, stamina_cost) in cases {
        let key = item.to_string();
        update_snapshot(root.path(), viewer, |player| {
            player["user_info"]["stamina"] = json!(100);
            player["item_list"][&key] = json!(0);
        });
        let body = start_body(viewer, category, quest, &format!("entry-{quest}"));
        let before = snapshot(root.path(), viewer);
        assert!(raw(&service, "single_battle_quest/start", body.clone())
            .contains("Not enough entry items"));
        assert_eq!(snapshot(root.path(), viewer), before);
        if stamina_cost > 0 {
            update_snapshot(root.path(), viewer, |player| {
                player["user_info"]["stamina"] = json!(0);
                player["item_list"][&key] = json!(1);
            });
            let before = snapshot(root.path(), viewer);
            assert!(raw(&service, "single_battle_quest/start", body.clone())
                .contains("Insufficient stamina"));
            assert_eq!(snapshot(root.path(), viewer), before);
        }
        update_snapshot(root.path(), viewer, |player| {
            player["user_info"]["stamina"] = json!(100);
            player["item_list"][&key] = json!(1);
        });
        let before = snapshot(root.path(), viewer);
        let result = request(&service, "single_battle_quest/start", body.clone());
        assert_eq!(result["item_list"][&key], 0);
        assert_eq!(result["user_info"]["stamina"], 100 - stamina_cost);
        let after = snapshot(root.path(), viewer);
        assert_eq!(after["item_list"][&key], 0);
        assert_eq!(
            after["user_info"]["total_stamina_used"].as_i64().unwrap(),
            before["user_info"]["total_stamina_used"]
                .as_i64()
                .unwrap_or_default()
                + stamina_cost
        );
        assert_eq!(
            request(&service, "single_battle_quest/start", body.clone()),
            result
        );
        assert_eq!(snapshot(root.path(), viewer), after);
        let response = raw(
            &service,
            "single_battle_quest/abort",
            json!({"viewer_id":viewer,"play_id":body["play_id"]}),
        );
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    }
    service.stop().unwrap();
}

// //// 房主原价和客机半价在重试和冲突时保持实际余额 [@x380kkm 2026-09-22] ////
#[test]
fn charges_host_and_guest_with_replay_and_conflict_isolation() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let host = signup(&service, 9302);
    let guest = signup(&service, 9303);
    let room = create_room(&service, host);
    request(
        &service,
        "multi_battle_quest/select_room",
        json!({"viewer_id":guest,"category":2,"quest_id":1002001,"party_id":1,"room_number":room}),
    );
    for viewer in [host, guest] {
        update_snapshot(root.path(), viewer, |player| {
            player["user_info"]["stamina"] = json!(0)
        });
    }
    let before = snapshot(root.path(), host);
    assert!(raw(
        &service,
        "multi_battle_quest/start",
        multi_start(host, &room, "host-fee")
    )
    .contains("insufficient_stamina"));
    assert_eq!(snapshot(root.path(), host), before);
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT battle_started FROM multiplayer_rooms WHERE room_number=?1",
            params![room],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    update_snapshot(root.path(), host, |player| {
        player["user_info"]["stamina"] = json!(100)
    });
    request(
        &service,
        "single_battle_quest/start",
        start_body(host, 1, 1002002, "host-fee"),
    );
    let active_snapshot = snapshot(root.path(), host);
    for play in ["host-fee", "another-play"] {
        assert!(raw(
            &service,
            "multi_battle_quest/start",
            multi_start(host, &room, play)
        )
        .contains("battle_start_conflict"));
        assert_eq!(snapshot(root.path(), host), active_snapshot);
    }
    assert_eq!(db.query_row(
        "SELECT quest_id FROM active_single_quests WHERE account_id=(SELECT account_id FROM sessions WHERE token=?1)",
        params![host.to_string()], |row| row.get::<_, i64>(0)).unwrap(), 1002002);
    let aborted = raw(
        &service,
        "single_battle_quest/abort",
        json!({"viewer_id":host,"play_id":"host-fee"}),
    );
    assert!(aborted.starts_with("HTTP/1.1 200"), "{aborted}");
    let mut invalid_boost = multi_start(host, &room, "invalid-boost");
    invalid_boost["use_boost_point"] = json!(true);
    let before = snapshot(root.path(), host);
    assert!(raw(&service, "multi_battle_quest/start", invalid_boost).contains("invalid_boost_kind"));
    assert_eq!(snapshot(root.path(), host), before);
    for (viewer, play, cost) in [(host, "host-fee", 13), (guest, "guest-fee", 6)] {
        update_snapshot(root.path(), viewer, |player| {
            player["user_info"]["stamina"] = json!(cost)
        });
        let body = multi_start(viewer, &room, play);
        let started = request(&service, "multi_battle_quest/start", body.clone());
        assert_eq!(started["user_info"]["stamina"], 0);
        let after = snapshot(root.path(), viewer);
        assert_eq!(after["user_info"]["stamina"], 0);
        let mut replay = body.clone();
        replay["api_count"] = json!(99);
        assert_eq!(
            request(&service, "multi_battle_quest/start", replay),
            started
        );
        assert_eq!(snapshot(root.path(), viewer), after);
        update_snapshot(root.path(), viewer, |player| {
            player["user_info"]["stamina"] = json!(100)
        });
        let mut conflict = body;
        conflict["play_id"] = json!(format!("{play}-conflict"));
        let before = snapshot(root.path(), viewer);
        assert!(
            raw(&service, "multi_battle_quest/start", conflict).contains("battle_start_conflict")
        );
        assert_eq!(snapshot(root.path(), viewer), before);
    }
    let guest_before = snapshot(root.path(), guest);
    let mut invalid = multi_start(guest, &room, "unissued-rescue");
    invalid["attention_key"] = json!("unissued");
    assert!(raw(&service, "multi_battle_quest/start", invalid).contains("invalid_attention_key"));
    assert_eq!(snapshot(root.path(), guest), guest_before);
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let replay = request(
        &service,
        "multi_battle_quest/start",
        multi_start(guest, &room, "guest-fee"),
    );
    assert_eq!(replay["user_info"]["stamina"], 0);
    assert_eq!(snapshot(root.path(), guest), guest_before);
    service.stop().unwrap();
}

// //// 加成点耗尽和重复结算保持非负余额 [@x380kkm 2026-09-22] ////
#[test]
fn settles_only_available_boost_points() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 9304);
    let before = snapshot(root.path(), viewer);
    for (category, quest, boost, boss_boost) in [
        (1, 1001002, false, true),
        (2, 1002001, true, false),
        (1, 1001002, true, true),
    ] {
        let mut body = start_body(viewer, category, quest, "invalid-boost");
        body["use_boost_point"] = json!(boost);
        body["use_boss_boost_point"] = json!(boss_boost);
        assert!(
            raw(&service, "single_battle_quest/start", body).contains("Invalid boost selection")
        );
        assert_eq!(snapshot(root.path(), viewer), before);
    }
    for (points, expected_drops, play) in [(0, 4, "boost-empty"), (1, 8, "boost-last")] {
        update_snapshot(root.path(), viewer, |player| {
            player["user_info"]["stamina"] = json!(100);
            player["user_info"]["boost_point"] = json!(points);
        });
        let mut body = start_body(viewer, 1, 1001002, play);
        body["use_boost_point"] = json!(true);
        request(&service, "single_battle_quest/start", body);
        assert_eq!(
            snapshot(root.path(), viewer)["user_info"]["boost_point"],
            points
        );
        let finish = finish_body(viewer, 1, 1001002, play);
        let result = request(&service, "single_battle_quest/finish", finish.clone());
        let drops: i64 = result["drop_score_reward_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|drop| drop["number"].as_i64().unwrap())
            .sum();
        assert_eq!(drops, expected_drops);
        assert_eq!(result["user_info"]["boost_point"], 0);
        let after = snapshot(root.path(), viewer);
        assert_eq!(
            request(&service, "single_battle_quest/finish", finish),
            result
        );
        assert_eq!(snapshot(root.path(), viewer), after);
    }
    for points in [0, 1] {
        update_snapshot(root.path(), viewer, |player| {
            player["user_info"]["stamina"] = json!(100);
            player["user_info"]["boss_boost_point"] = json!(points);
        });
        let room = create_room(&service, viewer);
        let play = format!("boss-boost-{points}");
        let mut body = multi_start(viewer, &room, &play);
        body["use_boss_boost_point"] = json!(true);
        request(&service, "multi_battle_quest/start", body);
        assert_eq!(
            snapshot(root.path(), viewer)["user_info"]["boss_boost_point"],
            points
        );
        let finish = finish_body(viewer, 2, 1002001, &play);
        let result = request(&service, "multi_battle_quest/finish", finish.clone());
        assert_eq!(result["user_info"]["boss_boost_point"], 0);
        let after = snapshot(root.path(), viewer);
        assert_eq!(
            request(&service, "multi_battle_quest/finish", finish),
            result
        );
        assert_eq!(snapshot(root.path(), viewer), after);
    }
    service.stop().unwrap();
}
