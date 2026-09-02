// audience: internal
// # personal-service-binary-tests
//
// 该文件验证正式个人服务进程的启动输出, CDN 根配置, loopback 健康接口和标准输入停止命令.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use tempfile::TempDir;

mod support;

use support::{request, request_with_headers};

// //// 启动正式个人服务进程并完成健康检查 [@x380kkm 2026-07-24] ////
#[test]
fn starts_health_check_and_stops_from_stdin() {
    let root = TempDir::new().expect("temporary service directory is created");
    let cdn_root = TempDir::new().expect("temporary CDN directory is created");
    let asset_path = cdn_root.path().join("EntityLists").join("Environment.csv");
    fs::create_dir_all(asset_path.parent().expect("asset parent exists"))
        .expect("asset directory is created");
    fs::write(&asset_path, "Id,Entity\r\n6,Environment\r\n").expect("asset is written");
    let mut child = Command::new(env!("CARGO_BIN_EXE_personal-service"))
        .args([
            "--root",
            root.path().to_str().expect("temporary path is UTF-8"),
        ])
        .args([
            "--port",
            "0",
            "--session-port",
            "0",
            "--show-management-token",
        ])
        .env("STARPOINT_PERSONAL_SERVICE_CDN_ROOT", cdn_root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("personal-service binary starts");

    let stdout = child.stdout.take().expect("binary stdout is captured");
    let mut lines = BufReader::new(stdout).lines();
    assert_eq!(
        lines.next().expect("ready line exists").unwrap(),
        "personal-service ready"
    );
    assert!(lines
        .next()
        .expect("root line exists")
        .unwrap()
        .starts_with("root="));
    assert!(lines
        .next()
        .expect("CDN root line exists")
        .unwrap()
        .starts_with("cdn_root="));
    let health_line = lines.next().expect("health line exists").unwrap();
    let port = health_line
        .strip_prefix("health=http://127.0.0.1:")
        .and_then(|value| value.strip_suffix("/health"))
        .and_then(|value| value.parse::<u16>().ok())
        .expect("health line contains a port");
    assert!(lines
        .next()
        .expect("management line exists")
        .unwrap()
        .starts_with("management=http://127.0.0.1:"));
    assert!(lines
        .next()
        .expect("management token line exists")
        .unwrap()
        .starts_with("management_token="));

    let health = request(port, "GET", "/health");
    assert!(health.starts_with("HTTP/1.1 200 OK"));
    assert!(health.ends_with("{\"status\":\"ok\",\"generation\":0}"));
    let asset = request(port, "GET", "/patch/cn/EntityLists/Environment.csv");
    assert!(asset.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(asset.ends_with("Id,Entity\r\n6,Environment\r\n"));

    child
        .stdin
        .take()
        .expect("binary stdin is captured")
        .write_all(b"stop\n")
        .expect("stop command is written");
    let status = child.wait().expect("personal-service process exits");
    assert!(status.success());
}
// //// /启动正式个人服务进程并完成健康检查 ////

// //// 正式进程使用显式 CN CDN 根目录 [@x380kkm 2026-08-11] ////
#[test]
fn serves_assets_from_explicit_cdn_root() {
    let root = TempDir::new().expect("temporary service directory is created");
    let cdn_root = TempDir::new().expect("temporary CDN directory is created");
    let asset_path = cdn_root.path().join("EntityLists").join("Custom.csv");
    fs::create_dir_all(asset_path.parent().expect("asset parent exists"))
        .expect("asset directory is created");
    fs::write(&asset_path, "Id,Entity\r\n7,Custom\r\n").expect("asset is written");

    let mut child = Command::new(env!("CARGO_BIN_EXE_personal-service"))
        .args([
            "--root",
            root.path().to_str().expect("temporary path is UTF-8"),
            "--cdn-root",
            cdn_root.path().to_str().expect("temporary path is UTF-8"),
            "--port",
            "0",
            "--session-port",
            "0",
        ])
        .env(
            "STARPOINT_PERSONAL_SERVICE_CDN_ROOT",
            root.path().join("ignored-cdn"),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("personal-service binary starts");

    let stdout = child.stdout.take().expect("binary stdout is captured");
    let mut lines = BufReader::new(stdout).lines();
    assert_eq!(
        lines.next().expect("ready line exists").unwrap(),
        "personal-service ready"
    );
    assert!(lines
        .next()
        .expect("root line exists")
        .unwrap()
        .starts_with("root="));
    assert!(lines
        .next()
        .expect("CDN root line exists")
        .unwrap()
        .starts_with("cdn_root="));
    let port = lines
        .next()
        .expect("health line exists")
        .unwrap()
        .strip_prefix("health=http://127.0.0.1:")
        .and_then(|value| value.strip_suffix("/health"))
        .and_then(|value| value.parse::<u16>().ok())
        .expect("health line contains a port");

    let response = request(port, "GET", "/patch/cn/EntityLists/Custom.csv");
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(response.ends_with("Id,Entity\r\n7,Custom\r\n"));

    child
        .stdin
        .take()
        .expect("stop command is writable")
        .write_all(b"stop\n")
        .expect("stop command is written");
    assert!(child
        .wait()
        .expect("personal-service process exits")
        .success());
}
// //// /正式进程使用显式 CN CDN 根目录 ////

// //// HTTP 访问日志排除正文、查询参数和请求头 [@x380kkm 2026-08-12] ////
#[test]
fn logs_only_safe_http_access_metadata() {
    let root = TempDir::new().expect("temporary service directory is created");
    let mut child = Command::new(env!("CARGO_BIN_EXE_personal-service"))
        .args([
            "--root",
            root.path().to_str().expect("temporary path is UTF-8"),
            "--port",
            "0",
            "--session-port",
            "0",
            "--log-http-access",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("personal-service binary starts");

    let stdout = child.stdout.take().expect("binary stdout is captured");
    let mut lines = BufReader::new(stdout).lines();
    assert_eq!(
        lines.next().expect("ready line exists").unwrap(),
        "personal-service ready"
    );
    lines.next().expect("root line exists").unwrap();
    lines.next().expect("CDN root line exists").unwrap();
    let port = lines
        .next()
        .expect("health line exists")
        .unwrap()
        .strip_prefix("health=http://127.0.0.1:")
        .and_then(|value| value.strip_suffix("/health"))
        .and_then(|value| value.parse::<u16>().ok())
        .expect("health line contains a port");
    lines.next().expect("management line exists").unwrap();

    let secret_query = "query-secret";
    let secret_header = "header-secret";
    let secret_body = b"body-secret";
    let response = request_with_headers(
        port,
        "POST",
        &format!("/v1/state/increment?token={secret_query}"),
        "application/octet-stream",
        &[("Authorization", secret_header)],
        secret_body,
    );
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    let injected_control = "\u{1b}[2J";
    let injected_path = format!("/missing/{injected_control}injected-log-line");
    assert!(request(port, "GET", &injected_path).starts_with("HTTP/1.1 404 Not Found\r\n"));

    child
        .stdin
        .take()
        .expect("stop command is writable")
        .write_all(b"stop\n")
        .expect("stop command is written");
    assert!(child
        .wait()
        .expect("personal-service process exits")
        .success());
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("binary stderr is captured")
        .read_to_string(&mut stderr)
        .expect("binary stderr is read");

    assert!(stderr.contains(
        "http_access method=POST path=/v1/state/increment status=200 OK content_type=application/json"
    ));
    assert!(!stderr.contains(secret_query));
    assert!(!stderr.contains(secret_header));
    assert!(!stderr.contains(std::str::from_utf8(secret_body).expect("secret body is UTF-8")));
    assert!(!stderr.contains("Authorization"));
    assert!(!stderr.contains(injected_control));
    assert!(stderr.contains("path=/missing/\\u{1b}[2Jinjected-log-line"));
}
// //// /HTTP 访问日志排除正文、查询参数和请求头 ////

// //// 默认关闭 HTTP 访问日志 [@x380kkm 2026-08-12] ////
#[test]
fn keeps_http_access_logging_disabled_by_default() {
    let root = TempDir::new().expect("temporary service directory is created");
    let mut child = Command::new(env!("CARGO_BIN_EXE_personal-service"))
        .args([
            "--root",
            root.path().to_str().expect("temporary path is UTF-8"),
            "--port",
            "0",
            "--session-port",
            "0",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("personal-service binary starts");

    let stdout = child.stdout.take().expect("binary stdout is captured");
    let mut lines = BufReader::new(stdout).lines();
    assert_eq!(
        lines.next().expect("ready line exists").unwrap(),
        "personal-service ready"
    );
    lines.next().expect("root line exists").unwrap();
    lines.next().expect("CDN root line exists").unwrap();
    let port = lines
        .next()
        .expect("health line exists")
        .unwrap()
        .strip_prefix("health=http://127.0.0.1:")
        .and_then(|value| value.strip_suffix("/health"))
        .and_then(|value| value.parse::<u16>().ok())
        .expect("health line contains a port");
    lines.next().expect("management line exists").unwrap();

    assert!(request(port, "GET", "/health").starts_with("HTTP/1.1 200 OK\r\n"));
    child
        .stdin
        .take()
        .expect("stop command is writable")
        .write_all(b"stop\n")
        .expect("stop command is written");
    assert!(child
        .wait()
        .expect("personal-service process exits")
        .success());
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("binary stderr is captured")
        .read_to_string(&mut stderr)
        .expect("binary stderr is read");
    assert!(!stderr.contains("http_access"));
}
// //// /默认关闭 HTTP 访问日志 ////
