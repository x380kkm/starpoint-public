// audience: internal
// # personal-service-cn-quest-tests
//
// 该文件验证 CN 任务助战响应的数组契约并校验 viewer session.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use serde::Serialize;
use serde_json::Value;
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

#[derive(Serialize)]
struct RecentPartyRequest {
    viewer_id: i64,
    category: i64,
    quest_id: i64,
}

// //// 验证 CN 任务助战数组和 viewer session [@x380kkm 2026-08-22] ////
#[test]
fn returns_a_typed_empty_recent_party_list() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 69 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let response = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/quest/get_recent_other_player_party",
        &encode_request(&RecentPartyRequest {
            viewer_id,
            category: 1,
            quest_id: 1,
        }),
    ));
    assert_eq!(
        response.data["recent_other_player_party"],
        Value::Array(Vec::new())
    );

    let invalid = cn_support::send_request(
        service.port(),
        "/api/index.php/quest/get_recent_other_player_party",
        &encode_request(&RecentPartyRequest {
            viewer_id: 999_999_999,
            category: 1,
            quest_id: 1,
        }),
    );
    assert!(invalid.starts_with("HTTP/1.1 400 Bad Request"));
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 任务助战数组和 viewer session ////
