// audience: internal
// # personal-service-product-history-tests
// 商品记录从持久化订单读取, 资格发放和界面确认分别展示, 分页保持存档隔离.

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
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

// //// 建立商品领取与管理查询的真实 HTTP 会话 [@x380kkm 2026-09-23] ////
fn game(service: &PersonalService, path: &str, body: Value) -> Value {
    let response = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        &format!("/api/index.php/{path}"),
        &encode_request(&body),
    ));
    assert_eq!(response.data_headers.result_code, 1);
    response.data
}

fn history(service: &PersonalService, slot: i64, path: &str) -> Value {
    let response = authorized_request(
        service,
        "GET",
        &format!("/v1/local-saves/{slot}/{path}"),
        None,
    );
    assert_status(&response, "200 OK");
    response_body(&response)
}

fn setup() -> (TempDir, PersonalService, i64, i64) {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    assert_status(
        &authorized_request(
            &service,
            "PUT",
            "/v1/time",
            Some(&json!({
                "enabled":true,"iso":"2024-06-10T00:00:00.000Z","rate":1,
            })),
        ),
        "200 OK",
    );
    let viewer = signup(service.port(), 923_410).data_headers.viewer_id;
    let slot = list_local_saves(&service)["slots"][0]["id"]
        .as_i64()
        .unwrap();
    (root, service, viewer, slot)
}

fn start(service: &PersonalService, viewer: i64, count: i64, product: &str) -> String {
    game(
        service,
        "payment/start",
        json!({"viewer_id":viewer,"api_count":count,"product_id":product}),
    )["order_id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn finish(service: &PersonalService, viewer: i64, order: &str) -> Value {
    game(
        service,
        "payment/finish",
        json!({"viewer_id":viewer,"order_id":order}),
    )
}

fn snapshot(root: &Path, slot: i64) -> String {
    Connection::open(root.join("personal-service.sqlite3"))
        .unwrap()
        .query_row(
            "SELECT snapshots.data_json FROM player_snapshots snapshots JOIN local_save_slots slots
         ON slots.account_id=snapshots.account_id WHERE slots.id=?1",
            [slot],
            |row| row.get(0),
        )
        .unwrap()
}
// //// /建立商品领取与管理查询的真实 HTTP 会话 ////

// //// 展示三种纯资格商品并保留重放重启和账号隔离 [@x380kkm 2026-09-23] ////
#[test]
fn shows_entitlement_products_before_and_after_client_confirmation() {
    let (root, service, viewer, slot) = setup();
    for count in 1..=5 {
        let order = start(&service, viewer, count, "com.leiting.wf.stone_50");
        finish(&service, viewer, &order);
    }
    let assets_before = history(&service, slot, "reward-history");
    let deliveries_before = history(&service, slot, "reward-deliveries");
    let products = [
        (
            "com.leiting.wf.stonepack_light_treasury_1",
            "growth_fund",
            "is_bought_fund_laite2",
        ),
        (
            "com.leiting.wf.stonepack_light_treasury_2",
            "growth_fund",
            "is_bought_fund_laite3",
        ),
        ("com.leiting.wf.pass_card", "pass_card", ""),
    ];
    let pass_catalog: Value =
        serde_json::from_str(include_str!("../assets/cn-pass-card.json")).unwrap();
    let pass_id = pass_catalog["events"]
        .as_object()
        .unwrap()
        .values()
        .find(|event| event["is_free"] == false)
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    let pass_before = game(
        &service,
        "Pass_card/get_pass_card",
        json!({"viewer_id":viewer,"pass_card_id":pass_id}),
    );
    assert_eq!(pass_before["is_buy"], false);

    for (index, (product, kind, flag)) in products.into_iter().enumerate() {
        let count = index as i64 + 6;
        let order = start(&service, viewer, count, product);
        let pending = history(&service, slot, "product-history");
        assert_eq!(pending["entries"].as_array().unwrap().len(), count as usize);
        assert_eq!(pending["entries"][0]["id"], count);
        assert_eq!(pending["entries"][0]["product_id"], product);
        assert_eq!(pending["entries"][0]["entitlement_kinds"], json!([kind]));
        assert!(pending["entries"][0]["created_at"].as_i64().unwrap() > 0);
        assert!(pending["entries"][0]["confirmed_at"].is_null());
        assert_eq!(start(&service, viewer, count, product), order);
        assert_eq!(history(&service, slot, "product-history"), pending);
        let finished = finish(&service, viewer, &order);
        if !flag.is_empty() {
            assert_eq!(finished["user_info"][flag], true);
        }
        let confirmed = history(&service, slot, "product-history");
        assert!(confirmed["entries"][0]["confirmed_at"].as_i64().unwrap() > 0);
        assert_eq!(
            confirmed["entries"][0]["created_at"],
            pending["entries"][0]["created_at"]
        );
        assert_eq!(finish(&service, viewer, &order), finished);
        assert_eq!(history(&service, slot, "product-history"), confirmed);
        assert_eq!(history(&service, slot, "reward-history"), assets_before);
        assert_eq!(
            history(&service, slot, "reward-deliveries"),
            deliveries_before
        );
        let serialized = confirmed.to_string();
        for private in [
            order.as_str(),
            "viewer_id",
            "api_count",
            "response",
            "payment_state",
            service.management_token(),
        ] {
            assert!(
                !serialized.contains(private),
                "product history exposes private state"
            );
        }
    }
    assert_eq!(
        game(
            &service,
            "Pass_card/get_pass_card",
            json!({"viewer_id":viewer,"pass_card_id":pass_id})
        )["is_buy"],
        true
    );
    let completed = history(&service, slot, "product-history");
    let stored = snapshot(root.path(), slot);
    assert_eq!(history(&service, slot, "product-history"), completed);
    assert_eq!(snapshot(root.path(), slot), stored);

    signup(service.port(), 923_411);
    let other_slot = list_local_saves(&service)["slots"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|entry| entry["id"].as_i64().filter(|id| *id != slot))
        .unwrap();
    assert_eq!(
        history(&service, other_slot, "product-history")["entries"],
        json!([])
    );
    assert_eq!(history(&service, slot, "product-history"), completed);
    service.stop().unwrap();
    let restarted = PersonalService::start(root.path(), 0).unwrap();
    assert_eq!(history(&restarted, slot, "product-history"), completed);
    assert_eq!(
        history(&restarted, other_slot, "product-history")["entries"],
        json!([])
    );
    restarted.stop().unwrap();
}
// //// /展示三种纯资格商品并保留重放重启和账号隔离 ////

// //// 在新增订单和时间回拨时保持领取记录分页连续 [@x380kkm 2026-09-23] ////
#[test]
fn paginates_persisted_orders_while_new_products_are_claimed() {
    let (root, service, viewer, slot) = setup();
    let product = "com.leiting.wf.stonepack_light_treasury_1";
    for count in 1..=55 {
        start(&service, viewer, count, product);
    }
    let first = history(&service, slot, "product-history");
    assert_eq!(first["entries"].as_array().unwrap().len(), 50);
    assert_eq!(first["entries"][0]["id"], 55);
    assert_eq!(first["next_before_id"], 6);
    assert_status(
        &authorized_request(
            &service,
            "PUT",
            "/v1/time",
            Some(&json!({
                "enabled":true,"iso":"2024-06-01T00:00:00.000Z","rate":1,
            })),
        ),
        "200 OK",
    );
    start(&service, viewer, 56, product);
    let second = history(&service, slot, "product-history?before_id=6");
    let ids = second["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["id"].as_i64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec![5, 4, 3, 2, 1]);
    assert!(second["next_before_id"].is_null());
    assert_eq!(
        history(&service, slot, "product-history")["entries"][0]["id"],
        56
    );

    let stored = snapshot(root.path(), slot);
    for path in [
        "product-history?before_id=0",
        "product-history?before_id=3&before_id=2",
    ] {
        assert_status(
            &authorized_request(
                &service,
                "GET",
                &format!("/v1/local-saves/{slot}/{path}"),
                None,
            ),
            "400 Bad Request",
        );
    }
    assert_status(
        &support::request_with_headers(
            service.port(),
            "GET",
            &format!("/v1/local-saves/{slot}/product-history"),
            "application/json",
            &[("Authorization", "Bearer invalid")],
            &[],
        ),
        "401 Unauthorized",
    );
    assert_eq!(snapshot(root.path(), slot), stored);
    service.stop().unwrap();
}
// //// /在新增订单和时间回拨时保持领取记录分页连续 ////
