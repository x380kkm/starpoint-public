// audience: internal
// # personal-service-cn-receive-history-tests
// 领取历史按玩家账号分页, 同秒记录保持确定顺序, 较早记录和重启后的页面均可访问.

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

// //// 创建独立玩家并取得会话 [@x380kkm 2026-09-23] ////
fn signup(service: &PersonalService, device_id: i64) -> i64 {
    decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id }),
    ))
    .data_headers
    .viewer_id
}
// //// /创建独立玩家并取得会话 ////

// //// 读取指定账号的领取记录页 [@x380kkm 2026-09-23] ////
fn read_page(service: &PersonalService, viewer: i64, page: i64) -> Value {
    decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/history/receive",
        &encode_request(&json!({"viewer_id": viewer, "page": page})),
    ))
    .data
}
// //// /读取指定账号的领取记录页 ////

// //// 建立跨页记录及另一账号的领取记录 [@x380kkm 2026-09-23] ////
fn seed_history(database: &mut Connection, viewer: i64, other: i64) {
    let transaction = database.transaction().unwrap();
    let account: i64 = transaction
        .query_row(
            "SELECT account_id FROM sessions WHERE token=?1",
            params![viewer.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    let other_account: i64 = transaction
        .query_row(
            "SELECT account_id FROM sessions WHERE token=?1",
            params![other.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    for number in 1..=502 {
        let time = if number == 502 {
            1_700_000_000
        } else {
            1_700_000_060
        };
        transaction
            .execute(
                "INSERT INTO player_receive_history
             (account_id, event_key, entry_index, kind, type_id, number, created_at)
             VALUES (?1, ?2, 0, 1, 106, ?3, ?4)",
                params![account, format!("history:{number}"), number, time],
            )
            .unwrap();
    }
    transaction
        .execute(
            "INSERT INTO player_receive_history
         (account_id, event_key, entry_index, kind, type_id, number, created_at)
         VALUES (?1, 'other-account', 0, 1, 106, 9000, 1700000120)",
            params![other_account],
        )
        .unwrap();
    transaction.commit().unwrap();
}
// //// /建立跨页记录及另一账号的领取记录 ////

// //// 跨过五百条历史并保持分页排序和账号隔离 [@x380kkm 2026-09-23] ////
#[test]
fn paginates_all_claims_with_stable_order_account_isolation_and_restart() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 993);
    let other = signup(&service, 994);
    let mut database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    seed_history(&mut database, viewer, other);

    let mut numbers = Vec::new();
    for page in 1..=6 {
        let result = read_page(&service, viewer, page);
        assert_eq!(result["total_count"], 502);
        let history = result["history"].as_array().unwrap();
        assert_eq!(history.len(), if page == 6 { 2 } else { 100 });
        numbers.extend(
            history
                .iter()
                .map(|entry| entry["number"].as_i64().unwrap()),
        );
    }
    let expected = (1..=501).rev().chain([502]).collect::<Vec<_>>();
    assert_eq!(numbers, expected);
    assert_eq!(
        read_page(&service, viewer, 7),
        json!({"history": [], "total_count": 502})
    );
    let other_page = read_page(&service, other, 1);
    assert_eq!(other_page["total_count"], 1);
    assert_eq!(other_page["history"][0]["number"], 9000);
    let first = read_page(&service, viewer, 1);
    let default_page = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/history/receive",
        &encode_request(&json!({"viewer_id": viewer})),
    ))
    .data;
    assert_eq!(default_page, first);
    let last = read_page(&service, viewer, 6);
    service.stop().unwrap();

    let service = PersonalService::start(root.path(), 0).unwrap();
    assert_eq!(read_page(&service, viewer, 1), first);
    assert_eq!(read_page(&service, viewer, 6), last);
    assert_eq!(read_page(&service, other, 1), other_page);
    service.stop().unwrap();
}
// //// /跨过五百条历史并保持分页排序和账号隔离 ////

// //// 无效页码保持领取记录可继续读取 [@x380kkm 2026-09-23] ////
#[test]
fn rejects_invalid_page_offsets_without_disrupting_valid_reads() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 995);
    for page in [0, -1, i64::MAX] {
        let response = cn_support::send_request(
            service.port(),
            "/api/index.php/history/receive",
            &encode_request(&json!({"viewer_id": viewer, "page": page})),
        );
        assert!(
            response.starts_with("HTTP/1.1 400 Bad Request"),
            "{response}"
        );
    }
    assert_eq!(
        read_page(&service, viewer, 1),
        json!({"history": [], "total_count": 0})
    );
    service.stop().unwrap();
}
// //// /无效页码保持领取记录可继续读取 ////
