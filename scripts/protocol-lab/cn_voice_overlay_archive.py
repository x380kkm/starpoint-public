# audience: internal
# # cn-voice-overlay-archive
#
# 此脚本把角色语音覆盖归档接入 CN iOS 资源版本链.
# CDN 安装更新根 path 和 iOS EntityLists, 可写覆盖安装生成同样的目录结构.
#
# /// script
# requires-python = ">=3.12,<3.13"
# ///

from __future__ import annotations

import argparse
import base64
import copy
import hashlib
import json
import os
import re
import tempfile
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any


IOS_ARCHIVE_DIRECTORY = "archive-ios-diff"
ARCHIVE_NAME_PREFIX = "starpoint-cn-voice-overlay-"
LEGACY_ARCHIVE_NAME_PREFIX = "starpoint-ios-voice-overlay-"
ENTITY_MANIFEST_DIRECTORY = "entities"
PATH_MANIFEST_NAME = "path"
VERSION_PATTERN = re.compile(r"^(?P<prefix>\d+(?:\.\d+)*)\.(?P<patch>\d+)$")
ENTRY_PATH_PATTERN = re.compile(r"^production/upload/[0-9a-f]{2}/[0-9a-f]{38}$")
ENTITY_DIGEST_PATTERN = re.compile(r"^[A-Za-z0-9_-]{43}$")


class VoiceArchiveError(RuntimeError):
    pass


@dataclass(frozen=True)
class VoiceEntity:
    logical_path: str
    entry_path: str
    byte_length: int
    digest: str


@dataclass(frozen=True)
class ExistingArchive:
    version: str
    original_version: str
    location: str
    relative_path: str


# //// 读取 JSON 和原子写入资源文件 [@x380kkm 2026-08-29] ////
def read_json_object(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8-sig"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise VoiceArchiveError(f"JSON 文件无法读取: {path}") from error
    if not isinstance(value, dict):
        raise VoiceArchiveError(f"JSON 根必须是对象: {path}")
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
# //// /读取 JSON 和原子写入资源文件 ////


# //// 验证语音报告和差分归档 [@x380kkm 2026-08-29] ////
def standard_archive_digest(data: bytes) -> str:
    return base64.b64encode(hashlib.sha256(data).digest()).decode("ascii")


def entity_digest(data: bytes) -> str:
    encoded = base64.b64encode(hashlib.sha256(data).digest()).decode("ascii")
    return encoded.rstrip("=").replace("+", "_").replace("/", "-")


def report_archive_path(report_path: Path, report: dict[str, Any]) -> Path:
    archive = report.get("archive")
    if not isinstance(archive, dict):
        raise VoiceArchiveError("语音报告缺少 archive")
    relative_path = archive.get("relative_path")
    if not isinstance(relative_path, str) or not relative_path:
        raise VoiceArchiveError("语音报告缺少归档相对路径")
    path = Path(relative_path)
    if path.is_absolute() or ".." in path.parts:
        raise VoiceArchiveError("语音报告归档路径无效")
    return report_path.parent / path


def report_entities(report: dict[str, Any]) -> list[VoiceEntity]:
    if report.get("missing_count") != 0 or report.get("role_count") != 17:
        raise VoiceArchiveError("语音报告未覆盖全部目标角色")
    groups = (("master", report.get("masters", [])), ("asset", report.get("assets", [])))
    if not any(records for _, records in groups):
        raise VoiceArchiveError("语音报告缺少资源记录")
    entities: list[VoiceEntity] = []
    seen: set[str] = set()
    for record_kind, records in groups:
        if not isinstance(records, list):
            raise VoiceArchiveError(f"语音报告的 {record_kind} 记录无效")
        for record in records:
            if not isinstance(record, dict):
                raise VoiceArchiveError("语音报告包含无效资源记录")
            logical_path = record.get("logical_path")
            entry_path = record.get("entry_path")
            byte_length = record.get("byte_length")
            digest = record.get("digest")
            if not isinstance(logical_path, str) or not logical_path:
                raise VoiceArchiveError("语音资源缺少 logical_path")
            if record_kind == "master" and "_iosbundled" in logical_path:
                raise VoiceArchiveError(f"语音覆盖不能替换 iOS bundled master: {logical_path}")
            if not isinstance(entry_path, str) or ENTRY_PATH_PATTERN.fullmatch(entry_path) is None:
                raise VoiceArchiveError(f"语音资源路径无效: {entry_path}")
            if not isinstance(byte_length, int) or byte_length <= 0:
                raise VoiceArchiveError(f"语音资源长度无效: {entry_path}")
            if not isinstance(digest, str) or ENTITY_DIGEST_PATTERN.fullmatch(digest) is None:
                raise VoiceArchiveError(f"语音资源摘要无效: {entry_path}")
            if entry_path in seen:
                raise VoiceArchiveError(f"语音报告包含重复资源: {entry_path}")
            seen.add(entry_path)
            entities.append(VoiceEntity(logical_path, entry_path, byte_length, digest))
    return entities


def load_verified_archive(
    report_path: Path,
) -> tuple[dict[str, Any], list[VoiceEntity], bytes]:
    report = read_json_object(report_path)
    entities = report_entities(report)
    archive_path = report_archive_path(report_path, report)
    try:
        data = archive_path.read_bytes()
    except OSError as error:
        raise VoiceArchiveError(f"语音归档无法读取: {archive_path}") from error
    archive_record = report.get("archive")
    if not isinstance(archive_record, dict):
        raise VoiceArchiveError("语音报告缺少 archive")
    if archive_record.get("zip64") is not False:
        raise VoiceArchiveError("语音归档不能使用 ZIP64")
    if archive_record.get("byte_length") != len(data):
        raise VoiceArchiveError("语音归档长度与报告不一致")
    if archive_record.get("sha256") != standard_archive_digest(data):
        raise VoiceArchiveError("语音归档摘要与报告不一致")
    if b"PK\x06\x06" in data or b"PK\x06\x07" in data:
        raise VoiceArchiveError("语音归档包含 ZIP64 记录")
    try:
        with zipfile.ZipFile(archive_path) as archive:
            infos = archive.infolist()
            expected_names = [entity.entry_path for entity in entities]
            if [info.filename for info in infos] != expected_names:
                raise VoiceArchiveError("语音归档条目顺序与报告不一致")
            for info, entity in zip(infos, entities, strict=True):
                body = archive.read(info)
                if info.file_size != entity.byte_length or len(body) != entity.byte_length:
                    raise VoiceArchiveError(f"语音归档条目长度无效: {entity.entry_path}")
                if entity_digest(body) != entity.digest:
                    raise VoiceArchiveError(f"语音归档条目摘要无效: {entity.entry_path}")
    except (OSError, RuntimeError, zipfile.BadZipFile) as error:
        raise VoiceArchiveError(f"语音归档无法校验: {archive_path}") from error
    if archive_record.get("entry_count") != len(entities):
        raise VoiceArchiveError("语音归档条目数量与报告不一致")
    return report, entities, data
# //// /验证语音报告和差分归档 ////


# //// 解析 CN path 和 iOS EntityLists [@x380kkm 2026-08-29] ////
def entity_manifest_paths(root: Path) -> list[Path]:
    entity_root = root / ENTITY_MANIFEST_DIRECTORY
    path_file = entity_root / "PathFile.csv"
    ios_files = sorted(entity_root.glob("*-ios_medium.csv"))
    if not path_file.is_file() or not ios_files:
        raise VoiceArchiveError(f"CN CDN 缺少 PathFile 或 iOS EntityLists: {root}")
    return [path_file, *ios_files]


def archive_groups(manifest: dict[str, Any]) -> list[dict[str, Any]]:
    full = manifest.get("full")
    diffs = manifest.get("diff")
    if not isinstance(full, dict) or not isinstance(diffs, list):
        raise VoiceArchiveError("path 归档分组无效")
    groups = [full, *diffs]
    if any(not isinstance(group, dict) or not isinstance(group.get("archive"), list) for group in groups):
        raise VoiceArchiveError("path 归档组无效")
    return groups


def current_target_version(manifest: dict[str, Any]) -> str:
    info = manifest.get("info")
    if not isinstance(info, dict):
        raise VoiceArchiveError("path 缺少 info")
    version = info.get("target_asset_version")
    if not isinstance(version, str) or VERSION_PATTERN.fullmatch(version) is None:
        raise VoiceArchiveError("path 目标版本无效")
    return version


def increment_version(version: str) -> str:
    match = VERSION_PATTERN.fullmatch(version)
    if match is None:
        raise VoiceArchiveError(f"资产版本格式无效: {version}")
    return f"{match.group('prefix')}.{int(match.group('patch')) + 1}"


def version_key(version: str) -> tuple[int, ...]:
    match = VERSION_PATTERN.fullmatch(version)
    if match is None:
        raise VoiceArchiveError(f"资产版本格式无效: {version}")
    return tuple(int(part) for part in match.group("prefix").split(".")) + (
        int(match.group("patch")),
    )


def build_archive_location(manifest: dict[str, Any], relative_path: str) -> str:
    for group in archive_groups(manifest):
        for archive in group["archive"]:
            if not isinstance(archive, dict):
                continue
            location = archive.get("location")
            if not isinstance(location, str):
                continue
            parts = location.replace("\\", "/").rsplit("/", 2)
            if len(parts) == 3:
                return f"{parts[0]}/{relative_path}"
    raise VoiceArchiveError("path 缺少可复用的 CDN 地址前缀")
# //// /解析 CN path 和 iOS EntityLists ////


# //// 识别当前和历史语音归档名 [@x380kkm 2026-08-29] ////
def is_voice_archive_name(name: str) -> bool:
    return name.startswith((ARCHIVE_NAME_PREFIX, LEGACY_ARCHIVE_NAME_PREFIX))
# //// /识别当前和历史语音归档名 ////


# //// 选择幂等的语音差分版本 [@x380kkm 2026-08-29] ////
def matching_existing_archive(
    roots: tuple[Path, ...], manifest: dict[str, Any], archive_data: bytes
) -> ExistingArchive | None:
    matches: list[ExistingArchive] = []
    diffs = manifest.get("diff")
    if not isinstance(diffs, list):
        raise VoiceArchiveError("path diff 无效")
    for group in diffs:
        if not isinstance(group, dict):
            raise VoiceArchiveError("path diff 组无效")
        version = group.get("version")
        original_version = group.get("original_version")
        archives = group.get("archive")
        if not isinstance(version, str) or not isinstance(original_version, str) or not isinstance(archives, list):
            raise VoiceArchiveError("path diff 版本无效")
        for archive in archives:
            if not isinstance(archive, dict):
                raise VoiceArchiveError("path diff 归档无效")
            location = archive.get("location")
            if not isinstance(location, str):
                continue
            parts = [part for part in location.replace("\\", "/").split("/") if part]
            if len(parts) < 2 or parts[-2] != IOS_ARCHIVE_DIRECTORY:
                continue
            name = parts[-1]
            if not is_voice_archive_name(name):
                continue
            relative_path = f"{IOS_ARCHIVE_DIRECTORY}/{name}"
            local_paths = [root / IOS_ARCHIVE_DIRECTORY / name for root in roots]
            if any(path.is_file() and path.read_bytes() == archive_data for path in local_paths):
                matches.append(
                    ExistingArchive(version, original_version, location, relative_path)
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


def add_voice_diff(
    manifest: dict[str, Any],
    original_version: str,
    target_version: str,
    location: str,
    archive_data: bytes,
) -> bytes:
    updated = copy.deepcopy(manifest)
    info = updated.get("info")
    diffs = updated.get("diff")
    if not isinstance(info, dict) or not isinstance(diffs, list):
        raise VoiceArchiveError("path 版本或 diff 无效")
    if any(isinstance(group, dict) and group.get("version") == target_version for group in diffs):
        raise VoiceArchiveError(f"path 已包含目标版本: {target_version}")
    info["target_asset_version"] = target_version
    info["eventual_target_asset_version"] = target_version
    diffs.append(
        {
            "version": target_version,
            "original_version": original_version,
            "archive": [
                {
                    "location": location,
                    "size": len(archive_data),
                    "sha256": standard_archive_digest(archive_data),
                }
            ],
        }
    )
    return json.dumps(updated, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
# //// /选择幂等的语音差分版本 ////


# //// 同步语音资源的 EntityLists 记录 [@x380kkm 2026-08-29] ////
def render_entity_manifest(
    source_path: Path, target_version: str, entities: list[VoiceEntity]
) -> bytes:
    original = source_path.read_bytes()
    text = original.decode("utf-8-sig")
    newline = "\r\n" if "\r\n" in text else "\n"
    had_final_newline = text.endswith(("\r\n", "\n"))
    replacements = {
        entity.entry_path: (
            f"{entity.entry_path},{target_version},{entity.byte_length},{entity.digest},common"
        )
        for entity in entities
    }
    counts = {entry_path: 0 for entry_path in replacements}
    output: list[str] = []
    for line in text.splitlines():
        entry_path = line.split(",", 1)[0]
        replacement = replacements.get(entry_path)
        if replacement is None:
            output.append(line)
            continue
        counts[entry_path] += 1
        output.append(replacement)
    duplicates = [entry_path for entry_path, count in counts.items() if count > 1]
    if duplicates:
        raise VoiceArchiveError(
            f"EntityLists 包含重复语音资源: {source_path} count={len(duplicates)}"
        )
    output.extend(
        replacements[entry_path]
        for entry_path in sorted(replacements)
        if counts[entry_path] == 0
    )
    rendered = newline.join(output)
    if had_final_newline or output:
        rendered += newline
    return rendered.encode("utf-8")
# //// /同步语音资源的 EntityLists 记录 ////


# //// 安装 CN iOS 语音覆盖链 [@x380kkm 2026-08-29] ////
def install_voice_archive(
    cdn_root: Path,
    report_path: Path,
    override_root: Path | None = None,
    audit_report_path: Path | None = None,
) -> dict[str, Any]:
    report, entities, archive_data = load_verified_archive(report_path)
    destination_root = override_root or cdn_root
    active_root = (
        destination_root
        if (destination_root / PATH_MANIFEST_NAME).is_file()
        else cdn_root
    )
    active_path = active_root / PATH_MANIFEST_NAME
    manifest = read_json_object(active_path)
    active_entity_paths = entity_manifest_paths(active_root)
    roots = tuple(dict.fromkeys((active_root, destination_root)))
    current_version = current_target_version(manifest)
    destination_entity_root = destination_root / ENTITY_MANIFEST_DIRECTORY
    existing = matching_existing_archive(roots, manifest, archive_data)

    if existing is None:
        original_version = current_target_version(manifest)
        target_version = increment_version(original_version)
        archive_name = f"{ARCHIVE_NAME_PREFIX}{original_version}-{target_version}.zip"
        archive_relative = f"{IOS_ARCHIVE_DIRECTORY}/{archive_name}"
        archive_location = build_archive_location(manifest, archive_relative)
        updated_path = add_voice_diff(
            manifest,
            original_version,
            target_version,
            archive_location,
            archive_data,
        )
        archive_path = destination_root / IOS_ARCHIVE_DIRECTORY / archive_name
        atomic_write(archive_path, archive_data)
        atomic_write(destination_root / PATH_MANIFEST_NAME, updated_path)
        reused = False
    else:
        original_version = existing.original_version
        target_version = existing.version
        archive_relative = existing.relative_path
        archive_location = existing.location
        archive_path = destination_root.joinpath(*archive_relative.split("/"))
        if destination_root != active_root and not archive_path.is_file():
            atomic_write(archive_path, archive_data)
        if destination_root != active_root:
            atomic_write(destination_root / PATH_MANIFEST_NAME, active_path.read_bytes())
        reused = True

    if existing is None or destination_root != active_root or target_version == current_version:
        for source_path in active_entity_paths:
            rendered = render_entity_manifest(source_path, target_version, entities)
            destination_path = destination_entity_root / source_path.name
            if not destination_path.is_file() or destination_path.read_bytes() != rendered:
                atomic_write(destination_path, rendered)

    audit = {
        "cdn_root": str(cdn_root),
        "override_root": str(override_root) if override_root is not None else None,
        "source_report": str(report_path),
        "original_version": original_version,
        "target_version": target_version,
        "archive": {
            "relative_path": archive_relative,
            "location": archive_location,
            "size": len(archive_data),
            "sha256": standard_archive_digest(archive_data),
            "zip64": False,
            "entry_count": len(entities),
        },
        "role_count": report["role_count"],
        "speech_mp3_count": report["speech_mp3_count"],
        "battle_mp3_count": report["battle_mp3_count"],
        "entity_manifests": [
            str(destination_entity_root / path.name) for path in active_entity_paths
        ],
        "reused": reused,
    }
    if audit_report_path is not None:
        atomic_write(
            audit_report_path,
            (json.dumps(audit, ensure_ascii=False, indent=2) + "\n").encode("utf-8"),
        )
    return audit
# //// /安装 CN iOS 语音覆盖链 ////


# //// 执行语音覆盖安装命令 [@x380kkm 2026-08-29] ////
def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cdn-root", required=True, type=Path)
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument("--override-root", type=Path)
    parser.add_argument("--audit-report", type=Path)
    arguments = parser.parse_args()
    audit = install_voice_archive(
        arguments.cdn_root.resolve(strict=True),
        arguments.report.resolve(strict=True),
        arguments.override_root.resolve() if arguments.override_root else None,
        arguments.audit_report.resolve() if arguments.audit_report else None,
    )
    print(json.dumps(audit, ensure_ascii=False, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except VoiceArchiveError as error:
        print(json.dumps({"error": str(error)}, ensure_ascii=False), file=os.sys.stderr)
        raise SystemExit(1) from None
# //// /执行语音覆盖安装命令 ////
