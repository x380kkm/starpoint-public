// audience: internal | external
// # personal-service-management-web
//
// 该模块从 loopback 个人服务同源提供打开即用的静态管理页面.

use crate::http::{HttpRequest, HttpResponse};
use std::fs;
use std::path::Path;

const INDEX_HTML: &[u8] = include_bytes!("../web/management/index.html");
const APP_JAVASCRIPT: &[u8] = include_bytes!("../web/management/app.js");
const AI_TEAM_CONTROLLER_JAVASCRIPT: &[u8] =
    include_bytes!("../web/management/ai-team-controller.js");
const ACTIVITY_CONTROLLER_JAVASCRIPT: &[u8] =
    include_bytes!("../web/management/activity-controller.js");
const ACTIVITY_VIEWS_JAVASCRIPT: &[u8] = include_bytes!("../web/management/activity-views.js");
const MAIL_REWARD_CONTROLLER_JAVASCRIPT: &[u8] =
    include_bytes!("../web/management/mail-reward-controller.js");
const VIEWS_JAVASCRIPT: &[u8] = include_bytes!("../web/management/views.js");
const STYLESHEET: &[u8] = include_bytes!("../web/management/style.css");
const ITEM_PLACEHOLDER: &[u8] = include_bytes!("../web/management/assets/item-placeholder.svg");
const ITEM_ICON_PREFIX: &str = "/manage/assets/item-icons/";
const MAX_ITEM_ICON_BYTES: u64 = 2 * 1024 * 1024;
const GAME_BACKDROP_PATH: &str = "/manage/assets/game-backdrop.png";
const GAME_BACKDROP_FILE: &str = "06b6a99fc52fb097c293c65439d76909053e4295.png";
const MAX_GAME_BACKDROP_BYTES: u64 = 512 * 1024;
const EMBEDDED_ITEM_ICONS: &[(&str, &[u8])] = &[
    (
        "currency.free-mana",
        include_bytes!("../web/management/assets/item-icons/currency.free-mana.png"),
    ),
    (
        "currency.free-vmoney",
        include_bytes!("../web/management/assets/item-icons/currency.free-vmoney.png"),
    ),
    (
        "growth.exp-pool",
        include_bytes!("../web/management/assets/item-icons/growth.exp-pool.png"),
    ),
    (
        "stamina.recovery.large",
        include_bytes!("../web/management/assets/item-icons/stamina.recovery.large.png"),
    ),
    (
        "stamina.recovery.medium",
        include_bytes!("../web/management/assets/item-icons/stamina.recovery.medium.png"),
    ),
    (
        "stamina.recovery.small",
        include_bytes!("../web/management/assets/item-icons/stamina.recovery.small.png"),
    ),
    (
        "stamina.recovery.tiny",
        include_bytes!("../web/management/assets/item-icons/stamina.recovery.tiny.png"),
    ),
    (
        "ticket.character.multi",
        include_bytes!("../web/management/assets/item-icons/ticket.character.multi.png"),
    ),
    (
        "ticket.character.single",
        include_bytes!("../web/management/assets/item-icons/ticket.character.single.png"),
    ),
    (
        "ticket.weapon.multi",
        include_bytes!("../web/management/assets/item-icons/ticket.weapon.multi.png"),
    ),
    (
        "ticket.weapon.single",
        include_bytes!("../web/management/assets/item-icons/ticket.weapon.single.png"),
    ),
];

// //// 提供无凭据的同源管理页面资源 [@x380kkm 2026-07-23] ////
pub(crate) fn route(request: &HttpRequest, cn_asset_root: &Path) -> Option<HttpResponse> {
    if request.path() == GAME_BACKDROP_PATH {
        if request.method() != "GET" {
            return Some(method_not_allowed_response());
        }
        return Some(game_backdrop_response(cn_asset_root));
    }
    if request.path().starts_with(ITEM_ICON_PREFIX) {
        if request.method() != "GET" {
            return Some(method_not_allowed_response());
        }
        return Some(match item_icon_key(request.path()) {
            Some(icon_key) => item_icon_response(cn_asset_root, icon_key),
            None => not_found_response(),
        });
    }
    let content = match request.path() {
        "/manage" | "/manage/" => Some(("text/html; charset=utf-8", INDEX_HTML)),
        "/manage/app.js" => Some(("text/javascript; charset=utf-8", APP_JAVASCRIPT)),
        "/manage/ai-team-controller.js" => Some((
            "text/javascript; charset=utf-8",
            AI_TEAM_CONTROLLER_JAVASCRIPT,
        )),
        "/manage/activity-controller.js" => Some((
            "text/javascript; charset=utf-8",
            ACTIVITY_CONTROLLER_JAVASCRIPT,
        )),
        "/manage/activity-views.js" => {
            Some(("text/javascript; charset=utf-8", ACTIVITY_VIEWS_JAVASCRIPT))
        }
        "/manage/mail-reward-controller.js" => Some((
            "text/javascript; charset=utf-8",
            MAIL_REWARD_CONTROLLER_JAVASCRIPT,
        )),
        "/manage/views.js" => Some(("text/javascript; charset=utf-8", VIEWS_JAVASCRIPT)),
        "/manage/style.css" => Some(("text/css; charset=utf-8", STYLESHEET)),
        "/manage/assets/item-placeholder.svg" => Some(("image/svg+xml", ITEM_PLACEHOLDER)),
        _ => None,
    }?;
    if request.method() != "GET" {
        return Some(method_not_allowed_response());
    }
    Some(static_response(content.0, content.1))
}

fn item_icon_key(path: &str) -> Option<&str> {
    let file_name = path.strip_prefix(ITEM_ICON_PREFIX)?;
    let key = file_name.strip_suffix(".png")?;
    let is_safe = !key.is_empty()
        && key.len() <= 96
        && !key.starts_with('.')
        && !key.contains("..")
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'));
    is_safe.then_some(key)
}

fn item_icon_response(cn_asset_root: &Path, key: &str) -> HttpResponse {
    let path = cn_asset_root
        .join("management-assets")
        .join("item-icons")
        .join(format!("{key}.png"));
    let body = fs::metadata(&path)
        .ok()
        .filter(|metadata| metadata.is_file() && metadata.len() <= MAX_ITEM_ICON_BYTES)
        .and_then(|_| fs::read(path).ok())
        .filter(|body| body.starts_with(b"\x89PNG\r\n\x1a\n"));
    match body.as_deref().or_else(|| embedded_item_icon(key)) {
        Some(body) => static_response("image/png", body),
        None => not_found_response(),
    }
}

fn embedded_item_icon(key: &str) -> Option<&'static [u8]> {
    EMBEDDED_ITEM_ICONS
        .iter()
        .find_map(|(embedded_key, body)| (*embedded_key == key).then_some(*body))
}

fn game_backdrop_response(cn_asset_root: &Path) -> HttpResponse {
    let path = cn_asset_root
        .join("activity-banners")
        .join(GAME_BACKDROP_FILE);
    let body = fs::metadata(&path)
        .ok()
        .filter(|metadata| metadata.is_file() && metadata.len() <= MAX_GAME_BACKDROP_BYTES)
        .and_then(|_| fs::read(path).ok())
        .filter(|body| body.starts_with(b"\x89PNG\r\n\x1a\n"));
    match body {
        Some(body) => static_response("image/png", &body),
        None => not_found_response(),
    }
}

fn method_not_allowed_response() -> HttpResponse {
    HttpResponse::json(
        "405 Method Not Allowed",
        "{\"error\":\"method_not_allowed\"}".to_owned(),
    )
    .with_header("Allow", "GET")
}

fn not_found_response() -> HttpResponse {
    HttpResponse::json(
        "404 Not Found",
        "{\"error\":\"management_asset_not_found\"}".to_owned(),
    )
    .with_header("Cache-Control", "no-store")
    .with_header("X-Content-Type-Options", "nosniff")
}

fn static_response(content_type: &'static str, body: &[u8]) -> HttpResponse {
    HttpResponse::bytes("200 OK", content_type, body.to_vec())
        .with_header("Cache-Control", "no-store")
        .with_header("X-Content-Type-Options", "nosniff")
        .with_header("X-Frame-Options", "DENY")
        .with_header("Referrer-Policy", "no-referrer")
        .with_header(
            "Permissions-Policy",
            "camera=(), microphone=(), geolocation=()",
        )
        .with_header(
            "Content-Security-Policy",
            "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
        )
}
// //// /提供无凭据的同源管理页面资源 ////
