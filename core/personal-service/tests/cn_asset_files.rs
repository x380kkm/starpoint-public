// audience: internal
// # cn-asset-files
//
// 这些测试验证个人服务从配置的 CN CDN 根提供受约束的客户端静态文件.

use starpoint_personal_service::PersonalService;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

mod support;

use support::{request, request_with_headers};

// //// 写入临时 CN CSV 资产 [@x380kkm 2026-08-07] ////
fn write_asset(root: &Path, relative_path: &str, body: &str) {
    let asset_root = root.join("cdn").join("cn");
    write_asset_at(&asset_root, relative_path, body);
}
// //// /写入临时 CN CSV 资产 ////

// //// 写入临时覆盖资产 [@x380kkm 2026-08-21] ////
fn write_override_asset(root: &Path, relative_path: &str, body: &str) {
    let override_root = root.join("cdn").join("override");
    write_asset_at(&override_root, relative_path, body);
}
// //// /写入临时覆盖资产 ////

// //// 写入指定 CN CDN 根的资产 [@x380kkm 2026-08-11] ////
fn write_asset_at(asset_root: &Path, relative_path: &str, body: &str) {
    let path = asset_root.join(relative_path);
    fs::create_dir_all(path.parent().expect("asset has a parent directory"))
        .expect("asset directory is created");
    fs::write(path, body).expect("asset file is written");
}
// //// /写入指定 CN CDN 根的资产 ////

// //// 提取 HTTP 响应正文 [@x380kkm 2026-08-07] ////
fn response_body(response: &str) -> &str {
    response
        .split_once("\r\n\r\n")
        .expect("response contains a header terminator")
        .1
}
// //// /提取 HTTP 响应正文 ////

// //// 创建指向资产根外目录的链接 [@x380kkm 2026-08-07] ////
#[cfg(unix)]
fn create_directory_link(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).expect("asset symlink is created");
}

#[cfg(windows)]
fn create_directory_link(target: &Path, link: &Path) {
    let status = std::process::Command::new("pwsh")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$ErrorActionPreference = 'Stop'; New-Item -ItemType Junction -Path $env:STARPOINT_JUNCTION_LINK -Target $env:STARPOINT_JUNCTION_TARGET | Out-Null",
        ])
        .env("STARPOINT_JUNCTION_LINK", link)
        .env("STARPOINT_JUNCTION_TARGET", target)
        .status()
        .expect("PowerShell creates the asset junction");
    assert!(status.success(), "asset junction is created");
}
// //// /创建指向资产根外目录的链接 ////

// //// 提供带查询串的 entities CSV [@x380kkm 2026-08-07] ////
#[test]
fn serves_an_entities_csv_with_security_headers() {
    let root = TempDir::new().expect("temporary service directory is created");
    let csv = "Id,Name\r\n1,Alice\r\n";
    write_asset(root.path(), "entities/Character.csv", csv);
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    let response = request(
        service.port(),
        "GET",
        "/patch/cn/entities/Character.csv?version=1",
    );

    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(response.contains("Content-Type: text/csv; charset=utf-8\r\n"));
    assert!(response.contains("Cache-Control: no-store\r\n"));
    assert!(response.contains("X-Content-Type-Options: nosniff\r\n"));
    assert_eq!(response_body(&response), csv);
    service.stop().expect("service stops cleanly");
}
// //// /提供带查询串的 entities CSV ////

// //// 提供 EntityLists CSV [@x380kkm 2026-08-07] ////
#[test]
fn serves_an_entity_lists_csv() {
    let root = TempDir::new().expect("temporary service directory is created");
    let csv = "Id,Entity\n1,Character\n";
    write_asset(root.path(), "EntityLists/EntityList.csv", csv);
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    let response = request(
        service.port(),
        "GET",
        "/patch/cn/EntityLists/EntityList.csv",
    );

    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert_eq!(response_body(&response), csv);
    service.stop().expect("service stops cleanly");
}
// //// /提供 EntityLists CSV ////

// //// 从显式 CN CDN 根提供实体清单 [@x380kkm 2026-08-11] ////
#[test]
fn serves_an_entity_lists_csv_from_explicit_cdn_root() {
    let root = TempDir::new().expect("temporary service directory is created");
    let cdn_root = TempDir::new().expect("temporary CDN directory is created");
    let csv = "Id,Entity\r\n2,CustomRoot\r\n";
    write_asset_at(cdn_root.path(), "EntityLists/EntityList.csv", csv);
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
        .expect("service starts");

    let response = request(
        service.port(),
        "GET",
        "/patch/cn/EntityLists/EntityList.csv",
    );

    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert_eq!(response_body(&response), csv);
    service.stop().expect("service stops cleanly");
}
// //// /从显式 CN CDN 根提供实体清单 ////

// //// 从 entities 目录回退提供 EntityLists 请求 [@x380kkm 2026-08-11] ////
#[test]
fn serves_an_entity_lists_request_from_an_entities_directory() {
    let root = TempDir::new().expect("temporary service directory is created");
    let csv = "Id,Entity\r\n3,Alias\r\n";
    write_asset(root.path(), "entities/EntityList.csv", csv);
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    let response = request(
        service.port(),
        "GET",
        "/patch/cn/EntityLists/EntityList.csv",
    );

    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert_eq!(response_body(&response), csv);
    service.stop().expect("service stops cleanly");
}
// //// /从 entities 目录回退提供 EntityLists 请求 ////

// //// 提供版本检查使用的空实体清单 [@x380kkm 2026-08-21] ////
#[test]
fn serves_empty_entity_lists_for_version_checks() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    for directory in ["entities", "EntityLists"] {
        let response = request(
            service.port(),
            "GET",
            &format!("/patch/cn/{directory}/empty.csv"),
        );

        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.contains("Content-Type: text/csv; charset=utf-8\r\n"));
        assert_eq!(response_body(&response), "");
    }
    service.stop().expect("service stops cleanly");
}
// //// /提供版本检查使用的空实体清单 ////

// //// 拒绝空的显式 CN CDN 根 [@x380kkm 2026-08-11] ////
#[test]
fn rejects_an_empty_explicit_cdn_root() {
    let root = TempDir::new().expect("temporary service directory is created");
    let result = PersonalService::start_with_cdn_root(root.path(), 0, Path::new(""));

    assert!(result.is_err());
}
// //// /拒绝空的显式 CN CDN 根 ////

// //// 提供本地 CN archive 文件 [@x380kkm 2026-08-22] ////
#[test]
fn serves_an_archive_zip() {
    let root = TempDir::new().expect("temporary service directory is created");
    let archive = b"zip-fixture";
    let archive_path = root
        .path()
        .join("cdn")
        .join("cn")
        .join("archive-common-full")
        .join("pinball-1.4.0-1-test.zip");
    fs::create_dir_all(
        archive_path
            .parent()
            .expect("archive has a parent directory"),
    )
    .expect("archive directory is created");
    fs::write(&archive_path, archive).expect("archive is written");
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    let response = request(
        service.port(),
        "GET",
        "/patch/cn/archive-common-full/pinball-1.4.0-1-test.zip",
    );

    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(response.contains("Content-Type: application/zip\r\n"));
    assert!(response.contains("Accept-Ranges: bytes\r\n"));
    assert_eq!(response_body(&response).as_bytes(), archive);
    service.stop().expect("service stops cleanly");
}
// //// /提供本地 CN archive 文件 ////

// //// 提供补丁归档的 HEAD 和单区间响应 [@x380kkm 2026-08-22] ////
#[test]
fn serves_archive_head_and_single_byte_ranges() {
    let root = TempDir::new().expect("temporary service directory is created");
    let archive = "0123456789";
    let path = "archive-common-full/range-fixture.zip";
    write_asset(root.path(), path, archive);
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let request_path = format!("/patch/cn/{path}");

    let head_response = request(service.port(), "HEAD", &request_path);
    assert!(head_response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(head_response.contains("Content-Type: application/zip\r\n"));
    assert!(head_response.contains("Content-Length: 10\r\n"));
    assert!(head_response.contains("Accept-Ranges: bytes\r\n"));
    assert_eq!(response_body(&head_response), "");

    for (range, content_range, expected_body) in [
        ("bytes=2-5", "bytes 2-5/10", "2345"),
        ("bytes=7-", "bytes 7-9/10", "789"),
        ("bytes=-3", "bytes 7-9/10", "789"),
    ] {
        let response = request_with_headers(
            service.port(),
            "GET",
            &request_path,
            "application/octet-stream",
            &[("Range", range)],
            b"",
        );

        assert!(response.starts_with("HTTP/1.1 206 Partial Content\r\n"));
        assert!(response.contains(&format!("Content-Range: {content_range}\r\n")));
        assert!(response.contains(&format!("Content-Length: {}\r\n", expected_body.len())));
        assert!(response.contains("Accept-Ranges: bytes\r\n"));
        assert_eq!(response_body(&response), expected_body);
    }
    service.stop().expect("service stops cleanly");
}
// //// /提供补丁归档的 HEAD 和单区间响应 ////

// //// 拒绝无法满足的补丁归档区间 [@x380kkm 2026-08-22] ////
#[test]
fn rejects_invalid_and_multiple_archive_ranges() {
    let root = TempDir::new().expect("temporary service directory is created");
    let archive = "0123456789";
    let path = "archive-common-full/range-error-fixture.zip";
    write_asset(root.path(), path, archive);
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let request_path = format!("/patch/cn/{path}");

    for range in ["bytes=10-", "items=0-1", "bytes=0-1,4-5"] {
        let response = request_with_headers(
            service.port(),
            "GET",
            &request_path,
            "application/octet-stream",
            &[("Range", range)],
            b"",
        );

        assert!(response.starts_with("HTTP/1.1 416 Range Not Satisfiable\r\n"));
        assert!(response.contains("Content-Range: bytes */10\r\n"));
        assert!(response.contains("Content-Length: 0\r\n"));
        assert!(response.contains("Accept-Ranges: bytes\r\n"));
        assert_eq!(response_body(&response), "");
    }
    service.stop().expect("service stops cleanly");
}
// //// /拒绝无法满足的补丁归档区间 ////

// //// 提供本地 CN iOS archive 文件 [@x380kkm 2026-08-18] ////
#[test]
fn serves_an_ios_archive_zip() {
    let root = TempDir::new().expect("temporary service directory is created");
    let archive = b"ios-zip-fixture";
    let archive_path = root
        .path()
        .join("cdn")
        .join("cn")
        .join("archive-ios-full")
        .join("pinball-1.4.0-1-ios.zip");
    fs::create_dir_all(
        archive_path
            .parent()
            .expect("archive has a parent directory"),
    )
    .expect("archive directory is created");
    fs::write(&archive_path, archive).expect("archive is written");
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    let response = request(
        service.port(),
        "GET",
        "/patch/cn/archive-ios-full/pinball-1.4.0-1-ios.zip",
    );

    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(response.contains("Content-Type: application/zip\r\n"));
    assert_eq!(response_body(&response).as_bytes(), archive);
    service.stop().expect("service stops cleanly");
}
// //// /提供本地 CN iOS archive 文件 ////

// //// 缺失资产返回 404 [@x380kkm 2026-08-07] ////
#[test]
fn returns_not_found_for_a_missing_csv() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    let response = request(service.port(), "GET", "/patch/cn/entities/Missing.csv");

    assert!(response.starts_with("HTTP/1.1 404 Not Found\r\n"));
    service.stop().expect("service stops cleanly");
}
// //// /缺失资产返回 404 ////

// //// 非 GET 资产请求返回 405 [@x380kkm 2026-08-07] ////
#[test]
fn rejects_a_non_get_asset_request() {
    let root = TempDir::new().expect("temporary service directory is created");
    write_asset(root.path(), "entities/Character.csv", "Id\n1\n");
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    let response = request(service.port(), "POST", "/patch/cn/entities/Character.csv");

    assert!(response.starts_with("HTTP/1.1 405 Method Not Allowed\r\n"));
    assert!(response.contains("Allow: GET\r\n"));
    service.stop().expect("service stops cleanly");
}
// //// /非 GET 资产请求返回 405 ////

// //// 拒绝点段和反斜杠目录穿越 [@x380kkm 2026-08-07] ////
#[test]
fn rejects_directory_traversal_paths() {
    let root = TempDir::new().expect("temporary service directory is created");
    write_asset(root.path(), "entities/Character.csv", "Id\n1\n");
    write_asset(root.path(), "secret.csv", "secret\n");
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    for path in [
        "/patch/cn/entities/./Character.csv",
        "/patch/cn/entities/../secret.csv",
        "/patch/cn/entities/..\\secret.csv",
        "/patch/cn/entities\\Character.csv",
    ] {
        let response = request(service.port(), "GET", path);
        assert!(
            response.starts_with("HTTP/1.1 404 Not Found\r\n"),
            "path must be rejected: {path}"
        );
        assert!(!response.contains("secret\n"));
    }

    service.stop().expect("service stops cleanly");
}
// //// /拒绝点段和反斜杠目录穿越 ////

// //// 拒绝非 CSV 和资产根外目录链接 [@x380kkm 2026-08-07] ////
#[test]
fn rejects_non_csv_and_linked_assets() {
    let root = TempDir::new().expect("temporary service directory is created");
    write_asset(root.path(), "entities/Character.json", "private-json\n");
    let outside_directory = root.path().join("outside-assets");
    fs::create_dir(&outside_directory).expect("outside asset directory is created");
    fs::write(outside_directory.join("Secret.csv"), "outside-root\n")
        .expect("outside asset is written");
    let link_path = root
        .path()
        .join("cdn")
        .join("cn")
        .join("entities")
        .join("external");
    create_directory_link(&outside_directory, &link_path);
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    for (path, hidden_body) in [
        ("/patch/cn/entities/Character.json", "private-json\n"),
        ("/patch/cn/entities/external/Secret.csv", "outside-root\n"),
    ] {
        let response = request(service.port(), "GET", path);
        assert!(
            response.starts_with("HTTP/1.1 404 Not Found\r\n"),
            "asset path must be rejected: {path}"
        );
        assert!(!response.contains(hidden_body));
    }

    service.stop().expect("service stops cleanly");
}
// //// /拒绝非 CSV 和资产根外目录链接 ////

// //// 提供通用静态文件的 GET 和 HEAD 响应 [@x380kkm 2026-08-21] ////
#[test]
fn serves_general_static_assets_with_media_types_and_head_length() {
    let root = TempDir::new().expect("temporary service directory is created");
    let version_body = "[{\"version\":\"1669961576892\"}]\n";
    let area_body = "{\"open\":\"1\"}\n";
    write_asset(
        root.path(),
        "protocols/leiting/sensitive/part/common_version.txt",
        version_body,
    );
    write_asset(root.path(), "area/config.json", area_body);
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    let get_response = request(
        service.port(),
        "GET",
        "/protocols/leiting/sensitive/part/common_version.txt?version=1",
    );
    assert!(get_response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(get_response.contains("Content-Type: text/plain; charset=utf-8\r\n"));
    assert_eq!(response_body(&get_response), version_body);

    let area_response = request(service.port(), "GET", "/area/config.json");
    assert!(area_response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(area_response.contains("Content-Type: application/json\r\n"));
    assert_eq!(response_body(&area_response), area_body);

    let head_response = request(service.port(), "HEAD", "/area/config%2Ejson");
    assert!(head_response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(head_response.contains("Content-Type: application/json\r\n"));
    assert!(head_response.contains(&format!("Content-Length: {}\r\n", area_body.len())));
    assert_eq!(response_body(&head_response), "");
    service.stop().expect("service stops cleanly");
}
// //// /提供通用静态文件的 GET 和 HEAD 响应 ////

// //// 返回游戏敏感词版本清单的空重定向 [@x380kkm 2026-08-21] ////
#[test]
fn redirects_absent_game_sensitive_version_lists() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    for path in [
        "/protocols/leiting/sensitive/part/wf_version.txt",
        "/protocols/leiting/sensitive/part/wf-text_version.txt",
    ] {
        let response = request(service.port(), "GET", path);
        assert!(
            response.starts_with("HTTP/1.1 302 Found\r\n"),
            "path must return the empty redirect: {path}"
        );
        assert_eq!(response_body(&response), "");
    }

    service.stop().expect("service stops cleanly");
}
// //// /返回游戏敏感词版本清单的空重定向 ////

// //// 拒绝通用静态文件路径逃逸 [@x380kkm 2026-08-21] ////
#[test]
fn rejects_unsafe_general_static_asset_paths() {
    let root = TempDir::new().expect("temporary service directory is created");
    let outside_directory = root.path().join("outside-static");
    fs::create_dir(&outside_directory).expect("outside directory is created");
    fs::write(outside_directory.join("secret.txt"), "outside-root\n")
        .expect("outside file is written");
    let link_path = root.path().join("cdn").join("cn").join("linked");
    fs::create_dir_all(link_path.parent().expect("link has a parent directory"))
        .expect("asset root is created");
    create_directory_link(&outside_directory, &link_path);
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    for path in [
        "/../outside-static/secret.txt",
        "/%2e%2e/outside-static/secret.txt",
        "/area/%2e%2e/config.json",
        "/area%2fconfig.json",
        "/area%5cconfig.json",
        "/linked/secret.txt",
    ] {
        let response = request(service.port(), "GET", path);
        assert!(
            response.starts_with("HTTP/1.1 404 Not Found\r\n"),
            "path must be rejected: {path}"
        );
        assert!(!response.contains("outside-root"));
    }

    service.stop().expect("service stops cleanly");
}
// //// /拒绝通用静态文件路径逃逸 ////

// //// 优先提供覆盖层中的补丁和通用静态文件 [@x380kkm 2026-08-21] ////
#[test]
fn prioritizes_override_assets() {
    let root = TempDir::new().expect("temporary service directory is created");
    write_asset(root.path(), "entities/Character.csv", "bundle-character\n");
    write_override_asset(
        root.path(),
        "entities/Character.csv",
        "override-character\n",
    );
    write_asset(root.path(), "area/config.json", "{\"source\":\"bundle\"}\n");
    write_override_asset(
        root.path(),
        "area/config.json",
        "{\"source\":\"override\"}\n",
    );
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    let patch_response = request(service.port(), "GET", "/patch/cn/entities/Character.csv");
    assert_eq!(response_body(&patch_response), "override-character\n");

    let static_response = request(service.port(), "GET", "/area/config.json");
    assert_eq!(
        response_body(&static_response),
        "{\"source\":\"override\"}\n"
    );
    service.stop().expect("service stops cleanly");
}
// //// /优先提供覆盖层中的补丁和通用静态文件 ////

// //// 为精确展示文档路径提供空文本格式 [@x380kkm 2026-08-21] ////
#[test]
fn serves_optional_text_documents() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    for path in [
        "/protocols/leiting/privacy/wf.txt",
        "/protocols/leiting/license/common.txt",
        "/protocols/leiting/updateTips/iOS/common.txt",
    ] {
        let response = request(service.port(), "GET", path);
        assert!(
            response.starts_with("HTTP/1.1 200 OK\r\n"),
            "path must return a text response: {path}"
        );
        assert!(response.contains("Content-Type: text/plain; charset=utf-8\r\n"));
        assert_eq!(response_body(&response), "");
    }

    let unknown_response = request(
        service.port(),
        "GET",
        "/protocols/leiting/privacy/unknown.txt",
    );
    assert!(unknown_response.starts_with("HTTP/1.1 404 Not Found\r\n"));
    service.stop().expect("service stops cleanly");
}
// //// /为精确展示文档路径提供空文本格式 ////

// //// 返回 SKAN 查询的文本兼容响应 [@x380kkm 2026-08-21] ////
#[test]
fn serves_skan_query_detail_compatibility_response() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    let response = request(
        service.port(),
        "GET",
        "/api/skan/query_detail?gameCode=wf&sign=expired&timestamp=0",
    );
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(response.contains("Content-Type: text/plain; charset=utf-8\r\n"));
    assert_eq!(
        response_body(&response),
        "{status=1, message=签名已过失效时间}"
    );
    service.stop().expect("service stops cleanly");
}
// //// /返回 SKAN 查询的文本兼容响应 ////
