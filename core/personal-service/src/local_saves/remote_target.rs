// audience: internal
// # local-save-remote-target
//
// 该模块限制个人服务可以连接的远端地址并创建不读取系统代理的 HTTP 客户端.
// 明文 HTTP 只允许所有解析地址均为 loopback 的测试目标.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};
use std::time::Duration;
use url::Host;

pub(super) const RESPONSE_LIMIT: usize = 8 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) enum RemoteTargetError {
    Invalid,
    Unavailable,
}

// //// 创建受限远端连接 [@x380kkm 2026-08-03] ////
pub(super) fn create_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .try_proxy_from_env(false)
        .redirects(0)
        .timeout_connect(REQUEST_TIMEOUT)
        .timeout_read(REQUEST_TIMEOUT)
        .timeout_write(REQUEST_TIMEOUT)
        .build()
}

pub(super) fn origin(scheme: &str, host: &str, port: i64) -> Result<String, RemoteTargetError> {
    let port = u16::try_from(port).map_err(|_| RemoteTargetError::Invalid)?;
    let host = Host::parse(host).map_err(|_| RemoteTargetError::Invalid)?;
    let (resolved_host, encoded_host) = match host {
        Host::Domain(host) => (host.clone(), host),
        Host::Ipv4(host) => {
            let host = host.to_string();
            (host.clone(), host)
        }
        Host::Ipv6(host) => (host.to_string(), format!("[{host}]")),
    };
    let addresses = (resolved_host.as_str(), port)
        .to_socket_addrs()
        .map_err(|_| RemoteTargetError::Unavailable)?
        .collect::<Vec<_>>();
    if addresses.is_empty() {
        return Err(RemoteTargetError::Unavailable);
    }
    if addresses
        .iter()
        .any(|address| is_forbidden_address(address.ip()))
    {
        return Err(RemoteTargetError::Invalid);
    }
    let all_loopback = addresses.iter().all(|address| address.ip().is_loopback());
    if scheme != "https" && !(scheme == "http" && all_loopback) {
        return Err(RemoteTargetError::Invalid);
    }
    Ok(format!("{scheme}://{encoded_host}:{port}"))
}
// //// /创建受限远端连接 ////

// //// 拒绝不可路由和链路本地地址 [@x380kkm 2026-08-03] ////
fn is_forbidden_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_forbidden_ipv4(address),
        IpAddr::V6(address) => address
            .to_ipv4()
            .map_or_else(|| is_forbidden_ipv6(address), is_forbidden_ipv4),
    }
}

fn is_forbidden_ipv4(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    octets[0] == 0 || (octets[0] == 169 && octets[1] == 254) || octets[0] >= 224
}

fn is_forbidden_ipv6(address: Ipv6Addr) -> bool {
    address.is_unspecified() || address.is_multicast() || (address.segments()[0] & 0xffc0) == 0xfe80
}
// //// /拒绝不可路由和链路本地地址 ////
