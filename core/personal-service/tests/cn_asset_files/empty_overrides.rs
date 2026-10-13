// audience: internal
// # cn-asset-empty-overrides
// 下载清单, 文件正文和区间响应读取同一份可用资源.

use super::*;

// //// 从空覆盖文件回退并接续完整覆盖资源 [@x380kkm 2026-09-26] ////
#[test]
fn falls_back_from_empty_archives_with_matching_head_and_range() {
    let root = TempDir::new().unwrap();
    let relative = "archive-common-full/resume.zip";
    write_asset(root.path(), relative, "0123456789");
    write_override_asset(root.path(), relative, "");
    let service = PersonalService::start(root.path(), 0).unwrap();
    let path = format!("/patch/cn/{relative}");

    let download = request(service.port(), "GET", &path);
    assert!(download.starts_with("HTTP/1.1 200 OK"));
    assert_eq!(response_body(&download), "0123456789");
    let head = request(service.port(), "HEAD", &path);
    assert!(head.contains("Content-Length: 10\r\n"));
    assert_eq!(response_body(&head), "");
    let partial = request_with_headers(
        service.port(),
        "GET",
        &path,
        "application/octet-stream",
        &[("Range", "bytes=2-5")],
        b"",
    );
    assert!(partial.starts_with("HTTP/1.1 206 Partial Content"));
    assert!(partial.contains("Content-Range: bytes 2-5/10\r\n"));
    assert_eq!(response_body(&partial), "2345");

    write_override_asset(root.path(), relative, "replacement");
    assert_eq!(
        response_body(&request(service.port(), "GET", &path)),
        "replacement"
    );
    write_override_asset(root.path(), relative, "");
    write_asset(root.path(), relative, "");
    for method in ["GET", "HEAD"] {
        assert!(request(service.port(), method, &path).starts_with("HTTP/1.1 404 Not Found"));
    }
    service.stop().unwrap();
}

// //// 区分必需资源和允许为空的差分清单 [@x380kkm 2026-09-26] ////
#[test]
fn falls_back_from_empty_images_json_and_complete_entity_lists() {
    let root = TempDir::new().unwrap();
    let resources = [
        ("images/banner.png", "image bytes", "/images/banner.png"),
        ("config.json", "{\"ready\":true}", "/config.json"),
        (
            "entities/PathFile.csv",
            "complete manifest",
            "/patch/cn/EntityLists/PathFile.csv",
        ),
        (
            "entities/10939-ios_medium.csv",
            "ios manifest",
            "/patch/cn/entities/10939-ios_medium.csv",
        ),
        (
            "entities/10939-android_medium.csv",
            "android manifest",
            "/patch/cn/entities/10939-android_medium.csv",
        ),
    ];
    for (relative, body, _) in resources {
        write_asset(root.path(), relative, body);
        write_override_asset(root.path(), relative, "");
    }
    write_asset(root.path(), "EntityLists/PathFile.csv", "");
    write_override_asset(root.path(), "EntityLists/PathFile.csv", "");
    write_override_asset(root.path(), "entities/1.4.0-1.4.1.csv", "");
    let service = PersonalService::start(root.path(), 0).unwrap();
    for (relative, body, path) in resources {
        let response = request(service.port(), "GET", path);
        assert!(
            response.starts_with("HTTP/1.1 200 OK"),
            "{path}: {response}"
        );
        assert_eq!(response_body(&response), body, "{path}");
        write_asset(root.path(), relative, "");
        assert!(
            request(service.port(), "GET", path).starts_with("HTTP/1.1 404 Not Found"),
            "{path}"
        );
    }
    let empty_diff = request(service.port(), "GET", "/patch/cn/entities/1.4.0-1.4.1.csv");
    assert!(empty_diff.starts_with("HTTP/1.1 200 OK"));
    assert_eq!(response_body(&empty_diff), "");
    service.stop().unwrap();
}
