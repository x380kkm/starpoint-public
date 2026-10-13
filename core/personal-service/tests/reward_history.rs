// audience: internal
// # personal-service-reward-history-tests
// 管理请求验证商品到账, 订阅邮件领取状态, 存档隔离和历史分页.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[path = "support/local_saves.rs"]
#[allow(dead_code)]
mod local_save_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request};
use local_save_support::{
    assert_status, authorized_request, list_local_saves, response_body, signup,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

fn cn_request(service: &PersonalService, route: &str, body: Value) -> Value {
    let response = cn_support::send_request(
        service.port(),
        &format!("/api/index.php/{route}"),
        &encode_request(&body),
    );
    let envelope = decode_response::<Value>(&response);
    assert_eq!(envelope.data_headers.result_code, 1, "{response}");
    envelope.data
}

fn history(service: &PersonalService, slot_id: i64, action: &str) -> Value {
    let response = authorized_request(
        service,
        "GET",
        &format!("/v1/local-saves/{slot_id}/{action}"),
        None,
    );
    assert_status(&response, "200 OK");
    response_body(&response)
}

fn set_time(service: &PersonalService, iso: &str) {
    assert_status(
        &authorized_request(
            service,
            "PUT",
            "/v1/time",
            Some(&json!({"enabled": true, "iso": iso, "rate": 1})),
        ),
        "200 OK",
    );
}

// //// 在管理台区分即时到账与订阅到期后的邮件领取 [@x380kkm 2026-09-22] ////
#[test]
fn shows_product_assets_and_subscription_deliveries_until_claimed() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    set_time(&service, "2026-01-01T12:00:00.000Z");
    let player = signup(service.port(), 81);
    let viewer_id = player.data_headers.viewer_id;
    let slot_id = list_local_saves(&service)["slots"][0]["id"]
        .as_i64()
        .unwrap();
    cn_request(
        &service,
        "load",
        json!({"viewer_id": viewer_id, "keychain": viewer_id}),
    );
    cn_request(
        &service,
        "payment/start",
        json!({"viewer_id": viewer_id, "api_count": 1, "payment": {"product_id": "com.leiting.wf.stone_50"}}),
    );
    let received = history(&service, slot_id, "reward-history");
    assert!(received["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["source"] == "product" && entry["kind"] == 3 && entry["number"] == 50));
    let serialized = received.to_string();
    for secret in [
        service.management_token(),
        player.data.login_token.as_str(),
        "account_id",
        "event_key",
        "payment_orders",
    ] {
        assert!(
            !serialized.contains(secret),
            "reward history contains private state"
        );
    }

    let catalog: Value =
        serde_json::from_str(include_str!("../assets/cn-payment-catalog.json")).unwrap();
    let product = catalog["products"]
        .as_object()
        .unwrap()
        .iter()
        .find(|(_, product)| {
            product["direct_purchase"] == true
                && product["entitlements"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|entitlement| {
                        entitlement["entitlement"] == "subscription"
                            && entitlement["id"] == "month_card"
                    })
        })
        .unwrap()
        .0;
    cn_request(
        &service,
        "payment/start",
        json!({"viewer_id": viewer_id, "api_count": 2, "payment": {"product_id": product}}),
    );
    set_time(&service, "2026-01-31T12:00:00.000Z");
    cn_request(
        &service,
        "load",
        json!({"viewer_id": viewer_id, "keychain": viewer_id}),
    );
    let deliveries = history(&service, slot_id, "reward-deliveries");
    let subscription_mails = deliveries["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["title"] == "每日奖励补领")
        .collect::<Vec<_>>();
    assert_eq!(subscription_mails.len(), 2);
    assert!(subscription_mails
        .iter()
        .all(|entry| entry["status"] == "pending" && entry["received_at"].is_null()));
    assert!(subscription_mails
        .iter()
        .flat_map(|entry| entry["rewards"].as_array().unwrap())
        .any(|reward| reward["kind"] == 4 && reward["number"].as_i64().unwrap() > 0));
    assert!(!history(&service, slot_id, "reward-history")["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["source"] == "mail"));

    cn_request(
        &service,
        "mail/receive_all",
        json!({"viewer_id": viewer_id}),
    );
    let claimed = history(&service, slot_id, "reward-deliveries");
    assert!(claimed["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["title"] == "每日奖励补领")
        .all(|entry| entry["status"] == "received" && entry["received_at"].is_i64()));
    assert!(history(&service, slot_id, "reward-history")["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["source"] == "mail" && entry["number"].as_i64().unwrap() > 0));

    let created = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{slot_id}/mails"),
        Some(
            &json!({"title": "限时奖励", "body": "活动赠礼", "sender": "星点", "rewards": {"freeMana": 12}, "expires_at": 1_770_000_000}),
        ),
    );
    assert_status(&created, "201 Created");
    set_time(&service, "2026-02-10T12:00:00.000Z");
    let expired = history(&service, slot_id, "reward-deliveries");
    assert!(expired["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["title"] == "限时奖励" && entry["status"] == "expired"));
    assert!(expired["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["title"] == "每日奖励补领")
        .all(|entry| entry["status"] == "received"));

    let other = signup(service.port(), 82);
    assert_ne!(other.data_headers.viewer_id, viewer_id);
    let other_slot = list_local_saves(&service)["slots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|slot| slot["id"] != slot_id)
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    assert!(history(&service, other_slot, "reward-history")["entries"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(
        history(&service, other_slot, "reward-deliveries")["entries"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    service.stop().unwrap();
}
// //// /在管理台区分即时到账与订阅到期后的邮件领取 ////

// //// 在虚拟时间回拨时保持历史分页和只读边界 [@x380kkm 2026-09-22] ////
#[test]
fn paginates_receipts_by_insertion_and_rejects_invalid_management_requests() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    signup(service.port(), 83);
    let slot_id = list_local_saves(&service)["slots"][0]["id"]
        .as_i64()
        .unwrap();
    let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    for index in 1..=55 {
        database.execute("INSERT INTO player_receive_history (account_id, event_key, entry_index, kind, type_id, number, reason_id, subject, created_at)
            SELECT account_id, ?2, 0, 1, 106, ?3, 17, '关卡奖励', ?4 FROM local_save_slots WHERE id=?1",
            params![slot_id, format!("mission-receipt:private-{index}"), index, 1000 - index]).unwrap();
    }
    let first = history(&service, slot_id, "reward-history");
    assert_eq!(first["entries"].as_array().unwrap().len(), 50);
    assert_eq!(first["entries"][0]["number"], 55);
    let cursor = first["next_before_id"].as_i64().unwrap();
    let second = history(
        &service,
        slot_id,
        &format!("reward-history?before_id={cursor}"),
    );
    assert_eq!(second["entries"].as_array().unwrap().len(), 5);
    assert_eq!(second["entries"][0]["number"], 5);
    assert!(second["next_before_id"].is_null());
    assert_eq!(second["entries"][0]["type_id"], 106);
    assert_eq!(second["entries"][0]["reason_id"], 17);
    assert_eq!(second["entries"][0]["subject"], "关卡奖励");
    for action in [
        "reward-history?before_id=0",
        "reward-history?before_id=2&before_id=3",
        "reward-deliveries?before_id=-1",
        "reward-deliveries?before_id=9223372036854775808",
    ] {
        assert_status(
            &authorized_request(
                &service,
                "GET",
                &format!("/v1/local-saves/{slot_id}/{action}"),
                None,
            ),
            "400 Bad Request",
        );
    }
    assert_status(
        &authorized_request(
            &service,
            "POST",
            &format!("/v1/local-saves/{slot_id}/reward-history"),
            None,
        ),
        "405 Method Not Allowed",
    );
    assert_status(
        &authorized_request(
            &service,
            "GET",
            "/v1/local-saves/999999/reward-history",
            None,
        ),
        "404 Not Found",
    );
    let rejected = support::request_with_headers(
        service.port(),
        "GET",
        &format!("/v1/local-saves/{slot_id}/reward-history"),
        "application/json",
        &[("Authorization", "Bearer invalid")],
        &[],
    );
    assert_status(&rejected, "401 Unauthorized");
    assert_eq!(
        database
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM player_receive_history", [], |row| row
                .get(0))
            .unwrap(),
        55
    );
    service.stop().unwrap();
}
// //// /在虚拟时间回拨时保持历史分页和只读边界 ////

// //// 在翻页期间新增邮件时保持投递记录连续 [@x380kkm 2026-09-22] ////
#[test]
fn keeps_delivery_cursor_stable_when_another_mail_arrives() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    signup(service.port(), 84);
    let slot_id = list_local_saves(&service)["slots"][0]["id"]
        .as_i64()
        .unwrap();
    let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    for index in 1..=52 {
        database.execute("INSERT INTO player_mails (account_id, title, body, sender, rewards_json, created_at)
            SELECT account_id, ?2, '活动赠礼', '星点', '{\"freeVmoney\":5}', ?3 FROM local_save_slots WHERE id=?1",
            params![slot_id, format!("奖励 {index}"), 1000 - index]).unwrap();
    }
    let first = history(&service, slot_id, "reward-deliveries");
    let first_entries = first["entries"].as_array().unwrap();
    assert_eq!(first_entries.len(), 50);
    assert_eq!(first_entries[0]["title"], "奖励 52");
    let cursor = first["next_before_id"].as_i64().unwrap();
    let created = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{slot_id}/mails"),
        Some(
            &json!({"title": "追加投递", "body": "活动赠礼", "sender": "星点", "rewards": {"freeVmoney": 10}}),
        ),
    );
    assert_status(&created, "201 Created");
    let second = history(
        &service,
        slot_id,
        &format!("reward-deliveries?before_id={cursor}"),
    );
    let second_entries = second["entries"].as_array().unwrap();
    assert_eq!(second_entries.len(), 2);
    assert_eq!(second_entries[0]["title"], "奖励 2");
    assert_eq!(second_entries[1]["title"], "奖励 1");
    assert!(second_entries.iter().all(|entry| !first_entries
        .iter()
        .any(|previous| previous["id"] == entry["id"])));
    assert!(second["next_before_id"].is_null());
    assert_eq!(second["total"], 53);
    assert_eq!(
        history(&service, slot_id, "reward-deliveries")["entries"][0]["title"],
        "追加投递"
    );
    service.stop().unwrap();
}
// //// /在翻页期间新增邮件时保持投递记录连续 ////
