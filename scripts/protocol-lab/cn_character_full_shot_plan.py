# audience: internal
# # cn-character-full-shot-plan
#
# 此模块把 full-shot 映射解析为 CN entity CSV 和 archive 替换计划.
# 每个替换项使用当前实体校验值定位有效的客户端资源.

from __future__ import annotations

import base64
import csv
import hashlib
import os
import uuid
import zipfile
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path, PurePosixPath

from cn_character_full_shot_assets import (
    MAX_PNG_BYTES,
    PngInfo,
    PortraitMapping,
    PortraitStagingError,
    _target_asset_paths,
    hash_cn_asset_path,
    inspect_png,
    render_replacement,
)

ARCHIVE_DIRECTORIES = (
    "archive-common-full",
    "archive-medium-full",
    "archive-android-full",
    "archive-ios-full",
    "archive-common-diff",
    "archive-medium-diff",
    "archive-android-diff",
    "archive-ios-diff",
)


@dataclass(frozen=True)
class EntityRecord:
    csv_path: Path
    asset_path: str
    version: str
    size: int
    sha256_encoded: str
    category: str


@dataclass(frozen=True)
class PayloadArtifact:
    payload_path: Path
    sha256: str
    sha256_encoded: str
    size: int


@dataclass(frozen=True)
class ArchiveOccurrence:
    archive_path: Path
    asset_path: str
    payload: PayloadArtifact


def _encode_cn_sha256(digest: bytes) -> str:
    encoded = base64.b64encode(digest).decode("ascii").rstrip("=")
    return encoded.replace("+", "_").replace("/", "-")


def _spool_archive_entry(
    archive: zipfile.ZipFile,
    info: zipfile.ZipInfo,
    payload_root: Path,
) -> PayloadArtifact:
    temporary_path = payload_root / f".{uuid.uuid4().hex}.tmp"
    digest = hashlib.sha256()
    size = 0
    try:
        with archive.open(info, "r") as source, temporary_path.open("xb") as output:
            for block in iter(lambda: source.read(1024 * 1024), b""):
                size += len(block)
                if size > MAX_PNG_BYTES:
                    raise PortraitStagingError(
                        "archive_asset_too_large",
                        "CN archive target exceeds the configured PNG limit",
                        asset_path=info.filename,
                        size=size,
                    )
                digest.update(block)
                output.write(block)
        digest_bytes = digest.digest()
        digest_hex = digest_bytes.hex()
        payload_path = payload_root / digest_hex
        if payload_path.exists():
            if payload_path.stat().st_size != size:
                raise PortraitStagingError(
                    "spooled_archive_asset_collision",
                    "spooled CN archive targets share a digest but not a size",
                    asset_path=info.filename,
                )
        else:
            os.replace(temporary_path, payload_path)
        return PayloadArtifact(
            payload_path=payload_path,
            sha256=digest_hex,
            sha256_encoded=_encode_cn_sha256(digest_bytes),
            size=size,
        )
    finally:
        temporary_path.unlink(missing_ok=True)


# //// 定位带盐哈希对应的实体记录和 archive 条目 [@x380kkm 2026-08-21] ////
def collect_entity_records(
    cdn_root: Path, wanted_paths: set[str]
) -> dict[str, list[EntityRecord]]:
    records = {path: [] for path in wanted_paths}
    csv_paths = sorted(
        path
        for directory_name in ("entities", "EntityLists")
        for path in (cdn_root / directory_name).glob("*.csv")
        if path.is_file()
    )
    if not csv_paths:
        raise PortraitStagingError(
            "missing_entity_lists", "CN CDN has no entity CSV files"
        )
    for csv_path in csv_paths:
        try:
            text = csv_path.read_text(encoding="utf-8-sig")
        except (OSError, UnicodeError) as error:
            raise PortraitStagingError(
                "invalid_entity_list", "entity CSV cannot be read", path=str(csv_path)
            ) from error
        for line in text.splitlines():
            if not line:
                continue
            try:
                fields = next(csv.reader([line], strict=True))
            except csv.Error as error:
                raise PortraitStagingError(
                    "invalid_entity_list", "entity CSV contains an invalid row"
                ) from error
            if len(fields) < 5 or fields[0] not in records:
                continue
            try:
                size = int(fields[2])
            except ValueError as error:
                raise PortraitStagingError(
                    "invalid_entity_list", "entity CSV size is invalid"
                ) from error
            records[fields[0]].append(
                EntityRecord(
                    csv_path=csv_path,
                    asset_path=fields[0],
                    version=fields[1],
                    size=size,
                    sha256_encoded=fields[3],
                    category=fields[4],
                )
            )
    return {path: entries for path, entries in records.items() if entries}


def find_archive_occurrences(
    cdn_root: Path,
    wanted_paths: set[str],
    payload_root: Path,
) -> dict[str, list[ArchiveOccurrence]]:
    payload_root.mkdir(parents=True, exist_ok=True)
    occurrences = {path: [] for path in wanted_paths}
    archives = sorted(
        archive
        for directory in ARCHIVE_DIRECTORIES
        for archive in (cdn_root / directory).glob("*.zip")
        if archive.is_file()
    )
    if not archives:
        raise PortraitStagingError(
            "missing_archives", "CN CDN has no supported archive files"
        )
    for archive_path in archives:
        try:
            with zipfile.ZipFile(archive_path, "r") as archive:
                infos = archive.infolist()
                names = [info.filename for info in infos]
                if len(names) != len(set(names)):
                    raise PortraitStagingError(
                        "duplicate_archive_entry",
                        "CN archive contains duplicate entries",
                        archive=str(archive_path),
                    )
                for info in infos:
                    asset_path = info.filename
                    if asset_path not in wanted_paths:
                        continue
                    if info.file_size > MAX_PNG_BYTES:
                        raise PortraitStagingError(
                            "archive_asset_too_large",
                            "CN archive target exceeds the configured PNG limit",
                            archive=str(archive_path),
                            asset_path=asset_path,
                            size=info.file_size,
                        )
                    payload = _spool_archive_entry(archive, info, payload_root)
                    occurrences[asset_path].append(
                        ArchiveOccurrence(
                            archive_path=archive_path,
                            asset_path=asset_path,
                            payload=payload,
                        )
                    )
        except PortraitStagingError:
            raise
        except (OSError, RuntimeError, zipfile.BadZipFile) as error:
            raise PortraitStagingError(
                "invalid_archive",
                "CN archive cannot be read",
                archive=str(archive_path),
            ) from error
    return {path: entries for path, entries in occurrences.items() if entries}


def read_archive_occurrence(occurrence: ArchiveOccurrence) -> bytes:
    digest = hashlib.sha256()
    data = bytearray()
    try:
        with occurrence.payload.payload_path.open("rb") as stream:
            for block in iter(lambda: stream.read(1024 * 1024), b""):
                data.extend(block)
                digest.update(block)
    except OSError as error:
        raise PortraitStagingError(
            "spooled_archive_asset_unavailable",
            "spooled CN archive target cannot be read",
            asset_path=occurrence.asset_path,
        ) from error
    digest_bytes = digest.digest()
    if (
        len(data) != occurrence.payload.size
        or digest_bytes.hex() != occurrence.payload.sha256
        or _encode_cn_sha256(digest_bytes) != occurrence.payload.sha256_encoded
    ):
        raise PortraitStagingError(
            "spooled_archive_asset_mismatch",
            "spooled CN archive target no longer matches its recorded identity",
            asset_path=occurrence.asset_path,
        )
    return bytes(data)


# //// /定位带盐哈希对应的实体记录和 archive 条目 ////


# //// 构造并校验每个物理资源的替换数据 [@x380kkm 2026-08-21] ////
def _sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _sha256_cn_base64(data: bytes) -> str:
    return _encode_cn_sha256(hashlib.sha256(data).digest())


def _read_source(source_root: Path, logical_path: str) -> tuple[bytes, PngInfo]:
    source_path = source_root.joinpath(*PurePosixPath(logical_path).parts)
    try:
        resolved_root = source_root.resolve(strict=True)
        resolved_source = source_path.resolve(strict=True)
    except OSError as error:
        raise PortraitStagingError(
            "missing_source_portrait",
            "source portrait does not exist",
            logical_path=logical_path,
        ) from error
    if (
        not resolved_source.is_relative_to(resolved_root)
        or not resolved_source.is_file()
    ):
        raise PortraitStagingError(
            "invalid_source_portrait", "source portrait escapes the source root"
        )
    data = resolved_source.read_bytes()
    return data, inspect_png(data, logical_path)


def build_replacement_plan(
    cdn_root: Path,
    source_root: Path,
    mappings: Sequence[PortraitMapping],
    spool_root: Path,
) -> tuple[
    dict[Path, dict[str, PayloadArtifact]],
    dict[str, PayloadArtifact],
    list[dict[str, object]],
]:
    mapping_asset_paths = {
        mapping.target: _target_asset_paths(mapping.target) for mapping in mappings
    }
    wanted_paths = set().union(*mapping_asset_paths.values())
    entity_records = collect_entity_records(cdn_root, wanted_paths)
    occurrences = find_archive_occurrences(
        cdn_root,
        set(entity_records),
        spool_root / "originals",
    )

    archive_replacements: dict[Path, dict[str, PayloadArtifact]] = {}
    asset_replacements: dict[str, PayloadArtifact] = {}
    manifest_entries: list[dict[str, object]] = []
    for mapping in mappings:
        source_data, source_info = _read_source(source_root, mapping.source)
        target_paths = sorted(
            mapping_asset_paths[mapping.target].intersection(entity_records)
        )
        if not target_paths:
            raise PortraitStagingError(
                "target_not_in_entity_lists",
                "target full-shot is absent from CN entity lists",
                logical_path=mapping.target,
                asset_hash=hash_cn_asset_path(mapping.target),
            )
        variants: list[dict[str, object]] = []
        for target_path in target_paths:
            records = entity_records[target_path]
            entity_identities = {
                (record.size, record.sha256_encoded) for record in records
            }
            if len(entity_identities) != 1:
                raise PortraitStagingError(
                    "inconsistent_entity_records",
                    "entity CSV files disagree about a full-shot resource",
                    asset_path=target_path,
                )
            expected_size, expected_digest = next(iter(entity_identities))
            target_occurrences = occurrences.get(target_path, [])
            if not target_occurrences:
                raise PortraitStagingError(
                    "target_not_in_archives",
                    "target full-shot is absent from CN archives",
                    asset_path=target_path,
                )
            current = next(
                (
                    occurrence
                    for occurrence in target_occurrences
                    if occurrence.payload.size == expected_size
                    and occurrence.payload.sha256_encoded == expected_digest
                ),
                None,
            )
            if current is None:
                raise PortraitStagingError(
                    "entity_archive_mismatch",
                    "no archive entry matches the current entity checksum",
                    asset_path=target_path,
                )
            current_data = read_archive_occurrence(current)
            target_info = inspect_png(current_data, mapping.target)
            replacement = render_replacement(
                source_data,
                current_data,
                source_info,
                target_info,
            )
            replacement_info = inspect_png(replacement, mapping.target)
            if replacement_info != target_info:
                raise PortraitStagingError(
                    "replacement_format_mismatch",
                    "replacement does not match the target PNG resource shape",
                    asset_path=target_path,
                )
            replacement_sha256 = _sha256(replacement)
            replacement_path = spool_root / "replacements" / replacement_sha256
            replacement_path.parent.mkdir(parents=True, exist_ok=True)
            if not replacement_path.exists():
                replacement_path.write_bytes(replacement)
            artifact = PayloadArtifact(
                payload_path=replacement_path,
                sha256=replacement_sha256,
                sha256_encoded=_sha256_cn_base64(replacement),
                size=len(replacement),
            )
            asset_replacements[target_path] = artifact
            archive_paths: list[str] = []
            for occurrence in target_occurrences:
                relative_archive = occurrence.archive_path.relative_to(cdn_root)
                archive_replacements.setdefault(relative_archive, {})[target_path] = (
                    artifact
                )
                archive_paths.append(relative_archive.as_posix())
            variants.append(
                {
                    "asset_path": target_path,
                    "archives": sorted(set(archive_paths)),
                    "entity_files": sorted(
                        {
                            record.csv_path.relative_to(cdn_root).as_posix()
                            for record in records
                        }
                    ),
                    "entity_versions": sorted({record.version for record in records}),
                    "entity_categories": sorted(
                        {record.category for record in records}
                    ),
                    "original": {
                        "bytes": current.payload.size,
                        "height": target_info.height,
                        "mode": target_info.mode,
                        "resource_format": target_info.resource_format,
                        "sha256": current.payload.sha256,
                        "width": target_info.width,
                    },
                    "replacement": {
                        "bytes": artifact.size,
                        "height": replacement_info.height,
                        "mode": replacement_info.mode,
                        "resource_format": replacement_info.resource_format,
                        "sha256": artifact.sha256,
                        "width": replacement_info.width,
                    },
                }
            )
        manifest_entries.append(
            {
                "client_version": mapping.client_version,
                "fallback_reason": mapping.fallback_reason,
                "replacement_status": mapping.replacement_status,
                "source": mapping.source,
                "source_archive_entry": mapping.source_archive_entry,
                "source_png": {
                    "bytes": len(source_data),
                    "height": source_info.height,
                    "mode": source_info.mode,
                    "resource_format": source_info.resource_format,
                    "sha256": _sha256(source_data),
                    "width": source_info.width,
                },
                "source_region": mapping.source_region,
                "source_url": mapping.source_url,
                "target": mapping.target,
                "target_hash": hash_cn_asset_path(mapping.target),
                "variants": variants,
            }
        )
    return archive_replacements, asset_replacements, manifest_entries


# //// /构造并校验每个物理资源的替换数据 ////
