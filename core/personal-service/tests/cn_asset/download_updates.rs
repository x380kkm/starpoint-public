// audience: internal
// # cn-asset-download-updates
// 这些检查通过客户端请求链验证活动资源差分和已安装资源的复用.

use super::*;

// //// 从管理接口保存活动与虚拟时钟 [@x380kkm 2026-09-21] ////
fn update_management(service: &PersonalService, path: &str, body: Value) {
    let authorization = format!("Bearer {}", service.management_token());
    let response = request_with_headers(
        service.port(),
        "PUT",
        path,
        "application/json",
        &[("Authorization", &authorization)],
        &serde_json::to_vec(&body).unwrap(),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
}
// //// /从管理接口保存活动与虚拟时钟 ////

// //// 请求两平台的安装版本并核对活动差分 [@x380kkm 2026-09-21] ////
fn assert_projection_update(service: &PersonalService, installed: &str) -> String {
    let mut target = None;
    for device in ["1", "2"] {
        let response = send_request_with_device_kind(
            service.port(),
            "/api/index.php/asset/get_path",
            &encode_request(&json!({})),
            installed,
            device,
        );
        let path = decode_response::<Value>(&response);
        assert!(path.data_headers.asset_update);
        assert!(path.data["full"].is_null());
        assert_eq!(path.data["info"]["is_initial"], false);
        let groups = path.data["diff"].as_array().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["original_version"], installed);
        let archives = groups[0]["archive"].as_array().unwrap();
        assert_eq!(archives.len(), 1);
        assert!(archives[0]["location"]
            .as_str()
            .unwrap()
            .contains("/archive-common-diff/starpoint-cn-activity-projection-"));
        let download_url = url::Url::parse(archives[0]["location"].as_str().unwrap()).unwrap();
        let download = support::request_bytes(service.port(), "GET", download_url.path());
        assert!(download.starts_with(b"HTTP/1.1 200 OK"));
        let offset = download
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .unwrap()
            + 4;
        let body = &download[offset..];
        assert_eq!(body.len() as u64, archives[0]["size"].as_u64().unwrap());
        assert_eq!(STANDARD.encode(Sha256::digest(body)), archives[0]["sha256"]);
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(body)).unwrap();
        let mut payload = Vec::new();
        std::io::Read::read_to_end(&mut archive.by_index(0).unwrap(), &mut payload).unwrap();
        assert!(!payload.is_empty());
        let version = groups[0]["version"].as_str().unwrap();
        assert_eq!(path.data["info"]["eventual_target_asset_version"], version);
        if let Some(previous) = &target {
            assert_eq!(previous, version);
        }
        target = Some(version.to_owned());
        println!(
            "device={device} {installed}->{version}: {} bytes",
            archives[0]["size"]
        );
    }
    target.unwrap()
}
// //// /请求两平台的安装版本并核对活动差分 ////

// //// 修改活动窗口和虚拟时间后仅传送活动主数据差分 [@x380kkm 2026-09-21] ////
#[test]
fn updates_activity_windows_without_reinstalling_assets() {
    let root = TempDir::new().unwrap();
    write_archive(
        root.path(),
        "archive-common-full",
        "base.zip",
        b"installed asset",
    );
    write_path_manifest(
        root.path(),
        json!({
            "info": {"client_asset_version": "", "target_asset_version": "1.4.54", "eventual_target_asset_version": "1.4.54", "is_initial": true, "latest_maj_first_version": "1.4.54"},
            "full": {"version": "1.4.54", "archive": [{"location": "archive-common-full/base.zip", "size": 15, "sha256": STANDARD.encode(Sha256::digest(b"installed asset"))}]},
            "diff": [], "asset_version_hash": "activity-update"
        }),
    );
    let entities = root.path().join("cdn/cn/entities");
    fs::create_dir_all(&entities).unwrap();
    fs::write(
        entities.join("PathFile.csv"),
        "base.atf,1.4.54,15,base,common\n",
    )
    .unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    update_management(
        &service,
        "/v1/activities/calendar/tower-dungeon:1",
        json!({"enabled": true, "start_at": "2026-03-01T00:00:00.000Z", "end_at": "2026-04-30T00:00:00.000Z"}),
    );
    let initial = decode_response::<Value>(&send_request_with_resource_version(
        service.port(),
        "/api/index.php/asset/get_path",
        &encode_request(&json!({})),
        "",
    ));
    assert!(initial.data["full"].is_object());
    let installed = initial.data["info"]["eventual_target_asset_version"]
        .as_str()
        .unwrap();
    update_management(
        &service,
        "/v1/time",
        json!({"enabled": true, "iso": "2026-03-01T12:00:00.000Z", "rate": 1.0}),
    );
    update_management(
        &service,
        "/v1/activities/calendar/advent:1",
        json!({"enabled": true, "start_at": "2026-03-02T00:00:00.000Z", "end_at": "2026-03-04T00:00:00.000Z"}),
    );
    update_management(
        &service,
        "/v1/time",
        json!({"enabled": true, "iso": "2026-03-03T12:00:00.000Z", "rate": 1.0}),
    );
    let installed = assert_projection_update(&service, installed);
    update_management(
        &service,
        "/v1/activities/calendar/advent:1",
        json!({"enabled": true, "start_at": "2026-03-02T00:00:00.000Z", "end_at": "2026-03-05T00:00:00.000Z"}),
    );
    let installed = assert_projection_update(&service, &installed);
    for iso in ["2026-03-03T18:00:00.000Z", "2026-03-06T12:00:00.000Z"] {
        update_management(
            &service,
            "/v1/time",
            json!({"enabled": true, "iso": iso, "rate": 1.0}),
        );
        for device in ["1", "2"] {
            let unchanged = decode_response::<Value>(&send_request_with_device_kind(
                service.port(),
                "/api/index.php/asset/get_path",
                &encode_request(&json!({})),
                &installed,
                device,
            ));
            assert!(
                !unchanged.data_headers.asset_update,
                "device={device}, time={iso}"
            );
            assert_eq!(
                unchanged.data["info"]["eventual_target_asset_version"],
                installed
            );
            assert!(unchanged.data["full"].is_null());
            assert!(unchanged.data["diff"].is_null());
        }
    }
    update_management(
        &service,
        "/v1/time",
        json!({"enabled": true, "iso": "2026-03-16T12:00:00.000Z", "rate": 1.0}),
    );
    assert_projection_update(&service, &installed);
    service.stop().unwrap();
}
// //// /修改活动窗口和虚拟时间后仅传送活动主数据差分 ////

// //// 沿客户端覆盖顺序选择连通差分并保留缺链恢复 [@x380kkm 2026-09-21] ////
#[test]
fn selects_connected_updates_and_restores_incomplete_chains() {
    let root = TempDir::new().unwrap();
    write_archive(root.path(), "archive-common-full", "base.zip", b"base");
    let mut diffs = Vec::new();
    for (original, version, name) in [
        ("1.4.0", "1.4.1", "older"),
        ("1.4.1", "1.4.2", "superseded"),
        ("1.4.1", "1.4.3", "replacement"),
        ("1.4.3", "1.4.4", "latest"),
        ("1.4.2", "1.4.9", "incomplete"),
    ] {
        let file = format!("{name}.zip");
        write_archive(root.path(), "archive-common-diff", &file, name.as_bytes());
        diffs.push(json!({"original_version": original, "version": version, "archive": [{"location": format!("archive-common-diff/{file}"), "size": name.len(), "sha256": STANDARD.encode(Sha256::digest(name.as_bytes()))}]}));
    }
    write_path_manifest(
        root.path(),
        json!({
            "info": {"client_asset_version": "", "target_asset_version": "1.4.4", "eventual_target_asset_version": "1.4.4", "is_initial": true, "latest_maj_first_version": "1.4.0"},
            "full": {"version": "1.4.0", "archive": [{"location": "archive-common-full/base.zip", "size": 4, "sha256": STANDARD.encode(Sha256::digest(b"base"))}]},
            "diff": diffs, "asset_version_hash": "connected-update"
        }),
    );
    let service = PersonalService::start(root.path(), 0).unwrap();
    let incremental = decode_response::<Value>(&send_request_with_resource_version(
        service.port(),
        "/api/index.php/asset/get_path",
        &encode_request(&json!({})),
        "1.4.1",
    ));
    assert!(incremental.data["full"].is_null());
    let groups = incremental.data["diff"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0]["original_version"], "1.4.1");
    assert_eq!(groups[0]["version"], "1.4.3");
    assert_eq!(groups[1]["original_version"], "1.4.3");
    assert_eq!(groups[1]["version"], "1.4.4");
    let recovery = decode_response::<Value>(&send_request_with_resource_version(
        service.port(),
        "/api/index.php/asset/get_path",
        &encode_request(&json!({})),
        "1.4.2",
    ));
    assert!(recovery.data_headers.asset_update);
    assert_eq!(recovery.data["info"]["is_initial"], true);
    assert_eq!(recovery.data["full"]["version"], "1.4.0");
    assert_eq!(
        recovery.data["full"]["archive"].as_array().unwrap().len(),
        1
    );
    assert_eq!(recovery.data["diff"].as_array().unwrap().len(), diffs.len());
    service.stop().unwrap();
}
// //// /沿客户端覆盖顺序选择连通差分并保留缺链恢复 ////
