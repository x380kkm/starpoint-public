// audience: internal
// # personal-service-lifecycle
//
// 这些测试验证 loopback 绑定、事务持久化和进程内服务重启恢复.

use rusqlite::Connection;
use starpoint_personal_service::PersonalService;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

mod support;

use support::request;

// //// 验证暂停刷盘和重启恢复 [@x380kkm 2026-07-22] ////
#[test]
fn persists_state_across_flush_stop_and_restart() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    let initial = request(service.port(), "GET", "/health");
    assert!(initial.starts_with("HTTP/1.1 200 OK"));
    assert!(initial.ends_with("{\"status\":\"ok\",\"generation\":0}"));

    let changed = request(service.port(), "POST", "/v1/state/increment");
    assert!(changed.ends_with("{\"generation\":1}"));
    service.flush().expect("service flushes SQLite WAL");
    service.stop().expect("service stops cleanly");

    let database = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("database remains readable after shutdown");
    let generation: i64 = database
        .query_row(
            "SELECT generation FROM service_state WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .expect("persisted generation is read");
    assert_eq!(generation, 1);
    drop(database);

    let restarted = PersonalService::start(root.path(), 0).expect("service restarts");
    let restored = request(restarted.port(), "GET", "/health");
    assert!(restored.ends_with("{\"status\":\"ok\",\"generation\":1}"));
    restarted.stop().expect("restarted service stops cleanly");
}
// //// /验证暂停刷盘和重启恢复 ////

// //// 拒绝占用中的固定端口 [@x380kkm 2026-07-22] ////
#[test]
fn rejects_an_unavailable_port_without_stopping_the_first_service() {
    let first_root = TempDir::new().expect("first service directory is created");
    let second_root = TempDir::new().expect("second service directory is created");
    let first = PersonalService::start(first_root.path(), 0).expect("first service starts");

    let second = PersonalService::start(second_root.path(), first.port());
    assert!(second.is_err());
    assert!(first.is_running());
    assert!(request(first.port(), "GET", "/health").starts_with("HTTP/1.1 200 OK"));
    first.stop().expect("first service stops cleanly");
}
// //// /拒绝占用中的固定端口 ////

// //// 返回 iOS 和 Android 的 CN 版本配置 [@x380kkm 2026-08-07] ////
#[test]
fn returns_the_local_cn_api_endpoint_for_ios_and_android() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let body = format!(
        "// StarPoint CN compatibility endpoint\r\n{{\"default\":{{\"apiScheme\":\"http\",\"apiPath\":\"127.0.0.1:{}\"}}}}",
        service.port()
    );

    for path in [
        "/shijtswy/version/client_release_ios.dis?cache=1",
        "/shijtswy/version/client_release_android.dis?cache=2",
    ] {
        let response = request(service.port(), "GET", path);
        let expected = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        assert_eq!(
            response, expected,
            "version path must stay compatible: {path}"
        );
    }

    service.stop().expect("service stops cleanly");
}
// //// /返回 iOS 和 Android 的 CN 版本配置 ////

// //// 在客户端提前断开后继续处理请求 [@x380kkm 2026-07-22] ////
#[test]
fn continues_after_client_disconnects_during_response() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let mut stream = TcpStream::connect(("127.0.0.1", service.port()))
        .expect("client connects to personal service");
    stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\n\r\n")
        .expect("request is written");
    stream
        .shutdown(Shutdown::Both)
        .expect("client connection is closed");
    drop(stream);
    thread::sleep(Duration::from_millis(100));

    assert!(service.is_running());
    assert!(request(service.port(), "GET", "/health").starts_with("HTTP/1.1 200 OK"));
    service.stop().expect("service stops cleanly");
}
// //// /在客户端提前断开后继续处理请求 ////

// //// 拒绝溢出的 HTTP 正文长度并保持服务运行 [@x380kkm 2026-07-22] ////
#[test]
fn rejects_overflowing_content_length_without_stopping_service() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let mut stream = TcpStream::connect(("127.0.0.1", service.port()))
        .expect("client connects to personal service");
    write!(
        stream,
        "POST /health HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n\r\n",
        usize::MAX
    )
    .expect("oversized request header is written");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("error response is read");

    assert!(response.starts_with("HTTP/1.1 400 Bad Request"));
    assert!(service.is_running());
    assert!(request(service.port(), "GET", "/health").starts_with("HTTP/1.1 200 OK"));
    service.stop().expect("service stops cleanly");
}
// //// /拒绝溢出的 HTTP 正文长度并保持服务运行 ////
