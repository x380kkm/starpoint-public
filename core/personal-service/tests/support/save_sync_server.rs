// audience: internal
// # personal-service-save-sync-test-server
//
// 该模块提供只监听 loopback 的密文存档测试服务器并在 Drop 时停止线程.

use serde_json::Value;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const SESSION_COOKIE: &str = "starpoint_management_session=test-session";

struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

#[derive(Default)]
struct State {
    encrypted_body: Option<Vec<u8>>,
    encrypted_objects: HashMap<String, Vec<u8>>,
    object_etags: HashMap<String, u64>,
    etag_revision: u64,
    capacity_exceeded: bool,
    upload_delay: Duration,
}

pub(crate) struct MockSaveSyncServer {
    port: u16,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl MockSaveSyncServer {
    // //// 启动 loopback 密文存档测试服务器 [@x380kkm 2026-07-23] ////
    pub(crate) fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("sync listener binds");
        let port = listener.local_addr().expect("sync address exists").port();
        let state = Arc::new(Mutex::new(State::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_state = Arc::clone(&state);
        let thread_stop = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            while !thread_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if let Some(request) = read_request(&mut stream) {
                            handle_request(request, &mut stream, &thread_state);
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            port,
            state,
            stop,
            thread: Some(thread),
        }
    }

    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    pub(crate) fn encrypted_body(&self) -> Value {
        let state = self.state.lock().expect("sync state is available");
        serde_json::from_slice(
            state
                .encrypted_body
                .as_deref()
                .expect("encrypted body was uploaded"),
        )
        .expect("encrypted body is JSON")
    }

    pub(crate) fn diverge_etag(&self) {
        let mut state = self.state.lock().expect("sync state is available");
        state.etag_revision += 1;
        let revision = state.etag_revision;
        for object_revision in state.object_etags.values_mut() {
            *object_revision = revision;
        }
    }

    pub(crate) fn set_capacity_exceeded(&self, capacity_exceeded: bool) {
        self.state
            .lock()
            .expect("sync state is available")
            .capacity_exceeded = capacity_exceeded;
    }

    pub(crate) fn set_upload_delay(&self, upload_delay: Duration) {
        self.state
            .lock()
            .expect("sync state is available")
            .upload_delay = upload_delay;
    }

    pub(crate) fn etag_revision(&self) -> u64 {
        self.state
            .lock()
            .expect("sync state is available")
            .etag_revision
    }
    // //// /启动 loopback 密文存档测试服务器 ////
}

impl Drop for MockSaveSyncServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(thread) = self.thread.take() {
            thread.join().expect("sync server thread stops");
        }
    }
}

// //// 处理登录, 登出和密文存档请求 [@x380kkm 2026-07-23] ////
fn handle_request(request: Request, stream: &mut TcpStream, state: &Arc<Mutex<State>>) {
    match (request.method.as_str(), request.path.as_str()) {
        ("POST", "/manage/api/auth/login") => {
            let body = serde_json::from_slice::<Value>(&request.body).expect("login body is JSON");
            if body["username"] == "sync-user" && body["password"] == "sync-password" {
                respond(
                    stream,
                    "200 OK",
                    &[(
                        "Set-Cookie",
                        &format!("{SESSION_COOKIE}; Path=/manage; HttpOnly"),
                    )],
                    br#"{"loggedIn":true}"#,
                );
            } else {
                respond(
                    stream,
                    "401 Unauthorized",
                    &[],
                    br#"{"error":"invalid_credentials"}"#,
                );
            }
        }
        ("POST", "/manage/api/auth/logout") => {
            respond(stream, "200 OK", &[], br#"{"loggedOut":true}"#);
        }
        ("PUT", path) if path.starts_with("/manage/api/encrypted-saves/") => {
            if request.headers.get("cookie").map(String::as_str) != Some(SESSION_COOKIE) {
                respond(
                    stream,
                    "401 Unauthorized",
                    &[],
                    br#"{"error":"unauthorized"}"#,
                );
                return;
            }
            let upload_delay = state.lock().expect("sync state is available").upload_delay;
            if !upload_delay.is_zero() {
                thread::sleep(upload_delay);
            }
            let mut state = state.lock().expect("sync state is available");
            if state.capacity_exceeded {
                respond(
                    stream,
                    "409 Conflict",
                    &[],
                    br#"{"error":"encrypted_save_capacity_exceeded"}"#,
                );
                return;
            }
            let object_id = path
                .strip_prefix("/manage/api/encrypted-saves/")
                .expect("encrypted save object path has a prefix");
            let previous_revision = state.object_etags.get(object_id).copied();
            let condition_matches = if previous_revision.is_none() {
                request.headers.get("if-none-match").map(String::as_str) == Some("*")
            } else {
                request.headers.get("if-match").map(String::as_str)
                    == Some(format!("\"{}\"", etag(previous_revision.unwrap())).as_str())
            };
            if !condition_matches {
                respond(
                    stream,
                    "412 Precondition Failed",
                    &[],
                    br#"{"error":"encrypted_save_precondition_failed"}"#,
                );
                return;
            }
            serde_json::from_slice::<Value>(&request.body).expect("encrypted save body is JSON");
            let body = request.body;
            state.encrypted_body = Some(body.clone());
            state.etag_revision += 1;
            let revision = state.etag_revision;
            state.object_etags.insert(object_id.to_owned(), revision);
            state.encrypted_objects.insert(object_id.to_owned(), body);
            let etag = format!("\"{}\"", etag(revision));
            respond(stream, "200 OK", &[("ETag", &etag)], br#"{"stored":true}"#);
        }
        ("GET", path) if path.starts_with("/manage/api/encrypted-saves/") => {
            if request.headers.get("cookie").map(String::as_str) != Some(SESSION_COOKIE) {
                respond(
                    stream,
                    "401 Unauthorized",
                    &[],
                    br#"{"error":"unauthorized"}"#,
                );
                return;
            }
            let state = state.lock().expect("sync state is available");
            let object_id = path
                .strip_prefix("/manage/api/encrypted-saves/")
                .expect("encrypted save object path has a prefix");
            match state.encrypted_objects.get(object_id) {
                Some(body) => {
                    let revision = state
                        .object_etags
                        .get(object_id)
                        .copied()
                        .expect("encrypted object has an ETag");
                    let etag = format!("\"{}\"", etag(revision));
                    respond(stream, "200 OK", &[("ETag", &etag)], body);
                }
                None => respond(stream, "404 Not Found", &[], br#"{"error":"not_found"}"#),
            }
        }
        _ => respond(stream, "404 Not Found", &[], br#"{"error":"not_found"}"#),
    }
}
// //// /处理登录, 登出和密文存档请求 ////

fn etag(revision: u64) -> String {
    format!("{revision:064x}")
}

fn respond(stream: &mut TcpStream, status: &str, headers: &[(&str, &str)], body: &[u8]) {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    )
    .expect("sync response head is written");
    for (name, value) in headers {
        write!(stream, "{name}: {value}\r\n").expect("sync response header is written");
    }
    stream
        .write_all(b"\r\n")
        .expect("sync response separator is written");
    stream
        .write_all(body)
        .expect("sync response body is written");
}

// //// 读取一个有 Content-Length 的测试请求 [@x380kkm 2026-07-23] ////
fn read_request(stream: &mut TcpStream) -> Option<Request> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("sync request timeout is configured");
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            return None;
        }
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(header_end) = find_header_end(&bytes) {
            let header_text = std::str::from_utf8(&bytes[..header_end]).ok()?;
            let mut lines = header_text.lines();
            let mut request_line = lines.next()?.split_ascii_whitespace();
            let method = request_line.next()?.to_owned();
            let path = request_line.next()?.to_owned();
            let mut headers = HashMap::new();
            for line in lines {
                if let Some((name, value)) = line.split_once(':') {
                    headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
                }
            }
            let content_length = headers
                .get("content-length")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0);
            let body_start = header_end + 4;
            if bytes.len() < body_start + content_length {
                continue;
            }
            return Some(Request {
                method,
                path,
                headers,
                body: bytes[body_start..body_start + content_length].to_vec(),
            });
        }
    }
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}
// //// /读取一个有 Content-Length 的测试请求 ////
