// audience: internal
// # cn-asset-files
//
// 该模块依次从可写覆盖根和个人服务配置的 CN CDN 根提供客户端静态文件.
// /patch/cn/ 路由覆盖 entities, EntityLists 和 archive 目录.
// 通用回退把规范化的 URL 路径映射到资产根, 并保持规范路径位于资产根内.
// 当客户端使用 EntityLists 而资源包只有 entities 时, 路由只回退到同名文件.
// 两个实体清单目录的 empty.csv 路径在文件缺失时返回空 CSV.
// 三个雷霆展示文档路径以空文本响应作为兼容格式.

use crate::cn_asset::EMPTY_ENTITY_LIST_NAME;
use crate::http::{decode_path_segment, HttpRequest, HttpResponse};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

const PATCH_PREFIX: &str = "/patch/cn/";
const CSV_CONTENT_TYPE: &str = "text/csv; charset=utf-8";
const ARCHIVE_CONTENT_TYPE: &str = "application/zip";
const TEXT_CONTENT_TYPE: &str = "text/plain; charset=utf-8";
const GAME_SENSITIVE_VERSION_PATHS: [&str; 2] = [
    "protocols/leiting/sensitive/part/wf_version.txt",
    "protocols/leiting/sensitive/part/wf-text_version.txt",
];
const OPTIONAL_TEXT_DOCUMENT_PATHS: [&str; 3] = [
    "protocols/leiting/privacy/wf.txt",
    "protocols/leiting/license/common.txt",
    "protocols/leiting/updateTips/iOS/common.txt",
];
const ARCHIVE_DIRECTORIES: [&str; 8] = [
    "archive-common-full",
    "archive-medium-full",
    "archive-android-full",
    "archive-common-diff",
    "archive-medium-diff",
    "archive-android-diff",
    "archive-ios-full",
    "archive-ios-diff",
];

// //// 路由 CN 补丁静态文件 [@x380kkm 2026-08-22] ////
pub(crate) fn route(
    request: &HttpRequest,
    override_root: &Path,
    asset_root: &Path,
) -> Option<HttpResponse> {
    let relative_path = request.path().strip_prefix(PATCH_PREFIX)?;
    let is_csv_path = matches!(relative_path, "entities" | "EntityLists")
        || relative_path.starts_with("entities/")
        || relative_path.starts_with("entities\\")
        || relative_path.starts_with("EntityLists/")
        || relative_path.starts_with("EntityLists\\");
    let archive_directory = ARCHIVE_DIRECTORIES.iter().copied().find(|directory| {
        relative_path == *directory
            || relative_path.starts_with(&format!("{directory}/"))
            || relative_path.starts_with(&format!("{directory}\\"))
    });
    if !is_csv_path && archive_directory.is_none() {
        return None;
    }

    let allowed_methods = if is_csv_path { "GET" } else { "GET, HEAD" };
    if (is_csv_path && request.method() != "GET")
        || (!is_csv_path && !matches!(request.method(), "GET" | "HEAD"))
    {
        return Some(method_not_allowed(allowed_methods));
    }

    let expected_extension = if is_csv_path { "csv" } else { "zip" };
    let Some(relative_path) = validate_relative_path(relative_path, expected_extension) else {
        return Some(not_found());
    };
    let response = if is_csv_path {
        read_prioritized_csv_asset(override_root, asset_root, &relative_path, CSV_CONTENT_TYPE)
    } else {
        read_prioritized_archive(request, override_root, asset_root, &relative_path)
    };
    Some(response)
}
// //// /路由 CN 补丁静态文件 ////

// //// 从 CN CDN 根回退提供通用静态文件 [@x380kkm 2026-08-21] ////
pub(crate) fn route_fallback(
    request: &HttpRequest,
    override_root: &Path,
    asset_root: &Path,
) -> Option<HttpResponse> {
    if !matches!(request.method(), "GET" | "HEAD") {
        return None;
    }
    if request.path() == "/" {
        return None;
    }

    let Some(relative_path) = static_relative_path(request.path()) else {
        return Some(not_found());
    };
    let is_head = request.method() == "HEAD";
    read_static_asset(override_root, &relative_path, is_head)
        .or_else(|| read_static_asset(asset_root, &relative_path, is_head))
        .or_else(|| optional_text_document_response(&relative_path, is_head))
        .or_else(|| sensitive_version_response(&relative_path))
}
// //// /从 CN CDN 根回退提供通用静态文件 ////

// //// 规范化静态文件 URL 路径 [@x380kkm 2026-08-21] ////
fn static_relative_path(request_path: &str) -> Option<PathBuf> {
    let relative_path = request_path.strip_prefix('/')?;
    if relative_path.is_empty() || relative_path.starts_with('/') {
        return None;
    }

    let mut path = PathBuf::new();
    for encoded_component in relative_path.split('/') {
        let component = decode_path_segment(encoded_component)?;
        if component.is_empty()
            || component.contains(['/', '\\', ':'])
            || component.chars().any(char::is_control)
        {
            return None;
        }
        let mut components = Path::new(&component).components();
        if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
            return None;
        }
        path.push(component);
    }
    Some(path)
}
// //// /规范化静态文件 URL 路径 ////

// //// 读取资产根内的通用静态文件 [@x380kkm 2026-08-21] ////
fn read_static_asset(
    asset_root: &Path,
    relative_path: &Path,
    is_head: bool,
) -> Option<HttpResponse> {
    let canonical_root = asset_root.canonicalize().ok()?;
    let canonical_path = asset_root.join(relative_path).canonicalize().ok()?;
    if !canonical_path.starts_with(&canonical_root) || !canonical_path.is_file() {
        return None;
    }

    let content_type = static_content_type(relative_path);
    let response = if is_head {
        let content_length = usize::try_from(fs::metadata(canonical_path).ok()?.len()).ok()?;
        HttpResponse::head("200 OK", content_type, content_length)
    } else {
        HttpResponse::bytes("200 OK", content_type, fs::read(canonical_path).ok()?)
    };
    Some(
        response
            .with_header("Cache-Control", "no-store")
            .with_header("X-Content-Type-Options", "nosniff"),
    )
}
// //// /读取资产根内的通用静态文件 ////

// //// 选择静态文件媒体类型 [@x380kkm 2026-08-21] ////
fn static_content_type(relative_path: &Path) -> &'static str {
    if relative_path == Path::new("chat-sdk/sdk/user/v2/config.action")
        || relative_path == Path::new("chat-sdk/sdk/user/v2/appInit.action")
    {
        return "application/json";
    }
    match relative_path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("json") => "application/json",
        Some("txt") => TEXT_CONTENT_TYPE,
        Some("csv") => CSV_CONTENT_TYPE,
        Some("html") | Some("htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("xml") => "application/xml",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("zip") => ARCHIVE_CONTENT_TYPE,
        Some("gz") => "application/gzip",
        Some("wasm") => "application/wasm",
        Some("mp3") => "audio/mpeg",
        Some("ogg") => "audio/ogg",
        Some("mp4") => "video/mp4",
        _ => "application/octet-stream",
    }
}
// //// /选择静态文件媒体类型 ////

// //// 返回游戏敏感词版本清单的空重定向 [@x380kkm 2026-08-21] ////
fn sensitive_version_response(relative_path: &Path) -> Option<HttpResponse> {
    GAME_SENSITIVE_VERSION_PATHS
        .iter()
        .any(|path| relative_path == Path::new(path))
        .then(|| HttpResponse::bytes("302 Found", TEXT_CONTENT_TYPE, Vec::new()))
}
// //// /返回游戏敏感词版本清单的空重定向 ////

// //// 返回雷霆展示文档的空文本兼容响应 [@x380kkm 2026-08-21] ////
fn optional_text_document_response(relative_path: &Path, is_head: bool) -> Option<HttpResponse> {
    if !OPTIONAL_TEXT_DOCUMENT_PATHS
        .iter()
        .any(|path| relative_path == Path::new(path))
    {
        return None;
    }

    let response = if is_head {
        HttpResponse::head("200 OK", TEXT_CONTENT_TYPE, 0)
    } else {
        HttpResponse::bytes("200 OK", TEXT_CONTENT_TYPE, Vec::new())
    };
    Some(
        response
            .with_header("Cache-Control", "no-store")
            .with_header("X-Content-Type-Options", "nosniff"),
    )
}
// //// /返回雷霆展示文档的空文本兼容响应 ////

// //// 验证资产相对路径 [@x380kkm 2026-08-07] ////
fn validate_relative_path(relative_path: &str, expected_extension: &str) -> Option<PathBuf> {
    let components = relative_path.split('/').collect::<Vec<_>>();
    if components.len() < 2
        || components.iter().any(|component| {
            component.is_empty() || matches!(*component, "." | "..") || component.contains('\\')
        })
    {
        return None;
    }

    let mut path = PathBuf::new();
    for component in components {
        path.push(component);
    }
    path.extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| extension.eq_ignore_ascii_case(expected_extension))?;
    Some(path)
}
// //// /验证资产相对路径 ////

// //// 兼容 EntityLists 和 entities 目录名 [@x380kkm 2026-08-11] ////
fn resolve_csv_path(asset_root: &Path, relative_path: &Path) -> PathBuf {
    if asset_root.join(relative_path).is_file() {
        return relative_path.to_owned();
    }
    let Ok(suffix) = relative_path.strip_prefix("EntityLists") else {
        return relative_path.to_owned();
    };
    let fallback = Path::new("entities").join(suffix);
    if asset_root.join(&fallback).is_file() {
        fallback
    } else {
        relative_path.to_owned()
    }
}
// //// /兼容 EntityLists 和 entities 目录名 ////

// //// 按覆盖根和 CDN 根读取 EntityLists 兼容资产 [@x380kkm 2026-08-21] ////
fn read_prioritized_csv_asset(
    override_root: &Path,
    asset_root: &Path,
    relative_path: &Path,
    content_type: &'static str,
) -> HttpResponse {
    for root in [override_root, asset_root] {
        let resolved_path = resolve_csv_path(root, relative_path);
        if let Some(response) = read_asset(root, &resolved_path, content_type) {
            return response;
        }
    }
    empty_entity_list_response(relative_path).unwrap_or_else(not_found)
}
// //// /按覆盖根和 CDN 根读取 EntityLists 兼容资产 ////

// //// 返回版本检查使用的空实体清单 [@x380kkm 2026-08-21] ////
fn empty_entity_list_response(relative_path: &Path) -> Option<HttpResponse> {
    ["entities", "EntityLists"]
        .iter()
        .map(|directory| Path::new(directory).join(EMPTY_ENTITY_LIST_NAME))
        .any(|empty_path| relative_path == empty_path)
        .then(|| {
            HttpResponse::bytes("200 OK", CSV_CONTENT_TYPE, Vec::new())
                .with_header("Cache-Control", "no-store")
                .with_header("X-Content-Type-Options", "nosniff")
        })
}
// //// /返回版本检查使用的空实体清单 ////

// //// 按覆盖根和 CDN 根读取补丁归档 [@x380kkm 2026-08-22] ////
fn read_prioritized_archive(
    request: &HttpRequest,
    override_root: &Path,
    asset_root: &Path,
    relative_path: &Path,
) -> HttpResponse {
    read_archive(request, override_root, relative_path)
        .or_else(|| read_archive(request, asset_root, relative_path))
        .unwrap_or_else(not_found)
}
// //// /按覆盖根和 CDN 根读取补丁归档 ////

// //// 表示闭区间字节范围 [@x380kkm 2026-08-22] ////
struct ByteRange {
    start: u64,
    end: u64,
}
// //// /表示闭区间字节范围 ////

// //// 读取支持单区间请求的补丁归档 [@x380kkm 2026-08-22] ////
fn read_archive(
    request: &HttpRequest,
    asset_root: &Path,
    relative_path: &Path,
) -> Option<HttpResponse> {
    let canonical_root = asset_root.canonicalize().ok()?;
    let canonical_path = asset_root.join(relative_path).canonicalize().ok()?;
    if !canonical_path.starts_with(&canonical_root) || !canonical_path.is_file() {
        return None;
    }

    let full_length = fs::metadata(&canonical_path).ok()?.len();
    let response = if request.method() == "HEAD" {
        HttpResponse::head(
            "200 OK",
            ARCHIVE_CONTENT_TYPE,
            usize::try_from(full_length).ok()?,
        )
    } else if let Some(value) = request.header("Range") {
        let Ok(range) = parse_byte_range(value, full_length) else {
            return Some(range_not_satisfiable(full_length));
        };
        read_archive_range(&canonical_path, range, full_length)?
    } else {
        HttpResponse::bytes(
            "200 OK",
            ARCHIVE_CONTENT_TYPE,
            fs::read(canonical_path).ok()?,
        )
    };
    Some(with_archive_headers(response))
}
// //// /读取支持单区间请求的补丁归档 ////

// //// 解析单个字节区间 [@x380kkm 2026-08-22] ////
fn parse_byte_range(value: &str, full_length: u64) -> Result<ByteRange, ()> {
    let (unit, value) = value.split_once('=').ok_or(())?;
    if !unit.trim().eq_ignore_ascii_case("bytes") || value.contains(',') || full_length == 0 {
        return Err(());
    }

    let (start, end) = value.trim().split_once('-').ok_or(())?;
    if start.is_empty() {
        let suffix_length = end.parse::<u64>().map_err(|_| ())?;
        if suffix_length == 0 {
            return Err(());
        }
        return Ok(ByteRange {
            start: full_length.saturating_sub(suffix_length),
            end: full_length - 1,
        });
    }

    let start = start.parse::<u64>().map_err(|_| ())?;
    if start >= full_length {
        return Err(());
    }
    let end = if end.is_empty() {
        full_length - 1
    } else {
        end.parse::<u64>().map_err(|_| ())?.min(full_length - 1)
    };
    if end < start {
        return Err(());
    }
    Ok(ByteRange { start, end })
}
// //// /解析单个字节区间 ////

// //// 读取补丁归档的字节区间 [@x380kkm 2026-08-22] ////
fn read_archive_range(path: &Path, range: ByteRange, full_length: u64) -> Option<HttpResponse> {
    let range_length = range.end.checked_sub(range.start)?.checked_add(1)?;
    let mut body = vec![0; usize::try_from(range_length).ok()?];
    let mut file = File::open(path).ok()?;
    file.seek(SeekFrom::Start(range.start)).ok()?;
    file.read_exact(&mut body).ok()?;

    Some(
        HttpResponse::bytes("206 Partial Content", ARCHIVE_CONTENT_TYPE, body).with_header_value(
            "Content-Range",
            format!("bytes {}-{}/{}", range.start, range.end, full_length),
        ),
    )
}
// //// /读取补丁归档的字节区间 ////

// //// 添加补丁归档下载响应头 [@x380kkm 2026-08-22] ////
fn with_archive_headers(response: HttpResponse) -> HttpResponse {
    response
        .with_header("Accept-Ranges", "bytes")
        .with_header("Cache-Control", "no-store")
}
// //// /添加补丁归档下载响应头 ////

// //// 返回无法满足的字节区间响应 [@x380kkm 2026-08-22] ////
fn range_not_satisfiable(full_length: u64) -> HttpResponse {
    with_archive_headers(
        HttpResponse::empty("416 Range Not Satisfiable")
            .with_header_value("Content-Range", format!("bytes */{full_length}")),
    )
}
// //// /返回无法满足的字节区间响应 ////

// //// 读取单个资产根内的文件 [@x380kkm 2026-08-21] ////
fn read_asset(
    asset_root: &Path,
    relative_path: &Path,
    content_type: &'static str,
) -> Option<HttpResponse> {
    let canonical_root = asset_root.canonicalize().ok()?;
    let canonical_path = asset_root.join(relative_path).canonicalize().ok()?;
    if !canonical_path.starts_with(&canonical_root) || !canonical_path.is_file() {
        return None;
    }

    let body = fs::read(canonical_path).ok()?;
    Some(
        HttpResponse::bytes("200 OK", content_type, body)
            .with_header("Cache-Control", "no-store")
            .with_header("X-Content-Type-Options", "nosniff"),
    )
}
// //// /读取单个资产根内的文件 ////

// //// 返回资产缺失响应 [@x380kkm 2026-08-07] ////
fn not_found() -> HttpResponse {
    HttpResponse::json("404 Not Found", "{\"error\":\"not_found\"}".to_owned())
}
// //// /返回资产缺失响应 ////

// //// 返回方法拒绝响应 [@x380kkm 2026-08-22] ////
fn method_not_allowed(allowed_methods: &'static str) -> HttpResponse {
    HttpResponse::json(
        "405 Method Not Allowed",
        "{\"error\":\"method_not_allowed\"}".to_owned(),
    )
    .with_header("Allow", allowed_methods)
}
// //// /返回方法拒绝响应 ////
