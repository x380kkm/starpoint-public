// audience: internal
// # personal-service-activity-catalog-manifest
//
// 该模块读取并验证 CN 资源提取器生成的活动 manifest. manifest 只包含活动元数据,
// 候选图片来源和资源相对键, 不包含客户端资源正文或本机绝对路径.
// 图片白名单缓存按 manifest 长度和修改时间失效.

use crate::database::is_valid_activity_id;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

const ACTIVITY_CATALOG_MANIFEST: &str = "activity-catalog.json";
const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ACTIVITY_COUNT: usize = 4096;
const MAX_IMAGE_KEY_CACHE_ENTRIES: usize = 16;
const MAX_IMAGE_CANDIDATE_COUNT: usize = 32;
const MAX_TAG_COUNT: usize = 32;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ActivityCatalogManifest {
    pub(super) format_version: u32,
    #[serde(default)]
    pub(super) region: Option<String>,
    #[serde(default)]
    pub(super) client_version: Option<String>,
    #[serde(default)]
    pub(super) asset_version: Option<String>,
    #[serde(default)]
    pub(super) generated_at: Option<String>,
    pub(super) activities: Vec<ActivityDefinition>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ActivityDefinition {
    pub(super) activity_id: String,
    pub(super) name: String,
    pub(super) kind: String,
    #[serde(default)]
    pub(super) tags: Vec<String>,
    #[serde(default)]
    pub(super) description: String,
    #[serde(default)]
    pub(super) banner_key: Option<String>,
    #[serde(default)]
    pub(super) banner_width: Option<u32>,
    #[serde(default)]
    pub(super) banner_height: Option<u32>,
    #[serde(default)]
    pub(super) image_candidates: Vec<ActivityImageCandidate>,
    #[serde(default)]
    pub(super) default_start_at_ms: Option<i64>,
    #[serde(default)]
    pub(super) default_end_at_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ActivityImageCandidate {
    pub(super) key: String,
    #[serde(default)]
    pub(super) width: Option<u32>,
    #[serde(default)]
    pub(super) height: Option<u32>,
    pub(super) source_type: String,
    #[serde(default)]
    pub(super) evidence: Option<String>,
}

pub(super) enum LoadedManifest {
    Missing,
    Present(ActivityCatalogManifest),
}

struct ImageKeyCacheEntry {
    identity: ManifestIdentity,
    keys: HashSet<String>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct ManifestIdentity {
    byte_length: u64,
    modified_at: SystemTime,
}

static IMAGE_KEY_CACHE: OnceLock<Mutex<HashMap<PathBuf, ImageKeyCacheEntry>>> = OnceLock::new();

// //// 缓存已验证的活动图片白名单 [@x380kkm 2026-08-20] ////
pub(super) fn contains_image_key(asset_root: &Path, image_key: &str) -> bool {
    let Ok(Some(manifest_path)) = validated_manifest_path(asset_root) else {
        return false;
    };
    let identity_before = manifest_identity(&manifest_path);
    if let Some(identity) = identity_before {
        let cache = image_key_cache()
            .lock()
            .unwrap_or_else(|lock| lock.into_inner());
        if let Some(entry) = cache.get(&manifest_path) {
            if entry.identity == identity {
                return entry.keys.contains(image_key);
            }
        }
    }

    let Ok(manifest) = load_from_path(&manifest_path) else {
        return false;
    };
    let keys = collect_image_keys(&manifest);
    let contains = keys.contains(image_key);
    let identity_after = manifest_identity(&manifest_path);
    if identity_before.is_some() && identity_before == identity_after {
        let mut cache = image_key_cache()
            .lock()
            .unwrap_or_else(|lock| lock.into_inner());
        if cache.len() >= MAX_IMAGE_KEY_CACHE_ENTRIES && !cache.contains_key(&manifest_path) {
            cache.clear();
        }
        cache.insert(
            manifest_path,
            ImageKeyCacheEntry {
                identity: identity_after.expect("matching manifest identity exists"),
                keys,
            },
        );
    }
    contains
}

fn image_key_cache() -> &'static Mutex<HashMap<PathBuf, ImageKeyCacheEntry>> {
    IMAGE_KEY_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn manifest_identity(manifest_path: &Path) -> Option<ManifestIdentity> {
    let metadata = manifest_path.metadata().ok()?;
    Some(ManifestIdentity {
        byte_length: metadata.len(),
        modified_at: metadata.modified().ok()?,
    })
}

fn collect_image_keys(manifest: &ActivityCatalogManifest) -> HashSet<String> {
    let mut keys = HashSet::new();
    for activity in &manifest.activities {
        if let Some(key) = &activity.banner_key {
            keys.insert(key.clone());
        }
        keys.extend(
            activity
                .image_candidates
                .iter()
                .map(|candidate| candidate.key.clone()),
        );
    }
    keys
}
// //// /缓存已验证的活动图片白名单 ////

// //// 读取并验证可重建活动 manifest [@x380kkm 2026-08-19] ////
pub(super) fn load(asset_root: &Path) -> Result<LoadedManifest, ()> {
    let Some(manifest_path) = validated_manifest_path(asset_root)? else {
        return Ok(LoadedManifest::Missing);
    };
    load_from_path(&manifest_path).map(LoadedManifest::Present)
}

fn validated_manifest_path(asset_root: &Path) -> Result<Option<PathBuf>, ()> {
    let manifest_path = asset_root.join(ACTIVITY_CATALOG_MANIFEST);
    if !manifest_path.exists() {
        return Ok(None);
    }
    let canonical_root = asset_root.canonicalize().map_err(|_| ())?;
    let canonical_path = manifest_path.canonicalize().map_err(|_| ())?;
    if !canonical_path.starts_with(&canonical_root) || !canonical_path.is_file() {
        return Err(());
    }
    Ok(Some(canonical_path))
}

fn load_from_path(manifest_path: &Path) -> Result<ActivityCatalogManifest, ()> {
    let metadata = manifest_path.metadata().map_err(|_| ())?;
    if metadata.len() > MAX_MANIFEST_BYTES {
        return Err(());
    }
    let body = fs::read(manifest_path).map_err(|_| ())?;
    let manifest = serde_json::from_slice::<ActivityCatalogManifest>(&body).map_err(|_| ())?;
    validate(&manifest)?;
    Ok(manifest)
}

fn validate(manifest: &ActivityCatalogManifest) -> Result<(), ()> {
    if manifest.format_version != 1
        || manifest.activities.len() > MAX_ACTIVITY_COUNT
        || !valid_optional_text(manifest.region.as_deref(), 64)
        || !valid_optional_text(manifest.client_version.as_deref(), 64)
        || !valid_optional_text(manifest.asset_version.as_deref(), 64)
        || !valid_optional_text(manifest.generated_at.as_deref(), 64)
    {
        return Err(());
    }
    let mut activity_ids = HashSet::with_capacity(manifest.activities.len());
    for activity in &manifest.activities {
        if !is_valid_activity_id(&activity.activity_id)
            || !activity_ids.insert(activity.activity_id.as_str())
            || !valid_required_text(&activity.name, 256)
            || !valid_required_text(&activity.kind, 64)
            || !valid_text(&activity.description, 4096)
            || activity.tags.len() > MAX_TAG_COUNT
            || activity
                .tags
                .iter()
                .any(|tag| !valid_required_text(tag, 64))
            || !valid_default_window(activity.default_start_at_ms, activity.default_end_at_ms)
            || activity
                .banner_key
                .as_deref()
                .is_some_and(|key| super::banner::validate_key(key).is_none())
            || !valid_banner_dimensions(activity.banner_width, activity.banner_height)
            || activity.image_candidates.len() > MAX_IMAGE_CANDIDATE_COUNT
            || activity
                .image_candidates
                .iter()
                .any(|candidate| !valid_image_candidate(candidate))
        {
            return Err(());
        }
    }
    Ok(())
}

fn valid_image_candidate(candidate: &ActivityImageCandidate) -> bool {
    super::banner::validate_key(&candidate.key).is_some()
        && valid_banner_dimensions(candidate.width, candidate.height)
        && valid_required_text(&candidate.source_type, 64)
        && valid_optional_text(candidate.evidence.as_deref(), 512)
}

fn valid_banner_dimensions(width: Option<u32>, height: Option<u32>) -> bool {
    match (width, height) {
        (None, None) => true,
        (Some(width), Some(height)) => (1..=8192).contains(&width) && (1..=4096).contains(&height),
        _ => false,
    }
}

fn valid_default_window(start_at_ms: Option<i64>, end_at_ms: Option<i64>) -> bool {
    match (start_at_ms, end_at_ms) {
        (None, None) => true,
        (Some(start), Some(end)) => start >= 0 && end > start,
        _ => false,
    }
}

fn valid_optional_text(value: Option<&str>, max_length: usize) -> bool {
    value.map_or(true, |value| valid_text(value, max_length))
}

fn valid_required_text(value: &str, max_length: usize) -> bool {
    !value.trim().is_empty() && valid_text(value, max_length)
}

fn valid_text(value: &str, max_length: usize) -> bool {
    value.len() <= max_length && !value.chars().any(char::is_control)
}
// //// /读取并验证可重建活动 manifest ////
