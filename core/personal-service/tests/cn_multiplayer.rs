// audience: internal
// # personal-service-cn-multiplayer-tests
//
// 该文件验证 16 个联机 HTTP 路由, NUL JSON 大厅与战斗帧和 SQLite 房间恢复.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use rusqlite::Connection;
use serde_json::{json, Map, Value};
use starpoint_personal_service::{PersonalService, PersonalServiceOptions};
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::time::Duration;
use tempfile::TempDir;

const CATEGORY: i64 = 1;
const QUEST_ID: i64 = 1_001_002;

struct FramedSocket {
    stream: TcpStream,
    buffer: Vec<u8>,
}

impl FramedSocket {
    fn connect(port: u16) -> Self {
        let stream = TcpStream::connect(("127.0.0.1", port)).expect("session listener accepts TCP");
        stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .expect("read timeout is configured");
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .expect("write timeout is configured");
        Self {
            stream,
            buffer: Vec::new(),
        }
    }

    fn send(&mut self, value: Value) {
        serde_json::to_writer(&mut self.stream, &value).expect("TCP frame is encoded");
        self.stream.write_all(&[0]).expect("TCP frame is delimited");
    }

    fn receive(&mut self) -> Value {
        loop {
            if let Some(separator) = self.buffer.iter().position(|byte| *byte == 0) {
                let frame = self.buffer.drain(..separator).collect::<Vec<_>>();
                self.buffer.drain(..1);
                return serde_json::from_slice(&frame).expect("TCP frame is JSON");
            }
            let mut chunk = [0_u8; 16 * 1024];
            let read = self.stream.read(&mut chunk).expect("TCP frame is received");
            assert!(read > 0, "TCP connection closed before a complete frame");
            self.buffer.extend_from_slice(&chunk[..read]);
        }
    }

    fn expects_no_frame(&mut self) {
        assert!(self.buffer.is_empty(), "TCP frame was already buffered");
        self.stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .expect("short read timeout is configured");
        let mut byte = [0_u8; 1];
        match self.stream.read(&mut byte) {
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Ok(0) => panic!("TCP connection closed while waiting for peers"),
            Ok(_) => panic!("TCP connection started the battle before every player was ready"),
            Err(error) => panic!("TCP readiness probe failed: {error}"),
        }
        self.stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .expect("read timeout is restored");
    }

    fn closes_without_frame(&mut self) {
        let mut byte = [0_u8; 1];
        match self.stream.read(&mut byte) {
            Ok(0) => {}
            Ok(_) => panic!("TCP connection returned an unexpected frame"),
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                panic!("TCP connection remained open")
            }
            Err(error) => panic!("TCP close failed: {error}"),
        }
    }
}

fn start_service(root: &Path, session_port: u16) -> PersonalService {
    let cn_root = root.join("cdn").join("cn");
    PersonalService::start_with_options(
        PersonalServiceOptions::new(root, 0, cn_root).with_multiplayer_session_port(session_port),
    )
    .expect("service starts with multiplayer listener")
}

fn send(service: &PersonalService, path: &str, body: Value) -> Value {
    decode_response::<Value>(&cn_support::send_request(
        service.port(),
        path,
        &encode_request(&body),
    ))
    .data
}

// //// 设置联机结算使用的战斗道具倍率 [@x380kkm 2026-08-29] ////
fn set_drop_multiplier(service: &PersonalService, multiplier: i64) {
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let body = format!(r#"{{"drop_multiplier":{multiplier}}}"#);
    let response = support::request_with_headers(
        service.port(),
        "PUT",
        "/v1/gameplay-settings",
        "application/json",
        &authorization,
        body.as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"));
}
// //// /设置联机结算使用的战斗道具倍率 ////

fn signup(service: &PersonalService, device_id: i64) -> i64 {
    decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id }),
    ))
    .data_headers
    .viewer_id
}

fn create_room(service: &PersonalService, viewer_id: i64, api_count: i64) -> Value {
    send(
        service,
        "/api/index.php/multi_battle_quest/create_room",
        json!({
            "category": CATEGORY,
            "party_id": 1,
            "quest_id": QUEST_ID,
            "viewer_id": viewer_id,
            "api_count": api_count,
        }),
    )
}

// //// 验证联机开始响应的加载字段 [@x380kkm 2026-08-24] ////
fn assert_multi_start_response(response: &Value, play_id: &str) {
    assert_eq!(response["user_info"]["last_main_quest_id"], QUEST_ID);
    assert_eq!(response["category_id"], CATEGORY);
    assert_eq!(response["is_multi"], "multi");
    assert!(response["start_time"].as_i64().is_some());
    assert_eq!(response["quest_name"], "");
    assert!(response["follow_bonus_info"].is_null());
    assert!(response["client_checks"].is_null());
    assert_eq!(response["play_id"], play_id);
}

// //// 验证 AI 战斗成员载荷 [@x380kkm 2026-08-24] ////
fn assert_ai_battle_player(player: &Value) {
    assert!(player["connectionId"]
        .as_str()
        .is_some_and(|connection_id| !connection_id.is_empty()));
    assert!(player["name"].as_str().is_some_and(|name| !name.is_empty()));
    assert_eq!(player["playerRoleKind"], 99);
    assert!(player["party"].is_object());
    assert!(player["autoplayMode"].is_boolean());
    assert!(player["autoskillMode"].is_i64());
    assert!(player["autoSpeedLevel"].is_i64());
    assert!(player["autoStart"].is_boolean());
    assert!(player["skillAbilityBehaviorMode"].is_i64());
    assert!(player["dashBehaviorMode"].is_i64());
    assert!(player["allowHealFromOtherPlayers"].is_boolean());
    assert!(player["viewerId"].is_i64());
    assert!(player["entryTime"].is_i64());
    assert!(player["comId"].is_i64());
    assert!(player["rank"].is_i64());
    assert!(player["degreeId"].is_i64());
    for character in player["party"]["characters"]
        .as_array()
        .expect("AI party characters are present")
    {
        if character[0] == 0 {
            assert_eq!(character[1]["ex_boost"], json!([1]));
        }
    }
}

fn lobby_party() -> Value {
    json!({
        "characters": [[0, {
            "id": 1,
            "evolution_level": 0,
            "exp": 10,
            "over_limit_step": 0,
            "mana_node_ids": {},
            "illustration_settings": [1],
            "ex_boost": [1]
        }], [1], [1]],
        "unison_characters": [[1], [1], [1]],
        "equipments": [[1], [1], [1]],
        "abilitySoulIds": [[1], [1], [1]],
        "options": null,
    })
}

fn enter_player(viewer_id: i64, connection_id: &str, name: &str, is_host: bool) -> Value {
    json!([0, [0, {
        "viewerId": viewer_id,
        "playerId": 999,
        "name": name,
        "rank": 1,
        "degreeId": 1,
        "mainCharacterId": 999,
        "party": lobby_party(),
        "connectionId": connection_id,
        "playerRoleKind": 1,
        "isNewbie": false,
        "isHost": is_host,
        "entryTime": 0,
        "currentPartyId": 1,
        "autoplayMode": false,
        "autoskillMode": 1,
        "autoSpeedLevel": 1,
        "autoStart": false,
        "skillAbilityBehaviorMode": 1,
        "dashBehaviorMode": 1,
        "allowHealFromOtherPlayers": true,
        "state": [0]
    }, 1]])
}

fn connect_lobby(
    session_port: u16,
    viewer_id: i64,
    room_number: &str,
    name: &str,
    is_host: bool,
) -> (FramedSocket, String, Value) {
    let mut lobby = FramedSocket::connect(session_port);
    lobby.send(json!({
        "socklet": "cooperation_room",
        "viewerId": viewer_id,
        "roomNumber": room_number,
        "questCategory": CATEGORY,
        "questId": QUEST_ID,
        "reconnected": 0,
    }));
    let handshake = lobby.receive();
    assert_eq!(handshake[0], 0);
    assert_eq!(handshake[2], room_number);
    let connection_id = handshake[1]
        .as_str()
        .expect("lobby handshake returns a connection id")
        .to_owned();
    lobby.send(enter_player(viewer_id, &connection_id, name, is_host));
    let welcome = lobby.receive();
    assert_eq!(welcome[0], 1);
    assert_eq!(welcome[1][0], 0);
    assert_eq!(welcome[1][1]["viewerId"], viewer_id);
    assert_eq!(welcome[1][1]["connectionId"], connection_id);
    (lobby, connection_id, welcome)
}

fn lobby_ai_party(client_party: &Value) -> Value {
    let characters = client_party["characters"]
        .as_array()
        .unwrap()
        .iter()
        .map(lobby_character_option)
        .collect::<Vec<_>>();
    let unison_characters = client_party["unison_characters"]
        .as_array()
        .unwrap()
        .iter()
        .map(lobby_character_option)
        .collect::<Vec<_>>();
    let equipments = client_party["equipments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|equipment| {
            equipment.as_object().map_or_else(
                || json!([1]),
                |equipment| {
                    json!([0, {
                        "equipmentId": equipment["equipment_id"],
                        "level": equipment["level"],
                        "enhancementLevel": equipment["enhancement_level"],
                    }])
                },
            )
        })
        .collect::<Vec<_>>();
    let ability_soul_ids = client_party["ability_soul_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| {
            if value.is_null() {
                json!([1])
            } else {
                json!([0, value])
            }
        })
        .collect::<Vec<_>>();
    json!({
        "characters": characters,
        "unison_characters": unison_characters,
        "equipments": equipments,
        "abilitySoulIds": ability_soul_ids,
        "options": null,
    })
}

fn lobby_character_option(value: &Value) -> Value {
    let Some(character) = value.as_object() else {
        return json!([1]);
    };
    let mut character = character.clone();
    let mana_nodes = character
        .remove("mana_node_ids")
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|node| node.as_i64())
        .map(|node| (node.to_string(), Value::from(0)))
        .collect::<Map<_, _>>();
    character.insert("mana_node_ids".to_owned(), Value::Object(mana_nodes));
    character.insert("illustration_settings".to_owned(), json!([1]));
    let ex_boost = character
        .remove("ex_boost")
        .map(|value| json!([0, value]))
        .unwrap_or_else(|| json!([1]));
    character.insert("ex_boost".to_owned(), ex_boost);
    json!([0, Value::Object(character)])
}

fn ai_request(mate: &Value, name: &str) -> Value {
    json!({
        "degreeId": mate["degree_id"],
        "rank": mate["rank"],
        "name": name,
        "comId": mate["com_id"],
        "party": lobby_ai_party(&mate["party"]),
    })
}

fn start_body(viewer_id: i64, room_number: &str, play_id: &str, api_count: i64) -> Value {
    json!({
        "quest_id": QUEST_ID,
        "use_boss_boost_point": false,
        "use_boost_point": false,
        "category": CATEGORY,
        "viewer_id": viewer_id,
        "play_id": play_id,
        "is_auto_start_mode": false,
        "party_id": 1,
        "room_number": room_number,
        "mate_player_ids": [],
        "mate_party_ids": [],
        "api_count": api_count,
    })
}

fn finish_body(viewer_id: i64, play_id: &str, api_count: i64) -> Value {
    json!({
        "viewer_id": viewer_id,
        "quest_id": QUEST_ID,
        "category": CATEGORY,
        "clear_phase": 1,
        "quest_statistics": {
            "party": {"characters": [], "unison_characters": [], "equipments": [], "ability_soul_ids": []}
        },
        "play_id": play_id,
        "battle_time": 1,
        "battle_ended_at": 1,
        "api_count": api_count,
        "mate_player_ids": [],
        "mate_com_ids": [],
        "is_auto_start_mode": false,
        "combat_power": 1,
        "use_boss_boost_point": false,
        "use_boost_point": false,
        "is_accomplished": true,
    })
}

// //// 构造并读取联机结算统计 [@x380kkm 2026-08-25] ////
fn finish_body_with_statistics(viewer_id: i64, play_id: &str, api_count: i64) -> Value {
    let mut body = finish_body(viewer_id, play_id, api_count);
    body["quest_statistics"] = json!({
        "party": {
            "characters": [{"id": 231001}, {"id": 1}, {"id": 161027}],
            "unison_characters": [null, null, null],
            "equipments": [null, null, null],
            "ability_soul_ids": [null, null, null],
        },
        "zones": [{
            "use_power_flip_count": 5,
            "use_dash_count": 6,
            "use_skill_count": 7,
        }],
    });
    body
}

// //// 验证联机结算响应契约 [@x380kkm 2026-08-30] ////
fn assert_multi_finish_response(response: &Value, host_finished: bool) {
    let response = response
        .as_object()
        .expect("multiplayer finish response is an object");
    assert_eq!(
        response.get("host_finished"),
        Some(&Value::Bool(host_finished))
    );
    for key in [
        "aborted_play_id",
        "drawn_quest",
        "follow_info",
        "party_info",
        "unfinished_play_id",
        "carnival_event",
        "ranking_event",
        "score_attack_event",
        "solo_time_attack_event",
    ] {
        assert_eq!(response.get(key), Some(&Value::Null), "field {key}");
    }
    for key in [
        "presigned_quest_category",
        "user_notice_list",
        "user_periodic_reward_point_list",
    ] {
        assert!(
            matches!(response.get(key), Some(Value::Array(values)) if values.is_empty()),
            "field {key}"
        );
    }
    assert!(response.contains_key("rush_event"));
    assert!(response.contains_key("user_daily_challenge_point_list"));
}
// //// /验证联机结算响应契约 ////

fn stored_player_snapshots(root: &Path) -> Vec<Value> {
    let database = Connection::open(root.join("personal-service.sqlite3"))
        .expect("multiplayer database is opened");
    let mut statement = database
        .prepare("SELECT data_json FROM player_snapshots ORDER BY account_id")
        .expect("player snapshots are selected");
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("player snapshots are read")
        .map(|snapshot| {
            serde_json::from_str::<Value>(&snapshot.expect("player snapshot is present"))
                .expect("player snapshot is JSON")
        })
        .collect()
}
// //// /构造并读取联机结算统计 ////

// //// 验证大厅, COM 和全部战斗帧族 [@x380kkm 2026-08-22] ////
#[test]
fn handles_lobby_com_and_every_battle_frame_family() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = start_service(root.path(), 0);
    set_drop_multiplier(&service, 3);
    let session_port = service.multiplayer_session_port().unwrap();
    let host_viewer = signup(&service, 71);
    let guest_viewer = signup(&service, 72);
    let created = create_room(&service, host_viewer, 1);
    let room_number = created["room_number"].as_str().unwrap().to_owned();

    let selected = send(
        &service,
        "/api/index.php/multi_battle_quest/select_room",
        json!({
            "category": CATEGORY,
            "quest_id": QUEST_ID,
            "party_id": 1,
            "accepted_type": 0,
            "viewer_id": guest_viewer,
            "room_number": room_number,
            "api_count": 2,
        }),
    );
    assert_eq!(selected["raising_state"], 2);

    let (mut host_lobby, host_connection, host_welcome) =
        connect_lobby(session_port, host_viewer, &room_number, "Host", true);
    assert_eq!(host_welcome[1][2].as_array().map(Vec::len), Some(1));
    let listed = send(
        &service,
        "/api/index.php/multi_battle_quest/get_rooms",
        json!({"viewer_id": host_viewer, "category_id": CATEGORY, "api_count": 3}),
    );
    assert_eq!(listed["rooms"].as_array().map(Vec::len), Some(1));
    let listed_room = &listed["rooms"][0];
    assert_eq!(listed_room["establisher_character"], 1);
    assert_eq!(listed_room["establisher_character_evolution_img_level"], 0);
    assert_eq!(listed_room["establisher_follow"], 1);
    assert_eq!(
        listed_room["establisher_name"],
        format!("Player{host_viewer}")
    );
    assert_eq!(listed_room["is_pickup"], false);
    assert_eq!(listed_room["mates"], 1);
    assert_eq!(listed_room["room_member_count"], 1);
    let guest_prepared = send(
        &service,
        "/api/index.php/multi_battle_quest/prepare",
        json!({
            "viewer_id": guest_viewer,
            "category": CATEGORY,
            "quest_id": QUEST_ID,
            "room_number": room_number,
            "api_count": 4,
        }),
    );
    assert_eq!(guest_prepared["raising_state"], 1);

    let summoned = send(
        &service,
        "/api/index.php/multi_battle_quest/summon",
        json!({
            "category_id": CATEGORY,
            "quest_id": QUEST_ID,
            "room_number": room_number,
            "viewer_id": host_viewer,
            "api_count": 5,
        }),
    );
    assert!(summoned["mate1"].is_object());
    assert!(summoned["mate2"].is_null());

    let (mut guest_lobby, _, guest_welcome) =
        connect_lobby(session_port, guest_viewer, &room_number, "Guest", false);
    assert_eq!(guest_welcome[1][2].as_array().map(Vec::len), Some(1));
    let guest_mates = guest_lobby.receive();
    let host_mates = host_lobby.receive();
    assert_eq!(guest_mates[1][0], 1);
    assert_eq!(host_mates, guest_mates);
    assert_eq!(guest_mates[1][1].as_array().map(Vec::len), Some(3));
    let lobby_messages = json!(["lobby-relay"]);
    let lobby_broadcast = json!([1, lobby_messages]);
    host_lobby.send(lobby_broadcast.clone());
    assert_eq!(host_lobby.receive(), lobby_broadcast);
    assert_eq!(guest_lobby.receive(), lobby_broadcast);

    drop(guest_lobby);
    let after_leave = host_lobby.receive();
    assert_eq!(after_leave[1][0], 1);
    assert_eq!(after_leave[1][1].as_array().map(Vec::len), Some(2));
    let (reconnected_lobby, reconnected_connection, reconnected_welcome) =
        connect_lobby(session_port, guest_viewer, &room_number, "Guest", false);
    guest_lobby = reconnected_lobby;
    let guest_connection = reconnected_connection;
    assert_eq!(reconnected_welcome[1][2].as_array().map(Vec::len), Some(1));
    let guest_rejoined = guest_lobby.receive();
    let host_rejoined = host_lobby.receive();
    assert_eq!(host_rejoined, guest_rejoined);
    assert_eq!(guest_rejoined[1][1].as_array().map(Vec::len), Some(3));
    let mut changed_host = enter_player(host_viewer, &host_connection, "Host", true)[1][1].clone();
    changed_host["currentPartyId"] = Value::from(2);
    host_lobby.send(json!([0, [2, changed_host, false, 2]]));
    let changed_roster = host_lobby.receive();
    assert_eq!(changed_roster[1][1][0]["currentPartyId"], 2);
    assert_eq!(guest_lobby.receive(), changed_roster);
    let restored_host = enter_player(host_viewer, &host_connection, "Host", true)[1][1].clone();
    host_lobby.send(json!([0, [2, restored_host, false, 1]]));
    let restored_roster = host_lobby.receive();
    assert_eq!(restored_roster[1][1][0]["currentPartyId"], 1);
    assert_eq!(guest_lobby.receive(), restored_roster);
    let lobby_send = json!([2, guest_viewer, ["lobby-direct"]]);
    host_lobby.send(lobby_send.clone());
    assert_eq!(guest_lobby.receive(), lobby_send);
    host_lobby.expects_no_frame();

    guest_lobby.send(json!([0, [3, [1]]]));
    assert_eq!(host_lobby.receive()[1][0], 2);
    assert_eq!(guest_lobby.receive()[1][0], 2);
    assert_eq!(host_lobby.receive()[1][0], 2);
    assert_eq!(guest_lobby.receive()[1][0], 2);

    host_lobby.send(json!([
        0,
        [10, [ai_request(&summoned["mate1"], "COM Mate")]]
    ]));
    let host_frames = (0..3).map(|_| host_lobby.receive()).collect::<Vec<_>>();
    let guest_frames = (0..3).map(|_| guest_lobby.receive()).collect::<Vec<_>>();
    assert_eq!(guest_frames, host_frames);
    assert_eq!(host_frames[0][1][0], 1);
    let joined_roster = host_frames[0][1][1]
        .as_array()
        .expect("joined lobby roster is present");
    assert_eq!(joined_roster.len(), 3);
    assert_ai_battle_player(&joined_roster[2]);
    assert_eq!(
        host_frames[1],
        json!([1, [2, format!("{room_number}-npc-1"), [1]]])
    );
    assert_eq!(host_frames[2], json!([1, [10, 2]]));

    guest_lobby.send(json!([0, [4]]));
    assert_eq!(guest_lobby.receive(), json!([1, [11, guest_connection]]));
    host_lobby.send(json!([0, [7, true, true]]));
    assert_eq!(host_lobby.receive()[1][0], 3);
    assert_eq!(guest_lobby.receive()[1][0], 3);
    host_lobby.send(json!([0, [8, true]]));
    assert_eq!(host_lobby.receive()[1][0], 4);
    assert_eq!(guest_lobby.receive()[1][0], 4);
    host_lobby.send(json!([0, [6]]));
    let host_start = host_lobby.receive();
    assert_eq!(host_start[1][0], 5);
    let started_roster = host_start[1][1]
        .as_array()
        .expect("started lobby roster is present");
    assert_eq!(started_roster.len(), 3);
    assert!(started_roster
        .iter()
        .all(|player| player["state"] == json!([1])));
    assert_ai_battle_player(&started_roster[2]);
    assert_eq!(guest_lobby.receive()[1][0], 5);

    let started = send(
        &service,
        "/api/index.php/multi_battle_quest/start",
        start_body(host_viewer, &room_number, "multi-play-1", 6),
    );
    assert_multi_start_response(&started, "multi-play-1");
    assert_eq!(
        send(
            &service,
            "/api/index.php/multi_battle_quest/start",
            start_body(host_viewer, &room_number, "multi-play-1", 7),
        ),
        started
    );
    let guest_started = send(
        &service,
        "/api/index.php/multi_battle_quest/start",
        start_body(guest_viewer, &room_number, "guest-multi-play-1", 7),
    );
    assert_multi_start_response(&guest_started, "guest-multi-play-1");
    let guest_loaded = send(
        &service,
        "/api/index.php/load",
        json!({"viewer_id": guest_viewer, "api_count": 7}),
    );
    assert_eq!(
        guest_loaded["unfinished_multi_quest_list"],
        json!([{"play_id": "guest-multi-play-1", "continue_count": 0}])
    );
    let loaded_during_battle = send(
        &service,
        "/api/index.php/load",
        json!({
            "viewer_id": host_viewer,
            "api_count": 7,
        }),
    );
    let free_vmoney_before_continue = loaded_during_battle["user_info"]["free_vmoney"]
        .as_i64()
        .unwrap();
    let vmoney_before_continue = loaded_during_battle["user_info"]["vmoney"]
        .as_i64()
        .unwrap();
    assert_eq!(loaded_during_battle["unfinished_quest_list"], json!([]));
    assert_eq!(
        loaded_during_battle["unfinished_multi_quest_list"],
        json!([{"play_id": "multi-play-1", "continue_count": 0}])
    );
    service.stop().expect("active battle service stops cleanly");
    host_lobby.closes_without_frame();
    guest_lobby.closes_without_frame();

    let service = start_service(root.path(), session_port);
    let continued = send(
        &service,
        "/api/index.php/multi_battle_quest/play_continue",
        json!({
            "viewer_id": host_viewer,
            "api_count": 8,
        }),
    );
    assert_eq!(continued, json!({"continue_count": 1}));
    let loaded_after_restart = send(
        &service,
        "/api/index.php/load",
        json!({
            "viewer_id": host_viewer,
            "api_count": 9,
        }),
    );
    assert_eq!(loaded_after_restart["unfinished_quest_list"], json!([]));
    assert_eq!(
        loaded_after_restart["user_info"]["free_vmoney"],
        free_vmoney_before_continue
    );
    assert_eq!(
        loaded_after_restart["user_info"]["vmoney"],
        vmoney_before_continue
    );
    assert_eq!(
        loaded_after_restart["unfinished_multi_quest_list"],
        json!([{"play_id": "multi-play-1", "continue_count": 1}])
    );

    let mut host_battle = FramedSocket::connect(session_port);
    host_battle.send(json!({
        "socklet": "cooperation_battle",
        "roomNumber": room_number,
        "connectionId": host_connection,
        "reconnected": 1,
    }));
    assert_eq!(host_battle.receive(), json!([0, room_number, ""]));
    host_battle.send(json!([0, [0]]));
    host_battle.expects_no_frame();
    let mut guest_battle = FramedSocket::connect(session_port);
    guest_battle.send(json!({
        "socklet": "cooperation_battle",
        "roomNumber": room_number,
        "connectionId": guest_connection,
        "reconnected": 1,
    }));
    assert_eq!(guest_battle.receive(), json!([0, room_number, ""]));
    let mut superseding_guest = FramedSocket::connect(session_port);
    superseding_guest.send(json!({
        "socklet": "cooperation_battle",
        "roomNumber": room_number,
        "connectionId": guest_connection,
        "reconnected": 1,
    }));
    assert_eq!(superseding_guest.receive(), json!([0, room_number, ""]));
    guest_battle.closes_without_frame();
    host_battle.expects_no_frame();
    drop(superseding_guest);
    assert_eq!(
        host_battle.receive(),
        json!([1, [0, guest_connection.as_str()]])
    );
    assert_eq!(host_battle.receive(), json!([1, [1]]));
    host_battle.send(json!([0, [0]]));
    host_battle.expects_no_frame();

    let mut guest_battle = FramedSocket::connect(session_port);
    guest_battle.send(json!({
        "socklet": "cooperation_battle",
        "roomNumber": room_number,
        "connectionId": guest_connection,
        "reconnected": 1,
    }));
    assert_eq!(guest_battle.receive(), json!([0, room_number, ""]));
    guest_battle.send(json!([0, [0]]));
    host_battle.expects_no_frame();
    assert_eq!(guest_battle.receive(), json!([1, [1]]));

    drop(host_battle);
    assert_eq!(
        guest_battle.receive(),
        json!([1, [0, host_connection.as_str()]])
    );
    let mut host_battle = FramedSocket::connect(session_port);
    host_battle.send(json!({
        "socklet": "cooperation_battle",
        "roomNumber": room_number,
        "connectionId": host_connection,
        "reconnected": 1,
    }));
    assert_eq!(host_battle.receive(), json!([0, room_number, ""]));
    host_battle.send(json!([0, [0]]));
    host_battle.expects_no_frame();
    guest_battle.expects_no_frame();

    host_battle.send(json!([0, [3, 42, 12.5]]));
    let measured = host_battle.receive();
    assert_eq!(measured[0], 1);
    assert_eq!(measured[1][0], 3);
    assert_eq!(measured[1][1], 42);
    assert_eq!(measured[1][2], 12.5);
    host_battle.send(json!([0, [3, 43, 13.5]]));
    let alternate_measurement = host_battle.receive();
    assert_eq!(alternate_measurement[1][0], 3);
    assert_eq!(alternate_measurement[1][1], 43);
    assert_eq!(alternate_measurement[1][2], 13.5);
    host_battle.send(json!([0, [3, 0.25]]));
    host_battle.send(json!([0, [4]]));
    host_battle.send(json!([0, [5]]));
    host_battle.expects_no_frame();

    let messages = json!([[0, 1, 2, 3, 4, "payload"]]);
    host_battle.send(json!([1, messages]));
    let host_broadcast = host_battle.receive();
    let guest_broadcast = guest_battle.receive();
    let expected_broadcast = json!([2, host_connection, messages]);
    assert_eq!(host_broadcast, expected_broadcast);
    assert_eq!(guest_broadcast, expected_broadcast);
    host_battle.expects_no_frame();
    let direct = json!([0, [7, "direct"]]);
    host_battle.send(json!([2, [guest_connection], direct]));
    assert_eq!(guest_battle.receive(), json!([3, host_connection, direct]));
    host_battle.expects_no_frame();

    let mut reconnected_guest = FramedSocket::connect(session_port);
    reconnected_guest.send(json!({
        "socklet": "cooperation_battle",
        "roomNumber": room_number,
        "connectionId": guest_connection,
        "reconnected": 1,
    }));
    assert_eq!(reconnected_guest.receive(), json!([0, room_number, ""]));
    guest_battle.closes_without_frame();
    host_battle.send(json!([0, [3, 44, 14.5]]));
    let after_reconnect = host_battle.receive();
    assert_eq!(after_reconnect[1][0], 3);
    assert_eq!(after_reconnect[1][1], 44);
    reconnected_guest.send(json!([0, [2]]));
    assert_eq!(reconnected_guest.receive(), json!([1, [2]]));
    reconnected_guest.expects_no_frame();

    let mut dropped_guest = FramedSocket::connect(session_port);
    dropped_guest.send(json!({
        "socklet": "cooperation_battle",
        "roomNumber": room_number,
        "connectionId": guest_connection,
        "reconnected": 1,
    }));
    assert_eq!(dropped_guest.receive(), json!([0, room_number, ""]));
    drop(dropped_guest);
    assert_eq!(host_battle.receive(), json!([1, [0, guest_connection]]));
    host_battle.send(json!([0, [2]]));
    assert_eq!(host_battle.receive(), json!([1, [2]]));
    host_battle.expects_no_frame();

    let host_finish = send(
        &service,
        "/api/index.php/multi_battle_quest/finish",
        finish_body_with_statistics(host_viewer, "multi-play-1", 10),
    );
    assert_eq!(host_finish["is_multi"], "multi");
    assert_multi_finish_response(&host_finish, true);
    assert_eq!(host_finish["item_list"]["13"], 3);
    let guest_finish = send(
        &service,
        "/api/index.php/multi_battle_quest/finish",
        finish_body(guest_viewer, "guest-multi-play-1", 10),
    );
    assert_multi_finish_response(&guest_finish, false);
    let snapshots = stored_player_snapshots(root.path());
    let tracked = snapshots
        .iter()
        .find(|snapshot| snapshot["character_leader_multi_clear_counts"]["231001"] == 1)
        .expect("multiplayer statistics are persisted for the reporting player");
    assert_eq!(tracked["character_clear_counts"]["1"], 1);
    assert_eq!(tracked["character_clear_counts"]["161027"], 1);
    assert_eq!(tracked["character_clear_counts"]["231001"], 1);
    assert_eq!(tracked["character_multi_clear_counts"]["1"], 1);
    assert_eq!(tracked["character_leader_clear_counts"]["231001"], 1);
    assert_eq!(tracked["character_leader_power_flip_counts"]["231001"], 5);
    assert_eq!(tracked["party_member_co_clear_counts"]["1_161027"], 1);
    assert_eq!(tracked["party_race_clear_counts"]["Devil+Dragon+Human"], 1);
    assert_eq!(tracked["user_info"]["total_powerflips"], 5);
    assert_eq!(tracked["user_info"]["total_dashes"], 6);
    assert_eq!(tracked["user_info"]["total_skills"], 7);
    let stored_progress = tracked["quest_progress"]["1"]
        .as_array()
        .expect("multiplayer quest progress is stored")
        .iter()
        .find(|progress| progress["quest_id"] == QUEST_ID)
        .expect("finished multiplayer quest progress is stored");
    assert_eq!(stored_progress["leader_character_id"], 231001);
    let loaded_after_finish = send(
        &service,
        "/api/index.php/load",
        json!({"viewer_id": host_viewer, "api_count": 11}),
    );
    let loaded_progress = loaded_after_finish["quest_progress"]["1"]
        .as_array()
        .expect("multiplayer quest progress is returned")
        .iter()
        .find(|progress| progress["quest_id"] == QUEST_ID)
        .expect("finished multiplayer quest progress is returned");
    assert!(loaded_progress
        .as_object()
        .is_some_and(|progress| !progress.contains_key("leader_character_id")));
    assert!(snapshots.iter().any(|snapshot| {
        snapshot["character_multi_clear_counts"].is_null()
            && snapshot["user_info"]["total_powerflips"].is_null()
    }));

    let abort_room = create_room(&service, host_viewer, 12);
    let abort_room_number = abort_room["room_number"].as_str().unwrap().to_owned();
    let (mut abort_lobby, _, _) =
        connect_lobby(session_port, host_viewer, &abort_room_number, "Host", true);
    abort_lobby.send(json!([0, [3, [1]]]));
    assert_eq!(abort_lobby.receive()[1][0], 2);
    abort_lobby.send(json!([0, [6]]));
    assert_eq!(abort_lobby.receive()[1][0], 5);
    send(
        &service,
        "/api/index.php/multi_battle_quest/start",
        start_body(host_viewer, &abort_room_number, "multi-play-abort", 13),
    );
    let active_abort = send(
        &service,
        "/api/index.php/multi_battle_quest/abort",
        json!({
            "viewer_id": host_viewer,
            "play_id": "multi-play-abort",
            "api_count": 14,
        }),
    );
    assert_eq!(active_abort["category_id"], CATEGORY);
    service.stop().expect("service stops cleanly");
}
// //// /验证大厅, COM 和全部战斗帧族 ////

// //// 验证 1.8.x 大厅载荷和枚举索引 [@x380kkm 2026-08-22] ////
#[test]
fn accepts_legacy_lobby_enter_com_and_heartbeat_frames() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = start_service(root.path(), 0);
    let session_port = service.multiplayer_session_port().unwrap();
    let viewer_id = signup(&service, 74);
    let created = create_room(&service, viewer_id, 1);
    let room_number = created["room_number"].as_str().unwrap().to_owned();

    let mut lobby = FramedSocket::connect(session_port);
    lobby.send(json!({
        "socklet": "cooperation_room",
        "viewerId": viewer_id,
        "roomNumber": room_number,
        "questCategory": CATEGORY,
        "questId": QUEST_ID,
        "reconnected": 0,
    }));
    let handshake = lobby.receive();
    let connection_id = handshake[1].as_str().unwrap().to_owned();
    lobby.send(enter_player(viewer_id, &connection_id, "Legacy", true));
    let welcome = lobby.receive();
    assert_eq!(welcome[1][0], 0);
    assert_eq!(welcome[1][1]["viewerId"], viewer_id);
    assert_eq!(welcome[1][1]["connectionId"], connection_id);
    assert_eq!(welcome[1][2].as_array().map(Vec::len), Some(1));

    lobby.send(json!([0, [99]]));
    lobby.expects_no_frame();
    lobby.send(json!([0, [4]]));
    assert_eq!(lobby.receive(), json!([1, [11, connection_id]]));
    let summoned = send(
        &service,
        "/api/index.php/multi_battle_quest/summon",
        json!({
            "category_id": CATEGORY,
            "quest_id": QUEST_ID,
            "room_number": room_number,
            "viewer_id": viewer_id,
            "api_count": 2,
        }),
    );
    assert!(summoned["mate1"].is_object());
    assert!(summoned["mate2"].is_object());
    lobby.send(json!([0, [10, [{"name": "Legacy A"}, {"name": "Legacy B"}]]]));
    let frames = (0..5).map(|_| lobby.receive()).collect::<Vec<_>>();
    assert_eq!(frames[0][1][0], 1);
    assert_eq!(
        frames[1],
        json!([1, [2, format!("{room_number}-npc-1"), [1]]])
    );
    assert_eq!(
        frames[2],
        json!([1, [2, format!("{room_number}-npc-2"), [1]]])
    );
    assert_eq!(frames[3], json!([1, [2, connection_id.as_str(), [1]]]));
    assert_eq!(frames[4], json!([1, [10, 2]]));
    lobby.send(json!([0, [6]]));
    let start_frame = lobby.receive();
    assert_eq!(start_frame[1][0], 5);
    let started_roster = start_frame[1][1]
        .as_array()
        .expect("started lobby roster is present");
    assert_eq!(started_roster.len(), 3);
    assert!(started_roster
        .iter()
        .all(|player| player["state"] == json!([1])));
    assert_ai_battle_player(&started_roster[1]);
    assert_ai_battle_player(&started_roster[2]);
    let started = send(
        &service,
        "/api/index.php/multi_battle_quest/start",
        start_body(viewer_id, &room_number, "solo-ai-play", 3),
    );
    assert_multi_start_response(&started, "solo-ai-play");
    let mut battle = FramedSocket::connect(session_port);
    battle.send(json!({
        "socklet": "cooperation_battle",
        "roomNumber": room_number,
        "connectionId": connection_id,
        "reconnected": 0,
    }));
    assert_eq!(battle.receive(), json!([0, room_number, ""]));
    battle.send(json!([0, [0]]));
    assert_eq!(battle.receive(), json!([1, [1]]));
    battle.send(json!([0, [3, 42, 12.5]]));
    let measurement = battle.receive();
    assert_eq!(measurement[1][0], 3);
    assert_eq!(measurement[1][1], 42);
    battle.send(json!([0, [4, 0.25]]));
    battle.send(json!([0, [5]]));
    battle.expects_no_frame();
    battle.send(json!([0, [2]]));
    assert_eq!(battle.receive(), json!([1, [2]]));
    battle.expects_no_frame();
    let finished = send(
        &service,
        "/api/index.php/multi_battle_quest/finish",
        finish_body(viewer_id, "solo-ai-play", 4),
    );
    assert_eq!(finished["is_multi"], "multi");
    assert_eq!(finished["host_finished"], true);
    service.stop().expect("service stops cleanly");
}
// //// /验证 1.8.x 大厅载荷和枚举索引 ////

// //// 验证 HTTP 缺房轮询, 社交路由, 解散和重启恢复 [@x380kkm 2026-08-22] ////
#[test]
fn returns_http_polling_shapes_and_persists_frozen_ai() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = start_service(root.path(), 0);
    let session_port = service.multiplayer_session_port().unwrap();
    let viewer_id = signup(&service, 73);
    let outsider_viewer = signup(&service, 75);
    let missing_room = "999999";

    let searched = send(
        &service,
        "/api/index.php/multi_battle_quest/search_room",
        json!({"viewer_id": viewer_id, "room_number": missing_room, "api_count": 1}),
    );
    assert_eq!(searched["room_exists"], false);
    assert_eq!(searched["room_number"], missing_room);
    let selected = send(
        &service,
        "/api/index.php/multi_battle_quest/select_room",
        json!({
            "category": CATEGORY,
            "quest_id": QUEST_ID,
            "party_id": 1,
            "accepted_type": 0,
            "viewer_id": viewer_id,
            "room_number": missing_room,
            "api_count": 2,
        }),
    );
    assert_eq!(selected["raising_state"], 9);
    let prepared = send(
        &service,
        "/api/index.php/multi_battle_quest/prepare",
        json!({
            "category": CATEGORY,
            "quest_id": QUEST_ID,
            "viewer_id": viewer_id,
            "room_number": missing_room,
            "api_count": 3,
        }),
    );
    assert_eq!(prepared["raising_state"], 9);
    let restored_missing = send(
        &service,
        "/api/index.php/multi_battle_quest/restore_room",
        json!({
            "viewer_id": viewer_id,
            "room_number": missing_room,
            "room_sequence": 1,
            "api_count": 4
        }),
    );
    assert_eq!(restored_missing["raising_state"], 9);
    assert_eq!(restored_missing["is_same_room"], true);

    let created = create_room(&service, viewer_id, 5);
    let room_number = created["room_number"].as_str().unwrap().to_owned();
    let empty_list = send(
        &service,
        "/api/index.php/multi_battle_quest/get_rooms",
        json!({"viewer_id": viewer_id, "category_id": CATEGORY, "api_count": 6}),
    );
    assert!(empty_list["rooms"].as_array().unwrap().is_empty());
    let verified = send(
        &service,
        "/api/index.php/multi_battle_quest/verify_access_token",
        json!({"viewer_id": viewer_id, "access_token": created["access_token"], "api_count": 7}),
    );
    assert_eq!(verified, json!({"is_valid": true}));
    let missing_token = send(
        &service,
        "/api/index.php/multi_battle_quest/verify_access_token",
        json!({"viewer_id": viewer_id, "access_token": "missing", "api_count": 8}),
    );
    assert_eq!(missing_token, json!({"is_valid": true}));
    for path in ["micro_community", "publish_room", "share_room"] {
        let data = send(
            &service,
            &format!("/api/index.php/multi_battle_quest/{path}"),
            json!({"viewer_id": viewer_id, "room_number": room_number, "api_count": 9}),
        );
        assert!(data.as_object().is_some_and(Map::is_empty));
    }
    let prepared_room = send(
        &service,
        "/api/index.php/multi_battle_quest/prepare",
        json!({
            "category": CATEGORY,
            "quest_id": QUEST_ID,
            "viewer_id": viewer_id,
            "room_number": room_number,
            "api_count": 10,
        }),
    );
    let room_sequence = prepared_room["room_sequence"].as_i64().unwrap();
    let stale = send(
        &service,
        "/api/index.php/multi_battle_quest/restore_room",
        json!({
            "viewer_id": viewer_id,
            "room_number": room_number,
            "room_sequence": room_sequence + 1,
            "api_count": 10,
        }),
    );
    assert_eq!(stale["raising_state"], 10);
    assert!(stale["ip_address"].as_str().is_some());
    assert!(stale["port"].as_i64().is_some());
    assert!(stale["share_room_options"].as_i64().is_some());
    let not_mate = send(
        &service,
        "/api/index.php/multi_battle_quest/restore_room",
        json!({
            "viewer_id": outsider_viewer,
            "room_number": room_number,
            "room_sequence": room_sequence,
            "api_count": 1,
        }),
    );
    assert_eq!(not_mate["raising_state"], 13);
    assert_eq!(not_mate["is_same_room"], false);
    assert!(not_mate["ip_address"].as_str().is_some());
    assert!(not_mate["port"].as_i64().is_some());
    assert!(not_mate["share_room_options"].as_i64().is_some());
    let summoned = send(
        &service,
        "/api/index.php/multi_battle_quest/summon",
        json!({
            "category_id": CATEGORY,
            "quest_id": QUEST_ID,
            "room_number": room_number,
            "viewer_id": viewer_id,
            "api_count": 10,
        }),
    );
    assert!(summoned["mate1"].is_object());
    assert!(summoned["mate2"].is_object());
    let (mut original_lobby, _, _) =
        connect_lobby(session_port, viewer_id, &room_number, "Host", true);
    service.stop().expect("service stops cleanly");
    original_lobby.closes_without_frame();

    let restarted = start_service(root.path(), session_port);
    let restored = send(
        &restarted,
        "/api/index.php/multi_battle_quest/restore_room",
        json!({
            "viewer_id": viewer_id,
            "room_number": room_number,
            "room_sequence": room_sequence,
            "api_count": 11
        }),
    );
    assert_eq!(restored["room_number"], room_number);
    assert_eq!(restored["raising_state"], 1);
    assert_eq!(restored["port"], session_port);
    let (mut restored_lobby, _, _) =
        connect_lobby(session_port, viewer_id, &room_number, "Host", true);
    let summoned_again = send(
        &restarted,
        "/api/index.php/multi_battle_quest/summon",
        json!({
            "category_id": CATEGORY,
            "quest_id": QUEST_ID,
            "room_number": room_number,
            "viewer_id": viewer_id,
            "api_count": 12,
        }),
    );
    assert_eq!(summoned_again, summoned);
    let disbanded = send(
        &restarted,
        "/api/index.php/multi_battle_quest/disband_room",
        json!({"viewer_id": viewer_id, "room_number": room_number, "api_count": 13}),
    );
    assert!(disbanded.as_object().is_some_and(Map::is_empty));
    assert_eq!(
        restored_lobby.receive(),
        json!([1, [6, "multibattle_room_dismissed"]])
    );
    restored_lobby.closes_without_frame();
    restarted.stop().expect("service stops cleanly");

    let occupied = TcpListener::bind("127.0.0.1:0").expect("session port is reserved");
    let occupied_port = occupied.local_addr().unwrap().port();
    let blocked_root = TempDir::new().expect("blocked service directory is created");
    let blocked = PersonalService::start_with_options(
        PersonalServiceOptions::new(
            blocked_root.path(),
            0,
            blocked_root.path().join("cdn").join("cn"),
        )
        .with_multiplayer_session_port(occupied_port),
    );
    assert!(blocked.is_err());
}
// //// /验证 HTTP 缺房轮询, 社交路由, 解散和重启恢复 ////
