// audience: internal
// # activity-projection-bundle-upgrade
//
// 包内 path 的差分归档描述资源更新. 可写版本链承接新包差分后再应用活动投影.
// 资源版本递增同时考虑已有可写版本和包内版本, 归档内容使用包内声明的大小和摘要.

use super::projection_files::atomic_write;
use super::{increment_version, parse_version};
use crate::PersonalServiceError;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

pub(super) struct BundleManifest {
    pub(super) value: Value,
    pub(super) signature: String,
    root: PathBuf,
}

pub(super) struct UpgradedPath {
    pub(super) manifest: Value,
    pub(super) repair_version: Option<String>,
}

// //// 读取包内资源清单身份 [@x380kkm 2026-09-20] ////
pub(super) fn read_bundle_manifest(
    asset_root: &Path,
) -> Result<BundleManifest, PersonalServiceError> {
    let bytes = fs::read(asset_root.join("path")).map_err(|error| {
        PersonalServiceError::new(format!("failed to read CN bundle path manifest: {error}"))
    })?;
    let value = serde_json::from_slice(&bytes).map_err(|error| {
        PersonalServiceError::new(format!("failed to decode CN bundle path manifest: {error}"))
    })?;
    Ok(BundleManifest {
        value,
        signature: URL_SAFE_NO_PAD.encode(Sha256::digest(&bytes)),
        root: asset_root.to_path_buf(),
    })
}
// //// /读取包内资源清单身份 ////

// //// 提取归档的本地路径标识 [@x380kkm 2026-09-20] ////
fn archive_key(archive: &Value) -> Result<String, PersonalServiceError> {
    let location = archive["location"]
        .as_str()
        .ok_or_else(|| PersonalServiceError::new("CN bundle archive location is invalid"))?;
    let mut parts = location.rsplit('/');
    let file_name = parts.next().filter(|name| {
        name.ends_with(".zip")
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
    });
    let directory = parts.next().filter(|name| name.starts_with("archive-"));
    match (directory, file_name) {
        (Some(directory), Some(file_name)) => Ok(format!("{directory}/{file_name}")),
        _ => Err(PersonalServiceError::new(
            "CN bundle archive path is invalid",
        )),
    }
}
// //// /提取归档的本地路径标识 ////

// //// 按差分版本读取归档清单 [@x380kkm 2026-09-20] ////
fn diff_archives(manifest: &Value) -> Result<Vec<(String, Value)>, PersonalServiceError> {
    let mut groups = manifest["diff"]
        .as_array()
        .ok_or_else(|| PersonalServiceError::new("CN asset path diff is invalid"))?
        .iter()
        .collect::<Vec<_>>();
    groups.sort_by_key(|group| group["version"].as_str().and_then(parse_version));
    let mut result = Vec::new();
    for group in groups {
        let archives = group["archive"]
            .as_array()
            .ok_or_else(|| PersonalServiceError::new("CN asset path archive list is invalid"))?;
        for archive in archives {
            result.push((archive_key(archive)?, archive.clone()));
        }
    }
    Ok(result)
}
// //// /按差分版本读取归档清单 ////

// //// 读取清单声明的最高资源版本 [@x380kkm 2026-09-20] ////
pub(super) fn highest_version(manifest: &Value) -> Result<String, PersonalServiceError> {
    let version = manifest["info"]["target_asset_version"]
        .as_str()
        .ok_or_else(|| PersonalServiceError::new("CN asset path target version is invalid"))?;
    let mut highest = parse_version(version)
        .ok_or_else(|| PersonalServiceError::new("CN asset path target version is invalid"))?;
    for value in manifest["diff"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|group| &group["version"])
        .chain([&manifest["info"]["eventual_target_asset_version"]])
    {
        if let Some(version) = value.as_str().and_then(parse_version) {
            highest = highest.max(version);
        }
    }
    Ok(format!("{}.{}.{}", highest[0], highest[1], highest[2]))
}
// //// /读取清单声明的最高资源版本 ////

// //// 将包内新增或变更差分接续到可写版本链 [@x380kkm 2026-09-20] ////
pub(super) fn upgrade_path(
    bundle: &BundleManifest,
    override_root: &Path,
    bundle_changed: bool,
) -> Result<UpgradedPath, PersonalServiceError> {
    let mut manifest = match fs::read(override_root.join("path")) {
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes).map_err(|error| {
            PersonalServiceError::new(format!(
                "failed to decode CN override path manifest: {error}"
            ))
        })?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => bundle.value.clone(),
        Err(error) => {
            return Err(PersonalServiceError::new(format!(
                "failed to read CN override path manifest: {error}"
            )))
        }
    };
    register_override_archives(&mut manifest, override_root)?;
    if !bundle_changed {
        return Ok(UpgradedPath {
            manifest,
            repair_version: None,
        });
    }
    let existing = diff_archives(&manifest)?
        .into_iter()
        .map(|(key, archive)| {
            (
                archive["bundle_location"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or(key),
                archive,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut replacements = BTreeSet::new();
    let mut additions = Vec::new();
    let mut selected = BTreeSet::new();
    for (key, mut archive) in diff_archives(&bundle.value)? {
        let shadowed = override_root.join(&key).is_file();
        let unchanged = existing.get(&key).is_some_and(|old| {
            old["size"] == archive["size"]
                && old["sha256"] == archive["sha256"]
                && (!shadowed || old["bundle_location"].as_str() == Some(key.as_str()))
        });
        if unchanged || !selected.insert(key.clone()) {
            continue;
        }
        if shadowed {
            publish_bundle_archive(bundle, override_root, &key, &mut archive)?;
        } else {
            replacements.insert(key);
        }
        additions.push(archive);
    }
    manifest["full"] = bundle.value["full"].clone();
    manifest["asset_version_hash"] = bundle.value["asset_version_hash"].clone();
    manifest["info"]["latest_maj_first_version"] =
        bundle.value["info"]["latest_maj_first_version"].clone();
    let current = highest_version(&manifest)?;
    let bundle_version = highest_version(&bundle.value)?;
    let highest = if parse_version(&current) >= parse_version(&bundle_version) {
        &current
    } else {
        &bundle_version
    };
    if additions.is_empty() && highest == &current {
        return Ok(UpgradedPath {
            manifest,
            repair_version: None,
        });
    }
    for group in manifest["diff"].as_array_mut().into_iter().flatten() {
        if let Some(archives) = group["archive"].as_array_mut() {
            archives.retain(|archive| {
                archive_key(archive).map_or(true, |key| !replacements.contains(&key))
            });
        }
    }
    let repair_version = increment_version(highest)?;
    append_diff(&mut manifest, &current, &repair_version, additions)?;
    Ok(UpgradedPath {
        manifest,
        repair_version: Some(repair_version),
    })
}
// //// /将包内新增或变更差分接续到可写版本链 ////

// //// 为同名覆盖下的包内修复归档建立独立下载地址 [@x380kkm 2026-09-20] ////
fn publish_bundle_archive(
    bundle: &BundleManifest,
    override_root: &Path,
    key: &str,
    archive: &mut Value,
) -> Result<(), PersonalServiceError> {
    let directory = key.split_once('/').unwrap().0;
    let archive_id = URL_SAFE_NO_PAD.encode(Sha256::digest(key.as_bytes()));
    let relative = format!(
        "{directory}/starpoint-cn-bundle-{}-{archive_id}-overlay.zip",
        bundle.signature
    );
    let bytes = fs::read(bundle.root.join(key)).map_err(|error| {
        PersonalServiceError::new(format!("failed to read CN bundle repair archive: {error}"))
    })?;
    atomic_write(&override_root.join(&relative), &bytes)?;
    archive["location"] = Value::from(relative);
    archive["bundle_location"] = Value::from(key);
    Ok(())
}
// //// /为同名覆盖下的包内修复归档建立独立下载地址 ////

// //// 将动态发现的覆盖归档写入既有版本链 [@x380kkm 2026-09-20] ////
fn register_override_archives(
    manifest: &mut Value,
    override_root: &Path,
) -> Result<(), PersonalServiceError> {
    let registered = diff_archives(manifest)?
        .into_iter()
        .map(|(key, _)| key)
        .collect::<BTreeSet<_>>();
    let mut additions = Vec::new();
    for directory in [
        "archive-common-diff",
        "archive-medium-diff",
        "archive-ios-diff",
        "archive-android-diff",
    ] {
        let entries = match fs::read_dir(override_root.join(directory)) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(PersonalServiceError::new(format!(
                    "failed to read CN override archive directory: {error}"
                )))
            }
        };
        for entry in entries {
            let entry = entry.map_err(|error| {
                PersonalServiceError::new(format!(
                    "failed to read CN override archive entry: {error}"
                ))
            })?;
            let name = entry.file_name();
            let Some(name) = name
                .to_str()
                .filter(|name| crate::cn_asset::is_additive_override_archive_name(name))
            else {
                continue;
            };
            let relative = format!("{directory}/{name}");
            if registered.contains(&relative) {
                continue;
            }
            let kind = entry.file_type().map_err(|error| {
                PersonalServiceError::new(format!("failed to inspect CN override archive: {error}"))
            })?;
            if !kind.is_file() {
                continue;
            }
            let mut file = fs::File::open(entry.path()).map_err(|error| {
                PersonalServiceError::new(format!("failed to open CN override archive: {error}"))
            })?;
            let size = file
                .metadata()
                .map_err(|error| {
                    PersonalServiceError::new(format!(
                        "failed to inspect CN override archive: {error}"
                    ))
                })?
                .len();
            let mut digest = Sha256::new();
            let mut bytes = [0_u8; 64 * 1024];
            loop {
                let count = file.read(&mut bytes).map_err(|error| {
                    PersonalServiceError::new(format!(
                        "failed to read CN override archive: {error}"
                    ))
                })?;
                if count == 0 {
                    break;
                }
                digest.update(&bytes[..count]);
            }
            additions.push(json!({"location": relative, "size": size, "sha256": STANDARD.encode(digest.finalize())}));
        }
    }
    if !additions.is_empty() {
        additions.sort_by(|left, right| left["location"].as_str().cmp(&right["location"].as_str()));
        let current = highest_version(manifest)?;
        append_diff(manifest, &current, &increment_version(&current)?, additions)?;
    }
    Ok(())
}
// //// /将动态发现的覆盖归档写入既有版本链 ////

// //// 追加资源过渡并同步最终版本 [@x380kkm 2026-09-20] ////
pub(super) fn append_diff(
    manifest: &mut Value,
    original_version: &str,
    target_version: &str,
    archives: Vec<Value>,
) -> Result<(), PersonalServiceError> {
    let info = manifest
        .get_mut("info")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("CN asset path info is invalid"))?;
    info.insert(
        "target_asset_version".to_owned(),
        Value::from(target_version),
    );
    info.insert(
        "eventual_target_asset_version".to_owned(),
        Value::from(target_version),
    );
    let diffs = manifest
        .get_mut("diff")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| PersonalServiceError::new("CN asset path diff is invalid"))?;
    diffs.push(json!({"version": target_version, "original_version": original_version, "archive": archives}));
    Ok(())
}
// //// /追加资源过渡并同步最终版本 ////

// //// 合并包内实体行并保留独立覆盖资源 [@x380kkm 2026-09-20] ////
pub(super) fn merge_entity_rows(base: &str, previous: &str, repair_version: &str) -> String {
    let old_rows = previous
        .lines()
        .map(|row| (row.split(',').next().unwrap_or_default(), row))
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    let mut rows = Vec::new();
    for row in base.lines() {
        let mut fields = row.splitn(3, ',');
        let path = fields.next().unwrap_or_default();
        let _ = fields.next();
        let Some(content) = fields.next() else {
            rows.push(row.to_owned());
            continue;
        };
        seen.insert(path);
        if let Some(old) = old_rows
            .get(path)
            .filter(|old| old.splitn(3, ',').nth(2) == Some(content))
        {
            rows.push((*old).to_owned());
        } else {
            rows.push(format!("{path},{repair_version},{content}"));
        }
    }
    rows.extend(
        previous
            .lines()
            .filter(|row| {
                let path = row.split(',').next().unwrap_or_default();
                !path.is_empty() && !seen.contains(path)
            })
            .map(str::to_owned),
    );
    let newline = if base.contains("\r\n") { "\r\n" } else { "\n" };
    format!("{}{newline}", rows.join(newline))
}
// //// /合并包内实体行并保留独立覆盖资源 ////
