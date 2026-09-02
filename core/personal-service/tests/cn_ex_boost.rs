// audience: internal
// # personal-service-cn-ex-boost-tests
//
// 该文件验证角色批量突破后抽取 EX 能力并持久化.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, LoadRequest, SignupData, SignupRequest};
use serde::Serialize;
use serde_json::Value;
use starpoint_personal_service::PersonalService;
use support::request_with_headers;
use tempfile::TempDir;

#[derive(Serialize)]
struct AddCharacterRequest {
    viewer_id: i64,
    character_id: i64,
    api_count: i64,
}

#[derive(Serialize)]
struct BulkOverLimitRequest {
    viewer_id: i64,
    api_count: i64,
}

#[derive(Serialize)]
struct ReceiveAllRequest {
    viewer_id: i64,
    mail_ids: Vec<i64>,
}

#[derive(Serialize)]
struct ExDrawRequest {
    viewer_id: i64,
    character_id: i64,
    cost_item_id: i64,
    api_count: i64,
}

#[derive(Serialize)]
struct ExSelectRequest {
    viewer_id: i64,
    is_confirm: bool,
    api_count: i64,
}

// //// 验证 EX 能力抽取扣除素材并写入角色 [@x380kkm 2026-08-22] ////
#[test]
fn draws_and_persists_first_ex_boost() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 72 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    for api_count in 1..=6 {
        decode_response::<Value>(&cn_support::send_request(
            service.port(),
            "/api/index.php/character/add_character_from_town",
            &encode_request(&AddCharacterRequest {
                viewer_id,
                character_id: 1,
                api_count,
            }),
        ));
    }
    let bulk = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/character/bulk_over_limit",
        &encode_request(&BulkOverLimitRequest {
            viewer_id,
            api_count: 7,
        }),
    ));
    assert_eq!(bulk.data["character_list"][0]["over_limit_step"], 6);

    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let created = request_with_headers(
        service.port(),
        "POST",
        "/v1/mails",
        "application/json",
        &authorization,
        format!(
            "{{\"viewer_id\":{viewer_id},\"title\":\"EX material\",\"body\":\"Use it\",\"sender\":\"Starpoint\",\"rewards\":{{\"itemList\":{{\"10001\":10}}}}}}"
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
    let claimed = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive_all",
        &encode_request(&ReceiveAllRequest {
            viewer_id,
            mail_ids: vec![mail_id],
        }),
    ));
    assert_eq!(claimed.data["item_list"]["10001"], 10);

    let drawn = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/ex_boost/first_draw",
        &encode_request(&ExDrawRequest {
            viewer_id,
            character_id: 1,
            cost_item_id: 10_001,
            api_count: 8,
        }),
    ));
    assert_eq!(drawn.data["item_list"]["10001"], 5);
    assert!(drawn.data["character_list"][0]["ex_boost"]["status_id"].is_i64());
    assert_eq!(
        drawn.data["character_list"][0]["ex_boost"]["ability_id_list"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(
        loaded.data["user_character_list"]["1"]["ex_boost"],
        drawn.data["character_list"][0]["ex_boost"]
    );

    let pending = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/ex_boost/draw",
        &encode_request(&ExDrawRequest {
            viewer_id,
            character_id: 1,
            cost_item_id: 10_001,
            api_count: 9,
        }),
    ));
    assert_eq!(pending.data["item_list"]["10001"], 0);
    let selected = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/ex_boost/select",
        &encode_request(&ExSelectRequest {
            viewer_id,
            is_confirm: true,
            api_count: 10,
        }),
    ));
    assert_eq!(
        selected.data["character_list"][0]["ex_boost"],
        pending.data["draw_result"]
    );
    service.stop().expect("service stops cleanly");
}
// //// /验证 EX 能力抽取扣除素材并写入角色 ////
