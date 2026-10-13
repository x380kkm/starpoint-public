// audience: internal
// # personal-service-cn-mail-claim-results
//
// 邮件领取结果区分实际发奖, 领取期限, 已领取记录和缺失邮件.

use super::*;
use rusqlite::{params, Connection};
use serde_json::json;

// //// 分批领取混合奖励并保留原邮件余量和领取记录 [@x380kkm 2026-09-28] ////
#[test]
fn retains_only_excess_rewards_and_claims_the_remainder_after_restart() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 26 }),
    ))
    .data_headers
    .viewer_id;
    let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let read_snapshot = || {
        let value: String = database
            .query_row("SELECT data_json FROM player_snapshots", [], |row| {
                row.get(0)
            })
            .unwrap();
        serde_json::from_str::<Value>(&value).unwrap()
    };
    let mut player = read_snapshot();
    player["item_list"]["14018"] = json!(99_998);
    database
        .execute(
            "UPDATE player_snapshots SET data_json=?1",
            params![player.to_string()],
        )
        .unwrap();
    let initial_vmoney = player["user_info"]["free_vmoney"].as_i64().unwrap();
    let authorization = format!("Bearer {}", service.management_token());
    let create_mail = |rewards: Value| {
        let body = json!({"viewer_id": viewer_id, "title": "材料奖励", "body": "奖励领取", "sender": "管理员", "rewards": rewards});
        let response = request_with_headers(
            service.port(),
            "POST",
            "/v1/mails",
            "application/json",
            &[("Authorization", authorization.as_str())],
            body.to_string().as_bytes(),
        );
        assert!(response.starts_with("HTTP/1.1 201 Created"), "{response}");
        response_json(&response)["id"].as_i64().unwrap()
    };
    let oversized = create_mail(json!({"item_list": {"14018": 2}, "free_vmoney": 100}));
    let fitting = create_mail(json!({"item_list": {"14018": 1}}));
    let currency = create_mail(json!({"free_vmoney": 25}));
    let body = encode_request(&MailReceiveAllRequest {
        viewer_id,
        mail_ids: vec![oversized, fitting, currency, oversized],
    });
    let received = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive_all",
        &body,
    ));
    assert_eq!(received.data["mail_ids"], json!([oversized, currency]));
    assert_eq!(received.data["max_overed_mail_count"], 0);
    assert_eq!(received.data["item_list"]["14018"], 99_999);
    assert_eq!(
        received.data["user_info"]["free_vmoney"],
        initial_vmoney + 125
    );
    assert_eq!(received.data["mail_arrived"], true);
    let received_at: Option<i64> = database
        .query_row(
            "SELECT received_at FROM player_mails WHERE id=?1",
            [oversized],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(received_at, None);
    let remaining = || {
        let value: String = database.query_row(
            "SELECT rewards_json FROM player_mails WHERE id=?1", [oversized], |row| row.get(0),
        ).unwrap();
        serde_json::from_str::<Value>(&value).unwrap()
    };
    assert_eq!(remaining()["itemList"]["14018"], 1);
    assert_eq!(remaining()["freeVmoney"], 0);
    let history_count = || database.query_row::<i64, _, _>(
        "SELECT COUNT(*) FROM player_receive_history WHERE event_key LIKE 'mail:%'", [], |row| row.get(0),
    ).unwrap();
    let credited_history_count = history_count();
    let before_blocked = read_snapshot();
    let blocked = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive",
        &encode_request(&MailReceiveRequest {
            viewer_id,
            mail_id: oversized,
        }),
    ));
    assert_eq!(blocked.data_headers.result_code, 1);
    assert_eq!(blocked.data["mail_arrived"], true);
    assert_eq!(read_snapshot(), before_blocked);
    let repeated = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive_all",
        &body,
    ));
    assert_eq!(repeated.data["mail_ids"], json!([]));
    assert_eq!(repeated.data["max_overed_mail_count"], 0);
    assert_eq!(repeated.data["already_mail_count"], 1);
    assert_eq!(history_count(), credited_history_count);
    let mut player = read_snapshot();
    player["item_list"]["14018"] = json!(99_996);
    database
        .execute(
            "UPDATE player_snapshots SET data_json=?1",
            params![player.to_string()],
        )
        .unwrap();
    let before_failed_claim = read_snapshot();
    let before_failed_remainder = remaining();
    database.execute_batch("CREATE TRIGGER reject_mail_receipt BEFORE INSERT ON player_receive_history BEGIN SELECT RAISE(ABORT, 'mail receipt rejected'); END;").unwrap();
    let failed = cn_support::send_request(
        service.port(), "/api/index.php/mail/receive",
        &encode_request(&MailReceiveRequest { viewer_id, mail_id: oversized }),
    );
    assert!(failed.starts_with("HTTP/1.1 500"), "{failed}");
    assert_eq!(read_snapshot(), before_failed_claim);
    assert_eq!(remaining(), before_failed_remainder);
    database.execute_batch("DROP TRIGGER reject_mail_receipt;").unwrap();
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let claimed = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive",
        &encode_request(&MailReceiveRequest {
            viewer_id,
            mail_id: oversized,
        }),
    ));
    assert_eq!(claimed.data_headers.result_code, 1);
    assert_eq!(claimed.data["item_list"]["14018"], 99_997);
    assert_eq!(
        read_snapshot()["user_info"]["free_vmoney"],
        initial_vmoney + 125
    );
    assert_eq!(claimed.data["mail_arrived"], true);
    let final_claim = decode_response::<Value>(&cn_support::send_request(
        service.port(), "/api/index.php/mail/receive_all", &body,
    ));
    assert_eq!(final_claim.data["mail_ids"], json!([fitting]));
    assert_eq!(final_claim.data["item_list"]["14018"], 99_998);
    assert_eq!(final_claim.data["mail_arrived"], false);
    let item_receipts: i64 = database.query_row(
        "SELECT SUM(number) FROM player_receive_history WHERE event_key LIKE 'mail:%' AND kind=1 AND type_id=14018",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(item_receipts, 3);
    service.stop().unwrap();
}

// //// 核对混合批量邮件的结果分类和资产持久化 [@x380kkm 2026-09-25] ////
#[test]
fn categorizes_batch_claims_and_preserves_rewards_across_replay() {
    let root = TempDir::new().expect("temporary service directory");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let set_time = |iso: &str| {
        let body = json!({"enabled": true, "iso": iso, "rate": 1.0});
        let response = request_with_headers(
            service.port(),
            "PUT",
            "/v1/time",
            "application/json",
            &authorization,
            body.to_string().as_bytes(),
        );
        assert!(response.starts_with("HTTP/1.1 200 OK"));
    };
    set_time("2030-01-01T12:00:00.000Z");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 25 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let create_mail = |amount: i64, expires_at: Option<i64>| {
        let body = json!({"viewer_id": viewer_id, "title": "材料奖励",
            "body": "邮件领取", "sender": "Starpoint", "expires_at": expires_at,
            "rewards": {"item_list": {"14018": amount}}});
        let response = request_with_headers(
            service.port(),
            "POST",
            "/v1/mails",
            "application/json",
            &authorization,
            body.to_string().as_bytes(),
        );
        assert!(response.starts_with("HTTP/1.1 201 Created"), "{response}");
        response_json(&response)["id"].as_i64().expect("mail id")
    };
    let previously_claimed = create_mail(2, None);
    let expired = create_mail(100, Some(1893542400));
    let available = create_mail(3, None);
    let missing = available + 1;
    let first = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive",
        &encode_request(&MailReceiveRequest {
            viewer_id,
            mail_id: previously_claimed,
        }),
    ));
    assert_eq!(first.data["item_list"]["14018"], 2);
    set_time("2030-01-03T12:00:00.000Z");
    let request = MailReceiveAllRequest {
        viewer_id,
        mail_ids: vec![previously_claimed, expired, available, missing],
    };
    let claimed = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive_all",
        &encode_request(&request),
    ));
    assert_eq!(claimed.data["mail_ids"], json!([available]));
    assert_eq!(claimed.data["already_mail_count"], 1);
    assert_eq!(claimed.data["outdated_mail_count"], 1);
    assert_eq!(claimed.data["deleted_mail_count"], 1);
    assert_eq!(claimed.data["dispose_expired_mail_count"], 0);
    assert_eq!(claimed.data["item_list"]["14018"], 5);
    assert_eq!(claimed.data["mail_arrived"], false);
    let missing_claim = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive",
        &encode_request(&MailReceiveRequest {
            viewer_id,
            mail_id: missing,
        }),
    ));
    assert_eq!(missing_claim.data_headers.result_code, 2001);
    set_time("2030-01-01T18:00:00.000Z");
    let replayed = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive_all",
        &encode_request(&request),
    ));
    assert_eq!(replayed.data["mail_ids"], json!([]));
    assert_eq!(replayed.data["already_mail_count"], 3);
    assert_eq!(replayed.data["outdated_mail_count"], 0);
    assert_eq!(replayed.data["deleted_mail_count"], 1);
    assert!(replayed.data.get("item_list").is_none());
    service.stop().expect("service stops");
    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            viewer_id,
            keychain: viewer_id,
        }),
    ));
    assert_eq!(loaded.data["item_list"]["14018"], 5);
    service.stop().expect("service stops");
}
