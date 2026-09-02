// audience: internal
// # personal-service-server-routing-tests
//
// 该测试验证 CN API 在本地和远端配置之间切换. 远端转发不传出管理凭据,
// 不连接当前个人服务端口, 并在传输失败后保持管理接口可用.

use starpoint_personal_service::PersonalService;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tempfile::TempDir;

#[path = "support/server_profiles.rs"]
mod server_profile_support;
mod support;

use server_profile_support::{activate_profile, create_remote_profile};
use support::{request, request_with_headers};

// //// 转发 CN API 并切回本地配置 [@x380kkm 2026-07-23] ////
#[test]
fn forwards_cn_requests_and_switches_back_to_local() {
    let (upstream_port, upstream) = start_upstream();
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let remote_id = create_remote_profile(&service, "Friends", "http", "127.0.0.1", upstream_port);
    activate_profile(&service, remote_id);

    let authorization = format!("Bearer {}", service.management_token());
    let response = request_with_headers(
        service.port(),
        "POST",
        "/api/index.php/load?profile=friends",
        "application/x-www-form-urlencoded",
        &[
            ("Authorization", &authorization),
            ("res_ver", "1.4.54"),
            ("X-Cn-Request", "kept"),
        ],
        b"AAECAw==",
    );
    assert!(response.starts_with("HTTP/1.1 201 Created"));
    assert!(response
        .to_ascii_lowercase()
        .contains("x-cn-server: stub\r\n"));
    assert!(response.ends_with("forwarded"));
    upstream.join().expect("upstream request is verified");

    activate_profile(&service, 1);
    let local = request_with_headers(
        service.port(),
        "POST",
        "/api/index.php/unknown",
        "application/x-www-form-urlencoded",
        &[],
        b"",
    );
    assert!(local.starts_with("HTTP/1.1 404 Not Found"));
    service.stop().expect("service stops");
}
// //// /转发 CN API 并切回本地配置 ////

// //// 隔离不安全和不可用的远端配置 [@x380kkm 2026-07-23] ////
#[test]
fn rejects_self_forwarding_and_reports_remote_failures() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    let self_id = create_remote_profile(&service, "Self", "http", "127.0.0.1", service.port());
    activate_profile(&service, self_id);
    let self_response = request_with_headers(
        service.port(),
        "POST",
        "/api/index.php/load",
        "application/x-www-form-urlencoded",
        &[],
        b"AA==",
    );
    assert!(self_response.starts_with("HTTP/1.1 502 Bad Gateway"));
    assert!(self_response.ends_with("{\"error\":\"remote_target_forbidden\"}"));

    activate_profile(&service, 1);
    let mapped_self_id = create_remote_profile(
        &service,
        "Mapped self",
        "http",
        "::ffff:127.0.0.1",
        service.port(),
    );
    activate_profile(&service, mapped_self_id);
    let mapped_self_response = request_with_headers(
        service.port(),
        "POST",
        "/api/index.php/load",
        "application/x-www-form-urlencoded",
        &[],
        b"AA==",
    );
    assert!(mapped_self_response.starts_with("HTTP/1.1 502 Bad Gateway"));
    assert!(mapped_self_response.ends_with("{\"error\":\"remote_target_forbidden\"}"));

    activate_profile(&service, 1);
    let unavailable_port = reserve_unused_port();
    let unavailable_id = create_remote_profile(
        &service,
        "Unavailable",
        "http",
        "127.0.0.1",
        unavailable_port,
    );
    activate_profile(&service, unavailable_id);
    let unavailable = request_with_headers(
        service.port(),
        "POST",
        "/api/index.php/load",
        "application/x-www-form-urlencoded",
        &[],
        b"AA==",
    );
    assert!(unavailable.starts_with("HTTP/1.1 502 Bad Gateway"));
    assert!(unavailable.ends_with("{\"error\":\"remote_server_unavailable\"}"));

    activate_profile(&service, 1);
    let https_id = create_remote_profile(&service, "TLS", "https", "example.test", 443);
    activate_profile(&service, https_id);
    let unsupported = request_with_headers(
        service.port(),
        "POST",
        "/api/index.php/load",
        "application/x-www-form-urlencoded",
        &[],
        b"AA==",
    );
    assert!(unsupported.starts_with("HTTP/1.1 502 Bad Gateway"));
    assert!(unsupported.ends_with("{\"error\":\"remote_scheme_unsupported\"}"));
    assert!(request(service.port(), "GET", "/health").starts_with("HTTP/1.1 200 OK"));
    service.stop().expect("service stops");
}
// //// /隔离不安全和不可用的远端配置 ////

// //// 运行一次受控远端 HTTP 服务 [@x380kkm 2026-07-23] ////
fn start_upstream() -> (u16, JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("upstream listener binds");
    let port = listener
        .local_addr()
        .expect("upstream address is available")
        .port();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("forwarded request connects");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("upstream read timeout is configured");
        let request = read_http_request(&mut stream);
        let request = String::from_utf8(request).expect("forwarded request is UTF-8");
        assert!(request.starts_with("POST /api/index.php/load?profile=friends HTTP/1.1\r\n"));
        assert!(request.contains(&format!("Host: 127.0.0.1:{port}\r\n")));
        assert!(request.contains("content-type: application/x-www-form-urlencoded\r\n"));
        assert!(request.contains("res_ver: 1.4.54\r\n"));
        assert!(request.contains("x-cn-request: kept\r\n"));
        assert!(!request.contains("authorization:"));
        assert!(request.ends_with("AAECAw=="));

        stream
            .write_all(
                b"HTTP/1.1 201 Created\r\nContent-Type: application/x-msgpack\r\nX-Cn-Server: stub\r\nContent-Length: 9\r\nConnection: close\r\n\r\nforwarded",
            )
            .expect("upstream response is written");
    });
    (port, handle)
}

fn read_http_request(stream: &mut TcpStream) -> Vec<u8> {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let length = stream.read(&mut chunk).expect("forwarded request is read");
        assert!(length > 0, "forwarded request has its declared body");
        request.extend_from_slice(&chunk[..length]);
        let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let headers =
            String::from_utf8(request[..header_end].to_vec()).expect("forwarded headers are UTF-8");
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length").then(|| {
                    value
                        .trim()
                        .parse::<usize>()
                        .expect("content length is valid")
                })
            })
            .expect("forwarded request has content length");
        if request.len() >= header_end + 4 + content_length {
            return request;
        }
    }
}

fn reserve_unused_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("temporary listener binds")
        .local_addr()
        .expect("temporary listener address is available")
        .port()
}
// //// /运行一次受控远端 HTTP 服务 ////
