// audience: internal
// # cn-comic
//
// 漫画目录位于 CDN 的 comic/index.json, kind 分别对应主漫画和教学漫画.
// 每类 episode 从 1 连续编号, 客户端用 total_count 读取最新话并计算页码.
// main 为 PNG, thumbnail_l 和 thumbnail_s 为 JPEG, 文件名使用原始 episode.
// 图片 URL 的缓存标识来自资源版本, 文件修改时间和长度.
// 指定尺寸不可读取时使用同一话的现有尺寸.

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::player_snapshot;
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const LIST_PATH: &str = "/api/index.php/comic/get_list";
const IMAGE_PATH: &str = "/api/index.php/comic/image";
const IMAGE_PREFIX: &str = "/comic/";
const PAGE_SIZE: usize = 9;
const MAX_CATALOG_BYTES: u64 = 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Deserialize)]
struct ComicRequest {
    viewer_id: i64,
    #[serde(default)]
    kind: i64,
    #[serde(default)]
    page_index: i64,
}

#[derive(Deserialize)]
struct ComicEntry {
    kind: i64,
    episode: i64,
    title: String,
}

struct ComicImage {
    kind: i64,
    episode: i64,
    size: &'static str,
}

// //// 分派漫画列表与本地图片请求 [@x380kkm 2026-09-20] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.path() == LIST_PATH {
        return Some(if request.method() == "POST" {
            list_comics(request, database, asset_root)
        } else {
            Ok(error_response(
                "405 Method Not Allowed",
                "method_not_allowed",
            ))
        });
    }
    let image = if request.path() == IMAGE_PATH {
        image_from_query(request)
    } else if let Some(path) = request.path().strip_prefix(IMAGE_PREFIX) {
        image_from_path(path)
    } else {
        return None;
    };
    Some(if request.method() != "GET" {
        Ok(error_response(
            "405 Method Not Allowed",
            "method_not_allowed",
        ))
    } else if let Some(image) = image {
        serve_image(asset_root, &image)
    } else {
        Ok(error_response("400 Bad Request", "invalid_comic_image"))
    })
}

// //// 按客户端原始话数生成倒序分页 [@x380kkm 2026-09-20] ////
fn list_comics(
    request: &HttpRequest,
    database: &ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ComicRequest>(request) {
        Ok(body) if (0..=1).contains(&body.kind) && body.page_index >= 0 => body,
        _ => return Ok(error_response("400 Bad Request", "invalid_comic_request")),
    };
    if let Err(response) = player_snapshot(database, body.viewer_id)? {
        return Ok(response);
    }
    let Some(host) = request.header("host").filter(|host| {
        !host.is_empty()
            && host.len() <= 255
            && host
                .chars()
                .all(|value| value.is_ascii_alphanumeric() || ".:-[]".contains(value))
    }) else {
        return Ok(error_response("400 Bad Request", "invalid_comic_host"));
    };
    let catalog_path = asset_root.join("comic/index.json");
    if !catalog_path.is_file() {
        return Ok(error_response(
            "503 Service Unavailable",
            "comic_catalog_unavailable",
        ));
    }
    let metadata = fs::metadata(&catalog_path).map_err(storage_error)?;
    if metadata.len() > MAX_CATALOG_BYTES {
        return Ok(error_response(
            "503 Service Unavailable",
            "comic_catalog_invalid",
        ));
    }
    let catalog: Vec<ComicEntry> = serde_json::from_slice(
        &fs::read(catalog_path).map_err(storage_error)?,
    )
    .map_err(|error| PersonalServiceError::new(format!("comic catalog is invalid: {error}")))?;
    let mut comics = catalog
        .into_iter()
        .filter(|entry| entry.kind == body.kind)
        .collect::<Vec<_>>();
    comics.sort_by_key(|entry| entry.episode);
    if comics.is_empty() {
        return Ok(error_response(
            "503 Service Unavailable",
            "comic_series_unavailable",
        ));
    }
    if comics
        .iter()
        .enumerate()
        .any(|(index, entry)| entry.episode != index as i64 + 1)
    {
        return Ok(error_response(
            "503 Service Unavailable",
            "comic_series_incomplete",
        ));
    }
    let total_count = comics.len();
    let start = usize::try_from(body.page_index)
        .unwrap_or(usize::MAX)
        .saturating_mul(PAGE_SIZE);
    let asset_version = read_asset_version(asset_root)?;
    let mut page = Vec::new();
    for comic in comics.into_iter().rev().skip(start).take(PAGE_SIZE) {
        let mut media_images = serde_json::Map::new();
        for size in ["main", "thumbnail_l", "thumbnail_s"] {
            let image = ComicImage {
                kind: comic.kind,
                episode: comic.episode,
                size,
            };
            let Some((available_image, path)) = resolve_image(asset_root, &image) else {
                return Ok(error_response(
                    "503 Service Unavailable",
                    "comic_image_missing",
                ));
            };
            media_images.insert(
                size.to_owned(),
                image_url(host, &available_image, &path, asset_version.as_deref())?.into(),
            );
        }
        page.push(json!({
            "episode": comic.episode,
            "title": comic.title,
            "media_image": media_images,
        }));
    }
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({
            "comic_list": page,
            "current_page_index": body.page_index,
            "total_count": total_count,
        }),
    )
}

// //// 读取漫画 URL 使用的本地资源版本 [@x380kkm 2026-09-21] ////
fn read_asset_version(asset_root: &Path) -> Result<Option<String>, PersonalServiceError> {
    let bytes = match fs::read(asset_root.join("path")) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(storage_error(error)),
    };
    let manifest: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
        PersonalServiceError::new(format!("comic asset manifest is invalid: {error}"))
    })?;
    Ok(["eventual_target_asset_version", "target_asset_version"]
        .iter()
        .find_map(|field| {
            manifest["info"][field]
                .as_str()
                .filter(|value| !value.is_empty())
        })
        .map(str::to_owned))
}
// //// /读取漫画 URL 使用的本地资源版本 ////

// //// 从图片文件状态生成可缓存的访问地址 [@x380kkm 2026-09-21] ////
fn image_url(
    host: &str,
    image: &ComicImage,
    path: &Path,
    asset_version: Option<&str>,
) -> Result<String, PersonalServiceError> {
    let metadata = fs::metadata(path).map_err(storage_error)?;
    let modified = metadata
        .modified()
        .map_err(storage_error)?
        .duration_since(UNIX_EPOCH)
        .map_err(|error| {
            PersonalServiceError::new(format!("comic modification time is invalid: {error}"))
        })?;
    let file_revision = format!("{}-{}", modified.as_nanos(), metadata.len());
    let revision = match asset_version {
        Some(version) => format!("{version}-{file_revision}"),
        None => file_revision,
    };
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("v", &revision)
        .finish();
    Ok(format!(
        "http://{host}{IMAGE_PREFIX}{}/{}/{}?{query}",
        image.kind,
        image.size,
        image_filename(image),
    ))
}
// //// /从图片文件状态生成可缓存的访问地址 ////

// //// 解析受限图片编号和尺寸 [@x380kkm 2026-09-20] ////
fn image_from_query(request: &HttpRequest) -> Option<ComicImage> {
    let (_, query) = request.target().split_once('?')?;
    let mut kind = 0;
    let mut episode = None;
    let mut size = "main";
    for (name, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match name.as_ref() {
            "kind" => kind = value.parse().ok()?,
            "episode" => episode = value.parse().ok(),
            "size" => {
                size = match value.as_ref() {
                    "s" | "thumbnail_s" => "thumbnail_s",
                    "l" | "thumbnail_l" => "thumbnail_l",
                    "main" => "main",
                    _ => return None,
                }
            }
            _ => {}
        }
    }
    valid_image(kind, episode?, size)
}

fn image_from_path(path: &str) -> Option<ComicImage> {
    let segments = path.split('/').collect::<Vec<_>>();
    let [kind, size, filename] = segments.as_slice() else {
        return None;
    };
    let (size, extension) = match *size {
        "main" => ("main", ".png"),
        "thumbnail_l" => ("thumbnail_l", ".jpg"),
        "thumbnail_s" => ("thumbnail_s", ".jpg"),
        _ => return None,
    };
    valid_image(
        kind.parse().ok()?,
        filename.strip_suffix(extension)?.parse().ok()?,
        size,
    )
}

fn valid_image(kind: i64, episode: i64, size: &'static str) -> Option<ComicImage> {
    ((0..=1).contains(&kind) && episode > 0).then_some(ComicImage {
        kind,
        episode,
        size,
    })
}

// //// 将编号解析到漫画目录内的真实图片 [@x380kkm 2026-09-20] ////
fn image_path(asset_root: &Path, image: &ComicImage) -> Option<PathBuf> {
    let root = asset_root.join("comic").canonicalize().ok()?;
    let path = root
        .join(image.kind.to_string())
        .join(image.size)
        .join(image_filename(image))
        .canonicalize()
        .ok()?;
    (path.starts_with(root) && path.is_file()).then_some(path)
}

// //// 根据漫画尺寸确定图片文件名 [@x380kkm 2026-09-21] ////
fn image_filename(image: &ComicImage) -> String {
    let extension = if image.size == "main" { "png" } else { "jpg" };
    format!("{}.{extension}", image.episode)
}

// //// 选择同话内具有非空内容的可用图片 [@x380kkm 2026-09-26] ////
fn resolve_image(asset_root: &Path, image: &ComicImage) -> Option<(ComicImage, PathBuf)> {
    let alternatives = ["main", "thumbnail_l", "thumbnail_s"];
    std::iter::once(image.size)
        .chain(alternatives.into_iter().filter(|size| *size != image.size))
        .find_map(|size| {
            let candidate = ComicImage {
                kind: image.kind,
                episode: image.episode,
                size,
            };
            let path = image_path(asset_root, &candidate)?;
            let length = fs::metadata(&path).ok()?.len();
            (length > 0 && length <= MAX_IMAGE_BYTES).then_some((candidate, path))
        })
}

fn serve_image(
    asset_root: &Path,
    image: &ComicImage,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some((available_image, path)) = resolve_image(asset_root, image) else {
        return Ok(error_response("404 Not Found", "comic_image_not_found"));
    };
    let bytes = fs::read(path).map_err(storage_error)?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Ok(error_response(
            "503 Service Unavailable",
            "comic_image_unavailable",
        ));
    }
    let content_type = if available_image.size == "main" {
        "image/png"
    } else {
        "image/jpeg"
    };
    let cache_control = if available_image.size == image.size {
        "public, max-age=86400"
    } else {
        "no-cache"
    };
    Ok(HttpResponse::bytes("200 OK", content_type, bytes)
        .with_header("Cache-Control", cache_control))
}

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, json!({"error":code}).to_string())
}

fn storage_error(error: std::io::Error) -> PersonalServiceError {
    PersonalServiceError::new(format!("comic resource cannot be read: {error}"))
}
