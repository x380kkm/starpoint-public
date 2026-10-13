// audience: internal
// # personal-service-cn-mail-exp-pool-tests
// 邮件按自然增长后的经验余额领取可容纳部分, 混合奖励按各自容量结算.

use super::*;
use rusqlite::Connection;
use serde_json::json;

// //// 将自然增长计入经验容量并保留邮件中的经验余量 [@x380kkm 2026-09-28] ////
#[test]
fn checks_accrued_exp_before_claiming_and_preserves_reward_timing() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 29 }),
    ))
    .data_headers
    .viewer_id;
    let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let read_player = || {
        let data: String = database
            .query_row("SELECT data_json FROM player_snapshots", [], |row| {
                row.get(0)
            })
            .unwrap();
        serde_json::from_str::<Value>(&data).unwrap()
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let mut player = read_player();
    let original_vmoney = player["user_info"]["free_vmoney"].as_i64().unwrap();
    player["user_info"]["exp_pool"] = json!(1_999_999_990);
    player["user_info"]["exp_pooled_time"] = json!(now - 1_500);
    database
        .execute(
            "UPDATE player_snapshots SET data_json=?1",
            [player.to_string()],
        )
        .unwrap();
    let authorization = format!("Bearer {}", service.management_token());
    let created = request_with_headers(
        service.port(),
        "POST",
        "/v1/mails",
        "application/json",
        &[("Authorization", authorization.as_str())],
        json!({"viewer_id":viewer_id, "title":"经验奖励", "body":"奖励领取", "sender":"管理员",
            "rewards":{"expPool":1, "free_vmoney":7}})
        .to_string()
        .as_bytes(),
    );
    assert!(created.starts_with("HTTP/1.1 201 Created"), "{created}");
    let mail_id = response_json(&created)["id"].as_i64().unwrap();
    let body = encode_request(&MailReceiveAllRequest {
        viewer_id,
        mail_ids: vec![mail_id],
    });
    let blocked = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive_all",
        &body,
    ));
    assert_eq!(blocked.data["mail_ids"], json!([mail_id]));
    assert_eq!(blocked.data["max_overed_mail_count"], 0);
    assert_eq!(read_player()["user_info"]["free_vmoney"], original_vmoney + 7);
    let received_at: Option<i64> = database
        .query_row(
            "SELECT received_at FROM player_mails WHERE id=?1",
            [mail_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(received_at, None);
    player = read_player();
    player["user_info"]["exp_pool"] = json!(0);
    player["user_info"]["exp_pooled_time"] = json!(now - 1500);
    database
        .execute(
            "UPDATE player_snapshots SET data_json=?1",
            [player.to_string()],
        )
        .unwrap();
    let claimed = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive_all",
        &body,
    ));
    assert_eq!(claimed.data["mail_ids"], json!([mail_id]));
    assert_eq!(claimed.data["user_info"]["exp_pool"], 21);
    assert_eq!(claimed.data["user_info"]["exp_pooled_time"], now - 300);
    assert_eq!(
        read_player()["user_info"]["free_vmoney"],
        original_vmoney + 7
    );
    let replay = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive_all",
        &body,
    ));
    assert_eq!(replay.data["mail_ids"], json!([]));
    assert_eq!(read_player()["user_info"]["exp_pool"], 21);
    service.stop().unwrap();
}
// //// /将自然增长计入经验容量并保留邮件中的经验余量 ////
