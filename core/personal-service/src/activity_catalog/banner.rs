// audience: internal
// # personal-service-activity-banner
//
// 该模块只从 CN 资产根下的 activity-banners 目录读取 Web 安全图片. 图片路径和
// 规范化后的文件都必须保留在配置的资产根内.

use crate::http::{HttpRequest, HttpResponse};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

use super::manifest;

const ACTIVITY_BANNER_DIRECTORY: &str = "activity-banners";
const MAX_BANNER_BYTES: u64 = 16 * 1024 * 1024;

// //// 只读提供白名单 banner 图片 [@x380kkm 2026-08-19] ////
pub(super) fn serve(request: &HttpRequest, asset_root: &Path, image_key: &str) -> HttpResponse {
    if request.method() != "GET" {
        return method_not_allowed();
    }
    let Some(relative_path) = validate_key(image_key) else {
        return not_found();
    };
    if !manifest::contains_image_key(asset_root, image_key) {
        return not_found();
    }
    let banner_root = asset_root.join(ACTIVITY_BANNER_DIRECTORY);
    let Ok(canonical_asset_root) = asset_root.canonicalize() else {
        return not_found();
    };
    let Ok(canonical_root) = banner_root.canonicalize() else {
        return not_found();
    };
    if !canonical_root.starts_with(&canonical_asset_root) {
        return not_found();
    }
    let Ok(canonical_path) = banner_root.join(relative_path).canonicalize() else {
        return not_found();
    };
    if !canonical_path.starts_with(&canonical_root) || !canonical_path.is_file() {
        return not_found();
    }
    let Ok(metadata) = canonical_path.metadata() else {
        return not_found();
    };
    if metadata.len() > MAX_BANNER_BYTES {
        return not_found();
    }
    let Ok(body) = fs::read(&canonical_path) else {
        return not_found();
    };
    if body.len() as u64 > MAX_BANNER_BYTES {
        return not_found();
    }
    let Some(content_type) = content_type(&canonical_path, &body) else {
        return not_found();
    };
    let etag = format!("\"{:x}\"", Sha256::digest(&body));
    if request
        .header("if-none-match")
        .is_some_and(|value| matches_etag(value, &etag))
    {
        return HttpResponse::bytes("304 Not Modified", content_type, Vec::new())
            .with_header("Cache-Control", "private, no-cache")
            .with_header_value("ETag", etag)
            .with_header("X-Content-Type-Options", "nosniff");
    }
    HttpResponse::bytes("200 OK", content_type, body)
        .with_header("Cache-Control", "private, no-cache")
        .with_header_value("ETag", etag)
        .with_header("X-Content-Type-Options", "nosniff")
}

fn matches_etag(value: &str, etag: &str) -> bool {
    value
        .split(',')
        .map(str::trim)
        .any(|candidate| candidate == "*" || candidate == etag)
}

pub(super) fn validate_key(image_key: &str) -> Option<PathBuf> {
    if image_key.is_empty() || image_key.len() > 512 || image_key.contains('\\') {
        return None;
    }
    let mut path = PathBuf::new();
    for component in image_key.split('/') {
        if component.is_empty()
            || matches!(component, "." | "..")
            || !component
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return None;
        }
        path.push(component);
    }
    let extension = path.extension()?.to_str()?;
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "webp"
    )
    .then_some(path)
}

fn content_type(path: &Path, body: &[u8]) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" if body.starts_with(b"\x89PNG\r\n\x1a\n") => Some("image/png"),
        "jpg" | "jpeg" if body.starts_with(&[0xff, 0xd8, 0xff]) => Some("image/jpeg"),
        "webp" if body.len() >= 12 && body.starts_with(b"RIFF") && &body[8..12] == b"WEBP" => {
            Some("image/webp")
        }
        _ => None,
    }
}

fn not_found() -> HttpResponse {
    HttpResponse::json(
        "404 Not Found",
        "{\"error\":\"activity_banner_not_found\"}".to_owned(),
    )
}

fn method_not_allowed() -> HttpResponse {
    HttpResponse::json(
        "405 Method Not Allowed",
        "{\"error\":\"method_not_allowed\"}".to_owned(),
    )
    .with_header("Allow", "GET")
}
// //// /只读提供白名单 banner 图片 ////
