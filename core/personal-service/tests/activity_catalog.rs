// audience: internal
// # personal-service-activity-catalog-tests
//
// 该文件验证活动目录筛选, 实例收藏, 默认时间表, 临时开放租约, 图片白名单和缓存失效.

#[allow(dead_code)]
mod support;

use rusqlite::Connection;
use serde_json::Value;
use starpoint_personal_service::PersonalService;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::path::Path;
use support::request_with_headers;
use tempfile::TempDir;

fn response_body(response: &str) -> &str {
    response
        .split_once("\r\n\r\n")
        .expect("response has a body")
        .1
}

fn response_header<'a>(response: &'a str, name: &str) -> Option<&'a str> {
    response
        .split("\r\n\r\n")
        .next()?
        .lines()
        .skip(1)
        .find_map(|line| {
            let (header_name, value) = line.split_once(':')?;
            header_name
                .eq_ignore_ascii_case(name)
                .then_some(value.trim())
        })
}

fn get_binary_response(port: u16, path: &str) -> Vec<u8> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("binary request connects");
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .expect("binary request is written");
    stream
        .shutdown(Shutdown::Write)
        .expect("binary request write side closes");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .expect("binary response is read");
    response
}

fn authorization(service: &PersonalService) -> [(String, String); 1] {
    [(
        "Authorization".to_owned(),
        format!("Bearer {}", service.management_token()),
    )]
}

fn write_manifest(cdn_root: &Path) {
    fs::create_dir_all(cdn_root.join("activity-banners/raid")).expect("banner directory exists");
    fs::write(
        cdn_root.join("activity-catalog.json"),
        r#"{
            "format_version": 1,
            "region": "cn",
            "client_version": "1.8.1",
            "asset_version": "1.4.54",
            "generated_at": "2030-01-01T00:00:00Z",
            "activities": [
                {
                    "activity_id": "raid:1",
                    "name": "春节讨伐",
                    "kind": "raid",
                    "tags": ["seasonal", "co-op"],
                    "description": "春节限时活动",
                    "banner_key": "raid/spring.webp",
                    "banner_width": 1280,
                    "banner_height": 720,
                    "image_candidates": [
                        {
                            "key": "raid/spring-home.webp",
                            "width": 1000,
                            "height": 184,
                            "source_type": "home_banner",
                            "evidence": "home_banner_master:raid:1"
                        }
                    ],
                    "default_start_at_ms": 4102444800000,
                    "default_end_at_ms": 4102617600000
                },
                {
                    "activity_id": "story:2",
                    "name": "主线活动",
                    "kind": "story",
                    "tags": ["main"],
                    "description": "常驻内容"
                }
            ]
        }"#,
    )
    .expect("manifest is written");
    fs::write(
        cdn_root.join("activity-banners/raid/spring.webp"),
        b"RIFF0000WEBPvalid-test-image",
    )
    .expect("banner is written");
    fs::write(
        cdn_root.join("activity-banners/raid/spring-home.webp"),
        b"RIFF0000WEBPhome-test-image",
    )
    .expect("home banner is written");
    fs::write(
        cdn_root.join("activity-banners/raid/unreferenced.webp"),
        b"RIFF0000WEBPvalid-test-image",
    )
    .expect("unreferenced banner is written");
}

fn write_permanent_activity_manifest(cdn_root: &Path) {
    fs::write(
        cdn_root.join("activity-catalog.json"),
        r#"{
            "format_version": 1,
            "region": "cn",
            "client_version": "1.8.4",
            "asset_version": "1.4.64",
            "activities": [
                {
                    "activity_id": "daily-week:1",
                    "name": "每日素材",
                    "kind": "daily",
                    "tags": ["daily"],
                    "description": "每日素材关卡",
                    "default_start_at_ms": 1483214400000,
                    "default_end_at_ms": 1514750400000
                }
            ]
        }"#,
    )
    .expect("permanent activity manifest is written");
}

fn get_catalog(service: &PersonalService, query: &str) -> Value {
    let auth = authorization(service);
    let headers = [("Authorization", auth[0].1.as_str())];
    let response = request_with_headers(
        service.port(),
        "GET",
        &format!("/v1/activities/catalog{query}"),
        "application/json",
        &headers,
        b"",
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    serde_json::from_str(response_body(&response)).expect("catalog response is JSON")
}

fn set_virtual_time(service: &PersonalService, iso: &str) {
    let auth = authorization(service);
    let headers = [("Authorization", auth[0].1.as_str())];
    let body = format!(r#"{{"enabled":true,"iso":"{iso}","rate":1.0}}"#);
    let response = request_with_headers(
        service.port(),
        "PUT",
        "/v1/time",
        "application/json",
        &headers,
        body.as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
}

fn update_activity(service: &PersonalService, method: &str, path: &str, body: &[u8]) -> Value {
    let auth = authorization(service);
    let headers = [("Authorization", auth[0].1.as_str())];
    let response = request_with_headers(
        service.port(),
        method,
        path,
        "application/json",
        &headers,
        body,
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    serde_json::from_str(response_body(&response)).expect("activity response is JSON")
}

fn catalog_activity<'a>(catalog: &'a Value, activity_id: &str) -> &'a Value {
    catalog["activities"]
        .as_array()
        .expect("activities are an array")
        .iter()
        .find(|activity| activity["activity_id"] == activity_id)
        .expect("activity exists in catalog")
}

// //// 验证未配置永久活动使用长期开放基线 [@x380kkm 2026-08-30] ////
#[test]
fn keeps_unscheduled_permanent_activity_open_in_catalog() {
    let root = TempDir::new().expect("service root exists");
    let cdn_root = TempDir::new().expect("CN asset root exists");
    write_permanent_activity_manifest(cdn_root.path());
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
        .expect("service starts");

    let catalog = get_catalog(&service, "?q=daily-week%3A1");
    let activity = catalog_activity(&catalog, "daily-week:1");
    assert_eq!(activity["status"], "open");
    assert_eq!(activity["underlying_status"], "open");
    assert_eq!(activity["is_open"], true);
    assert_eq!(activity["mode"], "always");
    let future_catalog = get_catalog(
        &service,
        "?q=daily-week%3A1&from=2099-01-01&to=2099-01-01",
    );
    assert_eq!(future_catalog["total"], 1);

    let created = update_activity(
        &service,
        "POST",
        "/v1/activities/daily-week%3A1/temporary-open",
        br#"{}"#,
    );
    assert_eq!(created["underlying_status"], "open");
    let deleted = update_activity(
        &service,
        "DELETE",
        "/v1/activities/daily-week%3A1/temporary-open",
        b"",
    );
    assert_eq!(deleted["underlying_status"], "open");
    assert_eq!(deleted["status"], "open");
}
// //// /验证未配置永久活动使用长期开放基线 ////

// //// 验证活动目录筛选和收藏持久化 [@x380kkm 2026-08-19] ////
#[test]
fn filters_catalog_and_persists_instance_favorite() {
    let root = TempDir::new().expect("service root exists");
    let cdn_root = TempDir::new().expect("CN asset root exists");
    write_manifest(cdn_root.path());
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
        .expect("service starts");

    let initial = get_catalog(&service, "?q=seasonal&kind=raid");
    assert_eq!(initial["total"], 1);
    assert_eq!(initial["asset_version"], "1.4.54");
    assert_eq!(initial["activities"][0]["activity_id"], "raid:1");
    assert_eq!(initial["activities"][0]["status"], "open");
    assert_eq!(initial["activities"][0]["is_open"], true);
    assert_eq!(initial["activities"][0]["mode"], "window");
    assert_eq!(initial["activities"][0]["period"], "once");
    assert_eq!(initial["activities"][0]["banner_width"], 1280);
    assert_eq!(initial["activities"][0]["banner_height"], 720);
    assert_eq!(
        initial["activities"][0]["banner_source_type"],
        "activity_banner"
    );
    assert_eq!(
        initial["activities"][0]["image_candidates"]
            .as_array()
            .expect("image candidates are an array")
            .len(),
        2
    );
    assert_eq!(
        initial["activities"][0]["image_candidates"][1]["source_type"],
        "home_banner"
    );
    assert_eq!(
        initial["activities"][0]["banner_url"],
        "/manage/assets/activity-banners/raid/spring.webp"
    );
    set_virtual_time(&service, "2100-01-01T00:01:00.000Z");
    let open = get_catalog(&service, "?q=seasonal&kind=raid");
    assert_eq!(open["activities"][0]["status"], "open");
    assert_eq!(open["activities"][0]["is_open"], true);
    assert_eq!(get_catalog(&service, "?from=2101-01-01")["total"], 0);
    assert_eq!(
        get_catalog(&service, "?from=4102617600000&to=4102617600000")["total"],
        0
    );
    assert_eq!(
        get_catalog(&service, "?from=2099-12-01&to=2100-12-31")["total"],
        1
    );

    let auth = authorization(&service);
    let headers = [
        ("Authorization", auth[0].1.as_str()),
        ("Content-Type", "application/json"),
    ];
    let favorite = request_with_headers(
        service.port(),
        "PUT",
        "/v1/activities/catalog/raid%3A1/favorite",
        "application/json",
        &headers,
        b"",
    );
    assert!(favorite.starts_with("HTTP/1.1 200 OK"), "{favorite}");

    let legacy_favorite = request_with_headers(
        service.port(),
        "PUT",
        "/v1/activities/catalog/favorite",
        "application/json",
        &headers,
        br#"{"activity_id":"story:2","favorite":true}"#,
    );
    assert!(
        legacy_favorite.starts_with("HTTP/1.1 200 OK"),
        "{legacy_favorite}"
    );

    let disabled = request_with_headers(
        service.port(),
        "PUT",
        "/v1/activities/calendar/raid%3A1",
        "application/json",
        &headers,
        br#"{
            "enabled": false,
            "start_at_ms": 4102444800000,
            "end_at_ms": 4102617600000
        }"#,
    );
    assert!(disabled.starts_with("HTTP/1.1 200 OK"), "{disabled}");
    let disabled_body: Value =
        serde_json::from_str(response_body(&disabled)).expect("legacy schedule response is JSON");
    assert_eq!(disabled_body["mode"], "window");
    assert_eq!(disabled_body["period"], "once");
    let disabled_entries = get_catalog(&service, "?status=disabled");
    assert_eq!(disabled_entries["total"], 1);
    assert_eq!(
        disabled_entries["activities"][0]["schedule"]["enabled"],
        false
    );

    let favorites = get_catalog(&service, "?favorite=true");
    assert_eq!(favorites["total"], 2);
    assert_eq!(favorites["activities"][0]["favorite"], true);
    service.stop().expect("service stops");

    let restarted = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
        .expect("service restarts");
    let persisted = get_catalog(&restarted, "?favorite=true");
    assert_eq!(persisted["total"], 2);
    assert!(persisted["activities"]
        .as_array()
        .expect("activities are an array")
        .iter()
        .any(|activity| activity["activity_id"] == "raid:1"));
    let restarted_auth = authorization(&restarted);
    let restarted_headers = [("Authorization", restarted_auth[0].1.as_str())];
    let removed = request_with_headers(
        restarted.port(),
        "DELETE",
        "/v1/activities/catalog/raid%3A1/favorite",
        "application/json",
        &restarted_headers,
        b"",
    );
    assert!(removed.starts_with("HTTP/1.1 200 OK"), "{removed}");
    assert_eq!(get_catalog(&restarted, "?favorite=true")["total"], 1);
    restarted.stop().expect("restarted service stops");
}
// //// /验证活动目录筛选和收藏持久化 ////

// //// 验证 banner 内容类型和路径隔离 [@x380kkm 2026-08-19] ////
#[test]
fn serves_only_valid_banner_files() {
    let root = TempDir::new().expect("service root exists");
    let cdn_root = TempDir::new().expect("CN asset root exists");
    write_manifest(cdn_root.path());
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
        .expect("service starts");

    let image = request_with_headers(
        service.port(),
        "GET",
        "/manage/assets/activity-banners/raid/spring.webp",
        "image/webp",
        &[],
        b"",
    );
    assert!(image.starts_with("HTTP/1.1 200 OK"), "{image}");
    assert!(image.contains("Content-Type: image/webp\r\n"));
    assert!(image.contains("Cache-Control: private, no-cache\r\n"));
    assert!(response_body(&image).starts_with("RIFF0000WEBP"));
    let etag = response_header(&image, "ETag").expect("banner response has an ETag");

    let home_image = request_with_headers(
        service.port(),
        "GET",
        "/manage/assets/activity-banners/raid/spring-home.webp",
        "image/webp",
        &[],
        b"",
    );
    assert!(home_image.starts_with("HTTP/1.1 200 OK"), "{home_image}");
    assert!(response_body(&home_image).starts_with("RIFF0000WEBP"));

    let manifest_path = cdn_root.path().join("activity-catalog.json");
    let mut manifest = serde_json::from_slice::<Value>(
        &fs::read(&manifest_path).expect("activity manifest is read"),
    )
    .expect("activity manifest is JSON");
    manifest["activities"][0]["image_candidates"] = Value::Array(Vec::new());
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).expect("activity manifest serializes"),
    )
    .expect("activity manifest is updated");
    let removed_home_image = request_with_headers(
        service.port(),
        "GET",
        "/manage/assets/activity-banners/raid/spring-home.webp",
        "image/webp",
        &[],
        b"",
    );
    assert!(
        removed_home_image.starts_with("HTTP/1.1 404 Not Found"),
        "{removed_home_image}"
    );

    let not_modified_headers = [("If-None-Match", etag)];
    let not_modified = request_with_headers(
        service.port(),
        "GET",
        "/manage/assets/activity-banners/raid/spring.webp",
        "image/webp",
        &not_modified_headers,
        b"",
    );
    assert!(
        not_modified.starts_with("HTTP/1.1 304 Not Modified"),
        "{not_modified}"
    );
    assert_eq!(response_header(&not_modified, "ETag"), Some(etag));
    assert_eq!(response_header(&not_modified, "Content-Length"), None);
    assert_eq!(response_body(&not_modified), "");

    fs::write(
        cdn_root.path().join("activity-banners/raid/spring.webp"),
        b"RIFF1111WEBPupdated-test-image",
    )
    .expect("updated banner is written");
    let refreshed = request_with_headers(
        service.port(),
        "GET",
        "/manage/assets/activity-banners/raid/spring.webp",
        "image/webp",
        &not_modified_headers,
        b"",
    );
    assert!(refreshed.starts_with("HTTP/1.1 200 OK"), "{refreshed}");
    assert_ne!(response_header(&refreshed, "ETag"), Some(etag));
    assert!(response_body(&refreshed).starts_with("RIFF1111WEBP"));

    let unreferenced = request_with_headers(
        service.port(),
        "GET",
        "/manage/assets/activity-banners/raid/unreferenced.webp",
        "image/webp",
        &[],
        b"",
    );
    assert!(
        unreferenced.starts_with("HTTP/1.1 404 Not Found"),
        "{unreferenced}"
    );

    let traversal = request_with_headers(
        service.port(),
        "GET",
        "/manage/assets/activity-banners/../activity-catalog.json",
        "image/png",
        &[],
        b"",
    );
    assert!(
        traversal.starts_with("HTTP/1.1 404 Not Found"),
        "{traversal}"
    );

    let wrong_extension = request_with_headers(
        service.port(),
        "GET",
        "/manage/assets/activity-banners/raid/spring.svg",
        "image/svg+xml",
        &[],
        b"",
    );
    assert!(
        wrong_extension.starts_with("HTTP/1.1 404 Not Found"),
        "{wrong_extension}"
    );
    service.stop().expect("service stops");
}
// //// /验证 banner 内容类型和路径隔离 ////

// //// 验证本机物化的完整 CN 活动目录 [@x380kkm 2026-08-19] ////
#[test]
fn loads_materialized_cn_activity_catalog_when_configured() {
    let Ok(cdn_root) = env::var("STARPOINT_CN_CDN_ROOT") else {
        return;
    };
    let root = TempDir::new().expect("service root exists");
    let service = PersonalService::start_with_cdn_root(root.path(), 0, &cdn_root)
        .expect("service starts with the materialized CN catalog");

    let catalog = get_catalog(&service, "");
    let activities = catalog["activities"]
        .as_array()
        .expect("activities are an array");
    assert_eq!(activities.len(), 1000);
    let activity_ids = activities
        .iter()
        .map(|activity| {
            activity["activity_id"]
                .as_str()
                .expect("activity ID is text")
        })
        .collect::<HashSet<_>>();
    assert_eq!(activity_ids.len(), activities.len());
    let activities_with_images = activities
        .iter()
        .filter(|activity| {
            activity["image_candidates"]
                .as_array()
                .is_some_and(|candidates| !candidates.is_empty())
        })
        .count();
    assert_eq!(activities_with_images, 704);
    let image_url = activities
        .iter()
        .find_map(|activity| activity["banner_url"].as_str())
        .expect("at least one activity has an image");
    let image = get_binary_response(service.port(), image_url);
    let header_end = image
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("binary response has headers");
    let image_headers = std::str::from_utf8(&image[..header_end]).expect("headers are UTF-8");
    assert!(image_headers.starts_with("HTTP/1.1 200 OK"));
    assert!(image_headers.contains("Content-Type: image/"));

    println!(
        "materialized CN activity catalog: total={}, with_images={}",
        activities.len(),
        activities_with_images
    );
    service.stop().expect("service stops");
}
// //// /验证本机物化的完整 CN 活动目录 ////

// //// 验证关闭, 临时开放和周期规则持久化 [@x380kkm 2026-08-19] ////
#[test]
fn persists_manual_and_periodic_activity_rules() {
    let root = TempDir::new().expect("service root exists");
    let cdn_root = TempDir::new().expect("CN asset root exists");
    write_manifest(cdn_root.path());
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
        .expect("service starts");

    let closed = update_activity(&service, "POST", "/v1/activities/raid%3A1/close", br#"{}"#);
    assert_eq!(closed["mode"], "manual");
    assert_eq!(closed["status"], "disabled");
    service.stop().expect("service stops");

    let restarted = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
        .expect("service restarts");
    let persisted = get_catalog(&restarted, "?status=disabled");
    let persisted_activity = catalog_activity(&persisted, "raid:1");
    assert_eq!(persisted_activity["mode"], "manual");
    assert_eq!(persisted_activity["is_open"], false);

    let opened = update_activity(&restarted, "POST", "/v1/activities/raid%3A1/open", br#"{}"#);
    assert!(opened["temporary_open_until_ms"].is_number());
    assert_eq!(opened["underlying_status"], "disabled");
    assert_eq!(opened["status"], "open");
    update_activity(
        &restarted,
        "DELETE",
        "/v1/activities/raid%3A1/temporary-open",
        b"",
    );

    let window = update_activity(
        &restarted,
        "PUT",
        "/v1/activities/raid%3A1/window",
        br#"{
            "start_at":"2030-01-31T00:00:00.000Z",
            "end_at":"2030-01-31T02:00:00.000Z"
        }"#,
    );
    assert_eq!(window["mode"], "window");
    assert_eq!(window["period"], "once");

    update_activity(
        &restarted,
        "PUT",
        "/v1/activities/raid%3A1/period",
        br#"{"period":"daily","interval_days":1}"#,
    );
    assert_eq!(
        get_catalog(
            &restarted,
            "?from=2029-01-01T00:00:00.000Z&to=2029-12-31T23:59:59.999Z"
        )["total"],
        0
    );
    assert_eq!(
        get_catalog(
            &restarted,
            "?from=2030-01-31T02:00:00.000Z&to=2030-01-31T02:00:00.000Z"
        )["total"],
        0
    );
    set_virtual_time(&restarted, "2030-02-01T01:00:00.000Z");
    let daily = get_catalog(&restarted, "?q=raid%3A1");
    let daily_activity = catalog_activity(&daily, "raid:1");
    assert_eq!(daily_activity["period"], "daily");
    assert_eq!(daily_activity["status"], "open");
    assert!(daily_activity["active_start_at_ms"].is_number());
    assert!(daily_activity["next_start_at_ms"].is_number());
    assert_eq!(
        get_catalog(&restarted, "?from=2031-06-01&to=2031-06-02")["total"],
        1
    );

    update_activity(
        &restarted,
        "PUT",
        "/v1/activities/raid%3A1/period",
        br#"{"period":"weekly","interval_days":1}"#,
    );
    set_virtual_time(&restarted, "2030-02-07T01:00:00.000Z");
    assert_eq!(
        catalog_activity(&get_catalog(&restarted, "?q=raid%3A1"), "raid:1")["status"],
        "open"
    );

    let interval = update_activity(
        &restarted,
        "PUT",
        "/v1/activities/raid%3A1/period",
        br#"{"period":"interval_days","interval_days":3}"#,
    );
    assert_eq!(interval["interval_days"], 3);
    set_virtual_time(&restarted, "2030-02-02T01:00:00.000Z");
    let gap = get_catalog(&restarted, "?q=raid%3A1");
    let gap_activity = catalog_activity(&gap, "raid:1");
    assert_eq!(gap_activity["status"], "ended");
    assert!(gap_activity["next_start_at_ms"].is_number());
    set_virtual_time(&restarted, "2030-02-03T01:00:00.000Z");
    assert_eq!(
        catalog_activity(&get_catalog(&restarted, "?q=raid%3A1"), "raid:1")["status"],
        "open"
    );

    update_activity(
        &restarted,
        "PUT",
        "/v1/activities/raid%3A1/period",
        br#"{"period":"monthly","interval_days":1}"#,
    );
    set_virtual_time(&restarted, "2030-03-31T01:00:00.000Z");
    assert_eq!(
        catalog_activity(&get_catalog(&restarted, "?q=raid%3A1"), "raid:1")["status"],
        "open"
    );
    assert_eq!(
        get_catalog(
            &restarted,
            "?from=2030-02-01T00:00:00.000Z&to=2030-02-27T23:59:59.999Z"
        )["total"],
        0
    );
    assert_eq!(
        get_catalog(
            &restarted,
            "?from=2030-02-28T00:00:00.000Z&to=2030-02-28T23:59:59.999Z"
        )["total"],
        1
    );
    set_virtual_time(&restarted, "2030-02-28T01:00:00.000Z");
    let moved_backward = get_catalog(&restarted, "?q=raid%3A1");
    let moved_backward_activity = catalog_activity(&moved_backward, "raid:1");
    assert_eq!(moved_backward_activity["period"], "monthly");
    assert_eq!(moved_backward_activity["status"], "open");

    update_activity(
        &restarted,
        "PUT",
        "/v1/activities/raid%3A1/window",
        br#"{
            "start_at":"2400-01-31T00:00:00.000Z",
            "end_at":"2400-01-31T02:00:00.000Z"
        }"#,
    );
    update_activity(
        &restarted,
        "PUT",
        "/v1/activities/raid%3A1/period",
        br#"{"period":"monthly","interval_days":1}"#,
    );
    assert_eq!(
        get_catalog(&restarted, "?from=1970-01-01&to=2200-12-31")["total"],
        0
    );

    let invalid_interval_auth = authorization(&restarted);
    let invalid_interval_headers = [("Authorization", invalid_interval_auth[0].1.as_str())];
    let invalid_interval = request_with_headers(
        restarted.port(),
        "PUT",
        "/v1/activities/raid%3A1/period",
        "application/json",
        &invalid_interval_headers,
        br#"{"period":"interval_days","interval_days":0}"#,
    );
    assert!(
        invalid_interval.starts_with("HTTP/1.1 400 Bad Request"),
        "{invalid_interval}"
    );

    update_activity(
        &restarted,
        "POST",
        "/v1/activities/raid%3A1/temporary-open",
        br#"{}"#,
    );
    let reset = update_activity(&restarted, "POST", "/v1/activities/reset", br#"{}"#);
    assert_eq!(reset["reset_schedule_count"], 1);
    assert_eq!(reset["reset_temporary_open_count"], 1);
    let reset_catalog = get_catalog(&restarted, "?q=raid%3A1");
    let reset_activity = catalog_activity(&reset_catalog, "raid:1");
    assert_eq!(reset_activity["mode"], "window");
    assert_eq!(reset_activity["temporary_open_until_ms"], Value::Null);
    restarted.stop().expect("restarted service stops");
}
// //// /验证手动开关和周期规则持久化 ////

// //// 验证默认窗口和关闭后的周期规则会重新启用 [@x380kkm 2026-08-19] ////
#[test]
fn uses_manifest_window_and_reenables_periodic_rules() {
    let root = TempDir::new().expect("service root exists");
    let cdn_root = TempDir::new().expect("CN asset root exists");
    write_manifest(cdn_root.path());
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
        .expect("service starts");

    let first_period = update_activity(
        &service,
        "PUT",
        "/v1/activities/raid%3A1/period",
        br#"{"period":"daily","interval_days":1}"#,
    );
    assert_eq!(first_period["mode"], "periodic");
    assert_eq!(first_period["enabled"], true);
    assert_eq!(first_period["start_at_ms"], 4102444800000_i64);
    assert_eq!(first_period["end_at_ms"], 4102617600000_i64);

    let closed = update_activity(&service, "POST", "/v1/activities/raid%3A1/close", br#"{}"#);
    assert_eq!(closed["enabled"], false);

    let window = update_activity(
        &service,
        "PUT",
        "/v1/activities/raid%3A1/window",
        br#"{
            "start_at":"2030-01-31T00:00:00.000Z",
            "end_at":"2030-01-31T02:00:00.000Z"
        }"#,
    );
    assert_eq!(window["enabled"], true);

    let closed_again = update_activity(&service, "POST", "/v1/activities/raid%3A1/close", br#"{}"#);
    assert_eq!(closed_again["enabled"], false);
    let period = update_activity(
        &service,
        "PUT",
        "/v1/activities/raid%3A1/period",
        br#"{"period":"weekly","interval_days":1}"#,
    );
    assert_eq!(period["enabled"], true);
    service.stop().expect("service stops");
}
// //// /验证默认窗口和关闭后的周期规则会重新启用 ////

// //// 验证旧活动表升级为单次窗口规则 [@x380kkm 2026-08-19] ////
#[test]
fn migrates_legacy_activity_schedule_rows() {
    let root = TempDir::new().expect("service root exists");
    let database_path = root.path().join("personal-service.sqlite3");
    let connection = Connection::open(database_path).expect("legacy database opens");
    connection
        .execute_batch(
            "CREATE TABLE activity_schedules (
                 activity_id TEXT PRIMARY KEY,
                 enabled INTEGER NOT NULL,
                 start_at_ms INTEGER NOT NULL,
                 end_at_ms INTEGER NOT NULL,
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL
             );
             INSERT INTO activity_schedules (
                 activity_id, enabled, start_at_ms, end_at_ms, created_at, updated_at
             ) VALUES (
                 'raid:77', 0, 1893542400000, 1893715200000,
                 '2030-01-01T00:00:00.000Z', '2030-01-01T00:00:00.000Z'
             );",
        )
        .expect("legacy activity table is created");
    drop(connection);

    let service = PersonalService::start(root.path(), 0).expect("service starts after migration");
    let auth = authorization(&service);
    let headers = [("Authorization", auth[0].1.as_str())];
    let response = request_with_headers(
        service.port(),
        "GET",
        "/v1/activities/calendar/raid%3A77",
        "application/json",
        &headers,
        b"",
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let schedule: Value =
        serde_json::from_str(response_body(&response)).expect("schedule response is JSON");
    assert_eq!(schedule["mode"], "window");
    assert_eq!(schedule["period"], "once");
    assert_eq!(schedule["status"], "disabled");
    service.stop().expect("service stops");
}
// //// /验证旧活动表升级为单次窗口规则 ////

// //// 验证临时开放按墙钟持久化并回落到底层状态 [@x380kkm 2026-08-24] ////
#[test]
fn temporary_open_lease_persists_and_restores_underlying_status() {
    let root = TempDir::new().expect("service root exists");
    let cdn_root = TempDir::new().expect("CN asset root exists");
    write_manifest(cdn_root.path());
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
        .expect("service starts");
    set_virtual_time(&service, "2099-12-31T00:00:00.000Z");

    let auth = authorization(&service);
    let headers = [("Authorization", auth[0].1.as_str())];
    let unknown = request_with_headers(
        service.port(),
        "POST",
        "/v1/activities/unknown%3A61/temporary-open",
        "application/json",
        &headers,
        br#"{}"#,
    );
    assert!(unknown.starts_with("HTTP/1.1 404 Not Found"), "{unknown}");

    let created = update_activity(
        &service,
        "POST",
        "/v1/activities/raid%3A1/temporary-open",
        br#"{}"#,
    );
    assert_eq!(created["underlying_status"], "not_started");
    assert_eq!(created["status"], "open");
    assert_eq!(created["is_open"], true);
    assert!(created["temporary_open_until_ms"]
        .as_i64()
        .is_some_and(|expires_at_ms| expires_at_ms > 0));
    let lease_duration_ms = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("activity database opens")
        .query_row(
            "SELECT expires_at_ms - opened_at_ms
             FROM activity_temporary_open_leases
             WHERE activity_id = 'raid:1'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("temporary activity lease duration is stored");
    assert_eq!(lease_duration_ms, 86_400_000);
    let active = get_catalog(&service, "");
    let activity = catalog_activity(&active, "raid:1");
    assert_eq!(activity["underlying_status"], "not_started");
    assert_eq!(activity["status"], "open");
    assert_eq!(
        activity["temporary_open_until_ms"],
        created["temporary_open_until_ms"]
    );

    service.stop().expect("service stops");
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
        .expect("service restarts");
    let persisted = get_catalog(&service, "");
    assert_eq!(catalog_activity(&persisted, "raid:1")["status"], "open");

    let connection = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("activity database opens");
    connection
        .execute(
            "UPDATE activity_temporary_open_leases
             SET expires_at_ms = opened_at_ms + 1
             WHERE activity_id = 'raid:1'",
            [],
        )
        .expect("temporary activity lease expires");
    drop(connection);
    let expired = get_catalog(&service, "");
    let activity = catalog_activity(&expired, "raid:1");
    assert_eq!(activity["temporary_open_until_ms"], Value::Null);
    assert_eq!(activity["status"], "not_started");

    update_activity(
        &service,
        "POST",
        "/v1/activities/raid%3A1/temporary-open",
        br#"{}"#,
    );
    let deleted = update_activity(
        &service,
        "DELETE",
        "/v1/activities/raid%3A1/temporary-open",
        b"",
    );
    assert_eq!(deleted["deleted"], true);
    assert_eq!(deleted["status"], "not_started");
    assert_eq!(deleted["temporary_open_until_ms"], Value::Null);

    set_virtual_time(&service, "2100-01-02T00:00:00.000Z");
    update_activity(
        &service,
        "POST",
        "/v1/activities/raid%3A1/temporary-open",
        br#"{}"#,
    );
    let deleted_while_open = update_activity(
        &service,
        "DELETE",
        "/v1/activities/raid%3A1/temporary-open",
        b"",
    );
    assert_eq!(deleted_while_open["underlying_status"], "open");
    assert_eq!(deleted_while_open["status"], "open");
    assert_eq!(deleted_while_open["is_open"], true);
    service.stop().expect("service stops");
}
// //// /验证临时开放按墙钟持久化并回落到底层状态 ////
