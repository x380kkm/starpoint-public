// audience: internal
// # personal-service-cn-lounge-tests
//
// 该文件验证 CN lounge 响应使用本地多人会话地址和客户端要求的字段类型.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use serde::Serialize;
use serde_json::Value;
use starpoint_personal_service::{PersonalService, PersonalServiceOptions};
use tempfile::TempDir;

#[derive(Serialize)]
struct LoungeRequest {
    viewer_id: i64,
}

// //// 验证 CN lounge 本地连接响应 [@x380kkm 2026-08-24] ////
#[test]
fn returns_local_lounge_connection() {
    let root = TempDir::new().expect("temporary service directory is created");
    let cdn_root = TempDir::new().expect("CN asset root is created");
    let service = PersonalService::start_with_options(
        PersonalServiceOptions::new(root.path(), 0, cdn_root.path())
            .with_multiplayer_session_port(0),
    )
    .expect("service starts");
    let viewer_id = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 77 }),
    ))
    .data_headers
    .viewer_id;
    let request = encode_request(&LoungeRequest { viewer_id });

    let created = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/lounge/create",
        &request,
    ));
    assert_eq!(created.data["lounge_id"], viewer_id);

    let selected = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/lounge/select",
        &request,
    ));
    assert_eq!(selected.data["ip_address"], "127.0.0.1");
    assert_eq!(selected.data["raising_state"], 2);
    assert_eq!(
        selected.data["port"].as_u64(),
        service.multiplayer_session_port().map(u64::from)
    );

    let searched = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/lounge/search",
        &request,
    ));
    assert_eq!(searched.data["lounge_exists"], false);
}
// //// /验证 CN lounge 本地连接响应 ////
