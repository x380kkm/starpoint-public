// audience: internal
// # personal-service-remote-request
//
// 该文件连接当前远端 HTTP 地址并写入一次 CN API 请求. 当前个人服务端口和非法地址不建立连接.

use super::headers;
use super::ForwardError;
use crate::database::ServerProfile;
use crate::http::HttpRequest;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

const CONNECT_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(2);

// //// 连接远端地址并发送请求 [@x380kkm 2026-07-23] ////
pub(super) fn connect_and_send(
    request: &HttpRequest,
    body: &[u8],
    profile: &ServerProfile,
    personal_service_port: u16,
    deadline: Instant,
) -> Result<TcpStream, ForwardError> {
    if profile.scheme.as_deref() != Some("http") {
        return Err(ForwardError::UnsupportedScheme);
    }
    let host = profile.host.as_deref().ok_or(ForwardError::InvalidTarget)?;
    let port = profile
        .port
        .and_then(|port| u16::try_from(port).ok())
        .ok_or(ForwardError::InvalidTarget)?;
    let addresses = resolve_addresses(host, port, personal_service_port, deadline)?;
    let mut stream = connect(addresses, deadline)?;
    write_request(&mut stream, request, body, host, port, deadline)?;
    Ok(stream)
}

fn resolve_addresses(
    host: &str,
    port: u16,
    personal_service_port: u16,
    deadline: Instant,
) -> Result<Vec<SocketAddr>, ForwardError> {
    let resolved = (host, port)
        .to_socket_addrs()
        .map_err(|_| ForwardError::Unavailable)?;
    if Instant::now() >= deadline {
        return Err(ForwardError::Unavailable);
    }
    let mut addresses = Vec::new();
    let mut rejected_address = false;
    for address in resolved {
        if is_forbidden_address(address, personal_service_port) {
            rejected_address = true;
        } else if !addresses.contains(&address) {
            addresses.push(address);
        }
    }
    if addresses.is_empty() {
        return Err(if rejected_address {
            ForwardError::ForbiddenTarget
        } else {
            ForwardError::Unavailable
        });
    }
    Ok(addresses)
}

fn connect(addresses: Vec<SocketAddr>, deadline: Instant) -> Result<TcpStream, ForwardError> {
    for address in addresses {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            break;
        };
        if let Ok(stream) =
            TcpStream::connect_timeout(&address, remaining.min(CONNECT_ATTEMPT_TIMEOUT))
        {
            return Ok(stream);
        }
    }
    Err(ForwardError::Unavailable)
}
// //// /连接远端地址并发送请求 ////

// //// 生成不含本机凭据的远端 HTTP 请求 [@x380kkm 2026-07-23] ////
fn write_request(
    stream: &mut TcpStream,
    request: &HttpRequest,
    body: &[u8],
    host: &str,
    port: u16,
    deadline: Instant,
) -> Result<(), ForwardError> {
    let host = host_header(host, port);
    let mut encoded = format!(
        "{} {} HTTP/1.1\r\nHost: {host}\r\n",
        request.method(),
        request.target()
    )
    .into_bytes();
    let connection_header = request.header("connection").unwrap_or_default();
    for (name, value) in request.headers() {
        if can_forward_header(name, value, connection_header) {
            encoded.extend_from_slice(name.as_bytes());
            encoded.extend_from_slice(b": ");
            encoded.extend_from_slice(value.as_bytes());
            encoded.extend_from_slice(b"\r\n");
        }
    }
    encoded.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    encoded.extend_from_slice(b"Connection: close\r\n\r\n");
    encoded.extend_from_slice(body);

    set_write_timeout(stream, deadline)?;
    stream
        .write_all(&encoded)
        .and_then(|_| stream.flush())
        .map_err(|_| ForwardError::Unavailable)
}

fn host_header(host: &str, port: u16) -> String {
    if host.parse::<Ipv6Addr>().is_ok() {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

fn can_forward_header(name: &str, value: &str, connection_header: &str) -> bool {
    headers::is_name(name)
        && headers::is_value(value)
        && !is_private_or_hop_by_hop_header(name)
        && !connection_header
            .split(',')
            .any(|token| token.trim().eq_ignore_ascii_case(name))
}

fn is_private_or_hop_by_hop_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "accept-encoding"
            | "authorization"
            | "connection"
            | "content-length"
            | "expect"
            | "host"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}
// //// /生成不含本机凭据的远端 HTTP 请求 ////

// //// 限制远端地址和写入时间 [@x380kkm 2026-07-23] ////
fn is_forbidden_address(address: SocketAddr, personal_service_port: u16) -> bool {
    let address_ip = normalize_ip_address(address.ip());
    if address_ip.is_loopback() && address.port() == personal_service_port {
        return true;
    }
    match address_ip {
        IpAddr::V4(address) => is_forbidden_ipv4(address),
        IpAddr::V6(address) => is_forbidden_ipv6(address),
    }
}

fn normalize_ip_address(address: IpAddr) -> IpAddr {
    match address {
        IpAddr::V6(address) => address.to_ipv4().map_or(IpAddr::V6(address), IpAddr::V4),
        address => address,
    }
}

fn is_forbidden_ipv4(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    octets[0] == 0 || (octets[0] == 169 && octets[1] == 254) || octets[0] >= 224
}

fn is_forbidden_ipv6(address: Ipv6Addr) -> bool {
    address.is_unspecified() || address.is_multicast() || (address.segments()[0] & 0xffc0) == 0xfe80
}

fn set_write_timeout(stream: &TcpStream, deadline: Instant) -> Result<(), ForwardError> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or(ForwardError::Unavailable)?;
    stream
        .set_write_timeout(Some(remaining))
        .map_err(|_| ForwardError::Unavailable)
}
// //// /限制远端地址和写入时间 ////
