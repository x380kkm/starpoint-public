// audience: internal
// # personal-service-server-profile-probe-tests
//
// 该文件验证远端健康探测和失败时保留当前服务器配置.

mod support;

use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use support::request_with_headers;
use tempfile::TempDir;

struct MockHealthServer {
    port: u16,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl MockHealthServer {
    fn start(body: Value) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("health listener binds");
        listener
            .set_nonblocking(true)
            .expect("health listener is nonblocking");
        let port = listener.local_addr().expect("health address exists").port();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let body = body.to_string();
        let worker = thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => respond(&mut stream, &body),
                    Err(error) if error.kind() == ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            port,
            stop,
            worker: Some(worker),
        }
    }

    fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for MockHealthServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(worker) = self.worker.take() {
            worker.join().expect("health worker joins");
        }
    }
}

fn respond(stream: &mut TcpStream, body: &str) {
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .expect("health read timeout is set");
    let mut request = [0_u8; 2048];
    let _ = stream.read(&mut request);
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body,
    );
    let _ = stream.write_all(response.as_bytes());
}

fn authorized_request(
    service: &PersonalService,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> String {
    let encoded = body.map_or_else(Vec::new, |value| value.to_string().into_bytes());
    let authorization = format!("Bearer {}", service.management_token());
    request_with_headers(
        service.port(),
        method,
        path,
        "application/json",
        &[("Authorization", &authorization)],
        &encoded,
    )
}

fn response_body(response: &str) -> Value {
    serde_json::from_str(
        response
            .split_once("\r\n\r\n")
            .expect("response contains a body")
            .1,
    )
    .expect("response body is JSON")
}

fn create_remote_profile(service: &PersonalService, name: &str, port: u16) -> i64 {
    let response = authorized_request(
        service,
        "POST",
        "/v1/server-profiles",
        Some(&json!({
            "name": name,
            "scheme": "http",
            "host": "127.0.0.1",
            "port": port,
        })),
    );
    assert!(response.starts_with("HTTP/1.1 201 Created"), "{response}");
    response_body(&response)["id"]
        .as_i64()
        .expect("profile id is numeric")
}

fn unused_loopback_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("temporary listener binds");
    listener
        .local_addr()
        .expect("temporary address exists")
        .port()
}

// //// 检测兼容服务器并仅在成功后切换 [@x380kkm 2026-07-24] ////
#[test]
fn probes_before_activation_and_preserves_the_active_profile_on_failure() {
    let compatible = MockHealthServer::start(json!({
        "status": "ok",
        "service": "starpoint",
        "serverDate": "2026-07-24T12:00:00.000Z",
        "httpPort": 8000,
        "sessionPort": 9000,
    }));
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("personal service starts");
    let compatible_id = create_remote_profile(&service, "Club", compatible.port());

    let probe = authorized_request(
        &service,
        "POST",
        &format!("/v1/server-profiles/{compatible_id}/probe"),
        None,
    );
    assert!(probe.starts_with("HTTP/1.1 200 OK"), "{probe}");
    let probe = response_body(&probe);
    assert_eq!(probe["reachable"], true);
    assert_eq!(probe["compatible"], true);
    assert_eq!(probe["session_port"], 9000);

    let activated = authorized_request(
        &service,
        "POST",
        &format!("/v1/server-profiles/{compatible_id}/activate-verified"),
        None,
    );
    assert!(activated.starts_with("HTTP/1.1 200 OK"), "{activated}");
    let activated = response_body(&activated);
    assert_eq!(activated["activated"], true);
    assert_eq!(activated["state"]["active_profile_id"], compatible_id);

    let local = authorized_request(&service, "POST", "/v1/server-profiles/1/activate", None);
    assert!(local.starts_with("HTTP/1.1 200 OK"), "{local}");
    let unreachable_id = create_remote_profile(&service, "Offline", unused_loopback_port());
    let unreachable = authorized_request(
        &service,
        "POST",
        &format!("/v1/server-profiles/{unreachable_id}/activate-verified"),
        None,
    );
    assert!(
        unreachable.starts_with("HTTP/1.1 409 Conflict"),
        "{unreachable}"
    );
    let unreachable = response_body(&unreachable);
    assert_eq!(unreachable["error"], "server_profile_unreachable");
    assert_eq!(unreachable["probe"]["reachable"], false);

    let incompatible = MockHealthServer::start(json!({
        "status": "ok",
        "service": "another-service",
        "serverDate": "2026-07-24T12:00:00.000Z",
        "httpPort": 8000,
        "sessionPort": 9000,
    }));
    let incompatible_id = create_remote_profile(&service, "Wrong service", incompatible.port());
    let incompatible = authorized_request(
        &service,
        "POST",
        &format!("/v1/server-profiles/{incompatible_id}/activate-verified"),
        None,
    );
    assert!(
        incompatible.starts_with("HTTP/1.1 409 Conflict"),
        "{incompatible}"
    );
    let incompatible = response_body(&incompatible);
    assert_eq!(incompatible["error"], "server_profile_incompatible");
    assert_eq!(incompatible["probe"]["reachable"], true);

    let state = authorized_request(&service, "GET", "/v1/server-profiles", None);
    assert_eq!(response_body(&state)["active_profile_id"], 1);
    service.stop().expect("personal service stops cleanly");
}
// //// /检测兼容服务器并仅在成功后切换 ////
