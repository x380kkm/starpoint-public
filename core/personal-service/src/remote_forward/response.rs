// audience: internal
// # personal-service-remote-response
//
// 该文件读取一次远端 HTTP 响应. 响应头限制为 32 KiB, 解码后的正文限制为 8 MiB.

use super::headers;
use super::ForwardError;
use crate::http::HttpResponse;
use std::io::{ErrorKind, Read};
use std::net::TcpStream;
use std::time::Instant;

const HEADER_LIMIT: usize = 32 * 1024;
const BODY_LIMIT: usize = 8 * 1024 * 1024;
const WIRE_LIMIT: usize = HEADER_LIMIT + BODY_LIMIT;

enum ResponseFraming {
    ContentLength(usize),
    Chunked,
    UntilClose,
}

struct ResponseHead {
    status: String,
    headers: Vec<(String, String)>,
    framing: ResponseFraming,
}

// //// 读取有界远端 HTTP 响应 [@x380kkm 2026-07-23] ////
pub(super) fn read(
    stream: &mut TcpStream,
    deadline: Instant,
) -> Result<HttpResponse, ForwardError> {
    let mut wire = Vec::with_capacity(4096);
    let header_end = loop {
        if let Some(header_end) = find_header_end(&wire) {
            break header_end;
        }
        if wire.len() >= HEADER_LIMIT {
            return Err(ForwardError::ResponseTooLarge);
        }
        read_more(stream, &mut wire, deadline)?;
    };
    if header_end > HEADER_LIMIT {
        return Err(ForwardError::ResponseTooLarge);
    }
    let head = parse_head(&wire[..header_end])?;
    let body_start = header_end + 4;
    let body = match head.framing {
        ResponseFraming::ContentLength(length) => {
            read_content_length_body(stream, &mut wire, body_start, length, deadline)?
        }
        ResponseFraming::Chunked => read_chunked_body(stream, &mut wire, body_start, deadline)?,
        ResponseFraming::UntilClose => read_until_close(stream, &mut wire, body_start, deadline)?,
    };
    Ok(HttpResponse::forwarded(head.status, head.headers, body))
}

fn read_content_length_body(
    stream: &mut TcpStream,
    wire: &mut Vec<u8>,
    body_start: usize,
    length: usize,
    deadline: Instant,
) -> Result<Vec<u8>, ForwardError> {
    if length > BODY_LIMIT {
        return Err(ForwardError::ResponseTooLarge);
    }
    let body_end = body_start
        .checked_add(length)
        .ok_or(ForwardError::ResponseTooLarge)?;
    while wire.len() < body_end {
        read_more(stream, wire, deadline)?;
    }
    Ok(wire[body_start..body_end].to_vec())
}

fn read_chunked_body(
    stream: &mut TcpStream,
    wire: &mut Vec<u8>,
    body_start: usize,
    deadline: Instant,
) -> Result<Vec<u8>, ForwardError> {
    loop {
        if let Some(body) = decode_chunked_body(&wire[body_start..])? {
            return Ok(body);
        }
        read_more(stream, wire, deadline)?;
    }
}

fn read_until_close(
    stream: &mut TcpStream,
    wire: &mut Vec<u8>,
    body_start: usize,
    deadline: Instant,
) -> Result<Vec<u8>, ForwardError> {
    while read_more_allowing_end(stream, wire, deadline)? {}
    let body = &wire[body_start..];
    if body.len() > BODY_LIMIT {
        return Err(ForwardError::ResponseTooLarge);
    }
    Ok(body.to_vec())
}
// //// /读取有界远端 HTTP 响应 ////

// //// 解析远端响应头和正文边界 [@x380kkm 2026-07-23] ////
fn parse_head(encoded: &[u8]) -> Result<ResponseHead, ForwardError> {
    let encoded = std::str::from_utf8(encoded).map_err(|_| ForwardError::InvalidResponse)?;
    let mut lines = encoded.split("\r\n");
    let status_line = lines.next().ok_or(ForwardError::InvalidResponse)?;
    let mut status_parts = status_line.split_ascii_whitespace();
    let version = status_parts.next().ok_or(ForwardError::InvalidResponse)?;
    let status_code = status_parts
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|value| (200..=599).contains(value))
        .ok_or(ForwardError::InvalidResponse)?;
    if !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return Err(ForwardError::InvalidResponse);
    }

    let mut raw_headers = Vec::new();
    let mut content_length = None;
    let mut transfer_encoding = None;
    let mut connection_tokens = Vec::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(ForwardError::InvalidResponse)?;
        let name = name.trim();
        let value = value.trim();
        if !headers::is_name(name) || !headers::is_value(value) {
            return Err(ForwardError::InvalidResponse);
        }
        if name.eq_ignore_ascii_case("content-length") {
            let length = value
                .parse::<usize>()
                .map_err(|_| ForwardError::InvalidResponse)?;
            if content_length.is_some_and(|current| current != length) {
                return Err(ForwardError::InvalidResponse);
            }
            content_length = Some(length);
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            if transfer_encoding.is_some() {
                return Err(ForwardError::InvalidResponse);
            }
            transfer_encoding = Some(value.to_ascii_lowercase());
        } else if name.eq_ignore_ascii_case("connection") {
            connection_tokens.extend(
                value
                    .split(',')
                    .map(|token| token.trim().to_ascii_lowercase()),
            );
        }
        raw_headers.push((name.to_owned(), value.to_owned()));
    }

    let framing = match (content_length, transfer_encoding.as_deref()) {
        (Some(_), Some(_)) => return Err(ForwardError::InvalidResponse),
        (Some(length), None) => ResponseFraming::ContentLength(length),
        (None, Some("chunked")) => ResponseFraming::Chunked,
        (None, Some(_)) => return Err(ForwardError::InvalidResponse),
        (None, None) => ResponseFraming::UntilClose,
    };
    let mut headers = raw_headers
        .into_iter()
        .filter(|(name, _)| can_forward_header(name, &connection_tokens))
        .collect::<Vec<_>>();
    if !headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
    {
        headers.push((
            "Content-Type".to_owned(),
            "application/octet-stream".to_owned(),
        ));
    }
    Ok(ResponseHead {
        status: response_status(status_code),
        headers,
        framing,
    })
}

fn can_forward_header(name: &str, connection_tokens: &[String]) -> bool {
    !matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "content-length"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    ) && !connection_tokens
        .iter()
        .any(|token| token.eq_ignore_ascii_case(name))
}

fn response_status(status_code: u16) -> String {
    let reason = match status_code {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Remote Response",
    };
    format!("{status_code} {reason}")
}
// //// /解析远端响应头和正文边界 ////

// //// 解码远端分块正文 [@x380kkm 2026-07-23] ////
fn decode_chunked_body(encoded: &[u8]) -> Result<Option<Vec<u8>>, ForwardError> {
    let mut body = Vec::new();
    let mut cursor = 0;
    loop {
        let Some(line_end) = find_crlf(&encoded[cursor..]) else {
            return Ok(None);
        };
        let size = std::str::from_utf8(&encoded[cursor..cursor + line_end])
            .ok()
            .and_then(|line| line.split(';').next())
            .and_then(|value| usize::from_str_radix(value.trim(), 16).ok())
            .ok_or(ForwardError::InvalidResponse)?;
        cursor += line_end + 2;
        if size == 0 {
            if encoded.len() < cursor + 2 {
                return Ok(None);
            }
            if &encoded[cursor..cursor + 2] == b"\r\n" {
                return Ok(Some(body));
            }
            return if find_header_end(&encoded[cursor..]).is_some() {
                Ok(Some(body))
            } else {
                Ok(None)
            };
        }
        if size > BODY_LIMIT.saturating_sub(body.len()) {
            return Err(ForwardError::ResponseTooLarge);
        }
        let chunk_end = cursor
            .checked_add(size)
            .ok_or(ForwardError::ResponseTooLarge)?;
        if encoded.len() < chunk_end + 2 {
            return Ok(None);
        }
        if &encoded[chunk_end..chunk_end + 2] != b"\r\n" {
            return Err(ForwardError::InvalidResponse);
        }
        body.extend_from_slice(&encoded[cursor..chunk_end]);
        cursor = chunk_end + 2;
    }
}
// //// /解码远端分块正文 ////

// //// 限制远端套接字读取 [@x380kkm 2026-07-23] ////
fn read_more(
    stream: &mut TcpStream,
    buffer: &mut Vec<u8>,
    deadline: Instant,
) -> Result<(), ForwardError> {
    if read_more_allowing_end(stream, buffer, deadline)? {
        Ok(())
    } else {
        Err(ForwardError::InvalidResponse)
    }
}

fn read_more_allowing_end(
    stream: &mut TcpStream,
    buffer: &mut Vec<u8>,
    deadline: Instant,
) -> Result<bool, ForwardError> {
    if buffer.len() >= WIRE_LIMIT {
        return Err(ForwardError::ResponseTooLarge);
    }
    set_read_timeout(stream, deadline)?;
    let mut chunk = [0_u8; 8192];
    let available = (WIRE_LIMIT - buffer.len()).min(chunk.len());
    match stream.read(&mut chunk[..available]) {
        Ok(0) => Ok(false),
        Ok(length) => {
            buffer.extend_from_slice(&chunk[..length]);
            Ok(true)
        }
        Err(error)
            if error.kind() == ErrorKind::WouldBlock || error.kind() == ErrorKind::TimedOut =>
        {
            Err(ForwardError::Unavailable)
        }
        Err(_) => Err(ForwardError::Unavailable),
    }
}

fn set_read_timeout(stream: &TcpStream, deadline: Instant) -> Result<(), ForwardError> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or(ForwardError::Unavailable)?;
    stream
        .set_read_timeout(Some(remaining))
        .map_err(|_| ForwardError::Unavailable)
}

fn find_header_end(encoded: &[u8]) -> Option<usize> {
    encoded.windows(4).position(|window| window == b"\r\n\r\n")
}

fn find_crlf(encoded: &[u8]) -> Option<usize> {
    encoded.windows(2).position(|window| window == b"\r\n")
}
// //// /限制远端套接字读取 ////

#[cfg(test)]
mod tests {
    use super::{decode_chunked_body, parse_head};

    // //// 解码完整和未完成的分块正文 [@x380kkm 2026-07-23] ////
    #[test]
    fn decodes_complete_chunked_body() {
        let complete = decode_chunked_body(b"4\r\nforw\r\n5\r\narded\r\n0\r\n\r\n")
            .expect("complete chunked body is valid");
        assert_eq!(complete.as_deref(), Some(b"forwarded".as_slice()));

        let incomplete = decode_chunked_body(b"4\r\nforw\r\n5\r\nard")
            .expect("incomplete chunked body remains valid");
        assert!(incomplete.is_none());
    }
    // //// /解码完整和未完成的分块正文 ////

    // //// 拒绝没有最终响应的远端响应头 [@x380kkm 2026-07-23] ////
    #[test]
    fn rejects_interim_response_as_final_response() {
        assert!(parse_head(b"HTTP/1.1 100 Continue\r\nContent-Length: 0").is_err());
    }
    // //// /拒绝没有最终响应的远端响应头 ////
}
