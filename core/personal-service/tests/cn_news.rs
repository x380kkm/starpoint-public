// audience: internal
// # personal-service-cn-news-tests
//
// 该文件验证 CN 新闻响应使用虚拟服务时间并校验 viewer session.

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
struct NewsRequest {
    viewer_id: i64,
    news_id: i64,
    api_count: i64,
}

// //// 验证 CN 新闻响应和虚拟时间 [@x380kkm 2026-07-24] ////
#[test]
fn returns_news_with_server_time() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 48 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let news = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/news/get_info",
        &encode_request(&NewsRequest {
            viewer_id,
            news_id: 1,
            api_count: 1,
        }),
    ));
    assert_eq!(news.data["id"], 1);
    assert_eq!(news.data["title"], "欢迎来到世界弹射物语");
    let date = news.data["date"].as_str().unwrap_or_default();
    assert_eq!(date.len(), 19);
    assert!(date.contains('-'));
    assert!(news.data["html"]
        .as_str()
        .unwrap_or_default()
        .contains("欢迎来到星见镇"));
    assert!(news.data["thumbnail_path"].is_null());
    assert_eq!(news.data["added_time"], "2023-08-31 12:00:00");

    let invalid = cn_support::send_request(
        service.port(),
        "/api/index.php/news/get_info",
        &encode_request(&NewsRequest {
            viewer_id: 999_999_999,
            news_id: 1,
            api_count: 2,
        }),
    );
    assert!(invalid.starts_with("HTTP/1.1 400 Bad Request"));
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 新闻响应和虚拟时间 ////
