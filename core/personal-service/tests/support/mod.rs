// audience: internal
// # personal-service-test-support
//
// 该模块发送个人服务集成测试使用的 loopback HTTP 请求.

use std::io::{Read, Write};
use std::net::TcpStream;

// //// 发送本机 HTTP 请求 [@x380kkm 2026-07-22] ////
pub(crate) fn request(port: u16, method: &str, path: &str) -> String {
    request_with_body(port, method, path, "application/octet-stream", b"")
}
// //// /发送本机 HTTP 请求 ////

// //// 发送带正文的本机 HTTP 请求 [@x380kkm 2026-07-22] ////
pub(crate) fn request_with_body(
    port: u16,
    method: &str,
    path: &str,
    content_type: &str,
    body: &[u8],
) -> String {
    request_with_headers(port, method, path, content_type, &[], body)
}
// //// /发送带正文的本机 HTTP 请求 ////

// //// 发送带自定义头的本机 HTTP 请求 [@x380kkm 2026-07-22] ////
pub(crate) fn request_with_headers(
    port: u16,
    method: &str,
    path: &str,
    content_type: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> String {
    String::from_utf8(request_bytes_with_headers(
        port,
        method,
        path,
        content_type,
        headers,
        body,
    ))
    .expect("response is valid UTF-8")
}
// //// /发送带自定义头的本机 HTTP 请求 ////

// //// 发送并保留二进制 HTTP 响应 [@x380kkm 2026-08-20] ////
pub(crate) fn request_bytes(port: u16, method: &str, path: &str) -> Vec<u8> {
    request_bytes_with_headers(port, method, path, "application/octet-stream", &[], b"")
}

fn request_bytes_with_headers(
    port: u16,
    method: &str,
    path: &str,
    content_type: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Vec<u8> {
    let mut stream =
        TcpStream::connect(("127.0.0.1", port)).expect("service accepts loopback requests");
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: {content_type}\r\n",
    )
    .expect("request headers are written");
    for (name, value) in headers {
        write!(stream, "{name}: {value}\r\n").expect("custom request header is written");
    }
    write!(stream, "Content-Length: {}\r\n\r\n", body.len()).expect("request length is written");
    stream.write_all(body).expect("request body is written");
    let mut response = Vec::new();
    stream.read_to_end(&mut response).expect("response is read");
    response
}
// //// /发送并保留二进制 HTTP 响应 ////
