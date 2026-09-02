// audience: internal | external
// # personal-service-player-web
// 该模块从个人服务同源提供普通玩家存档页面. 玩家 token 只由页面保存在内存中.

use crate::http::{HttpRequest, HttpResponse};

const INDEX_HTML: &[u8] = include_bytes!("../web/player/index.html");
const APP_JAVASCRIPT: &[u8] = include_bytes!("../web/player/app.js");

//// 提供无凭据的普通玩家页面资源 [@x380kkm 2026-07-24] ////
pub(crate) fn route(request: &HttpRequest) -> Option<HttpResponse> {
    let content = match request.path() {
        "/player" | "/player/" => Some(("text/html; charset=utf-8", INDEX_HTML)),
        "/player/app.js" => Some(("text/javascript; charset=utf-8", APP_JAVASCRIPT)),
        _ => None,
    }?;
    if request.method() != "GET" {
        return Some(
            HttpResponse::json(
                "405 Method Not Allowed",
                "{\"error\":\"method_not_allowed\"}".to_owned(),
            )
            .with_header("Allow", "GET"),
        );
    }
    Some(static_response(content.0, content.1))
}
//// /提供无凭据的普通玩家页面资源 ////

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
