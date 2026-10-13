// audience: internal
// # personal-service-ai-team-source-tests
// 隔离实例验证所选存档在建房, 救援和来源切换中的两个不可变队伍快照.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[path = "support/local_saves.rs"]
#[allow(dead_code)]
mod local_save_support;
#[allow(dead_code)]
mod support;

use local_save_support::{
    assert_status, authorized_request, list_local_saves, response_body, signup,
    update_slot_player_snapshot,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use starpoint_personal_service::{PersonalService, PersonalServiceOptions};
use std::path::Path;
use tempfile::TempDir;

const SOURCE_PATH: &str = "/v1/local-saves/ai-team-source";

fn start_service(root: &Path) -> PersonalService {
    PersonalService::start_with_options(
        PersonalServiceOptions::new(root, 0, root.join("cdn/cn")).with_multiplayer_session_port(0),
    )
    .unwrap()
}

fn manage(service: &PersonalService, method: &str, path: &str, body: Option<Value>) -> Value {
    let response = authorized_request(service, method, path, body.as_ref());
    assert_status(&response, "200 OK");
    response_body(&response)
}

fn play(service: &PersonalService, route: &str, body: Value) -> Value {
    let response = cn_support::send_request(
        service.port(),
        &format!("/api/index.php/{route}"),
        &cn_support::encode_request(&body),
    );
    assert_status(&response, "200 OK");
    let envelope = cn_support::decode_response::<Value>(&response);
    assert_eq!(
        envelope.data_headers.result_code, 1,
        "{route}: {}",
        envelope.data
    );
    envelope.data
}

fn copy_save(service: &PersonalService, slot_id: i64) -> i64 {
    let response = authorized_request(
        service,
        "POST",
        &format!("/v1/local-saves/{slot_id}/copy"),
        Some(&json!({"name":"AI source"})),
    );
    assert_status(&response, "201 Created");
    response_body(&response)["id"].as_i64().unwrap()
}

fn select_source(service: &PersonalService, slot_id: i64) -> Value {
    manage(
        service,
        "PUT",
        &format!("/v1/local-saves/{slot_id}/ai-teams"),
        Some(json!({"party_ids":[2,1]})),
    )
}

fn summon(service: &PersonalService, viewer: i64, room: &str) -> Value {
    play(
        service,
        "multi_battle_quest/summon",
        json!({"viewer_id":viewer,"category_id":1,"quest_id":1001002,"room_number":room}),
    )
}

fn create_room(service: &PersonalService, viewer: i64) -> String {
    play(
        service,
        "multi_battle_quest/create_room",
        json!({"viewer_id":viewer,"category":1,"quest_id":1001002,"party_id":1}),
    )["room_number"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn frozen_mates(db: &Connection, room: &str) -> Vec<Value> {
    db.prepare(
        "SELECT lobby_player_json FROM multiplayer_ai_mates WHERE room_number=?1 ORDER BY position",
    )
    .unwrap()
    .query_map(params![room], |row| row.get::<_, String>(0))
    .unwrap()
    .map(|row| serde_json::from_str(&row.unwrap()).unwrap())
    .collect()
}

fn parties(db: &Connection) -> Vec<(i64, String, String)> {
    db.prepare("SELECT account_id,json_extract(data_json,'$.user_party_group_list'),json_extract(data_json,'$.user_character_list') FROM player_snapshots ORDER BY account_id").unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap()
        .collect::<Result<_, _>>().unwrap()
}

fn assert_source(mates: &[Value], slot: i64, party_ids: [i64; 2]) {
    assert_eq!(mates.len(), 2);
    for (mate, party_id) in mates.iter().zip(party_ids) {
        assert_eq!(mate["sourceSlotId"], slot);
        assert_eq!(mate["sourcePartyId"], party_id);
        assert!(mate["snapshotId"].is_string());
    }
}

// //// 所选非当前存档供给两队并在切换和删除时保留房间快照 [@x380kkm 2026-09-29] ////
#[test]
fn uses_selected_save_and_falls_back_after_deletion() {
    let root = TempDir::new().unwrap();
    let service = start_service(root.path());
    let viewer = signup(service.port(), 932901).data_headers.viewer_id;
    let current = list_local_saves(&service)["slots"][0]["id"]
        .as_i64()
        .unwrap();
    let source = copy_save(&service, current);
    update_slot_player_snapshot(root.path(), source, |data| {
        data["user_character_list"]["231001"] = data["user_character_list"]["1"].clone();
        data["user_character_list"]["231001"]["exp"] = json!(777);
        for party in ["1", "2"] {
            data["user_party_group_list"]["1"]["list"][party]["character_ids"] =
                json!([231001, null, null]);
        }
    });
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    assert_eq!(
        manage(&service, "GET", SOURCE_PATH, None),
        json!({"slot_id":null,"mode":"current"})
    );
    let current_room = create_room(&service, viewer);
    summon(&service, viewer, &current_room);
    assert_source(&frozen_mates(&db, &current_room), current, [1, 2]);
    play(
        &service,
        "multi_battle_quest/disband_room",
        json!({"viewer_id":viewer,"room_number":current_room}),
    );

    let expected_parties = parties(&db);
    let selection = select_source(&service, source);
    assert_eq!(
        selection["source"],
        json!({"slot_id":source,"mode":"selected"})
    );
    assert_eq!(
        selection["teams"][0]["data"]["characters"]["231001"]["exp"],
        777
    );
    assert_eq!(parties(&db), expected_parties);
    assert_eq!(
        list_local_saves(&service)["devices"][0]["active_slot_id"],
        current
    );
    service.stop().unwrap();
    let service = start_service(root.path());
    let viewer = signup(service.port(), 932901).data_headers.viewer_id;
    assert_eq!(
        manage(&service, "GET", SOURCE_PATH, None)["slot_id"],
        source
    );
    let room = create_room(&service, viewer);
    let mates = summon(&service, viewer, &room);
    let frozen = frozen_mates(&db, &room);
    assert_source(&frozen, source, [2, 1]);
    assert_eq!(mates["mate1"]["party"]["characters"][0]["exp"], 777);
    assert_eq!(mates["mate2"]["party"]["characters"][0]["exp"], 777);
    assert_eq!(parties(&db), expected_parties);

    let blocked_delete = authorized_request(
        &service,
        "DELETE",
        &format!("/v1/local-saves/{source}"),
        Some(&json!({"confirm_name":"AI source"})),
    );
    assert_status(&blocked_delete, "409 Conflict");
    assert_eq!(response_body(&blocked_delete)["error"], "local_save_busy");
    manage(&service, "DELETE", SOURCE_PATH, None);
    assert_eq!(summon(&service, viewer, &room), mates);
    assert_eq!(frozen_mates(&db, &room), frozen);
    play(
        &service,
        "multi_battle_quest/disband_room",
        json!({"viewer_id":viewer,"room_number":room}),
    );
    select_source(&service, source);
    manage(
        &service,
        "DELETE",
        &format!("/v1/local-saves/{source}"),
        Some(json!({"confirm_name":"AI source"})),
    );
    assert_eq!(
        manage(&service, "GET", SOURCE_PATH, None),
        json!({"slot_id":null,"mode":"current"})
    );
    let room = create_room(&service, viewer);
    summon(&service, viewer, &room);
    assert_source(&frozen_mates(&db, &room), current, [1, 2]);
    service.stop().unwrap();
}

// //// 救援房主和队友共同读取管理页选择的存档快照 [@x380kkm 2026-09-29] ////
#[test]
fn selected_save_supplies_rescue_host_and_mate() {
    let root = TempDir::new().unwrap();
    let service = start_service(root.path());
    let viewer = signup(service.port(), 932902).data_headers.viewer_id;
    play(
        &service,
        "load",
        json!({"viewer_id":viewer,"keychain":viewer}),
    );
    let current = list_local_saves(&service)["slots"][0]["id"]
        .as_i64()
        .unwrap();
    let source = copy_save(&service, current);
    select_source(&service, source);
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    db.execute("UPDATE player_snapshots SET data_json=json_insert(data_json, '$.quest_progress.\"1\"[#]', json('{\"quest_id\":1008004,\"finished\":true}'))", []).unwrap();
    let expected_parties = parties(&db);
    manage(
        &service,
        "PUT",
        "/v1/rescue-settings",
        Some(
            json!({"enabled":true,"spawn_interval_seconds":60,"room_lifetime_seconds":180,"max_rooms":1,"quests":[{"category_id":2,"quest_id":1001001}]}),
        ),
    );
    let offer = play(
        &service,
        "attention/check",
        json!({"viewer_id":viewer,"request_number":3,"holding_number":0}),
    )["multi"][0]
        .clone();
    let room = offer["quest_info"]["room_number"].as_str().unwrap();
    assert_source(&frozen_mates(&db, room), source, [2, 1]);
    assert_eq!(parties(&db), expected_parties);
    service.stop().unwrap();
}

// //// 来源写入失败时回滚两个编队快照和当前指针 [@x380kkm 2026-09-29] ////
#[test]
fn rolls_back_snapshots_when_source_selection_cannot_be_saved() {
    let root = TempDir::new().unwrap();
    let service = start_service(root.path());
    signup(service.port(), 932903);
    let current = list_local_saves(&service)["slots"][0]["id"]
        .as_i64()
        .unwrap();
    let source = copy_save(&service, current);
    select_source(&service, current);
    let path = format!("/v1/local-saves/{source}/ai-teams");
    let before = manage(&service, "GET", &path, None)["teams"].clone();
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let count: i64 = db
        .query_row("SELECT count(*) FROM ai_team_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    db.execute_batch("CREATE TRIGGER block_ai_source BEFORE UPDATE ON ai_team_source BEGIN SELECT RAISE(ABORT, 'blocked source'); END;").unwrap();
    let response = authorized_request(&service, "PUT", &path, Some(&json!({"party_ids":[2,1]})));
    assert_status(&response, "500 Internal Server Error");
    assert_eq!(
        manage(&service, "GET", SOURCE_PATH, None)["slot_id"],
        current
    );
    assert_eq!(manage(&service, "GET", &path, None)["teams"], before);
    assert_eq!(
        db.query_row("SELECT count(*) FROM ai_team_snapshots", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        count
    );
    service.stop().unwrap();
}
