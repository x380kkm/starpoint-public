// audience: internal
// # personal-service-http
//
// 该模块实现最小 loopback HTTP 协议. 请求头和普通正文分别限制为 16 KiB,
// SDK 诊断正文限制为 256 KiB, 本地存档导入和 transfer 上传正文限制为 8 MiB.
// 每个连接处理一个请求.

use crate::cn;
use crate::cn_asset;
use crate::cn_asset_files;
use crate::database::ServiceDatabase;
use crate::gameplay_settings;
use crate::local_saves;
use crate::management_web;
use crate::player_web;
use crate::remote_forward;
use crate::sdk_compat;
use crate::server_profiles;
use crate::PersonalServiceError;
use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddrV4, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const REQUEST_HEADER_LIMIT: usize = 16 * 1024;
const DEFAULT_REQUEST_BODY_LIMIT: usize = 256 * 1024;
const SDK_REPORT_BODY_LIMIT: usize = 256 * 1024;
const LOCAL_SAVE_IMPORT_BODY_LIMIT: usize = 8 * 1024 * 1024;
const SDK_RESPONSE_WRITE_TIMEOUT: Duration = Duration::from_secs(2);
const TRANSPORT_ERROR_STATUS: &str = "599 Transport Error";
const LOCAL_SAVE_IMPORT_PATH: &str = "/v1/local-saves/import";
const LOCAL_SAVE_ENCRYPTED_IMPORT_PATH: &str = "/v1/local-saves/import-encrypted";
const PLAYER_LOCAL_SAVE_IMPORT_PATH: &str = "/v1/player/local-saves/import";
const PLAYER_LOCAL_SAVE_ENCRYPTED_IMPORT_PATH: &str = "/v1/player/local-saves/import-encrypted";
const TRANSFER_SLOT_PREFIX: &str = "/v1/transfer/v1/slots/";

pub(crate) struct LoopbackServer {
    listener: TcpListener,
    port: u16,
    cn_asset_root: PathBuf,
    cn_override_root: PathBuf,
    cn_asset_digest_cache: cn_asset::ArchiveDigestCache,
    log_http_access: bool,
}

pub(crate) struct HttpRequest {
    method: String,
    target: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

impl HttpRequest {
    pub(crate) fn method(&self) -> &str {
        &self.method
    }

    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    pub(crate) fn target(&self) -> &str {
        &self.target
    }

    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub(crate) fn body(&self) -> &[u8] {
        &self.body
    }

    pub(crate) fn headers(&self) -> impl Iterator<Item = (&str, &str)> {
        self.headers
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }
}

// //// 解码单个 URL 路径片段 [@x380kkm 2026-08-19] ////
pub(crate) fn decode_path_segment(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        let high = decode_hex_digit(*bytes.get(index + 1)?)?;
        let low = decode_hex_digit(*bytes.get(index + 2)?)?;
        decoded.push((high << 4) | low);
        index += 3;
    }
    String::from_utf8(decoded).ok()
}

fn decode_hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}
// //// /解码单个 URL 路径片段 ////

pub(crate) struct HttpResponse {
    status: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    content_length: usize,
}

impl HttpResponse {
    pub(crate) fn json(status: &'static str, body: String) -> Self {
        let body = body.into_bytes();
        Self {
            status: status.to_owned(),
            headers: vec![("Content-Type".to_owned(), "application/json".to_owned())],
            content_length: body.len(),
            body,
        }
    }

    fn text(body: String) -> Self {
        let body = body.into_bytes();
        Self {
            status: "200 OK".to_owned(),
            headers: vec![(
                "Content-Type".to_owned(),
                "text/plain; charset=utf-8".to_owned(),
            )],
            content_length: body.len(),
            body,
        }
    }

    pub(crate) fn bytes(status: &'static str, content_type: &'static str, body: Vec<u8>) -> Self {
        Self {
            status: status.to_owned(),
            headers: vec![("Content-Type".to_owned(), content_type.to_owned())],
            content_length: body.len(),
            body,
        }
    }

    pub(crate) fn empty(status: &'static str) -> Self {
        Self {
            status: status.to_owned(),
            headers: Vec::new(),
            body: Vec::new(),
            content_length: 0,
        }
    }

    // //// 构造保留资源长度的 HEAD 响应 [@x380kkm 2026-08-21] ////
    pub(crate) fn head(
        status: &'static str,
        content_type: &'static str,
        content_length: usize,
    ) -> Self {
        Self {
            status: status.to_owned(),
            headers: vec![("Content-Type".to_owned(), content_type.to_owned())],
            body: Vec::new(),
            content_length,
        }
    }
    // //// /构造保留资源长度的 HEAD 响应 ////

    pub(crate) fn with_header(mut self, name: &'static str, value: &'static str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }

    pub(crate) fn with_header_value(mut self, name: &'static str, value: String) -> Self {
        self.headers.push((name.to_owned(), value));
        self
    }

    pub(crate) fn forwarded(status: String, headers: Vec<(String, String)>, body: Vec<u8>) -> Self {
        Self {
            status,
            headers,
            content_length: body.len(),
            body,
        }
    }

    pub(crate) fn is_success(&self) -> bool {
        self.status.starts_with('2')
    }

    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub(crate) fn body(&self) -> &[u8] {
        &self.body
    }
}

impl LoopbackServer {
    // //// 仅绑定 IPv4 loopback 监听端口 [@x380kkm 2026-07-22] ////
    pub(crate) fn bind(
        requested_port: u16,
        cn_asset_root: PathBuf,
        cn_override_root: PathBuf,
        log_http_access: bool,
    ) -> Result<Self, PersonalServiceError> {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, requested_port))
            .map_err(|error| {
                PersonalServiceError::new(format!("failed to bind loopback listener: {error}"))
            })?;
        listener.set_nonblocking(true).map_err(|error| {
            PersonalServiceError::new(format!("failed to configure loopback listener: {error}"))
        })?;
        let port = listener
            .local_addr()
            .map_err(|error| {
                PersonalServiceError::new(format!("failed to read listener address: {error}"))
            })?
            .port();
        Ok(Self {
            listener,
            port,
            cn_asset_root,
            cn_override_root,
            cn_asset_digest_cache: cn_asset::ArchiveDigestCache::default(),
            log_http_access,
        })
    }
    // //// /仅绑定 IPv4 loopback 监听端口 ////

    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    pub(crate) fn try_handle_next_request(
        &mut self,
        database: &mut ServiceDatabase,
    ) -> Result<bool, PersonalServiceError> {
        match self.listener.accept() {
            Ok((stream, _)) => {
                if let Err(error) = handle_connection(
                    stream,
                    database,
                    self.port,
                    &self.cn_asset_root,
                    &self.cn_override_root,
                    &mut self.cn_asset_digest_cache,
                    self.log_http_access,
                ) {
                    eprintln!("personal service connection ended with error: {error}");
                }
                Ok(true)
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => Ok(false),
            Err(error) => Err(PersonalServiceError::new(format!(
                "failed to accept loopback request: {error}"
            ))),
        }
    }
}

// //// 读取单个有界 HTTP 请求 [@x380kkm 2026-08-23] ////
fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, PersonalServiceError> {
    stream.set_nonblocking(false).map_err(|error| {
        PersonalServiceError::new(format!("failed to configure request socket: {error}"))
    })?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut request = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];
    loop {
        let request_limit = if let Some(header_end) = find_header_end(&request) {
            if header_end > REQUEST_HEADER_LIMIT {
                return Err(PersonalServiceError::new("HTTP headers exceed size limit"));
            }
            let content_length = parse_content_length(&request[..header_end])?;
            let body_limit = request_body_limit(&request[..header_end])?;
            if content_length > body_limit {
                return Err(PersonalServiceError::new("HTTP request exceeds size limit"));
            }
            let body_start = header_end
                .checked_add(4)
                .ok_or_else(|| PersonalServiceError::new("HTTP request length overflow"))?;
            let request_length = body_start
                .checked_add(content_length)
                .ok_or_else(|| PersonalServiceError::new("HTTP request length overflow"))?;
            if request.len() >= request_length {
                request.truncate(request_length);
                return parse_request(request, header_end, content_length);
            }
            request_length
        } else {
            REQUEST_HEADER_LIMIT
        };
        if request.len() >= request_limit {
            return Err(PersonalServiceError::new("HTTP request exceeds size limit"));
        }
        let available = (request_limit - request.len()).min(chunk.len());
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| PersonalServiceError::new("HTTP request timed out"))?;
        stream.set_read_timeout(Some(remaining)).map_err(|error| {
            PersonalServiceError::new(format!("failed to configure request timeout: {error}"))
        })?;
        match stream.read(&mut chunk[..available]) {
            Ok(0) => {
                return Err(PersonalServiceError::new(
                    "HTTP request ended before its declared body",
                ))
            }
            Ok(length) => {
                request.extend_from_slice(&chunk[..length]);
            }
            Err(error)
                if error.kind() == ErrorKind::WouldBlock || error.kind() == ErrorKind::TimedOut =>
            {
                return Err(PersonalServiceError::new("HTTP request timed out"))
            }
            Err(error) => {
                return Err(PersonalServiceError::new(format!(
                    "failed to read request: {error}"
                )))
            }
        }
    }
}

fn find_header_end(request: &[u8]) -> Option<usize> {
    request.windows(4).position(|window| window == b"\r\n\r\n")
}

fn parse_content_length(headers: &[u8]) -> Result<usize, PersonalServiceError> {
    let headers = std::str::from_utf8(headers)
        .map_err(|_| PersonalServiceError::new("HTTP headers are not UTF-8"))?;
    for header in headers.lines().skip(1) {
        let Some((name, value)) = header.split_once(':') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("content-length") {
            return value
                .trim()
                .parse::<usize>()
                .map_err(|_| PersonalServiceError::new("HTTP content length is invalid"));
        }
    }
    Ok(0)
}

fn request_body_limit(headers: &[u8]) -> Result<usize, PersonalServiceError> {
    let headers = std::str::from_utf8(headers)
        .map_err(|_| PersonalServiceError::new("HTTP headers are not UTF-8"))?;
    let request_target = headers
        .lines()
        .next()
        .and_then(|line| line.split_ascii_whitespace().nth(1))
        .ok_or_else(|| PersonalServiceError::new("HTTP request target is missing"))?;
    let path = request_target.split('?').next().unwrap_or_default();
    Ok(
        if path == LOCAL_SAVE_IMPORT_PATH
            || path == LOCAL_SAVE_ENCRYPTED_IMPORT_PATH
            || path == PLAYER_LOCAL_SAVE_IMPORT_PATH
            || path == PLAYER_LOCAL_SAVE_ENCRYPTED_IMPORT_PATH
            || path.starts_with(TRANSFER_SLOT_PREFIX)
        {
            LOCAL_SAVE_IMPORT_BODY_LIMIT
        } else if sdk_compat::is_sdk_diagnostic_path(path) {
            SDK_REPORT_BODY_LIMIT
        } else {
            DEFAULT_REQUEST_BODY_LIMIT
        },
    )
}

fn parse_request(
    request: Vec<u8>,
    header_end: usize,
    content_length: usize,
) -> Result<HttpRequest, PersonalServiceError> {
    let headers = std::str::from_utf8(&request[..header_end])
        .map_err(|_| PersonalServiceError::new("HTTP headers are not UTF-8"))?;
    let mut lines = headers.lines();
    let request_line = lines
        .next()
        .ok_or_else(|| PersonalServiceError::new("HTTP request line is missing"))?;
    let mut request_parts = request_line.split_ascii_whitespace();
    let method = request_parts
        .next()
        .ok_or_else(|| PersonalServiceError::new("HTTP method is missing"))?;
    let request_target = request_parts
        .next()
        .ok_or_else(|| PersonalServiceError::new("HTTP request target is missing"))?;
    let mut parsed_headers = HashMap::new();
    for header in lines {
        let Some((name, value)) = header.split_once(':') else {
            continue;
        };
        parsed_headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
    }
    let body_start = header_end
        .checked_add(4)
        .ok_or_else(|| PersonalServiceError::new("HTTP request length overflow"))?;
    let body_end = body_start
        .checked_add(content_length)
        .filter(|body_end| *body_end <= request.len())
        .ok_or_else(|| PersonalServiceError::new("HTTP request body is incomplete"))?;
    Ok(HttpRequest {
        method: method.to_owned(),
        target: request_target.to_owned(),
        path: request_target
            .split('?')
            .next()
            .unwrap_or_default()
            .to_owned(),
        headers: parsed_headers,
        body: request[body_start..body_end].to_vec(),
    })
}
// //// /读取单个有界 HTTP 请求 ////

fn route_request(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    port: u16,
    cn_asset_root: &Path,
    cn_override_root: &Path,
    cn_asset_digest_cache: &mut cn_asset::ArchiveDigestCache,
) -> Result<HttpResponse, PersonalServiceError> {
    if let Some(response) = cn_asset::route(
        request,
        database,
        cn_asset_root,
        cn_override_root,
        cn_asset_digest_cache,
    ) {
        return response;
    }
    if let Some(response) = cn_asset_files::route(request, cn_override_root, cn_asset_root) {
        return Ok(response);
    }
    if let Some(response) = player_web::route(request) {
        return Ok(response);
    }
    if let Some(response) = management_web::route(request, cn_asset_root) {
        return Ok(response);
    }
    if let Some(response) = crate::virtual_time::route(request, database) {
        return response;
    }
    if let Some(response) = crate::activity_catalog::route(request, database, cn_asset_root) {
        return response;
    }
    if let Some(response) =
        crate::activity_calendar::route(request, database, cn_asset_root, cn_override_root)
    {
        return response;
    }
    if let Some(response) = crate::cn_activity::management_route(request, database) {
        return response;
    }
    if let Some(response) = crate::cn_mail::management_route(request, database) {
        return response;
    }
    if let Some(response) = crate::management::route(request, database) {
        return response;
    }
    if let Some(response) = gameplay_settings::route(request, database) {
        return response;
    }
    if let Some(response) = server_profiles::route(request, database) {
        return response;
    }
    if let Some(response) = local_saves::transfer_route(request, database) {
        return response;
    }
    if let Some(response) = local_saves::route(request, database) {
        return response;
    }
    if let Some(response) = local_saves::player_route(request, database) {
        return response;
    }
    let active_profile = database.active_server_profile()?;
    if remote_forward::should_forward(request, &active_profile) {
        return remote_forward::forward(request, &active_profile, port, database);
    }
    if let Some(response) = cn::route(
        request,
        database,
        cn_asset_root,
        cn_override_root,
        cn_asset_digest_cache,
    ) {
        return response;
    }
    match (request.method(), request.path()) {
        ("GET", "/health") => {
            let generation = database.generation()?;
            Ok(HttpResponse::json(
                "200 OK",
                format!("{{\"status\":\"ok\",\"generation\":{generation}}}"),
            ))
        }
        ("POST", "/v1/state/increment") => {
            let generation = database.increment_generation()?;
            Ok(HttpResponse::json(
                "200 OK",
                format!("{{\"generation\":{generation}}}"),
            ))
        }
        ("POST", "/v1/checkpoint") => {
            database.checkpoint()?;
            Ok(HttpResponse::json(
                "200 OK",
                "{\"status\":\"ok\"}".to_owned(),
            ))
        }
        ("GET", "/shijtswy/version/client_release_ios.dis")
        | ("GET", "/shijtswy/version/client_release_android.dis") => {
            Ok(HttpResponse::text(format!(
                "// StarPoint CN compatibility endpoint\r\n{{\"default\":{{\"apiScheme\":\"http\",\"apiPath\":\"127.0.0.1:{port}\"}}}}"
            )))
        }
        _ => Ok(
            cn_asset_files::route_fallback(request, cn_override_root, cn_asset_root)
                .or_else(|| sdk_compat::route(request))
                .unwrap_or_else(|| {
                    HttpResponse::json(
                        "404 Not Found",
                        "{\"error\":\"not_found\"}".to_owned(),
                    )
                }),
        ),
    }
}

// //// 返回单个有界 HTTP 响应 [@x380kkm 2026-08-23] ////
fn handle_connection(
    mut stream: TcpStream,
    database: &mut ServiceDatabase,
    port: u16,
    cn_asset_root: &Path,
    cn_override_root: &Path,
    cn_asset_digest_cache: &mut cn_asset::ArchiveDigestCache,
    log_http_access: bool,
) -> Result<(), PersonalServiceError> {
    let mut request_metadata = None;
    let response = match read_request(&mut stream) {
        Ok(request) => {
            request_metadata = Some((request.method().to_owned(), request.path().to_owned()));
            route_request(
                &request,
                database,
                port,
                cn_asset_root,
                cn_override_root,
                cn_asset_digest_cache,
            )
            .unwrap_or_else(|error| {
                eprintln!(
                    "personal service request failed: method={} path={} error={}",
                    escape_http_access_field(request.method()),
                    escape_http_access_field(request.path()),
                    escape_http_access_field(&error.to_string()),
                );
                HttpResponse::json(
                    "500 Internal Server Error",
                    "{\"error\":\"internal_error\"}".to_owned(),
                )
            })
        }
        Err(_) => HttpResponse::json(
            "400 Bad Request",
            "{\"error\":\"invalid_http_request\"}".to_owned(),
        ),
    };
    let mut response_head = format!("HTTP/1.1 {}\r\n", response.status);
    for (name, value) in &response.headers {
        response_head.push_str(name);
        response_head.push_str(": ");
        response_head.push_str(value);
        response_head.push_str("\r\n");
    }
    if !response.status.starts_with("304 ") {
        response_head.push_str(&format!("Content-Length: {}\r\n", response.content_length));
    }
    response_head.push_str("Connection: close\r\n\r\n");
    let uses_short_write_timeout = request_metadata
        .as_ref()
        .is_some_and(|(_, path)| sdk_compat::is_sdk_diagnostic_path(path));
    let write_result = (|| -> std::io::Result<()> {
        if uses_short_write_timeout {
            stream.set_write_timeout(Some(SDK_RESPONSE_WRITE_TIMEOUT))?;
        }
        stream
            .write_all(response_head.as_bytes())
            .and_then(|_| stream.write_all(&response.body))
            .and_then(|_| stream.flush())
            .and_then(|_| stream.shutdown(Shutdown::Write))
    })();
    let observed_status = if write_result.is_ok() {
        response.status.as_str()
    } else {
        TRANSPORT_ERROR_STATUS
    };
    if let Some((method, path)) = request_metadata.as_ref() {
        let _ = database.record_http_observation(method, path, observed_status);
    }
    if log_http_access {
        write_http_access_log(request_metadata.as_ref(), &response, observed_status);
    }
    write_result
        .map_err(|error| PersonalServiceError::new(format!("failed to write response: {error}")))
}
// //// /返回单个有界 HTTP 响应 ////

// //// 输出不含正文、查询参数和请求头的 HTTP 访问日志 [@x380kkm 2026-08-23] ////
fn write_http_access_log(
    request: Option<&(String, String)>,
    response: &HttpResponse,
    status: &str,
) {
    let (method, path) = request
        .map(|(method, path)| (method.as_str(), path.as_str()))
        .unwrap_or(("INVALID", "-"));
    let content_type = response.header("Content-Type").unwrap_or("-");
    eprintln!(
        "http_access method={} path={} status={} content_type={}",
        escape_http_access_field(method),
        escape_http_access_field(path),
        escape_http_access_field(status),
        escape_http_access_field(content_type),
    );
}
// //// /输出不含正文、查询参数和请求头的 HTTP 访问日志 ////

// //// 转义 HTTP 访问日志字段中的控制字符 [@x380kkm 2026-08-12] ////
fn escape_http_access_field(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\r' => escaped.push_str("\\r"),
            '\n' => escaped.push_str("\\n"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(escaped, "\\u{{{:x}}}", character as u32);
            }
            character => escaped.push(character),
        }
    }
    escaped
}
// //// /转义 HTTP 访问日志字段中的控制字符 ////
