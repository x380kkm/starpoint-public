// audience: internal
// # cn-asset-recovery
// 客户端恢复下载使用实体摘要或平台清单内的相对路径, 文件名继续由清单决定.
// 已登记的实体从可写目录或 CDN 归档读取, 返回内容与清单摘要一致.
// 清单兼容官方 Base64 字母替换与本地 URL_SAFE 编码; 清单和归档按实体请求关联, 目标文件变化即时刷新.

mod sources;
mod zip_reader;

use crate::cn_asset::{ALL_ARCHIVE_DIRECTORIES, ENTITY_MANIFESTS};
use crate::http::{HttpRequest, HttpResponse};
use base64::engine::general_purpose::{STANDARD_NO_PAD, URL_SAFE_NO_PAD};
use base64::Engine;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Default)]
pub(crate) struct RecoveryCache {
    fingerprint: Vec<FileStamp>,
    entities: HashMap<String, Entity>,
    paths: HashMap<String, String>,
    archives: zip_reader::Cache,
    manifests: Vec<sources::Manifest>,
    archive_sources: Vec<sources::Archive>,
    preferred_manifest: Option<usize>,
}

#[derive(PartialEq)]
struct FileStamp {
    path: PathBuf,
    size: u64,
    modified: Option<SystemTime>,
}

struct Entity {
    records: Vec<sources::Record>,
    expanded: bool,
}

// //// 将实体摘要请求映射到受清单约束的资源 [@x380kkm 2026-09-20] ////
pub(crate) fn route(
    request: &HttpRequest,
    override_root: &Path,
    asset_root: &Path,
    cache: &mut RecoveryCache,
) -> Option<io::Result<HttpResponse>> {
    let digest = request
        .path()
        .strip_prefix("/patch/cn/entities/")
        .or_else(|| request.path().strip_prefix("/patch/cn/EntityLists/"))?;
    if !is_digest(digest) && !digest.starts_with("by-path/") {
        return None;
    }
    if !matches!(request.method(), "GET" | "HEAD") {
        return Some(Ok(HttpResponse::bytes(
            "405 Method Not Allowed",
            "text/plain",
            Vec::new(),
        )
        .with_header("Allow", "GET, HEAD")));
    }
    Some(serve(
        request.method(),
        digest,
        [asset_root, override_root],
        cache,
    ))
}

fn serve(
    method: &str,
    digest: &str,
    roots: [&Path; 2],
    cache: &mut RecoveryCache,
) -> io::Result<HttpResponse> {
    let fingerprint = source_fingerprint(roots)?;
    if cache.fingerprint != fingerprint {
        refresh_sources(roots, cache, fingerprint)?;
    }
    let Some(digest) = resolve_digest(cache, digest)? else {
        return Ok(HttpResponse::json(
            "404 Not Found",
            "{\"error\":\"unknown_asset_digest\"}".to_owned(),
        ));
    };
    let mut body = read_registered_entity(roots, cache, &digest);
    if matches!(&body, Err(error) if error.kind() == io::ErrorKind::NotFound)
        && expand_entity(cache, &digest)?
    {
        body = read_registered_entity(roots, cache, &digest);
    }
    let body = body?;
    Ok(if method == "HEAD" {
        HttpResponse::head("200 OK", "application/octet-stream", body.len() as u64)
    } else {
        HttpResponse::bytes("200 OK", "application/octet-stream", body)
    }
    .with_header("Cache-Control", "no-store")
    .with_header("X-Content-Type-Options", "nosniff"))
}

// //// 登记清单和归档来源并释放过期缓存 [@x380kkm 2026-09-27] ////
fn refresh_sources(
    roots: [&Path; 2],
    cache: &mut RecoveryCache,
    fingerprint: Vec<FileStamp>,
) -> io::Result<()> {
    let archive_sources = sources::archives(roots)?;
    cache.manifests = sources::manifests(roots);
    cache.archive_sources = archive_sources;
    cache.entities.clear();
    cache.paths.clear();
    cache.preferred_manifest = None;
    cache.fingerprint = fingerprint;
    cache.archives.clear();
    Ok(())
}

// //// 按请求的平台路径或摘要读取清单记录 [@x380kkm 2026-09-27] ////
fn resolve_digest(cache: &mut RecoveryCache, requested: &str) -> io::Result<Option<String>> {
    let path_request = requested.strip_prefix("by-path/");
    if let Some(path) = path_request {
        if let Some(digest) = cache.paths.get(path) {
            return Ok(Some(digest.clone()));
        }
    } else if cache.entities.contains_key(requested) {
        return Ok(Some(requested.to_owned()));
    }
    let selection = path_request.and_then(|path| path.split_once('/'));
    if path_request.is_some() && selection.is_none() {
        return Ok(None);
    }
    let mut order: Vec<_> = (0..cache.manifests.len()).rev().collect();
    if selection.is_none() {
        order.sort_by_key(|index| cache.preferred_manifest != Some(*index));
    }
    for index in order {
        let records = if let Some((manifest, path)) = selection {
            if cache.manifests[index].name != manifest {
                continue;
            }
            cache.manifests[index].find(Some(path), None)?
        } else {
            cache.manifests[index].find(None, Some(requested))?
        };
        let Some(record) = records.first() else {
            continue;
        };
        let digest = record.digest.clone();
        let entity = cache
            .entities
            .entry(digest.clone())
            .or_insert_with(|| Entity {
                records: Vec::new(),
                expanded: false,
            });
        for record in records {
            if !entity.records.contains(&record) {
                entity.records.push(record);
            }
        }
        if let Some(path) = path_request {
            cache.paths.insert(path.to_owned(), digest.clone());
        }
        cache.preferred_manifest = Some(index);
        return Ok(Some(digest));
    }
    Ok(None)
}

// //// 从其它平台清单补充相同内容的可用位置 [@x380kkm 2026-09-27] ////
fn expand_entity(cache: &mut RecoveryCache, digest: &str) -> io::Result<bool> {
    let entity = cache
        .entities
        .get_mut(digest)
        .expect("requested digest is registered");
    if entity.expanded {
        return Ok(false);
    }
    let count = entity.records.len();
    for manifest in &mut cache.manifests {
        for record in manifest.find(None, Some(digest))? {
            if !entity.records.contains(&record) {
                entity.records.push(record);
            }
        }
    }
    entity.expanded = true;
    Ok(entity.records.len() != count)
}

// //// 以清单和归档目录元数据识别资源索引变化 [@x380kkm 2026-09-27] ////
fn source_fingerprint(roots: [&Path; 2]) -> io::Result<Vec<FileStamp>> {
    let mut paths = Vec::new();
    for root in roots {
        paths.push(root.join("path"));
        for directory in ["entities", "EntityLists"] {
            paths.extend(
                ENTITY_MANIFESTS
                    .iter()
                    .map(|name| root.join(directory).join(name)),
            );
        }
        paths.extend(ALL_ARCHIVE_DIRECTORIES.iter().map(|name| root.join(name)));
    }
    paths.into_iter().map(file_stamp).collect()
}

// //// 读取文件或目录的存在状态与元数据 [@x380kkm 2026-09-27] ////
fn file_stamp(path: PathBuf) -> io::Result<FileStamp> {
    match fs::metadata(&path) {
        Ok(metadata) => Ok(FileStamp {
            path,
            size: metadata.len(),
            modified: metadata.modified().ok(),
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(FileStamp {
            path,
            size: 0,
            modified: None,
        }),
        Err(error) => Err(error),
    }
}

// //// 按当前清单读取实体并延迟关联归档成员 [@x380kkm 2026-09-27] ////
fn read_registered_entity(
    roots: [&Path; 2],
    cache: &mut RecoveryCache,
    digest: &str,
) -> io::Result<Vec<u8>> {
    let entity = cache
        .entities
        .get(digest)
        .expect("requested digest is registered");
    for root in roots.into_iter().rev() {
        for record in &entity.records {
            if let Some(path) = within_root(root, &root.join(&record.name)) {
                if fs::metadata(&path)?.len() == record.size {
                    let body = fs::read(path)?;
                    if matches_digest(&body, digest) {
                        return Ok(body);
                    }
                }
            }
        }
    }
    let mut order: Vec<_> = (0..cache.archive_sources.len()).collect();
    order.sort_by_cached_key(|index| cache.archive_sources[*index].rank(&entity.records));
    for index in order {
        let source = &mut cache.archive_sources[index];
        let Ok(path) = source.resolve(&mut cache.archives) else {
            continue;
        };
        for record in &entity.records {
            if !source.members.contains(&record.name) {
                continue;
            }
            if let Ok(body) = cache.archives.read(&path, &record.name, record.size) {
                if body.len() as u64 == record.size && matches_digest(&body, digest) {
                    return Ok(body);
                }
            }
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!(
            "registered resource is unavailable: {}",
            entity
                .records
                .iter()
                .map(|record| record.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    ))
}

fn within_root(root: &Path, path: &Path) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    let path = path.canonicalize().ok()?;
    (path.starts_with(root) && path.is_file()).then_some(path)
}

fn is_digest(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn matches_digest(body: &[u8], expected: &str) -> bool {
    let digest = Sha256::digest(body);
    URL_SAFE_NO_PAD.encode(digest) == expected
        || STANDARD_NO_PAD
            .encode(digest)
            .replace('+', "_")
            .replace('/', "-")
            == expected
}
