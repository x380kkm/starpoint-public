// audience: internal
// # personal-service-cn-mail-rank-tests
// 段位邮件在跨段时恢复体力, 领取失败与重复请求保持资产和领取状态一致.

use super::*;
use rusqlite::{params, Connection};
use serde_json::json;

fn snapshot(database: &Connection) -> Value {
    let data: String = database
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    serde_json::from_str(&data).unwrap()
}

fn create_rank_mail(service: &PersonalService, viewer: i64, amount: i64) -> i64 {
    let token = format!("Bearer {}", service.management_token());
    let body = json!({"viewer_id":viewer,"title":"段位奖励","body":"段位点数","sender":"管理员","rewards":{"rankPoint":amount}});
    let response = request_with_headers(
        service.port(),
        "POST",
        "/v1/mails",
        "application/json",
        &[("Authorization", &token)],
        body.to_string().as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 201"), "{response}");
    response_json(&response)["id"].as_i64().unwrap()
}

// //// 邮件段位奖励同时更新体力并在失败后完整重试 [@x380kkm 2026-09-23] ////
#[test]
fn grants_rank_up_stamina_once_and_preserves_it_across_restart() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 992 }),
    ))
    .data_headers
    .viewer_id;
    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            viewer_id: viewer,
            keychain: viewer,
        }),
    ));
    let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let mut before = snapshot(&database);
    before["user_info"]["rank_point"] = json!(10);
    before["user_info"]["stamina"] = json!(4);
    before["user_info"]["stamina_heal_time"] = json!(loaded.data_headers.servertime - 600);
    database
        .execute(
            "UPDATE player_snapshots SET data_json=?1",
            params![before.to_string()],
        )
        .unwrap();
    let mail_id = create_rank_mail(&service, viewer, 31);
    let body = encode_request(&MailReceiveRequest {
        viewer_id: viewer,
        mail_id,
    });
    database
        .execute_batch(
            "CREATE TRIGGER reject_mail_history BEFORE INSERT ON player_receive_history
        BEGIN SELECT RAISE(ABORT, 'history unavailable'); END;",
        )
        .unwrap();
    let failed = cn_support::send_request(service.port(), "/api/index.php/mail/receive", &body);
    assert!(failed.starts_with("HTTP/1.1 500"), "{failed}");
    assert_eq!(snapshot(&database), before);
    database
        .execute_batch("DROP TRIGGER reject_mail_history;")
        .unwrap();
    let received = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive",
        &body,
    ));
    assert_eq!(received.data["user_info"]["rank_point"], 41);
    assert_eq!(received.data["user_info"]["stamina"], 30);
    let healed_at = received.data["user_info"]["stamina_heal_time"]
        .as_i64()
        .unwrap();
    assert!(
        (loaded.data_headers.servertime..=received.data_headers.servertime).contains(&healed_at)
    );
    let after = snapshot(&database);
    let repeated = cn_support::send_request(service.port(), "/api/index.php/mail/receive", &body);
    let repeated = decode_response::<Value>(&repeated);
    assert_eq!(repeated.data_headers.result_code, 2002);
    assert_eq!(snapshot(&database), after);
    let small = create_rank_mail(&service, viewer, 1);
    let batch = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive_all",
        &encode_request(&MailReceiveAllRequest {
            viewer_id: viewer,
            mail_ids: vec![small],
        }),
    ));
    assert_eq!(batch.data["user_info"]["rank_point"], 42);
    let after_batch = snapshot(&database);
    assert_eq!(
        after_batch["user_info"]["stamina"],
        after["user_info"]["stamina"]
    );
    assert_eq!(
        after_batch["user_info"]["stamina_heal_time"],
        after["user_info"]["stamina_heal_time"]
    );
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let reloaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            viewer_id: viewer,
            keychain: viewer,
        }),
    ));
    assert_eq!(reloaded.data["user_info"]["rank_point"], 42);
    assert_eq!(reloaded.data["user_info"]["stamina"], 30);
    service.stop().unwrap();
}
