# audience: internal
# # cn-gacha-banner-client-archive
#
# 此脚本把生成的 CN 卡池 banner 和当前 gacha master 写入 iOS 差分归档.
# 根 path 和 iOS EntityLists 始终引用同一组归档字节.
# 重复输入复用现有目标版本和归档.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

from __future__ import annotations

import argparse
import base64
import hashlib
import io
import json
import os
import re
import tempfile
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from cn_gacha_banner_assets import (
    decode_ordered_map,
    encode_entity_digest,
    hash_cn_asset_path,
    inspect_png,
    normalize_logical_path,
)


GACHA_MASTER_ENTRY = "production/upload/15/83d96aad4b9a46d19d19b6555d3f4232b29e25"
GACHA_MASTER_LOGICAL_PATH = "master/gacha/gacha.orderedmap"
IOS_ARCHIVE_DIRECTORY = "archive-ios-diff"
ARCHIVE_NAME_PREFIX = "starpoint-ios-gacha-banners-"
ENTITY_MANIFEST_DIRECTORY = "entities"
PATH_MANIFEST_NAME = "path"
VERSION_PATTERN = re.compile(r"^(?P<prefix>\d+(?:\.\d+)*)\.(?P<patch>\d+)$")
BANNER_DIMENSIONS = (510, 180)


class BannerArchiveError(RuntimeError):
    pass


@dataclass(frozen=True)
class EntityRecord:
    entry_path: str
    version: str
    byte_length: int
    digest: str
    asset_kind: str


@dataclass(frozen=True)
class AssetPayload:
    logical_path: str
    entry_path: str
    data: bytes

    @property
    def digest(self) -> str:
        return encode_entity_digest(self.data)


@dataclass(frozen=True)
class ExistingArchive:
    version: str
    original_version: str
    location: str
    relative_path: str


# //// 读取 JSON 和原子写入结果文件 [@x380kkm 2026-08-28] ////
def read_json_object(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8-sig"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise BannerArchiveError(f"JSON 文件无法读取: {path}") from error
    if not isinstance(value, dict):
        raise BannerArchiveError(f"JSON 根必须是对象: {path}")
    return value


def atomic_write(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_name = tempfile.mkstemp(
        prefix=f".{path.name}.", suffix=".tmp", dir=path.parent
    )
    temporary_path = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(data)
        os.replace(temporary_path, path)
    finally:
        temporary_path.unlink(missing_ok=True)
# //// /读取 JSON 和原子写入结果文件 ////


# //// 从生成报告读取全部 banner 资源 [@x380kkm 2026-08-28] ////
def banner_asset_logical_path(logical_path: str) -> str:
    normalized = normalize_logical_path(logical_path)
    return normalized if normalized.lower().endswith(".png") else f"{normalized}.png"


def resolve_report_path(report_path: Path, value: object, field: str) -> Path:
    if not isinstance(value, str) or not value:
        raise BannerArchiveError(f"banner 报告缺少 {field}")
    path = Path(value)
    return path if path.is_absolute() else report_path.parent / path


def load_banner_assets(report_path: Path, report: dict[str, Any]) -> list[AssetPayload]:
    groups = report.get("groups")
    if not isinstance(groups, list) or not groups:
        raise BannerArchiveError("banner 报告缺少 groups")

    assets: dict[str, AssetPayload] = {}
    for group in groups:
        if not isinstance(group, dict):
            raise BannerArchiveError("banner 报告包含无效 group")
        logical_path = group.get("logical_path")
        if not isinstance(logical_path, str) or not logical_path:
            raise BannerArchiveError("banner group 缺少 logical_path")
        asset_logical_path = banner_asset_logical_path(logical_path)
        asset_hash = hash_cn_asset_path(asset_logical_path)
        source_path = resolve_report_path(report_path, group.get("game_path"), "game_path")
        try:
            data = source_path.read_bytes()
        except OSError as error:
            raise BannerArchiveError(f"banner 游戏资源无法读取: {source_path}") from error
        if inspect_png(data) != BANNER_DIMENSIONS:
            raise BannerArchiveError(f"banner 尺寸无效: {source_path}")
        if source_path.parent.name != asset_hash[:2] or source_path.name != asset_hash[2:]:
            raise BannerArchiveError(f"banner 游戏资源路径与 logical path 不一致: {source_path}")
        entry_path = f"production/upload/{asset_hash[:2]}/{asset_hash[2:]}"
        payload = AssetPayload(asset_logical_path, entry_path, data)
        existing = assets.get(entry_path)
        if existing is not None and existing != payload:
            raise BannerArchiveError(f"banner 资源身份冲突: {entry_path}")
        assets[entry_path] = payload
    return [assets[entry_path] for entry_path in sorted(assets)]


def validate_override_report(
    report: dict[str, Any], assets: list[AssetPayload]
) -> list[dict[str, object]]:
    overrides = report.get("override_assets")
    policy = report.get("banner_path_overrides")
    if not isinstance(overrides, list) or not isinstance(policy, dict):
        raise BannerArchiveError("banner 报告缺少 override 证据")
    assets_by_hash = {
        asset.entry_path.rsplit("/", 2)[-2] + asset.entry_path.rsplit("/", 1)[-1]: asset
        for asset in assets
    }
    evidence: list[dict[str, object]] = []
    for override in overrides:
        if not isinstance(override, dict):
            raise BannerArchiveError("banner 报告包含无效 override")
        pool_id = override.get("pool_id")
        logical_path = override.get("logical_path")
        asset_hash = override.get("asset_hash")
        if not isinstance(pool_id, int) or not isinstance(logical_path, str):
            raise BannerArchiveError("banner override 缺少卡池身份")
        expected_hash = hash_cn_asset_path(banner_asset_logical_path(logical_path))
        if asset_hash != expected_hash or assets_by_hash.get(expected_hash) is None:
            raise BannerArchiveError(f"banner override 资源不完整: {pool_id}")
        if policy.get(str(pool_id)) != logical_path:
            raise BannerArchiveError(f"banner override 策略不一致: {pool_id}")
        evidence.append(
            {
                "pool_id": pool_id,
                "logical_path": logical_path,
                "asset_hash": expected_hash,
                "entry_path": assets_by_hash[expected_hash].entry_path,
            }
        )
    if report.get("override_asset_count") != len(evidence):
        raise BannerArchiveError("banner override 计数不一致")
    return evidence
# //// /从生成报告读取全部 banner 资源 ////


# //// 定位当前 EntityLists 资源记录 [@x380kkm 2026-08-28] ////
def entity_manifest_paths(cdn_root: Path) -> list[Path]:
    entity_root = cdn_root / ENTITY_MANIFEST_DIRECTORY
    path_manifest = entity_root / "PathFile.csv"
    ios_manifests = sorted(entity_root.glob("*-ios_medium.csv"))
    paths = [path_manifest, *ios_manifests]
    if not path_manifest.is_file() or not ios_manifests:
        raise BannerArchiveError("CN CDN 缺少 PathFile 或 iOS EntityLists")
    return paths


def read_entity_record(manifest_path: Path, entry_path: str) -> EntityRecord:
    matches: list[EntityRecord] = []
    try:
        for line in manifest_path.read_text(encoding="utf-8-sig").splitlines():
            if not line.startswith(f"{entry_path},"):
                continue
            fields = line.split(",")
            if len(fields) != 5:
                raise BannerArchiveError(f"EntityLists 行格式无效: {manifest_path}")
            matches.append(
                EntityRecord(
                    entry_path=fields[0],
                    version=fields[1],
                    byte_length=int(fields[2]),
                    digest=fields[3],
                    asset_kind=fields[4],
                )
            )
    except (OSError, UnicodeError, ValueError) as error:
        raise BannerArchiveError(f"EntityLists 无法读取: {manifest_path}") from error
    if len(matches) != 1:
        raise BannerArchiveError(
            f"EntityLists 目标行数量无效: {manifest_path} entry={entry_path} count={len(matches)}"
        )
    return matches[0]
# //// /定位当前 EntityLists 资源记录 ////


# //// 从当前 CDN 归档读取 gacha master [@x380kkm 2026-08-28] ////
def archive_relative_path(location: object) -> str:
    if not isinstance(location, str):
        raise BannerArchiveError("path 归档 location 无效")
    parts = [part for part in location.replace("\\", "/").split("/") if part]
    if len(parts) < 2:
        raise BannerArchiveError("path 归档 location 不完整")
    return "/".join(parts[-2:])


def archive_entries(manifest: dict[str, Any]) -> list[tuple[dict[str, Any], dict[str, Any]]]:
    full = manifest.get("full")
    diffs = manifest.get("diff")
    if not isinstance(full, dict) or not isinstance(diffs, list):
        raise BannerArchiveError("path 归档分组无效")
    pairs: list[tuple[dict[str, Any], dict[str, Any]]] = []
    for group in [*reversed(diffs), full]:
        if not isinstance(group, dict) or not isinstance(group.get("archive"), list):
            raise BannerArchiveError("path 归档组无效")
        for archive in group["archive"]:
            if not isinstance(archive, dict):
                raise BannerArchiveError("path 归档记录无效")
            pairs.append((group, archive))
    return pairs


def candidate_archive_paths(
    cdn_root: Path, manifest: dict[str, Any], asset_kind: str
) -> list[Path]:
    expected_directories = {
        f"archive-{asset_kind}-diff",
        f"archive-{asset_kind}-full",
    }
    paths: list[Path] = []
    seen: set[Path] = set()
    manifest_paths: list[Path] = []
    for _, archive in archive_entries(manifest):
        relative = archive_relative_path(archive.get("location"))
        directory = relative.split("/", 1)[0]
        path = cdn_root.joinpath(*relative.split("/"))
        if directory in expected_directories and path.is_file() and path not in seen:
            manifest_paths.append(path)
            seen.add(path)
    paths.extend(
        path
        for path in manifest_paths
        if "gacha" in path.name.lower()
    )
    paths.extend(path for path in manifest_paths if path not in paths)
    for directory in sorted(expected_directories):
        for path in sorted((cdn_root / directory).glob("*.zip")):
            if path not in seen:
                paths.append(path)
                seen.add(path)
    return paths


def read_current_asset(
    cdn_root: Path,
    manifest: dict[str, Any],
    record: EntityRecord,
) -> bytes:
    for archive_path in candidate_archive_paths(cdn_root, manifest, record.asset_kind):
        try:
            with zipfile.ZipFile(archive_path) as archive:
                info = archive.getinfo(record.entry_path)
                if info.file_size != record.byte_length:
                    continue
                data = archive.read(info)
        except KeyError:
            continue
        except (OSError, RuntimeError, zipfile.BadZipFile) as error:
            raise BannerArchiveError(f"CN 归档无法读取: {archive_path}") from error
        if encode_entity_digest(data) == record.digest:
            return data
    raise BannerArchiveError(f"CN 归档缺少当前资源: {record.entry_path}")


def validate_master_overrides(
    master_data: bytes, override_evidence: list[dict[str, object]]
) -> list[dict[str, object]]:
    master = decode_ordered_map(master_data)
    if not isinstance(master, dict):
        raise BannerArchiveError("gacha master 根不是映射")
    verified: list[dict[str, object]] = []
    for evidence in override_evidence:
        pool_id = int(evidence["pool_id"])
        row = master.get(str(pool_id))
        if not isinstance(row, list) or len(row) <= 3:
            raise BannerArchiveError(f"gacha master 缺少 override 卡池: {pool_id}")
        if row[3] != evidence["logical_path"]:
            raise BannerArchiveError(f"gacha master banner 路径不一致: {pool_id}")
        verified.append({**evidence, "master_banner_path": row[3]})
    return verified
# //// /从当前 CDN 归档读取 gacha master ////


# //// 生成确定的非 ZIP64 iOS 差分归档 [@x380kkm 2026-08-28] ////
def zip_entry(name: str) -> zipfile.ZipInfo:
    entry = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
    entry.create_system = 3
    entry.external_attr = 0o100644 << 16
    entry.compress_type = zipfile.ZIP_DEFLATED
    return entry


def build_archive(master_data: bytes, banner_assets: list[AssetPayload]) -> bytes:
    output = io.BytesIO()
    with zipfile.ZipFile(
        output,
        "w",
        compression=zipfile.ZIP_DEFLATED,
        compresslevel=9,
        allowZip64=False,
    ) as archive:
        archive.writestr(zip_entry(GACHA_MASTER_ENTRY), master_data)
        for asset in banner_assets:
            archive.writestr(zip_entry(asset.entry_path), asset.data)
    data = output.getvalue()
    if b"PK\x06\x06" in data or b"PK\x06\x07" in data:
        raise BannerArchiveError("iOS 差分归档使用了 ZIP64")
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        expected = [GACHA_MASTER_ENTRY, *(asset.entry_path for asset in banner_assets)]
        if archive.namelist() != expected:
            raise BannerArchiveError("iOS 差分归档条目顺序或数量无效")
    return data


def standard_archive_digest(data: bytes) -> str:
    return base64.b64encode(hashlib.sha256(data).digest()).decode("ascii")
# //// /生成确定的非 ZIP64 iOS 差分归档 ////


# //// 选择幂等的资产目标版本 [@x380kkm 2026-08-28] ////
def increment_version(version: str) -> str:
    match = VERSION_PATTERN.fullmatch(version)
    if match is None:
        raise BannerArchiveError(f"资产版本格式无效: {version}")
    return f"{match.group('prefix')}.{int(match.group('patch')) + 1}"


def version_key(version: str) -> tuple[int, ...]:
    match = VERSION_PATTERN.fullmatch(version)
    if match is None:
        raise BannerArchiveError(f"资产版本格式无效: {version}")
    return tuple(int(part) for part in match.group("prefix").split(".")) + (
        int(match.group("patch")),
    )


def current_target_version(manifest: dict[str, Any]) -> str:
    info = manifest.get("info")
    if not isinstance(info, dict):
        raise BannerArchiveError("path 缺少 info")
    version = info.get("target_asset_version")
    if not isinstance(version, str) or not version:
        raise BannerArchiveError("path 缺少 target_asset_version")
    return version


def matching_existing_archive(
    cdn_root: Path, manifest: dict[str, Any], archive_data: bytes
) -> ExistingArchive | None:
    matches: list[ExistingArchive] = []
    diffs = manifest.get("diff")
    if not isinstance(diffs, list):
        raise BannerArchiveError("path diff 无效")
    for group in diffs:
        if not isinstance(group, dict):
            continue
        version = group.get("version")
        if not isinstance(version, str):
            raise BannerArchiveError("path diff 版本无效")
        archives = group.get("archive")
        if not isinstance(archives, list):
            raise BannerArchiveError("path diff archive 无效")
        for archive in archives:
            if not isinstance(archive, dict):
                raise BannerArchiveError("path diff 归档记录无效")
            relative = archive_relative_path(archive.get("location"))
            directory, name = relative.split("/", 1)
            if directory != IOS_ARCHIVE_DIRECTORY or not name.startswith(ARCHIVE_NAME_PREFIX):
                continue
            local_path = cdn_root / directory / name
            if local_path.is_file() and local_path.read_bytes() == archive_data:
                original_version = group.get("original_version")
                location = archive.get("location")
                if not isinstance(original_version, str) or not isinstance(location, str):
                    raise BannerArchiveError("path iOS banner 差分版本无效")
                matches.append(
                    ExistingArchive(
                        version=version,
                        original_version=original_version,
                        location=location,
                        relative_path=relative,
                    )
                )
    if not matches:
        return None
    return max(
        matches,
        key=lambda match: (
            version_key(match.version),
            version_key(match.original_version),
            match.location,
        ),
    )


def select_versions(
    manifest: dict[str, Any],
    existing: ExistingArchive | None,
    requested_target: str | None,
) -> tuple[str, str]:
    current = current_target_version(manifest)
    if requested_target is not None:
        if existing is not None and requested_target == existing.version:
            return existing.original_version, existing.version
        if requested_target == current:
            raise BannerArchiveError("指定目标版本与当前版本相同且没有可复用归档")
        return current, requested_target
    if existing is not None:
        return existing.original_version, existing.version
    return current, increment_version(current)
# //// /选择幂等的资产目标版本 ////


# //// 同步 path 和 iOS EntityLists [@x380kkm 2026-08-28] ////
def build_archive_location(manifest: dict[str, Any], relative_path: str) -> str:
    for _, archive in archive_entries(manifest):
        location = archive.get("location")
        if not isinstance(location, str):
            continue
        parts = location.replace("\\", "/").rsplit("/", 2)
        if len(parts) == 3:
            return f"{parts[0]}/{relative_path}"
    raise BannerArchiveError("path 缺少可复用的 CDN 地址前缀")


def update_path_manifest(
    manifest: dict[str, Any],
    original_version: str,
    target_version: str,
    location: str,
    archive_data: bytes,
) -> bytes:
    info = manifest.get("info")
    diffs = manifest.get("diff")
    if not isinstance(info, dict) or not isinstance(diffs, list):
        raise BannerArchiveError("path 版本或 diff 无效")
    info["target_asset_version"] = target_version
    info["eventual_target_asset_version"] = target_version
    metadata = {
        "location": location,
        "size": len(archive_data),
        "sha256": standard_archive_digest(archive_data),
    }

    matching_groups = [
        group
        for group in diffs
        if isinstance(group, dict)
        and group.get("version") == target_version
        and group.get("original_version") == original_version
    ]
    if len(matching_groups) > 1:
        raise BannerArchiveError("path 包含重复的目标差分组")
    if matching_groups:
        group = matching_groups[0]
        archives = group.get("archive")
        if not isinstance(archives, list):
            raise BannerArchiveError("path 目标差分归档无效")
        group["archive"] = [
            archive
            for archive in archives
            if not (
                isinstance(archive, dict)
                and archive_relative_path(archive.get("location")).startswith(
                    f"{IOS_ARCHIVE_DIRECTORY}/{ARCHIVE_NAME_PREFIX}"
                )
            )
        ] + [metadata]
    else:
        diffs.append(
            {
                "version": target_version,
                "original_version": original_version,
                "archive": [metadata],
            }
        )
    return json.dumps(manifest, ensure_ascii=False, separators=(",", ":")).encode(
        "utf-8"
    )


def render_entity_manifest(
    manifest_path: Path,
    target_version: str,
    assets: list[AssetPayload],
) -> bytes:
    original = manifest_path.read_bytes()
    text = original.decode("utf-8-sig")
    newline = "\r\n" if "\r\n" in text else "\n"
    had_final_newline = text.endswith(("\r\n", "\n"))
    replacements = {
        asset.entry_path: (
            f"{asset.entry_path},{target_version},{len(asset.data)},{asset.digest},common"
        )
        for asset in assets
    }
    seen: dict[str, int] = {entry_path: 0 for entry_path in replacements}
    output_lines: list[str] = []
    for line in text.splitlines():
        entry_path = line.split(",", 1)[0]
        if entry_path in replacements:
            seen[entry_path] += 1
            output_lines.append(replacements[entry_path])
        else:
            output_lines.append(line)
    duplicates = sorted(entry_path for entry_path, count in seen.items() if count > 1)
    if duplicates:
        raise BannerArchiveError(
            f"EntityLists 包含重复目标行: {manifest_path} count={len(duplicates)}"
        )
    output_lines.extend(
        replacements[entry_path]
        for entry_path in sorted(replacements)
        if seen[entry_path] == 0
    )
    rendered = newline.join(output_lines)
    if had_final_newline or output_lines:
        rendered += newline
    return rendered.encode("utf-8")
# //// /同步 path 和 iOS EntityLists ////


# //// 安装并审计 iOS banner 差分 [@x380kkm 2026-08-28] ////
def install_banner_archive(
    cdn_root: Path,
    report_path: Path,
    requested_target: str | None = None,
    audit_report_path: Path | None = None,
) -> dict[str, Any]:
    report = read_json_object(report_path)
    banner_assets = load_banner_assets(report_path, report)
    override_evidence = validate_override_report(report, banner_assets)
    manifest_paths = entity_manifest_paths(cdn_root)
    path_manifest_path = cdn_root / PATH_MANIFEST_NAME
    path_manifest = read_json_object(path_manifest_path)
    master_record = read_entity_record(manifest_paths[0], GACHA_MASTER_ENTRY)
    master_data = read_current_asset(cdn_root, path_manifest, master_record)
    verified_overrides = validate_master_overrides(master_data, override_evidence)
    master_asset = AssetPayload(
        GACHA_MASTER_LOGICAL_PATH,
        GACHA_MASTER_ENTRY,
        master_data,
    )
    archive_data = build_archive(master_data, banner_assets)
    existing = matching_existing_archive(cdn_root, path_manifest, archive_data)
    original_version, target_version = select_versions(
        path_manifest, existing, requested_target
    )
    current_version = current_target_version(path_manifest)

    if existing is not None and existing.version == target_version:
        archive_relative = existing.relative_path
        archive_location_value = existing.location
    else:
        archive_name = (
            f"{ARCHIVE_NAME_PREFIX}{original_version}-{target_version}.zip"
        )
        archive_relative = f"{IOS_ARCHIVE_DIRECTORY}/{archive_name}"
        archive_location_value = build_archive_location(path_manifest, archive_relative)
    archive_path = cdn_root.joinpath(*archive_relative.split("/"))
    if not archive_path.is_file() or archive_path.read_bytes() != archive_data:
        atomic_write(archive_path, archive_data)

    if existing is None or target_version == current_version or requested_target is not None:
        updated_path = update_path_manifest(
            path_manifest,
            original_version,
            target_version,
            archive_location_value,
            archive_data,
        )
        if path_manifest_path.read_bytes() != updated_path:
            atomic_write(path_manifest_path, updated_path)

        entity_assets = [master_asset, *banner_assets]
        for manifest_path in manifest_paths:
            rendered = render_entity_manifest(manifest_path, target_version, entity_assets)
            if manifest_path.read_bytes() != rendered:
                atomic_write(manifest_path, rendered)

    audit = {
        "cdn_root": str(cdn_root),
        "source_report": str(report_path),
        "original_version": original_version,
        "target_version": target_version,
        "archive": {
            "relative_path": archive_relative,
            "location": archive_location_value,
            "size": len(archive_data),
            "sha256": standard_archive_digest(archive_data),
            "zip64": False,
            "entry_count": 1 + len(banner_assets),
        },
        "entity_manifests": [str(path) for path in manifest_paths],
        "banner_asset_count": len(banner_assets),
        "override_assets": verified_overrides,
        "master": {
            "logical_path": GACHA_MASTER_LOGICAL_PATH,
            "entry_path": GACHA_MASTER_ENTRY,
            "byte_length": len(master_data),
            "digest": encode_entity_digest(master_data),
        },
        "reused": existing is not None and existing.version == target_version,
    }
    if audit_report_path is not None:
        atomic_write(
            audit_report_path,
            (json.dumps(audit, ensure_ascii=False, indent=2) + "\n").encode("utf-8"),
        )
    return audit
# //// /安装并审计 iOS banner 差分 ////


# //// 执行 iOS banner 差分安装命令 [@x380kkm 2026-08-28] ////
def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cdn-root", required=True, type=Path)
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument("--target-version")
    parser.add_argument("--audit-report", type=Path)
    arguments = parser.parse_args()
    audit = install_banner_archive(
        arguments.cdn_root.resolve(strict=True),
        arguments.report.resolve(strict=True),
        arguments.target_version,
        arguments.audit_report.resolve() if arguments.audit_report else None,
    )
    print(json.dumps(audit, ensure_ascii=False, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except BannerArchiveError as error:
        print(json.dumps({"error": str(error)}, ensure_ascii=False), file=os.sys.stderr)
        raise SystemExit(1) from None
# //// /执行 iOS banner 差分安装命令 ////
