// audience: internal
// # personal-service-cn-comic-tests
//
// 该文件通过漫画列表和图片 HTTP 接口验证资源替换后的缓存地址.

#[path = "support/cn.rs"]
mod cn_support;
mod support;

use cn_support::{decode_response, encode_request, send_request, SignupData, SignupRequest};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::fs::{self, File, FileTimes};
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};
use tempfile::TempDir;

// //// 写入带明确修改时间的漫画图片 [@x380kkm 2026-09-21] ////
fn write_image(root: &Path, relative_path: &str, bytes: &[u8], timestamp: u64) {
    let path = root.join("cdn/cn/comic/0").join(relative_path);
    fs::create_dir_all(path.parent().expect("image has a parent"))
        .expect("image directory is created");
    fs::write(&path, bytes).expect("comic image is written");
    File::options()
        .write(true)
        .open(path)
        .expect("comic image is opened")
        .set_times(FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(timestamp)))
        .expect("comic image modification time is set");
}

// //// 创建独立漫画目录和玩家会话 [@x380kkm 2026-09-21] ////
fn start_comic_library() -> (TempDir, PersonalService, i64) {
    let root = TempDir::new().expect("temporary service directory is created");
    for relative_path in ["main/1.png", "thumbnail_l/1.jpg", "thumbnail_s/1.jpg"] {
        write_image(root.path(), relative_path, b"original comic", 1_700_000_000);
    }
    fs::write(
        root.path().join("cdn/cn/comic/index.json"),
        serde_json::to_vec(&json!([{"kind": 0, "episode": 1, "title": "漫画"}])).unwrap(),
    )
    .expect("comic catalog is written");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 7 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    (root, service, viewer_id)
}

// //// 读取漫画列表中的图片地址 [@x380kkm 2026-09-21] ////
fn image_urls(service: &PersonalService, viewer_id: i64) -> Value {
    let response = send_request(
        service.port(),
        "/api/index.php/comic/get_list",
        &encode_request(&json!({"viewer_id": viewer_id, "kind": 0, "page_index": 0})),
    );
    decode_response::<Value>(&response).data["comic_list"][0]["media_image"].clone()
}

// //// 核对带缓存查询参数的图片 HTTP 正文 [@x380kkm 2026-09-21] ////
fn assert_image_bytes(service: &PersonalService, image_url: &Value, expected: &[u8]) {
    let url =
        url::Url::parse(image_url.as_str().expect("image URL is text")).expect("image URL parses");
    let target = format!(
        "{}?{}",
        url.path(),
        url.query().expect("image URL has a revision")
    );
    let response = support::request_bytes(service.port(), "GET", &target);
    assert!(response.starts_with(b"HTTP/1.1 200 OK"));
    let body_start = response
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .unwrap()
        + 4;
    assert_eq!(&response[body_start..], expected);
}

// //// 资源版本和单张图片替换各自更新缓存地址 [@x380kkm 2026-09-21] ////
#[test]
fn changes_comic_urls_for_resource_versions_and_image_replacements() {
    let (root, service, viewer_id) = start_comic_library();
    let manifest_path = root.path().join("cdn/cn/path");
    fs::write(
        &manifest_path,
        br#"{"info":{"target_asset_version":"1.4.62","eventual_target_asset_version":"1.4.63"}}"#,
    )
    .expect("resource manifest is written");
    let original = image_urls(&service, viewer_id);
    assert_eq!(image_urls(&service, viewer_id), original);
    assert!(original["main"].as_str().unwrap().contains("?v=1.4.63-"));

    write_image(root.path(), "main/1.png", b"modified comic", 1_700_000_001);
    let replaced = image_urls(&service, viewer_id);
    assert_ne!(replaced["main"], original["main"]);
    assert_eq!(replaced["thumbnail_l"], original["thumbnail_l"]);
    assert_eq!(replaced["thumbnail_s"], original["thumbnail_s"]);
    assert_image_bytes(&service, &replaced["main"], b"modified comic");

    fs::write(
        manifest_path,
        br#"{"info":{"target_asset_version":"1.4.64","eventual_target_asset_version":""}}"#,
    )
    .expect("resource version is changed");
    let upgraded = image_urls(&service, viewer_id);
    for size in ["main", "thumbnail_l", "thumbnail_s"] {
        assert_ne!(upgraded[size], replaced[size]);
        assert!(upgraded[size].as_str().unwrap().contains("?v=1.4.64-"));
    }
}

// //// 独立漫画目录按文件替换更新缩略图地址 [@x380kkm 2026-09-21] ////
#[test]
fn changes_comic_urls_without_a_resource_manifest() {
    let (root, service, viewer_id) = start_comic_library();
    assert!(!root.path().join("cdn/cn/path").exists());
    let original = image_urls(&service, viewer_id);
    write_image(
        root.path(),
        "thumbnail_s/1.jpg",
        b"modified comic",
        1_700_000_001,
    );
    let replaced = image_urls(&service, viewer_id);
    assert_ne!(replaced["thumbnail_s"], original["thumbnail_s"]);
    assert_eq!(replaced["main"], original["main"]);
    assert_image_bytes(&service, &replaced["thumbnail_s"], b"modified comic");
}

// //// 空漫画图片复用同话尺寸且原图恢复后重新提供原图 [@x380kkm 2026-09-26] ////
#[test]
fn recovers_empty_comic_images_from_the_same_episode() {
    let (root, service, viewer_id) = start_comic_library();
    let original = image_urls(&service, viewer_id);
    write_image(root.path(), "main/1.png", b"", 1_700_000_001);
    write_image(root.path(), "thumbnail_s/1.jpg", b"", 1_700_000_001);
    let recovered = image_urls(&service, viewer_id);
    assert_eq!(recovered["main"], recovered["thumbnail_l"]);
    assert_eq!(recovered["thumbnail_s"], recovered["thumbnail_l"]);
    assert_image_bytes(&service, &original["main"], b"original comic");
    let fallback = support::request_bytes(service.port(), "GET", "/comic/0/main/1.png");
    let fallback_headers = String::from_utf8_lossy(&fallback);
    assert!(fallback_headers.contains("Content-Type: image/jpeg"));
    assert!(fallback_headers.contains("Cache-Control: no-cache"));

    write_image(root.path(), "main/1.png", b"restored comic", 1_700_000_002);
    let restored = image_urls(&service, viewer_id);
    assert_ne!(restored["main"], recovered["main"]);
    assert_image_bytes(&service, &restored["main"], b"restored comic");

    write_image(root.path(), "main/1.png", b"", 1_700_000_003);
    write_image(root.path(), "thumbnail_l/1.jpg", b"", 1_700_000_003);
    let missing = support::request_bytes(service.port(), "GET", "/comic/0/main/1.png");
    assert!(missing.starts_with(b"HTTP/1.1 404 Not Found"));
    let list = send_request(
        service.port(),
        "/api/index.php/comic/get_list",
        &encode_request(&json!({"viewer_id":viewer_id,"kind":0,"page_index":0})),
    );
    assert!(list.starts_with("HTTP/1.1 503 Service Unavailable"));
    service.stop().unwrap();
}
// //// /空漫画图片复用同话尺寸且原图恢复后重新提供原图 ////
