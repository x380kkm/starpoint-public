// audience: internal
// # personal-service-server-identity-routing-tests
//
// 该测试验证远端 signup 身份按服务器配置持久化. 服务重启后使用该配置时,
// 客户端携带的其他服务器 viewer 不会进入远端请求或本地玩家快照.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

#[path = "support/server_profiles.rs"]
mod server_profile_support;
mod support;

use server_profile_support::{activate_profile, authorized_request, create_remote_profile};
use support::{request, request_with_headers};

const DEVICE_ID: i64 = 9_100_001;
const FIRST_REMOTE_VIEWER_ID: i64 = 765_432_100;
const SECOND_REMOTE_VIEWER_ID: i64 = 765_432_200;

// //// 重启后隔离不同服务器配置的 viewer [@x380kkm 2026-07-23] ////
#[test]
fn persists_and_isolates_remote_profile_identities() {
    let first_listener = TcpListener::bind(("127.0.0.1", 0)).expect("first listener binds");
    let first_upstream_port = first_listener
        .local_addr()
        .expect("first upstream address is available")
        .port();
    let first_upstream = thread::spawn(move || {
        verify_signup_and_rewritten_load(first_listener, FIRST_REMOTE_VIEWER_ID, "first")
    });
    let second_listener = TcpListener::bind(("127.0.0.1", 0)).expect("second listener binds");
    let second_upstream_port = second_listener
        .local_addr()
        .expect("second upstream address is available")
        .port();
    let second_upstream = thread::spawn(move || {
        verify_signup_and_rewritten_load(second_listener, SECOND_REMOTE_VIEWER_ID, "second")
    });
    let root = TempDir::new().expect("temporary service directory is created");

    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let first_remote_id = create_remote_profile(
        &service,
        "First identity server",
        "http",
        "127.0.0.1",
        first_upstream_port,
    );
    activate_profile(&service, first_remote_id);
    let first_signup = send_cn_request(
        service.port(),
        "/api/index.php/tool/signup",
        json!({ "device_id": DEVICE_ID, "channelNo": "leiting" }),
    );
    assert!(first_signup.starts_with("HTTP/1.1 200 OK"));

    let second_remote_id = create_remote_profile(
        &service,
        "Second identity server",
        "http",
        "127.0.0.1",
        second_upstream_port,
    );
    activate_profile(&service, second_remote_id);
    let second_signup = send_cn_request(
        service.port(),
        "/api/index.php/tool/signup",
        json!({ "device_id": DEVICE_ID, "channelNo": "leiting" }),
    );
    assert!(second_signup.starts_with("HTTP/1.1 200 OK"));
    service.stop().expect("service stops after both signups");

    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    assert!(request(service.port(), "GET", "/health").starts_with("HTTP/1.1 200 OK"));
    let second_load = send_cn_request(
        service.port(),
        "/api/index.php/load",
        json!({
            "device_id": DEVICE_ID,
            "viewer_id": FIRST_REMOTE_VIEWER_ID,
            "keychain": FIRST_REMOTE_VIEWER_ID,
            "opaque": "second",
            "nested": { "viewer_id": 333_333_333 },
        }),
    );
    assert!(second_load.starts_with("HTTP/1.1 200 OK"));

    activate_profile(&service, first_remote_id);
    let first_load = send_cn_request(
        service.port(),
        "/api/index.php/load",
        json!({
            "device_id": DEVICE_ID,
            "viewer_id": SECOND_REMOTE_VIEWER_ID,
            "keychain": SECOND_REMOTE_VIEWER_ID,
            "opaque": "first",
            "nested": { "viewer_id": 333_333_333 },
        }),
    );
    assert!(first_load.starts_with("HTTP/1.1 200 OK"));
    let updated_first_profile = json!({
        "name": "First identity server moved",
        "scheme": "http",
        "host": "127.0.0.1",
        "port": second_upstream_port,
    });
    let updated = authorized_request(
        service.port(),
        service.management_token(),
        "PUT",
        &format!("/v1/server-profiles/{first_remote_id}"),
        Some(&updated_first_profile),
    );
    assert!(updated.starts_with("HTTP/1.1 200 OK"));
    service
        .stop()
        .expect("service stops after both rewritten loads");
    first_upstream
        .join()
        .expect("first upstream requests are verified");
    second_upstream
        .join()
        .expect("second upstream requests are verified");

    let database = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("personal service database opens");
    let local_account_count: i64 = database
        .query_row("SELECT COUNT(*) FROM accounts", [], |row| row.get(0))
        .expect("local account count is read");
    let local_snapshot_count: i64 = database
        .query_row("SELECT COUNT(*) FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .expect("local snapshot count is read");
    let remote_identity_count: i64 = database
        .query_row(
            "SELECT COUNT(*) FROM server_profile_identities",
            [],
            |row| row.get(0),
        )
        .expect("remote identity count is read");
    assert_eq!((local_account_count, local_snapshot_count), (0, 0));
    assert_eq!(remote_identity_count, 1);
}
// //// /重启后隔离不同服务器配置的 viewer ////

// //// 模拟远端 signup 和 load 协议 [@x380kkm 2026-07-23] ////
fn verify_signup_and_rewritten_load(
    listener: TcpListener,
    remote_viewer_id: i64,
    expected_opaque: &str,
) {
    let (signup_request, signup_stream) = accept_request(&listener);
    assert!(signup_request.starts_with(b"POST /api/index.php/tool/signup HTTP/1.1\r\n"));
    assert!(!request_headers(&signup_request).contains("accept-encoding:"));
    let signup_message = decode_request_body(&signup_request);
    assert_eq!(signup_message["device_id"], DEVICE_ID);
    write_cn_response(signup_stream, remote_viewer_id);

    let (load, load_stream) = accept_request(&listener);
    assert!(load.starts_with(b"POST /api/index.php/load HTTP/1.1\r\n"));
    let load_message = decode_request_body(&load);
    assert_eq!(load_message["viewer_id"], remote_viewer_id);
    assert_eq!(load_message["keychain"], remote_viewer_id);
    assert_eq!(load_message["opaque"], expected_opaque);
    assert_eq!(load_message["nested"]["viewer_id"], 333_333_333);
    write_cn_response(load_stream, remote_viewer_id);
}

fn accept_request(listener: &TcpListener) -> (Vec<u8>, TcpStream) {
    let (mut stream, _) = listener.accept().expect("forwarded request connects");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("upstream read timeout is configured");
    let request = read_http_request(&mut stream);
    (request, stream)
}

fn decode_request_body(request: &[u8]) -> Value {
    let header_end = find_header_end(request).expect("forwarded request has headers");
    let encoded = std::str::from_utf8(&request[header_end + 4..])
        .expect("forwarded request body is UTF-8")
        .trim();
    let packed = STANDARD
        .decode(encoded)
        .expect("forwarded request body is base64");
    rmp_serde::from_slice(&packed).expect("forwarded request body is MessagePack")
}

fn request_headers(request: &[u8]) -> String {
    let header_end = find_header_end(request).expect("forwarded request has headers");
    std::str::from_utf8(&request[..header_end])
        .expect("forwarded headers are UTF-8")
        .to_ascii_lowercase()
}

fn write_cn_response(mut stream: TcpStream, viewer_id: i64) {
    let response = json!({
        "data_headers": {
            "force_update": false,
            "asset_update": false,
            "short_udid": 0,
            "viewer_id": viewer_id,
            "servertime": 1_800_000_000,
            "result_code": 1,
        },
        "data": {},
    });
    let body = STANDARD.encode(
        rmp_serde::to_vec_named(&response).expect("upstream response is encoded as MessagePack"),
    );
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/x-msgpack\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .expect("upstream response is written");
}

fn read_http_request(stream: &mut TcpStream) -> Vec<u8> {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let length = stream.read(&mut chunk).expect("forwarded request is read");
        assert!(length > 0, "forwarded request has its declared body");
        request.extend_from_slice(&chunk[..length]);
        let Some(header_end) = find_header_end(&request) else {
            continue;
        };
        let headers =
            std::str::from_utf8(&request[..header_end]).expect("forwarded headers are UTF-8");
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

fn send_cn_request(port: u16, path: &str, message: Value) -> String {
    let body = STANDARD.encode(
        rmp_serde::to_vec_named(&message).expect("client request is encoded as MessagePack"),
    );
    request_with_headers(
        port,
        "POST",
        path,
        "application/x-www-form-urlencoded",
        &[("Accept-Encoding", "gzip, deflate")],
        body.as_bytes(),
    )
}

fn find_header_end(encoded: &[u8]) -> Option<usize> {
    encoded.windows(4).position(|window| window == b"\r\n\r\n")
}
// //// /模拟远端 signup 和 load 协议 ////
