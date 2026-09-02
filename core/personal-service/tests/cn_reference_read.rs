// audience: internal
// # personal-service-cn-reference-read-tests
//
// 该文件验证 CN 查询接口的类型化响应, viewer session 校验和百科已读持久化.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::fs;
use support::request_with_headers;
use tempfile::TempDir;

// //// 验证查询响应族使用客户端期望的容器 [@x380kkm 2026-08-22] ////
#[test]
fn returns_typed_cn_response_families() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(service.port(), 71);

    let attention = request(
        service.port(),
        "/api/index.php/attention/check",
        json!({"viewer_id": viewer_id, "holding_number": 0}),
    );
    assert_eq!(
        attention["config"]["attention_polling_interval_seconds_normal"],
        10
    );

    let comic = request(
        service.port(),
        "/api/index.php/comic/get_list",
        json!({"viewer_id": viewer_id, "kind": 0, "page_index": 2}),
    );
    assert_eq!(comic["comic_list"], json!([]));
    assert_eq!(comic["current_page_index"], 2);
    assert_eq!(comic["total_count"], 0);

    let receive_history = request(
        service.port(),
        "/api/index.php/history/receive",
        json!({"viewer_id": viewer_id}),
    );
    assert_eq!(receive_history, json!({"history": [], "total_count": 0}));

    let degree = request(
        service.port(),
        "/api/index.php/profile/get_degree_list",
        json!({"viewer_id": viewer_id}),
    );
    assert!(degree["degree_ids"].is_array());

    let news = request(
        service.port(),
        "/api/index.php/news/index",
        json!({"viewer_id": viewer_id, "page_index": 1}),
    );
    assert_eq!(news["current_page"], 1);
    assert_eq!(news["news_count"], 3);
    assert_eq!(news["news"].as_array().map(Vec::len), Some(3));
    let detail = request(
        service.port(),
        "/api/index.php/news/get_info",
        json!({"viewer_id": viewer_id, "news_id": news["news"][0]["id"]}),
    );
    assert_eq!(detail["title"], news["news"][0]["title"]);
    assert_eq!(detail["thumbnail_path"], news["news"][0]["thumbnail_path"]);
    assert_eq!(detail["added_time"], news["news"][0]["added_time"]);

    let assistant = request(
        service.port(),
        "/api/index.php/assistant/get_assistant_list",
        json!({"viewer_id": viewer_id}),
    );
    assert_eq!(assistant["all_box_info"], json!([]));
    assert_eq!(assistant["sales_list"], json!([]));

    let election = request(
        service.port(),
        "/api/index.php/character_election/get_vote_status",
        json!({"viewer_id": viewer_id, "election_id": 1}),
    );
    assert_eq!(election["is_voted"], false);

    let crazy_gacha = request(
        service.port(),
        "/api/index.php/gacha/crazy_gacha_save",
        json!({"viewer_id": viewer_id, "index": 0}),
    );
    assert_eq!(crazy_gacha["crazy_gacha_result_list"], json!([]));

    let gift = request(
        service.port(),
        "/api/index.php/gift/receive",
        json!({"viewer_id": viewer_id, "key": "offline"}),
    );
    assert_eq!(gift, json!({"all_gift_info": [], "result_code": 1}));

    let how_to_get = request(
        service.port(),
        "/api/index.php/how_to_get/get_list",
        json!({"viewer_id": viewer_id, "item_id": 99}),
    );
    assert!(!how_to_get["box_gacha_id_list"]
        .as_array()
        .expect("box gacha sources are an array")
        .is_empty());
    assert!(!how_to_get["shop_sales_list"]
        .as_array()
        .expect("shop sources are an array")
        .is_empty());

    let reproduce = request(
        service.port(),
        "/api/index.php/reproduce/post",
        json!({"viewer_id": viewer_id, "logs": []}),
    );
    assert_eq!(reproduce, json!([]));
    assert_eq!(
        request(
            service.port(),
            "/api/index.php/reproduce/post",
            json!({"viewer_id": null, "logs": []}),
        ),
        json!([])
    );
    assert_eq!(
        request(
            service.port(),
            "/api/index.php/reproduce/post",
            json!({"logs": []}),
        ),
        json!([])
    );

    service.stop().expect("service stops cleanly");
}
// //// /验证查询响应族使用客户端期望的容器 ////

// //// 验证如何获得只返回当前活动的箱池和商店来源 [@x380kkm 2026-08-25] ////
#[test]
fn filters_how_to_get_sources_by_activity_window() {
    let root = TempDir::new().expect("temporary service directory is created");
    let cdn_root = TempDir::new().expect("CN asset root is created");
    fs::write(
        cdn_root.path().join("activity-catalog.json"),
        r#"{
            "format_version": 1,
            "region": "cn",
            "client_version": "1.8.4",
            "asset_version": "1.4.54",
            "generated_at": "2024-06-22T00:00:00Z",
            "activities": [
                {
                    "activity_id": "box-gacha:1",
                    "name": "Closed box",
                    "kind": "box-gacha",
                    "tags": ["test"],
                    "description": "closed box source",
                    "default_start_at_ms": 1714521600000,
                    "default_end_at_ms": 1715817600000
                },
                {
                    "activity_id": "box-gacha:1011",
                    "name": "Open box",
                    "kind": "box-gacha",
                    "tags": ["test"],
                    "description": "open box source",
                    "default_start_at_ms": 1717200000000,
                    "default_end_at_ms": 1719792000000
                },
                {
                    "activity_id": "world-story:400003",
                    "name": "Open event shop",
                    "kind": "world-story",
                    "tags": ["test"],
                    "description": "open event shop source",
                    "default_start_at_ms": 1717200000000,
                    "default_end_at_ms": 1719792000000
                },
                {
                    "activity_id": "world-story:400006",
                    "name": "Closed event shop",
                    "kind": "world-story",
                    "tags": ["test"],
                    "description": "closed event shop source",
                    "default_start_at_ms": 1706745600000,
                    "default_end_at_ms": 1715817600000
                }
            ]
        }"#,
    )
    .expect("activity manifest is written");
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
        .expect("service starts");
    let viewer_id = signup(service.port(), 74);
    set_virtual_time(&service, "2024-06-22T12:00:00.000Z");
    disable_activity(&service, "box-gacha:1");
    disable_activity(&service, "world-story:400006");

    let sources = request(
        service.port(),
        "/api/index.php/how_to_get/get_list",
        json!({"viewer_id": viewer_id, "item_id": 2}),
    );
    let box_gacha_ids = sources["box_gacha_id_list"]
        .as_array()
        .expect("box gacha sources are an array");
    assert!(box_gacha_ids.contains(&Value::from(1_011)));
    assert!(!box_gacha_ids.contains(&Value::from(1)));
    let shop_sales = sources["shop_sales_list"]
        .as_array()
        .expect("shop sources are an array");
    let open_event_sale = shop_sales
        .iter()
        .find(|sale| sale["shop_type"] == 4 && sale["shop_item_id"] == 600_071)
        .expect("open event shop source is returned");
    assert_eq!(open_event_sale["stock_quantity"], 180);
    assert_eq!(open_event_sale["group_info"]["multi_stage"], false);
    assert!(!shop_sales
        .iter()
        .any(|sale| sale["shop_type"] == 4 && sale["shop_item_id"] == 600_159));
    assert!(shop_sales
        .iter()
        .any(|sale| sale["shop_type"] == 7 && sale["shop_item_id"] == 200_509));
    service.stop().expect("service stops cleanly");
}
// //// /验证如何获得只返回当前活动的箱池和商店来源 ////

// //// 验证百科已读状态写入玩家快照 [@x380kkm 2026-08-22] ////
#[test]
fn persists_encyclopedia_read_state_and_accepts_attention_without_snapshot() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(service.port(), 72);
    let encyclopedia_id = 990_001_001_i64;

    let updated = request(
        service.port(),
        "/api/index.php/encyclopedia/unlock_keyword",
        json!({
            "viewer_id": viewer_id,
            "encyclopedia_ids": [encyclopedia_id]
        }),
    );
    assert_eq!(
        updated["encyclopedia_list"][encyclopedia_id.to_string()]["read"],
        true
    );

    let index = request(
        service.port(),
        "/api/index.php/encyclopedia/index",
        json!({"viewer_id": viewer_id}),
    );
    assert_eq!(
        index["encyclopedia_list"][encyclopedia_id.to_string()]["read"],
        true
    );

    let action = request(
        service.port(),
        "/api/index.php/attention/action",
        json!({"viewer_id": 999_999_999, "priority_factors": []}),
    );
    assert_eq!(action["priority_action_score"], 0);
    assert_eq!(
        request(
            service.port(),
            "/api/index.php/attention/logger",
            json!({"viewer_id": 999_999_999}),
        ),
        json!({})
    );

    service.stop().expect("service stops cleanly");
}
// //// /验证百科已读状态写入玩家快照 ////

// //// 验证混合物品列表只消耗有效体力物品 [@x380kkm 2026-08-22] ////
#[test]
fn skips_invalid_entries_in_mixed_item_use_requests() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(service.port(), 73);
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let created = request_with_headers(
        service.port(),
        "POST",
        "/v1/mails",
        "application/json",
        &authorization,
        format!(
            "{{\"viewer_id\":{viewer_id},\"title\":\"Stamina\",\"body\":\"Use it\",\"sender\":\"Starpoint\",\"rewards\":{{\"itemList\":{{\"106\":1}}}}}}"
        )
        .as_bytes(),
    );
    assert!(created.starts_with("HTTP/1.1 201 Created"));
    let mail_id = serde_json::from_str::<Value>(
        created
            .split_once("\r\n\r\n")
            .expect("mail response body")
            .1,
    )
    .expect("mail response is JSON")["id"]
        .as_i64()
        .expect("mail id is numeric");
    request(
        service.port(),
        "/api/index.php/mail/receive_all",
        json!({"viewer_id": viewer_id, "mail_ids": [mail_id]}),
    );
    let history = request(
        service.port(),
        "/api/index.php/history/receive",
        json!({"viewer_id": viewer_id}),
    );
    assert_eq!(history["total_count"], 1);
    assert_eq!(history["history"][0]["type"], 1);
    assert_eq!(history["history"][0]["type_id"], 106);
    assert_eq!(history["history"][0]["number"], 1);
    request(
        service.port(),
        "/api/index.php/mail/receive_all",
        json!({"viewer_id": viewer_id, "mail_ids": [mail_id]}),
    );
    let repeated_history = request(
        service.port(),
        "/api/index.php/history/receive",
        json!({"viewer_id": viewer_id}),
    );
    assert_eq!(repeated_history["total_count"], 1);
    let used = request(
        service.port(),
        "/api/index.php/item/use_item",
        json!({
            "viewer_id": viewer_id,
            "items": [
                {"id": -1, "number": 1},
                {"id": 999_999_999, "number": 1},
                {"id": 106, "number": 1}
            ]
        }),
    );
    assert_eq!(used["item_list"]["106"], 0);
    assert!(used["user_info"]["stamina"]
        .as_i64()
        .is_some_and(|stamina| stamina > 20));
    service.stop().expect("service stops cleanly");
}
// //// /验证混合物品列表只消耗有效体力物品 ////

fn signup(port: u16, device_id: i64) -> i64 {
    decode_response::<SignupData>(&cn_support::send_request(
        port,
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id }),
    ))
    .data_headers
    .viewer_id
}

fn set_virtual_time(service: &PersonalService, iso: &str) {
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let response = request_with_headers(
        service.port(),
        "PUT",
        "/v1/time",
        "application/json",
        &authorization,
        format!(r#"{{"enabled":true,"iso":"{iso}","rate":1.0}}"#).as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
}

// //// 禁用指定活动来源 [@x380kkm 2026-08-30] ////
fn disable_activity(service: &PersonalService, activity_id: &str) {
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let response = request_with_headers(
        service.port(),
        "PUT",
        &format!("/v1/activities/calendar/{activity_id}"),
        "application/json",
        &authorization,
        br#"{"enabled":false,"start_at_ms":1706745600000,"end_at_ms":1719792000000}"#,
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
}
// //// /禁用指定活动来源 ////

fn request(port: u16, path: &str, body: Value) -> Value {
    decode_response::<Value>(&cn_support::send_request(
        port,
        path,
        &encode_request(&body),
    ))
    .data
}
