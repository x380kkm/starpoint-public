// audience: internal
// # personal-service-cn-asset
//
// 该模块实现 CN 客户端资产版本和路径接口. 响应只引用个人服务根目录下的本地 CDN.
// 标题版本检查引用当前平台的实体清单, 归档内容由 get_path 按设备平台严格列举.
// 可写覆盖层中的语音和资源差分归档形成下一资源版本.

use crate::cn::{decode_request, deserialize_optional_i64, msgpack_response_at};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const ASSET_PREFIX: &str = "/api/index.php/asset/";
const ASSET_IN_TITLE_VERSION_PATH: &str = "/api/index.php/assetintitle/version_info_in_title";
const PATH_MANIFEST_NAME: &str = "path";
pub(crate) const EMPTY_ENTITY_LIST_NAME: &str = "empty.csv";
const ANDROID_TITLE_ENTITY_LIST_NAME: &str = "10939-android_medium.csv";
const IOS_TITLE_ENTITY_LIST_NAME: &str = "10939-ios_medium.csv";
const FULL_ASSET_VERSION: &str = "1.4.0";
const DEFAULT_ASSET_VERSION: &str = "1.4.54";
const MAX_ARCHIVE_DIGEST_CACHE_ENTRIES: usize = 2_048;
const OVERRIDE_ARCHIVE_PREFIX: &str = "starpoint-cn-";
const IOS_VOICE_OVERRIDE_ARCHIVE_PREFIX: &str = "starpoint-ios-voice-overlay-";
const OVERRIDE_ARCHIVE_MARKER: &str = "-overlay";
const FULL_ARCHIVE_DIRECTORIES: [&str; 3] = [
    "archive-common-full",
    "archive-medium-full",
    "archive-android-full",
];
const DIFF_ARCHIVE_DIRECTORIES: [&str; 3] = [
    "archive-common-diff",
    "archive-medium-diff",
    "archive-android-diff",
];
const IOS_FULL_ARCHIVE_DIRECTORIES: [&str; 3] = [
    "archive-common-full",
    "archive-medium-full",
    "archive-ios-full",
];
const IOS_DIFF_ARCHIVE_DIRECTORIES: [&str; 3] = [
    "archive-common-diff",
    "archive-medium-diff",
    "archive-ios-diff",
];
const ALL_ARCHIVE_DIRECTORIES: [&str; 8] = [
    "archive-common-full",
    "archive-medium-full",
    "archive-android-full",
    "archive-common-diff",
    "archive-medium-diff",
    "archive-android-diff",
    "archive-ios-full",
    "archive-ios-diff",
];

#[derive(Serialize)]
struct VersionInfoData {
    base_url: String,
    files_list: String,
    total_size: u64,
    delayed_assets_size: u64,
}

#[derive(Serialize)]
struct TitleVersionInfoData {
    base_url: String,
    files_list: String,
    total_size: f64,
    delayed_assets_size: u64,
}

#[derive(Deserialize, Serialize)]
struct AssetPathData {
    info: AssetPathInfo,
    full: AssetArchiveGroup,
    diff: Vec<AssetDiffGroup>,
    asset_version_hash: String,
}

#[derive(Deserialize, Serialize)]
struct AssetPathInfo {
    client_asset_version: String,
    target_asset_version: String,
    eventual_target_asset_version: String,
    is_initial: bool,
    latest_maj_first_version: String,
}

#[derive(Deserialize, Serialize)]
struct AssetArchiveGroup {
    version: String,
    archive: Vec<AssetArchive>,
}

#[derive(Deserialize, Serialize)]
struct AssetDiffGroup {
    version: String,
    original_version: String,
    archive: Vec<AssetArchive>,
}

#[derive(Clone, Deserialize, Serialize)]
struct AssetArchive {
    location: String,
    size: u64,
    sha256: String,
}

#[derive(Deserialize)]
struct AssetPathRequest {
    #[serde(default, deserialize_with = "deserialize_optional_i64")]
    viewer_id: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClientPlatform {
    Android,
    Ios,
}

// //// 缓存可写覆盖归档的内容摘要 [@x380kkm 2026-08-23] ////
#[derive(Default)]
pub(crate) struct ArchiveDigestCache {
    entries: HashMap<PathBuf, ArchiveDigestCacheEntry>,
}

struct ArchiveDigestCacheEntry {
    identity: ArchiveIdentity,
    sha256: String,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct ArchiveIdentity {
    byte_length: u64,
    modified_at: SystemTime,
}

impl ArchiveDigestCache {
    fn sha256(&mut self, path: &Path, metadata: &fs::Metadata) -> Option<String> {
        let identity_before = archive_identity(metadata);
        if let Some(identity) = identity_before {
            if let Some(entry) = self.entries.get(path) {
                if entry.identity == identity {
                    return Some(entry.sha256.clone());
                }
            }
        }

        let sha256 = archive_sha256(path)?;
        let identity_after = fs::metadata(path).ok().as_ref().and_then(archive_identity);
        if identity_before.is_some() && identity_before == identity_after {
            if self.entries.len() >= MAX_ARCHIVE_DIGEST_CACHE_ENTRIES
                && !self.entries.contains_key(path)
            {
                self.entries.clear();
            }
            self.entries.insert(
                path.to_path_buf(),
                ArchiveDigestCacheEntry {
                    identity: identity_after.expect("matching archive identity exists"),
                    sha256: sha256.clone(),
                },
            );
        }
        Some(sha256)
    }
}

fn archive_identity(metadata: &fs::Metadata) -> Option<ArchiveIdentity> {
    Some(ArchiveIdentity {
        byte_length: metadata.len(),
        modified_at: metadata.modified().ok()?,
    })
}
// //// /缓存可写覆盖归档的内容摘要 ////

// //// 分派 CN 资产路径接口 [@x380kkm 2026-08-10] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &ServiceDatabase,
    asset_root: &Path,
    override_root: &Path,
    digest_cache: &mut ArchiveDigestCache,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let operation = if request.path() == ASSET_IN_TITLE_VERSION_PATH {
        "version_info_in_title"
    } else {
        request.path().strip_prefix(ASSET_PREFIX)?
    };
    if request.method() != "POST" {
        return Some(Ok(method_not_allowed()));
    }
    if let Err(error) = crate::activity_projection::sync(database, asset_root, override_root) {
        return Some(Err(error));
    }
    let base_url = asset_base_url(request);
    let response = match operation {
        "version_info" => version_info(request, database, asset_root, override_root, &base_url),
        "version_info_in_title" => {
            version_info_in_title(request, database, asset_root, override_root, &base_url)
        }
        "get_path" => get_path(
            request,
            database,
            asset_root,
            override_root,
            digest_cache,
            &base_url,
        ),
        _ => Ok(not_found()),
    };
    Some(response)
}
// //// /分派 CN 资产路径接口 ////

// //// 返回 CN 游戏内资产版本信息 [@x380kkm 2026-08-23] ////
fn version_info(
    request: &HttpRequest,
    database: &ServiceDatabase,
    asset_root: &Path,
    override_root: &Path,
    base_url: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let data = version_info_data(
        asset_root,
        override_root,
        base_url,
        request_platform(request),
    );
    msgpack_response(database, 0, false, data)
}
// //// /返回 CN 游戏内资产版本信息 ////

// //// 返回 CN 标题页资产版本信息 [@x380kkm 2026-08-23] ////
fn version_info_in_title(
    request: &HttpRequest,
    database: &ServiceDatabase,
    asset_root: &Path,
    override_root: &Path,
    base_url: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let platform = request_platform(request);
    let mut data = version_info_data(asset_root, override_root, base_url, platform);
    data.files_list = format!(
        "{base_url}/{}/{}",
        entity_lists_directory(asset_root),
        title_entity_list_name(platform)
    );
    msgpack_response(
        database,
        0,
        false,
        TitleVersionInfoData {
            base_url: data.base_url,
            files_list: data.files_list,
            total_size: data.total_size as f64,
            delayed_assets_size: data.delayed_assets_size,
        },
    )
}
// //// /返回 CN 标题页资产版本信息 ////

fn version_info_data(
    asset_root: &Path,
    override_root: &Path,
    base_url: &str,
    platform: ClientPlatform,
) -> VersionInfoData {
    let entity_directory = entity_lists_directory(asset_root);
    VersionInfoData {
        base_url: format!("{base_url}/{entity_directory}/"),
        files_list: format!("{base_url}/{entity_directory}/{EMPTY_ENTITY_LIST_NAME}"),
        total_size: downloadable_asset_size(asset_root, override_root, platform),
        delayed_assets_size: 0,
    }
}

// //// 返回 CN 资产下载路径 [@x380kkm 2026-08-10] ////
fn get_path(
    request: &HttpRequest,
    database: &ServiceDatabase,
    asset_root: &Path,
    override_root: &Path,
    digest_cache: &mut ArchiveDigestCache,
    base_url: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let request_data = if request
        .header("content-type")
        .is_some_and(|value| value.starts_with("application/json"))
    {
        match serde_json::from_slice::<AssetPathRequest>(request.body()) {
            Ok(data) => data,
            Err(_) => {
                return Ok(HttpResponse::json(
                    "400 Bad Request",
                    "{\"error\":\"invalid_request_body\"}".to_owned(),
                ))
            }
        }
    } else {
        match decode_request::<AssetPathRequest>(request) {
            Ok(data) => data,
            Err(response) => return Ok(response),
        }
    };
    let platform = request_platform(request);
    let client_asset_version = request.header("res_ver").unwrap_or_default().to_owned();
    let data = localized_path_data(
        asset_root,
        override_root,
        base_url,
        &client_asset_version,
        platform,
        digest_cache,
    )?;
    msgpack_response(database, request_data.viewer_id.unwrap_or(0), true, data)
}
// //// /返回 CN 资产下载路径 ////

fn localized_path_data(
    asset_root: &Path,
    override_root: &Path,
    base_url: &str,
    client_asset_version: &str,
    platform: ClientPlatform,
    digest_cache: &mut ArchiveDigestCache,
) -> Result<AssetPathData, PersonalServiceError> {
    let mut data = match load_path_manifest(asset_root, override_root)? {
        Some(data) => data,
        None => build_path_data(
            asset_root,
            base_url,
            client_asset_version.to_owned(),
            platform,
        ),
    };
    retain_platform_archives(&mut data, platform);
    data.info.client_asset_version = client_asset_version.to_owned();
    data.full.archive = localize_archives(
        asset_root,
        override_root,
        base_url,
        data.full.archive,
        digest_cache,
    )?;
    for diff in &mut data.diff {
        diff.archive = localize_archives(
            asset_root,
            override_root,
            base_url,
            std::mem::take(&mut diff.archive),
            digest_cache,
        )?;
    }
    let override_archives =
        list_override_diff_archives(override_root, base_url, platform, digest_cache);
    merge_override_diff_archives(&mut data, override_archives);
    data.diff.retain(|diff| !diff.archive.is_empty());
    Ok(data)
}

// //// 解析当前平台可用的 CN 资源版本 [@x380kkm 2026-08-29] ////
pub(crate) fn available_asset_version(
    database: &ServiceDatabase,
    asset_root: &Path,
    override_root: &Path,
    requested_asset_version: Option<&str>,
    platform: ClientPlatform,
    digest_cache: &mut ArchiveDigestCache,
) -> Result<String, PersonalServiceError> {
    crate::activity_projection::sync(database, asset_root, override_root)?;
    let fallback_version = requested_asset_version
        .map(str::to_owned)
        .or_else(|| std::env::var("CN_RES_VERSION").ok())
        .unwrap_or_else(|| DEFAULT_ASSET_VERSION.to_owned());
    let manifest = load_path_manifest(asset_root, override_root)?;
    let mut data = match manifest {
        Some(data) => data,
        None => {
            let mut data = build_path_data(asset_root, "", fallback_version.clone(), platform);
            data.info.target_asset_version = fallback_version.clone();
            data.info.eventual_target_asset_version = fallback_version.clone();
            data
        }
    };
    retain_platform_archives(&mut data, platform);
    let override_archives = list_override_diff_archives(override_root, "", platform, digest_cache);
    merge_override_diff_archives(&mut data, override_archives);
    if !data.info.eventual_target_asset_version.is_empty() {
        return Ok(data.info.eventual_target_asset_version);
    }
    Ok(fallback_version)
}

fn build_path_data(
    asset_root: &Path,
    base_url: &str,
    client_asset_version: String,
    platform: ClientPlatform,
) -> AssetPathData {
    let full = full_archive_directories(platform)
        .iter()
        .flat_map(|directory| list_archives(asset_root, directory, base_url))
        .collect::<Vec<_>>();
    let diff = list_diff_archives(asset_root, base_url, platform);
    let target_asset_version = diff
        .last()
        .map(|value| value.version.clone())
        .or_else(|| std::env::var("CN_RES_VERSION").ok())
        .unwrap_or_else(|| DEFAULT_ASSET_VERSION.to_owned());
    AssetPathData {
        info: AssetPathInfo {
            client_asset_version,
            target_asset_version: target_asset_version.clone(),
            eventual_target_asset_version: target_asset_version,
            is_initial: true,
            latest_maj_first_version: FULL_ASSET_VERSION.to_owned(),
        },
        full: AssetArchiveGroup {
            version: FULL_ASSET_VERSION.to_owned(),
            archive: full,
        },
        diff,
        asset_version_hash: String::new(),
    }
}

// //// 读取并本地化 CN 资产路径清单 [@x380kkm 2026-08-10] ////
fn load_path_manifest(
    asset_root: &Path,
    override_root: &Path,
) -> Result<Option<AssetPathData>, PersonalServiceError> {
    let path = if override_root.join(PATH_MANIFEST_NAME).is_file() {
        override_root.join(PATH_MANIFEST_NAME)
    } else {
        asset_root.join(PATH_MANIFEST_NAME)
    };
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(PersonalServiceError::new(format!(
                "failed to read CN asset path manifest: {error}"
            )))
        }
    };
    let data = serde_json::from_slice::<AssetPathData>(&bytes).map_err(|error| {
        PersonalServiceError::new(format!("failed to decode CN asset path manifest: {error}"))
    })?;
    Ok(Some(data))
}
// //// /读取并本地化 CN 资产路径清单 ////

fn localize_archives(
    asset_root: &Path,
    override_root: &Path,
    base_url: &str,
    archives: Vec<AssetArchive>,
    digest_cache: &mut ArchiveDigestCache,
) -> Result<Vec<AssetArchive>, PersonalServiceError> {
    let mut localized = Vec::with_capacity(archives.len());
    for mut archive in archives {
        localize_archive(
            asset_root,
            override_root,
            base_url,
            &mut archive,
            digest_cache,
        )?;
        localized.push(archive);
    }
    Ok(localized)
}

fn localize_archive(
    asset_root: &Path,
    override_root: &Path,
    base_url: &str,
    archive: &mut AssetArchive,
    digest_cache: &mut ArchiveDigestCache,
) -> Result<(), PersonalServiceError> {
    let (directory, file_name) = archive_location_parts(&archive.location).ok_or_else(|| {
        PersonalServiceError::new("CN asset path manifest contains an invalid archive path")
    })?;
    let is_known_directory = ALL_ARCHIVE_DIRECTORIES.contains(&directory.as_str());
    let is_safe_file_name = is_safe_archive_file_name(&file_name);
    let digest = STANDARD.decode(&archive.sha256).map_err(|_| {
        PersonalServiceError::new("CN asset path manifest contains an invalid SHA-256")
    })?;
    if !is_known_directory || !is_safe_file_name || digest.len() != 32 {
        return Err(PersonalServiceError::new(
            "CN asset path manifest contains invalid archive metadata",
        ));
    }
    if let Some(override_path) = existing_archive_path(override_root, &directory, &file_name) {
        let metadata = fs::metadata(&override_path).map_err(|error| {
            PersonalServiceError::new(format!(
                "failed to read CN override archive metadata: {error}"
            ))
        })?;
        archive.size = metadata.len();
        archive.sha256 = digest_cache
            .sha256(&override_path, &metadata)
            .ok_or_else(|| {
                PersonalServiceError::new("failed to calculate CN override archive SHA-256")
            })?;
    } else {
        let local_path = asset_root.join(&directory).join(&file_name);
        let metadata = fs::metadata(&local_path).map_err(|_| {
            PersonalServiceError::new("CN asset path manifest references a missing local archive")
        })?;
        if !metadata.is_file() || metadata.len() != archive.size {
            return Err(PersonalServiceError::new(
                "CN asset path manifest contains invalid local archive metadata",
            ));
        }
    }
    archive.location = format!("{base_url}/{directory}/{file_name}");
    Ok(())
}

// //// 解析覆盖根内可下载的归档路径 [@x380kkm 2026-08-22] ////
fn existing_archive_path(root: &Path, directory: &str, file_name: &str) -> Option<PathBuf> {
    let canonical_root = root.canonicalize().ok()?;
    let canonical_path = root.join(directory).join(file_name).canonicalize().ok()?;
    (canonical_path.starts_with(canonical_root) && canonical_path.is_file())
        .then_some(canonical_path)
}
// //// /解析覆盖根内可下载的归档路径 ////

fn archive_location_parts(location: &str) -> Option<(String, String)> {
    let normalized = location.replace('\\', "/");
    let mut components = normalized.rsplit('/');
    let file_name = components.next()?.to_owned();
    let directory = components.next()?.to_owned();
    (!file_name.is_empty() && !directory.is_empty()).then_some((directory, file_name))
}

// //// 验证归档文件名 [@x380kkm 2026-08-29] ////
fn is_safe_archive_file_name(file_name: &str) -> bool {
    !file_name.is_empty()
        && file_name.ends_with(".zip")
        && file_name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_.".contains(character))
}
// //// /验证归档文件名 ////

// //// 构造客户端可访问的 CDN 基地址 [@x380kkm 2026-08-10] ////
fn asset_base_url(request: &HttpRequest) -> String {
    if let Ok(configured) = std::env::var("CN_CDN_BASE_URL") {
        if !configured.is_empty() {
            return configured.trim_end_matches('/').to_owned();
        }
    }
    let host = request
        .header("host")
        .filter(|value| is_safe_host(value))
        .unwrap_or("127.0.0.1");
    format!("http://{host}/patch/cn")
}
// //// /构造客户端可访问的 CDN 基地址 ////

fn is_safe_host(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || ".:-[]".contains(character))
}

// //// 生成 CN 资产 MessagePack 响应 [@x380kkm 2026-08-10] ////
fn msgpack_response<T: Serialize>(
    database: &ServiceDatabase,
    viewer_id: i64,
    asset_update: bool,
    data: T,
) -> Result<HttpResponse, PersonalServiceError> {
    msgpack_response_at(
        viewer_id,
        asset_update,
        crate::cn::server_time(database)?,
        data,
    )
}
// //// /生成 CN 资产 MessagePack 响应 ////

fn entity_lists_directory(asset_root: &Path) -> &'static str {
    if asset_root.join("EntityLists").is_dir() {
        "EntityLists"
    } else if asset_root.join("entities").is_dir() {
        "entities"
    } else {
        "EntityLists"
    }
}

// //// 汇总客户端实际下载的归档大小 [@x380kkm 2026-08-24] ////
fn downloadable_asset_size(
    asset_root: &Path,
    override_root: &Path,
    platform: ClientPlatform,
) -> u64 {
    let mut sizes = BTreeMap::<String, u64>::new();
    for directory in full_archive_directories(platform)
        .iter()
        .chain(diff_archive_directories(platform).iter())
    {
        for entry in read_archive_entries(asset_root, directory) {
            let Some((key, size)) = archive_entry_size(asset_root, directory, &entry) else {
                continue;
            };
            sizes.insert(key, size);
        }
        for entry in read_archive_entries(override_root, directory) {
            let Some((key, size)) = archive_entry_size(override_root, directory, &entry) else {
                continue;
            };
            if sizes.contains_key(&key)
                || (diff_archive_directories(platform).contains(directory)
                    && is_additive_override_archive_key(&key))
            {
                sizes.insert(key, size);
            }
        }
    }
    sizes.values().copied().fold(0_u64, u64::saturating_add)
}
// //// /汇总客户端实际下载的归档大小 ////

fn list_archives(asset_root: &Path, directory: &str, base_url: &str) -> Vec<AssetArchive> {
    let mut archives = read_archive_entries(asset_root, directory)
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() || entry.path().extension()?.to_str()? != "zip" {
                return None;
            }
            let name = entry.file_name().to_str()?.to_owned();
            let sha256 = archive_sha256(&entry.path())?;
            Some(AssetArchive {
                location: format!("{base_url}/{directory}/{name}"),
                size: metadata.len(),
                sha256,
            })
        })
        .collect::<Vec<_>>();
    archives.sort_by(|left, right| left.location.cmp(&right.location));
    archives
}

fn read_archive_entries(asset_root: &Path, directory: &str) -> impl Iterator<Item = fs::DirEntry> {
    fs::read_dir(asset_root.join(directory))
        .into_iter()
        .flatten()
        .flatten()
}

// //// 读取归档条目的稳定大小键 [@x380kkm 2026-08-29] ////
fn archive_entry_size(root: &Path, directory: &str, entry: &fs::DirEntry) -> Option<(String, u64)> {
    let metadata = entry.metadata().ok()?;
    if !metadata.is_file() {
        return None;
    }
    let file_name = entry.file_name().to_str()?.to_owned();
    if !is_safe_archive_file_name(&file_name) {
        return None;
    }
    let path = existing_archive_path(root, directory, &file_name)?;
    let metadata = fs::metadata(path).ok()?;
    Some((format!("{directory}/{file_name}"), metadata.len()))
}
// //// /读取归档条目的稳定大小键 ////

fn list_diff_archives(
    asset_root: &Path,
    base_url: &str,
    platform: ClientPlatform,
) -> Vec<AssetDiffGroup> {
    let mut groups: BTreeMap<String, (String, Vec<AssetArchive>)> = BTreeMap::new();
    for directory in diff_archive_directories(platform) {
        for entry in read_archive_entries(asset_root, directory) {
            let Some(metadata) = entry.metadata().ok() else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some((original_version, target_version)) = diff_versions(&name) else {
                continue;
            };
            let Some(sha256) = archive_sha256(&entry.path()) else {
                continue;
            };
            groups
                .entry(target_version.clone())
                .or_insert_with(|| (original_version, Vec::new()))
                .1
                .push(AssetArchive {
                    location: format!("{base_url}/{directory}/{name}"),
                    size: metadata.len(),
                    sha256,
                });
        }
    }
    let mut result = groups
        .into_iter()
        .map(|(version, (original_version, mut archive))| {
            archive.sort_by(|left, right| left.location.cmp(&right.location));
            AssetDiffGroup {
                version,
                original_version,
                archive,
            }
        })
        .collect::<Vec<_>>();
    result.sort_by(|left, right| {
        parse_version(&left.version)
            .unwrap_or([u64::MAX; 3])
            .cmp(&parse_version(&right.version).unwrap_or([u64::MAX; 3]))
    });
    result
}

// //// 枚举平台隔离的可写差分归档 [@x380kkm 2026-08-29] ////
fn list_override_diff_archives(
    override_root: &Path,
    base_url: &str,
    platform: ClientPlatform,
    digest_cache: &mut ArchiveDigestCache,
) -> Vec<AssetArchive> {
    let mut archives = diff_archive_directories(platform)
        .iter()
        .flat_map(|directory| {
            read_archive_entries(override_root, directory).map(move |entry| (*directory, entry))
        })
        .filter_map(|(directory, entry)| {
            let name = entry.file_name().to_str()?.to_owned();
            if !is_additive_override_archive_name(&name) {
                return None;
            }
            let path = existing_archive_path(override_root, directory, &name)?;
            let metadata = fs::metadata(&path).ok()?;
            let sha256 = digest_cache.sha256(&path, &metadata)?;
            Some(AssetArchive {
                location: format!("{base_url}/{directory}/{name}"),
                size: metadata.len(),
                sha256,
            })
        })
        .collect::<Vec<_>>();
    archives.sort_by(|left, right| left.location.cmp(&right.location));
    archives.dedup_by(|left, right| left.location == right.location);
    archives
}
// //// /枚举平台隔离的可写差分归档 ////

// //// 合并可写差分归档并生成下一资源版本 [@x380kkm 2026-08-29] ////
fn merge_override_diff_archives(data: &mut AssetPathData, override_archives: Vec<AssetArchive>) {
    let mut additions = Vec::new();
    for archive in override_archives {
        if !replace_archive_by_key(&mut data.diff, &archive) {
            additions.push(archive);
        }
    }
    if additions.is_empty() {
        return;
    }

    let current_version = highest_diff_group_index(&data.diff)
        .map(|index| data.diff[index].version.clone())
        .or_else(|| valid_asset_version(&data.info.target_asset_version).map(str::to_owned))
        .unwrap_or_else(|| DEFAULT_ASSET_VERSION.to_owned());
    let Some(next_version) = next_asset_version(&current_version) else {
        return;
    };
    additions.sort_by(|left, right| left.location.cmp(&right.location));

    if let Some(group) = data
        .diff
        .iter_mut()
        .find(|group| group.version == next_version && group.original_version == current_version)
    {
        group.archive.extend(additions);
        group
            .archive
            .sort_by(|left, right| left.location.cmp(&right.location));
    } else {
        data.diff.push(AssetDiffGroup {
            version: next_version.clone(),
            original_version: current_version,
            archive: additions,
        });
    }

    data.info.target_asset_version = next_version.clone();
    data.info.eventual_target_asset_version = next_version;
}
// //// /合并可写差分归档并生成下一资源版本 ////

// //// 按归档路径替换已有差分记录 [@x380kkm 2026-08-29] ////
fn replace_archive_by_key(groups: &mut [AssetDiffGroup], replacement: &AssetArchive) -> bool {
    let Some(replacement_key) = archive_key(replacement) else {
        return false;
    };
    let mut replaced = false;
    for group in groups {
        let mut index = 0;
        while index < group.archive.len() {
            if archive_key(&group.archive[index]).as_ref() != Some(&replacement_key) {
                index += 1;
                continue;
            }
            if replaced {
                group.archive.remove(index);
                continue;
            }
            group.archive[index] = replacement.clone();
            replaced = true;
            index += 1;
        }
        if replaced {
            group
                .archive
                .sort_by(|left, right| left.location.cmp(&right.location));
        }
    }
    replaced
}
// //// /按归档路径替换已有差分记录 ////

// //// 选择最高差分版本组 [@x380kkm 2026-08-29] ////
fn highest_diff_group_index(groups: &[AssetDiffGroup]) -> Option<usize> {
    groups
        .iter()
        .enumerate()
        .filter_map(|(index, group)| parse_version(&group.version).map(|version| (version, index)))
        .max_by(|(left_version, left_index), (right_version, right_index)| {
            left_version
                .cmp(right_version)
                .then_with(|| left_index.cmp(right_index))
        })
        .map(|(_, index)| index)
}
// //// /选择最高差分版本组 ////

fn next_asset_version(version: &str) -> Option<String> {
    let [major, minor, patch] = parse_version(version)?;
    Some(format!("{major}.{minor}.{}", patch.checked_add(1)?))
}

fn archive_key(archive: &AssetArchive) -> Option<String> {
    archive_location_parts(&archive.location)
        .map(|(directory, file_name)| format!("{directory}/{file_name}"))
}

fn is_additive_override_archive_key(key: &str) -> bool {
    key.rsplit('/')
        .next()
        .is_some_and(is_additive_override_archive_name)
}

// //// 识别受约束的可写覆盖归档名 [@x380kkm 2026-08-29] ////
fn is_additive_override_archive_name(file_name: &str) -> bool {
    is_safe_archive_file_name(file_name)
        && (file_name.starts_with(OVERRIDE_ARCHIVE_PREFIX)
            || file_name.starts_with(IOS_VOICE_OVERRIDE_ARCHIVE_PREFIX))
        && file_name
            .strip_suffix(".zip")
            .is_some_and(|stem| stem.contains(OVERRIDE_ARCHIVE_MARKER))
}
// //// /识别受约束的可写覆盖归档名 ////

fn valid_asset_version(version: &str) -> Option<&str> {
    (!version.is_empty() && parse_version(version).is_some()).then_some(version)
}

// //// 识别真实 CN 客户端的资源平台 [@x380kkm 2026-08-18] ////
pub(crate) fn request_platform(request: &HttpRequest) -> ClientPlatform {
    match request.header("device").map(str::trim) {
        Some("1") => ClientPlatform::Ios,
        Some("2") => ClientPlatform::Android,
        _ => ClientPlatform::Android,
    }
}
// //// /识别真实 CN 客户端的资源平台 ////

// //// 选择标题页实体清单 [@x380kkm 2026-08-24] ////
fn title_entity_list_name(platform: ClientPlatform) -> &'static str {
    match platform {
        ClientPlatform::Android => ANDROID_TITLE_ENTITY_LIST_NAME,
        ClientPlatform::Ios => IOS_TITLE_ENTITY_LIST_NAME,
    }
}
// //// /选择标题页实体清单 ////

// //// 筛选当前平台的清单归档 [@x380kkm 2026-08-24] ////
fn retain_platform_archives(data: &mut AssetPathData, platform: ClientPlatform) {
    data.full
        .archive
        .retain(|archive| archive_matches_directories(archive, full_archive_directories(platform)));
    for diff in &mut data.diff {
        diff.archive.retain(|archive| {
            archive_matches_directories(archive, diff_archive_directories(platform))
        });
    }
}

fn archive_matches_directories(archive: &AssetArchive, directories: &[&str]) -> bool {
    let Some((directory, _)) = archive_location_parts(&archive.location) else {
        return true;
    };
    !ALL_ARCHIVE_DIRECTORIES.contains(&directory.as_str())
        || directories.contains(&directory.as_str())
}
// //// /筛选当前平台的清单归档 ////

fn full_archive_directories(platform: ClientPlatform) -> &'static [&'static str] {
    match platform {
        ClientPlatform::Android => &FULL_ARCHIVE_DIRECTORIES,
        ClientPlatform::Ios => &IOS_FULL_ARCHIVE_DIRECTORIES,
    }
}

fn diff_archive_directories(platform: ClientPlatform) -> &'static [&'static str] {
    match platform {
        ClientPlatform::Android => &DIFF_ARCHIVE_DIRECTORIES,
        ClientPlatform::Ios => &IOS_DIFF_ARCHIVE_DIRECTORIES,
    }
}

fn archive_sha256(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Some(STANDARD.encode(digest.finalize()))
}

fn diff_versions(name: &str) -> Option<(String, String)> {
    let stem = name.strip_prefix("pinball-")?.strip_suffix(".zip")?;
    let mut versions = stem.split('-');
    let original = versions.next()?.to_owned();
    let target = versions.next()?.to_owned();
    if parse_version(&original).is_none() || parse_version(&target).is_none() {
        return None;
    }
    Some((original, target))
}

fn parse_version(version: &str) -> Option<[u64; 3]> {
    let parts = version
        .split('.')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    (parts.len() == 3).then(|| [parts[0], parts[1], parts[2]])
}

fn not_found() -> HttpResponse {
    HttpResponse::json("404 Not Found", "{\"error\":\"not_found\"}".to_owned())
}

fn method_not_allowed() -> HttpResponse {
    HttpResponse::json(
        "405 Method Not Allowed",
        "{\"error\":\"method_not_allowed\"}".to_owned(),
    )
    .with_header("Allow", "POST")
}
