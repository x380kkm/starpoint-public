// audience: internal
// # personal-service-cn-battle-atomicity-tests
// 战斗最终收据写入失败时, 资产, 任务进度, 领取历史和活动成绩保持入场后的状态.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

// //// 读取隔离账号的战斗资产和任务持久化状态 [@x380kkm 2026-09-22] ////
fn send(service: &PersonalService, route: &str, body: &Value) -> String {
    cn_support::send_request(
        service.port(),
        &format!("/api/index.php/{route}"),
        &encode_request(body),
    )
}

fn data(service: &PersonalService, route: &str, body: &Value) -> Value {
    decode_response::<Value>(&send(service, route, body)).data
}

fn database(root: &Path) -> Connection {
    Connection::open(root.join("personal-service.sqlite3")).unwrap()
}

fn state(root: &Path) -> Value {
    let db = database(root);
    let snapshot: String = db
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    let mut statement = db.prepare("SELECT category,mission_id,value FROM player_mission_progress ORDER BY category,mission_id").unwrap();
    let progress = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let mut statement = db
        .prepare("SELECT pattern,value FROM player_mission_counters ORDER BY pattern")
        .unwrap();
    let counters = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let history: i64 = db
        .query_row("SELECT COUNT(*) FROM player_receive_history", [], |row| {
            row.get(0)
        })
        .unwrap();
    let active: i64 = db
        .query_row("SELECT COUNT(*) FROM active_single_quests", [], |row| {
            row.get(0)
        })
        .unwrap();
    json!({"player":serde_json::from_str::<Value>(&snapshot).unwrap(),"progress":progress,"counters":counters,"history":history,"active":active})
}

fn finish_request(viewer: i64, category: i64, quest: i64) -> Value {
    json!({"viewer_id":viewer,"category":category,"quest_id":quest,"play_id":"atomic-battle","api_count":2,
        "elapsed_time_ms":1000,"score":1000,"add_mana":7,"is_accomplished":true,"is_lose":false,
        "statistics":{"clear_phase":1,"is_host":true,"max_combo_count":500,"zones":[{"use_power_flip_count":2}],
            "party":{"characters":[{"id":1},null,null],"unison_characters":[null,null,null],
                "equipments":[null,null,null],"ability_soul_ids":[null,null,null]}}})
}

fn start_request(viewer: i64, category: i64, quest: i64) -> Value {
    json!({"viewer_id":viewer,"category":category,"quest_id":quest,"play_id":"atomic-battle","api_count":1,
        "party_id":1,"is_auto_start_mode":false,"use_boost_point":false,"use_boss_boost_point":false})
}
// //// /读取隔离账号的战斗资产和任务持久化状态 ////

// //// 最终单机收据失败时回滚任务和奖励并允许原战斗重试 [@x380kkm 2026-09-22] ////
#[test]
fn single_finish_rolls_back_all_rewards_before_retry() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer =
        decode_response::<SignupData>(&send(&service, "tool/signup", &json!({"device_id":922511})))
            .data_headers
            .viewer_id;
    data(
        &service,
        "single_battle_quest/start",
        &start_request(viewer, 1, 1002002),
    );
    let before = state(root.path());
    database(root.path())
        .execute_batch(
            "CREATE TRIGGER reject_battle_receipt BEFORE INSERT ON single_battle_finish_receipts
        BEGIN SELECT RAISE(ABORT, 'receipt unavailable'); END;",
        )
        .unwrap();
    let request = finish_request(viewer, 1, 1002002);
    let response = send(&service, "single_battle_quest/finish", &request);
    assert!(response.starts_with("HTTP/1.1 500"), "{response}");
    assert_eq!(state(root.path()), before);
    database(root.path())
        .execute_batch("DROP TRIGGER reject_battle_receipt;")
        .unwrap();
    let response = data(&service, "single_battle_quest/finish", &request);
    let settled = state(root.path());
    assert_eq!(settled["active"], 0);
    assert!(settled["history"].as_i64().unwrap() > before["history"].as_i64().unwrap());
    assert!(
        settled["player"]["user_info"]["free_vmoney"]
            .as_i64()
            .unwrap()
            > before["player"]["user_info"]["free_vmoney"]
                .as_i64()
                .unwrap()
    );
    assert_eq!(
        data(&service, "single_battle_quest/finish", &request),
        response
    );
    assert_eq!(state(root.path()), settled);
    service.stop().unwrap();
}
// //// /最终单机收据失败时回滚任务和奖励并允许原战斗重试 ////

// //// 最终联机收据失败时保留房间战斗并回滚任务奖励 [@x380kkm 2026-09-22] ////
#[test]
fn multi_finish_rolls_back_all_rewards_before_retry() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer =
        decode_response::<SignupData>(&send(&service, "tool/signup", &json!({"device_id":922512})))
            .data_headers
            .viewer_id;
    database(root.path())
        .execute(
            "UPDATE player_snapshots SET data_json=json_set(data_json,'$.user_info.stamina',100)",
            [],
        )
        .unwrap();
    let room = data(
        &service,
        "multi_battle_quest/create_room",
        &json!({"viewer_id":viewer,"category":2,"quest_id":1002001,"party_id":1}),
    );
    let mut start = start_request(viewer, 2, 1002001);
    start["room_number"] = room["room_number"].clone();
    data(&service, "multi_battle_quest/start", &start);
    let before = state(root.path());
    database(root.path()).execute_batch("CREATE TRIGGER reject_battle_receipt BEFORE INSERT ON multiplayer_battle_action_receipts
        WHEN NEW.action='finish' BEGIN SELECT RAISE(ABORT, 'receipt unavailable'); END;").unwrap();
    let mut request = finish_request(viewer, 2, 1002001);
    request["score"] = json!(4_294_967_296.0);
    request["contribution_score"] = json!(4_294_967_297.0);
    request["mate_player_result"] = json!([{
        "viewer_id": 4_294_967_298.0, "com_id": null,
        "score": 4_294_967_296.0, "contribution_score": 4_294_967_297.0
    }]);
    let response = send(&service, "multi_battle_quest/finish", &request);
    assert!(response.starts_with("HTTP/1.1 500"), "{response}");
    assert_eq!(state(root.path()), before);
    let participants: i64 = database(root.path())
        .query_row(
            "SELECT COUNT(*) FROM multiplayer_battle_players",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(participants, 1);
    database(root.path())
        .execute_batch("DROP TRIGGER reject_battle_receipt;")
        .unwrap();
    let response = data(&service, "multi_battle_quest/finish", &request);
    assert_eq!(response["contribution_score"], 4_294_967_297_i64);
    assert_eq!(response["mate_player_result"], request["mate_player_result"]);
    let settled = state(root.path());
    assert_eq!(settled["active"], 0);
    assert!(settled["history"].as_i64().unwrap() > before["history"].as_i64().unwrap());
    assert_eq!(
        data(&service, "multi_battle_quest/finish", &request),
        response
    );
    assert_eq!(state(root.path()), settled);
    service.stop().unwrap();
}
// //// /最终联机收据失败时保留房间战斗并回滚任务奖励 ////
