// audience: internal
// # server-profile-probe
//
// 该模块读取 Starpoint /healthz 响应. 探测请求不使用系统代理, 不改变当前服务器配置.

use crate::database::{ServerProfile, ServerProfileMode};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::time::{Duration, Instant};
use url::Host;

const HEALTH_RESPONSE_LIMIT: usize = 16 * 1024;
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Serialize)]
pub(super) struct ServerProfileProbe {
    pub(super) profile_id: i64,
    pub(super) reachable: bool,
    pub(super) compatible: bool,
    pub(super) latency_ms: u64,
    pub(super) server_date: Option<String>,
    pub(super) http_port: Option<u16>,
    pub(super) session_port: Option<u16>,
    pub(super) failure: Option<&'static str>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HealthResponse {
    status: String,
    service: String,
    server_date: String,
    http_port: u16,
    session_port: u16,
}

// //// 探测本机或远端服务器配置 [@x380kkm 2026-07-24] ////
pub(super) fn probe_server_profile(profile: &ServerProfile) -> ServerProfileProbe {
    if profile.mode == ServerProfileMode::Local {
        return ServerProfileProbe {
            profile_id: profile.id,
            reachable: true,
            compatible: true,
            latency_ms: 0,
            server_date: None,
            http_port: None,
            session_port: None,
            failure: None,
        };
    }
    let started_at = Instant::now();
    let Some(health_url) = health_url(profile) else {
        return failed_probe(profile.id, started_at, false, "invalid_endpoint");
    };
    let agent = ureq::AgentBuilder::new()
        .try_proxy_from_env(false)
        .redirects(0)
        .timeout_connect(PROBE_TIMEOUT)
        .timeout_read(PROBE_TIMEOUT)
        .timeout_write(PROBE_TIMEOUT)
        .build();
    let response = match agent.get(&health_url).call() {
        Ok(response) => response,
        Err(ureq::Error::Status(_, _)) => {
            return failed_probe(profile.id, started_at, true, "unexpected_status")
        }
        Err(ureq::Error::Transport(_)) => {
            return failed_probe(profile.id, started_at, false, "connection_failed")
        }
    };
    let Some(health) = read_health_response(response) else {
        return failed_probe(profile.id, started_at, true, "invalid_response");
    };
    if health.status != "ok" || health.service != "starpoint" {
        return failed_probe(profile.id, started_at, true, "incompatible_service");
    }
    ServerProfileProbe {
        profile_id: profile.id,
        reachable: true,
        compatible: true,
        latency_ms: elapsed_milliseconds(started_at),
        server_date: Some(health.server_date),
        http_port: Some(health.http_port),
        session_port: Some(health.session_port),
        failure: None,
    }
}

fn health_url(profile: &ServerProfile) -> Option<String> {
    let scheme = profile.scheme.as_deref()?;
    let host = Host::parse(profile.host.as_deref()?).ok()?;
    let encoded_host = match host {
        Host::Domain(host) => host,
        Host::Ipv4(host) => host.to_string(),
        Host::Ipv6(host) => format!("[{host}]"),
    };
    let port = u16::try_from(profile.port?).ok()?;
    Some(format!("{scheme}://{encoded_host}:{port}/healthz"))
}

fn read_health_response(response: ureq::Response) -> Option<HealthResponse> {
    let mut body = Vec::new();
    response
        .into_reader()
        .take((HEALTH_RESPONSE_LIMIT + 1) as u64)
        .read_to_end(&mut body)
        .ok()?;
    if body.len() > HEALTH_RESPONSE_LIMIT {
        return None;
    }
    serde_json::from_slice(&body).ok()
}

fn failed_probe(
    profile_id: i64,
    started_at: Instant,
    reachable: bool,
    failure: &'static str,
) -> ServerProfileProbe {
    ServerProfileProbe {
        profile_id,
        reachable,
        compatible: false,
        latency_ms: elapsed_milliseconds(started_at),
        server_date: None,
        http_port: None,
        session_port: None,
        failure: Some(failure),
    }
}

fn elapsed_milliseconds(started_at: Instant) -> u64 {
    u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX)
}
// //// /探测本机或远端服务器配置 ////
