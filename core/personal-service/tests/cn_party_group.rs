// audience: internal
// # personal-service-cn-party-group-tests
//
// 该文件验证 CN 编队组颜色编辑和快照持久化.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, LoadRequest, SignupData, SignupRequest};
use serde::Serialize;
use serde_json::Value;
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

#[derive(Serialize)]
struct PartyGroupEditRequest {
    viewer_id: i64,
    api_count: i64,
    retry_count: i64,
    party_group_edit_params_list: Vec<PartyGroupEdit>,
}

#[derive(Serialize)]
struct PartyGroupEdit {
    party_group_id: i64,
    party_category: i64,
    party_group_color_id: i64,
}

// //// 验证 CN 编队组颜色和快照持久化 [@x380kkm 2026-07-24] ////
#[test]
fn edits_party_group_color_and_persists_state() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 45 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let edited = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/party_group/edit",
        &encode_request(&PartyGroupEditRequest {
            viewer_id,
            api_count: 1,
            retry_count: 0,
            party_group_edit_params_list: vec![PartyGroupEdit {
                party_group_id: 1,
                party_category: 1,
                party_group_color_id: 6,
            }],
        }),
    ));
    assert_eq!(edited.data, serde_json::json!({}));

    let missing_group = cn_support::send_request(
        service.port(),
        "/api/index.php/party_group/edit",
        &encode_request(&PartyGroupEditRequest {
            viewer_id,
            api_count: 2,
            retry_count: 0,
            party_group_edit_params_list: vec![PartyGroupEdit {
                party_group_id: 999,
                party_category: 1,
                party_group_color_id: 4,
            }],
        }),
    );
    assert!(missing_group.starts_with("HTTP/1.1 400 Bad Request"));

    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(loaded.data["user_party_group_list"]["1"]["color_id"], 6);
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 编队组颜色和快照持久化 ////
