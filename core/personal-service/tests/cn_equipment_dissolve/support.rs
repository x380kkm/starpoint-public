// audience: internal
// # personal-service-cn-equipment-dissolve-test-support
// 隔离存档通过真实奖励投递准备装备, 数据库快照覆盖资产, 任务与领取记录.

use super::{cn_support, support};
use cn_support::{decode_response, encode_request, SignupData};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;

pub const FIRST: i64 = 5_030_037;
pub const SECOND: i64 = 5_040_028;
pub const FIXED: i64 = 100_001;

// //// 准备玩家和装备并读取实际保存状态 [@x380kkm 2026-09-23] ////
pub fn send(service: &PersonalService, route: &str, body: &Value) -> String {
    cn_support::send_request(
        service.port(),
        &format!("/api/index.php/{route}"),
        &encode_request(body),
    )
}

pub fn data(service: &PersonalService, route: &str, body: &Value) -> Value {
    let response = send(service, route, body);
    assert!(response.starts_with("HTTP/1.1 200"), "{route}: {response}");
    decode_response::<Value>(&response).data
}

pub fn signup(service: &PersonalService, device: i64) -> i64 {
    decode_response::<SignupData>(&send(service, "tool/signup", &json!({"device_id":device})))
        .data_headers
        .viewer_id
}

pub fn grant(service: &PersonalService, viewer: i64, equipment: Value, items: Value) {
    let token = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        "POST",
        "/v1/mails",
        "application/json",
        &[("Authorization", token.as_str())],
        json!({
            "viewer_id":viewer, "title":"装备", "body":"分解素材.", "sender":"Starpoint",
            "rewards":{"equipmentList":equipment,"itemList":items},
        })
        .to_string()
        .as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 201"), "{response}");
    data(service, "mail/receive_all", &json!({"viewer_id":viewer}));
}

pub fn prepare(service: &PersonalService, device: i64, equipment: Value) -> i64 {
    let viewer = signup(service, device);
    data(
        service,
        "load",
        &json!({"viewer_id":viewer,"keychain":viewer}),
    );
    grant(service, viewer, equipment, json!({}));
    viewer
}

pub fn database(root: &Path) -> Connection {
    Connection::open(root.join("personal-service.sqlite3")).unwrap()
}

pub fn player(root: &Path) -> Value {
    let document: String = database(root)
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    serde_json::from_str(&document).unwrap()
}

pub fn edit_player(root: &Path, edit: impl FnOnce(&mut Value)) {
    let mut document = player(root);
    edit(&mut document);
    database(root)
        .execute(
            "UPDATE player_snapshots SET data_json = ?1",
            [document.to_string()],
        )
        .unwrap();
}

pub fn state(root: &Path) -> Value {
    let db = database(root);
    let mut query = db.prepare("SELECT category,mission_id,value FROM player_mission_progress ORDER BY category,mission_id").unwrap();
    let progress = query
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
    let mut query = db
        .prepare("SELECT pattern,value FROM player_mission_counters ORDER BY pattern")
        .unwrap();
    let counters = query
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
    json!({"player":player(root),"progress":progress,"counters":counters,"history":history})
}

pub fn history_amount(root: &Path, kind: i64, item_id: Option<i64>, action: &str) -> i64 {
    database(root).query_row(
        "SELECT COALESCE(SUM(number),0) FROM player_receive_history WHERE kind=?1 AND type_id IS ?2 AND event_key LIKE ?3",
        rusqlite::params![kind,item_id,format!("equipment:{action}:%")], |row| row.get(0),
    ).unwrap()
}

pub fn request(viewer: i64, equipment: i64, count: Option<i64>, api_count: i64) -> Value {
    json!({"viewer_id":viewer,"api_count":api_count,"equipment_list":[{"equipment_id":equipment,"number":count}]})
}

pub fn assert_rejected(
    service: &PersonalService,
    root: &Path,
    route: &str,
    request: &Value,
    code: &str,
) {
    let before = state(root);
    let response = send(service, route, request);
    assert!(
        response.starts_with("HTTP/1.1 400") && response.contains(code),
        "{response}"
    );
    assert_eq!(state(root), before);
}
// //// /准备玩家和装备并读取实际保存状态 ////
