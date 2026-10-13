// audience: internal
// # personal-service-cn-rescue-runtime-tests
//
// 隔离实例覆盖无人房回收, BOT 房主开战, 续战和跨关闭结算.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;
#[path = "cn_rescue_runtime/clock.rs"]
mod clock;
#[path = "cn_rescue_runtime/ai_sources.rs"]
mod ai_sources;

use cn_support::{decode_response, encode_request, SignupData};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::{PersonalService, PersonalServiceOptions};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn data(service: &PersonalService, route: &str, body: Value) -> Value {
    let response = cn_support::send_request(
        service.port(),
        &format!("/api/index.php/{route}"),
        &encode_request(&body),
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{route}: {response}");
    let response = decode_response::<Value>(&response);
    assert_eq!(response.data_headers.result_code, 1, "{route}");
    response.data
}

fn management(service: &PersonalService, route: &str, body: Value) {
    let token = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        "PUT",
        route,
        "application/json",
        &[("Authorization", &token)],
        body.to_string().as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

fn start_player(root: &Path) -> (PersonalService, i64) {
    let service = PersonalService::start_with_options(
        PersonalServiceOptions::new(root, 0, root.join("cdn/cn")).with_multiplayer_session_port(0),
    )
    .unwrap();
    let response = cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&json!({"device_id":923401})),
    );
    let viewer = decode_response::<SignupData>(&response)
        .data_headers
        .viewer_id;
    data(
        &service,
        "load",
        json!({"viewer_id":viewer,"keychain":viewer}),
    );
    (service, viewer)
}

fn start_service(root: &Path) -> (PersonalService, i64) {
    let (service, viewer) = start_player(root);
    unlock_attention(&database(root));
    management(
        &service,
        "/v1/rescue-settings",
        json!({"enabled":true,"spawn_interval_seconds":60,"room_lifetime_seconds":180,"max_rooms":2,
        "quests":[{"category_id":2,"quest_id":1001001}]}),
    );
    (service, viewer)
}

fn unlock_attention(db: &Connection) {
    db.execute(
        "UPDATE player_snapshots SET data_json=json_insert(data_json, '$.quest_progress.\"1\"[#]', json('{\"quest_id\":1008004,\"finished\":true}'))",
        [],
    ).unwrap();
}

fn database(root: &Path) -> Connection {
    Connection::open(root.join("personal-service.sqlite3")).unwrap()
}

#[track_caller]
fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "runtime condition did not become true"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn rooms(db: &Connection) -> i64 {
    db.query_row(
        "SELECT COUNT(*) FROM multiplayer_attention_rooms",
        [],
        |row| row.get(0),
    )
    .unwrap()
}

fn catalog_quest(service: &PersonalService, category: i64, quest: i64) -> Value {
    let token = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        "GET",
        "/v1/rescue-settings/quests",
        "application/json",
        &[("Authorization", &token)],
        &[],
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let response: Value = serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    response["quests"].as_array().unwrap().iter()
        .find(|entry| entry["category_id"] == category && entry["quest_id"] == quest)
        .unwrap().clone()
}

// //// 当前存档通关前置后同步启用目录和真实铃铛投递 [@x380kkm 2026-09-25] ////
#[test]
fn unlocks_catalog_and_bell_delivery_after_the_source_main_quest_clear() {
    let root = TempDir::new().unwrap();
    let (service, viewer) = start_player(root.path());
    let db = database(root.path());
    management(&service, "/v1/rescue-settings", json!({
        "enabled":true, "quests":[{"category_id":2,"quest_id":1001001}]
    }));
    assert_eq!(catalog_quest(&service, 2, 1_001_001)["is_open"], false);
    let locked = data(&service, "attention/check", json!({
        "viewer_id":viewer, "request_number":3, "holding_number":0
    }));
    assert!(locked["multi"].as_array().unwrap().is_empty());
    assert_eq!(rooms(&db), 0);

    unlock_attention(&db);
    assert_eq!(catalog_quest(&service, 2, 1_001_001)["is_open"], true);
    let available = data(&service, "attention/check", json!({
        "viewer_id":viewer, "request_number":3, "holding_number":0
    }));
    assert_eq!(available["multi"].as_array().unwrap().len(), 1);
    assert_eq!(available["multi"][0]["quest_info"]["quest_id"], 1_001_001);
    assert_eq!(rooms(&db), 1);
    service.stop().unwrap();
}

struct Socket {
    stream: TcpStream,
    buffer: Vec<u8>,
}

impl Socket {
    fn connect(port: u16) -> Self {
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(6)))
            .unwrap();
        Self {
            stream,
            buffer: Vec::new(),
        }
    }
    fn send(&mut self, frame: Value) {
        serde_json::to_writer(&mut self.stream, &frame).unwrap();
        self.stream.write_all(&[0]).unwrap();
    }
    fn receive(&mut self) -> Value {
        loop {
            if let Some(end) = self.buffer.iter().position(|byte| *byte == 0) {
                let frame = self.buffer.drain(..end).collect::<Vec<_>>();
                self.buffer.remove(0);
                return serde_json::from_slice(&frame).unwrap();
            }
            let mut bytes = [0; 16384];
            let length = self.stream.read(&mut bytes).unwrap();
            assert!(length > 0, "socket closed before expected frame");
            self.buffer.extend_from_slice(&bytes[..length]);
        }
    }
}

fn enter_lobby(service: &PersonalService, viewer: i64, room: &Value) -> (Socket, String) {
    let prepared = data(
        service,
        "multi_battle_quest/prepare",
        json!({"viewer_id":viewer,"category":room["category_id"],"quest_id":room["quest_id"],"room_number":room["room_number"]}),
    );
    assert_eq!(prepared["raising_state"], 1);
    let mut socket = Socket::connect(prepared["port"].as_u64().unwrap() as u16);
    socket.send(json!({"socklet":"cooperation_room","viewerId":viewer,"roomNumber":room["room_number"],"questCategory":room["category_id"],"questId":room["quest_id"],"reconnected":0}));
    let connection = socket.receive()[1].as_str().unwrap().to_owned();
    let party = json!({"characters":[[0,{"id":1,"evolution_level":0,"exp":10,"over_limit_step":0,"mana_node_ids":{},"illustration_settings":[1],"ex_boost":[1]}],[1],[1]],
        "unison_characters":[[1],[1],[1]],"equipments":[[1],[1],[1]],"abilitySoulIds":[[1],[1],[1]],"options":null});
    socket.send(json!([0,[0,{"viewerId":viewer,"name":"Player","rank":1,"degreeId":1,"party":party,"connectionId":connection,
        "playerRoleKind":1,"isNewbie":false,"isHost":false,"entryTime":0,"currentPartyId":1,"autoplayMode":false,
        "autoskillMode":1,"autoSpeedLevel":1,"autoStart":true,"skillAbilityBehaviorMode":1,"dashBehaviorMode":1,
        "allowHealFromOtherPlayers":true,"state":[0]},1]]));
    let welcome = socket.receive();
    assert_eq!(welcome[1][2][0]["viewerId"], 900000001);
    assert_eq!(welcome[1][2][0]["isHost"], true);
    socket.send(json!([0, [3, [1]]]));
    for _ in 0..12 {
        let frame = socket.receive();
        if frame[1][0] == 5 {
            assert_eq!(frame[1][1][0]["viewerId"], 900000001);
            assert!(frame[1][1]
                .as_array()
                .unwrap()
                .iter()
                .any(|mate| mate["viewerId"] == viewer && mate["isHost"] == false));
            return (socket, connection);
        }
    }
    panic!("BOT host did not start after guest readiness");
}

fn start(service: &PersonalService, viewer: i64, room: &Value, key: &Value, play: &str) -> Value {
    data(
        service,
        "multi_battle_quest/start",
        json!({"viewer_id":viewer,"category":room["category_id"],"quest_id":room["quest_id"],"party_id":1,
        "room_number":room["room_number"],"attention_key":key,"play_id":play,"use_boost_point":false,"use_boss_boost_point":false,"is_auto_start_mode":true}),
    )
}

fn finish(service: &PersonalService, viewer: i64, play: &str) -> Value {
    data(
        service,
        "multi_battle_quest/finish",
        json!({"viewer_id":viewer,"category":2,"quest_id":1001001,"play_id":play,
        "is_accomplished":true,"elapsed_time_ms":10000,"score":1000,"add_mana":0,
        "statistics":{"zones":[],"party":{"characters":[{"id":1},null,null],"unison_characters":[null,null,null],"equipments":[null,null,null],"ability_soul_ids":[null,null,null]}}}),
    )
}

// //// 在无人轮询和游戏时间缓慢推进时限制并回收救援房 [@x380kkm 2026-09-23] ////
#[test]
fn maintains_empty_rooms_without_attention_polling() {
    let root = TempDir::new().unwrap();
    let (service, _) = start_service(root.path());
    let db = database(root.path());
    management(
        &service,
        "/v1/time",
        json!({"enabled":true,"iso":"2024-06-05T12:00:00.000Z","rate":0.0001}),
    );
    wait_until(|| rooms(&db) == 1);
    db.execute(
        "UPDATE multiplayer_attention_schedule SET next_spawn_ms=0",
        [],
    )
    .unwrap();
    wait_until(|| rooms(&db) == 2);
    db.execute(
        "UPDATE multiplayer_attention_schedule SET next_spawn_ms=0",
        [],
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(1100));
    assert_eq!(rooms(&db), 2);
    db.execute(
        "UPDATE multiplayer_attention_schedule SET next_spawn_ms=9223372036854775807",
        [],
    )
    .unwrap();
    db.execute("UPDATE multiplayer_rooms SET expiry_anchor_ms=0", [])
        .unwrap();
    wait_until(|| rooms(&db) == 0);
    service.stop().unwrap();
}

// //// 客机沿结算后的选房协议续战并保留已结算奖励 [@x380kkm 2026-09-26] ////
#[test]
fn settles_rescue_and_newbie_rewards_then_continues_until_closed() {
    let root = TempDir::new().unwrap();
    let (service, viewer) = start_service(root.path());
    let db = database(root.path());
    wait_until(|| rooms(&db) == 1);
    let offer = data(
        &service,
        "attention/check",
        json!({"viewer_id":viewer,"request_number":3,"holding_number":0}),
    )["multi"][0]
        .clone();
    assert!(offer["attention_key"].is_string());
    let unaccepted_resume = data(
        &service,
        "multi_battle_quest/select_room",
        json!({"viewer_id":viewer,"category":2,"quest_id":1001001,"party_id":1,"accepted_type":6,"room_number":offer["quest_info"]["room_number"]}),
    );
    assert_eq!(unaccepted_resume["raising_state"], 9);
    let room = data(
        &service,
        "multi_battle_quest/select_room",
        json!({"viewer_id":viewer,"category":2,"quest_id":1001001,"party_id":1,"accepted_type":2,"room_number":offer["quest_info"]["room_number"]}),
    );
    db.execute("UPDATE player_snapshots SET data_json=json_set(data_json, '$.mission_stage_receipts.\"1:10:1\"', json('true'))", []).unwrap();
    db.execute("INSERT INTO player_mission_progress(account_id,category,mission_id,value) SELECT account_id,5,70004,9 FROM player_snapshots", []).unwrap();
    db.execute("UPDATE multiplayer_ai_mates SET lobby_player_json=json_set(lobby_player_json,'$.isNewbie',json('true'))", []).unwrap();
    data(
        &service,
        "attention/action",
        json!({"viewer_id":viewer,"priority_factors":["Clear"]}),
    );
    data(
        &service,
        "attention/logger",
        json!({"viewer_id":viewer,"client_logs":[{"log_type":"Accept","attention_key":offer["attention_key"]}]}),
    );
    assert_eq!(
        db.query_row(
            "SELECT value FROM player_mission_progress WHERE category=5 AND mission_id=70004",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        9
    );
    let (lobby, connection) = enter_lobby(&service, viewer, &room);
    let stamina: i64 = db
        .query_row(
            "SELECT json_extract(data_json,'$.user_info.stamina') FROM player_snapshots",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let started = start(
        &service,
        viewer,
        &room,
        &offer["attention_key"],
        "rescue-one",
    );
    assert_eq!(started["user_info"]["stamina"], stamina);
    let mut battle = Socket::connect(room["port"].as_u64().unwrap() as u16);
    battle.send(json!({"socklet":"cooperation_battle","roomNumber":room["room_number"],"connectionId":connection,"reconnected":0}));
    assert_eq!(battle.receive()[0], 0);
    battle.send(json!([0, [0]]));
    assert_eq!(battle.receive(), json!([1, [1]]));
    drop(lobby);
    db.execute("UPDATE multiplayer_ai_mates SET lobby_player_json=json_set(lobby_player_json,'$.isNewbie',json('false'))", []).unwrap();
    let result = finish(&service, viewer, "rescue-one");
    assert_eq!(result["host_finished"], false);
    assert!(
        result["mission_info"]
            .as_array()
            .unwrap()
            .iter()
            .any(|mission| mission["mission_id"] == 62),
        "missions: {}",
        result["mission_info"]
    );
    assert!(result["mission_info"]
        .as_array()
        .unwrap()
        .iter()
        .any(|mission| mission["mission_id"] == 70004));
    assert_eq!(finish(&service, viewer, "rescue-one"), result);
    let snapshot: String = db
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    let snapshot: Value = serde_json::from_str(&snapshot).unwrap();
    assert!(snapshot["degree_ids"]
        .as_array()
        .unwrap()
        .contains(&json!(80009)));
    assert_eq!(
        db.query_row(
            "SELECT value FROM player_mission_progress WHERE category=5 AND mission_id=70004",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        10
    );
    assert_eq!(rooms(&db), 1);
    let waiting: (bool, bool, bool) = db.query_row(
        "SELECT rooms.battle_started, members.entered, members.ready
         FROM multiplayer_rooms AS rooms
         JOIN multiplayer_room_members AS members USING(room_number)",
        [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).unwrap();
    assert_eq!(waiting, (false, false, false));
    let searched = data(&service, "multi_battle_quest/search_room", json!({
        "viewer_id":viewer, "room_number":room["room_number"]
    }));
    assert_eq!(searched["room_exists"], true);
    assert_eq!(searched["establisher_viewer_id"], 900000001);
    db.execute("UPDATE multiplayer_attention_deliveries SET delivered_at_ms=0", []).unwrap();
    let resume_request = json!({
        "viewer_id":viewer, "category":2, "quest_id":1001001, "party_id":1,
        "accepted_type":6, "room_number":room["room_number"]
    });
    let resumed = data(&service, "multi_battle_quest/select_room", resume_request.clone());
    assert_eq!(resumed["raising_state"], 1);
    assert_eq!(resumed["room_sequence"], room["room_sequence"]);
    assert_eq!(data(&service, "multi_battle_quest/select_room", resume_request), resumed);
    let (second_lobby, _) = enter_lobby(&service, viewer, &resumed);
    start(
        &service,
        viewer,
        &room,
        &offer["attention_key"],
        "rescue-two",
    );
    management(&service, "/v1/rescue-settings", json!({"enabled":false}));
    assert!(
        data(&service, "attention/check", json!({"viewer_id":viewer}))["multi"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(rooms(&db), 1);
    finish(&service, viewer, "rescue-two");
    assert_eq!(
        db.query_row(
            "SELECT value FROM player_mission_progress WHERE category=5 AND mission_id=70004",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        10
    );
    wait_until(|| rooms(&db) == 0);
    assert_eq!(
        data(
            &service,
            "multi_battle_quest/restore_room",
            json!({"viewer_id":viewer,"room_number":room["room_number"],"room_sequence":room["room_sequence"]})
        )["raising_state"],
        9
    );
    drop((second_lobby, battle));
    service.stop().unwrap();
}

// //// 客机退出和重试保留 BOT 房主的待战房间 [@x380kkm 2026-09-26] ////
#[test]
fn guest_abort_and_disband_preserve_bot_host() {
    let root = TempDir::new().unwrap();
    let (service, viewer) = start_service(root.path());
    let db = database(root.path());
    wait_until(|| rooms(&db) == 1);
    let offer = data(&service, "attention/check", json!({"viewer_id":viewer}))["multi"][0].clone();
    let room = data(&service, "multi_battle_quest/select_room", json!({
        "viewer_id":viewer, "category":2, "quest_id":1001001, "party_id":1,
        "accepted_type":2, "room_number":offer["quest_info"]["room_number"]
    }));
    let (lobby, _) = enter_lobby(&service, viewer, &room);
    start(&service, viewer, &room, &offer["attention_key"], "rescue-abort");
    let abort_request = json!({"viewer_id":viewer, "play_id":"rescue-abort", "api_count":4});
    let aborted = data(&service, "multi_battle_quest/abort", abort_request.clone());
    assert_eq!(data(&service, "multi_battle_quest/abort", abort_request), aborted);
    assert_eq!(rooms(&db), 1);
    assert_eq!(db.query_row("SELECT COUNT(*) FROM multiplayer_room_members", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
    assert_eq!(db.query_row("SELECT COUNT(*) FROM active_single_quests", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
    data(&service, "multi_battle_quest/disband_room", json!({
        "viewer_id":viewer, "room_number":room["room_number"]
    }));
    let searched = data(&service, "multi_battle_quest/search_room", json!({
        "viewer_id":viewer, "room_number":room["room_number"]
    }));
    assert_eq!(searched["room_exists"], true);
    assert_eq!(searched["establisher_viewer_id"], 900000001);
    let resumed = data(&service, "multi_battle_quest/select_room", json!({
        "viewer_id":viewer, "category":2, "quest_id":1001001, "party_id":1,
        "accepted_type":6, "room_number":room["room_number"]
    }));
    assert_eq!(resumed["raising_state"], 9);
    drop(lobby);
    service.stop().unwrap();
}
