# audience: internal
# # prepare-cn-small-resources
#
# Small 图片由当前 CN Medium 图片等比缩放得到, 共享带盐逻辑路径和美术内容.
# 图片尺寸遵循原包配对资源的 7/10 最近整数规则, 半数向上取整.
# 每个 common 差分归档约 20 MiB, 各平台 EntityLists 共享图片内容.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

from __future__ import annotations

import argparse
import io
import json
import os
import tempfile
import zipfile
from collections import deque
from concurrent.futures import Future, ThreadPoolExecutor
from pathlib import Path
from typing import Any, Iterator

from PIL import Image

from cn_gacha_banner_assets import (
    ARCHIVE_SUFFIXES,
    MAX_ASSET_BYTES,
    EntityCatalog,
    EntityRecord,
    encode_entity_digest,
    pseudo_png,
    standard_png,
)
from cn_voice_overlay_archive import (
    archive_groups,
    atomic_write,
    build_archive_location,
    current_target_version,
    increment_version,
    read_json_object,
    render_entity_manifest,
    version_key,
)
from restore_cn_gacha_feature_images import entity_manifest_paths
from cn_resource_archive_parts import (
    ARCHIVE_DIRECTORY,
    MAX_ARCHIVE_BYTES,
    ResourceArchiveWriter,
    archive_part_records,
    install_archive_parts,
)


MEDIUM_PREFIX = "production/medium_upload/"
SMALL_PREFIX = "production/small_upload/"
ARCHIVE_NAME_PREFIX = "starpoint-cn-small-"


# //// 读取各平台共同的当前 Medium 资源 [@x380kkm 2026-09-20] ////
def medium_records(catalog: EntityCatalog) -> dict[str, EntityRecord]:
    return {
        path: record
        for path, record in catalog.records.items()
        if path.startswith(MEDIUM_PREFIX)
    }


def load_medium_records(cdn_root: Path) -> dict[str, EntityRecord]:
    records = medium_records(EntityCatalog.load(cdn_root))
    if not records:
        raise RuntimeError("CN EntityLists 缺少 Medium 图片")
    for path in entity_manifest_paths(cdn_root):
        platform_records = medium_records(EntityCatalog.load(cdn_root, path))
        if platform_records != records:
            raise RuntimeError(f"各平台的 Medium 资源内容存在差异: {path}")
    return records


def read_current_images(
    cdn_root: Path, records: dict[str, EntityRecord], missing: set[str]
) -> Iterator[tuple[EntityRecord, bytes]]:
    for kind in sorted({record.asset_kind for record in records.values()}):
        for suffix in ARCHIVE_SUFFIXES:
            for path in sorted((cdn_root / f"archive-{kind}-{suffix}").glob("*.zip")):
                if not missing:
                    return
                with zipfile.ZipFile(path) as archive:
                    for info in archive.infolist():
                        if info.filename not in missing:
                            continue
                        record = records[info.filename]
                        if info.file_size != record.byte_length:
                            continue
                        if info.file_size > MAX_ASSET_BYTES:
                            raise RuntimeError(f"Medium 图片超过资源读取限制: {info.filename}")
                        data = archive.read(info)
                        if encode_entity_digest(data) != record.digest:
                            continue
                        missing.remove(info.filename)
                        yield record, data
# //// /读取各平台共同的当前 Medium 资源 ////


# //// 按原包尺寸规则派生 CN Small 图片 [@x380kkm 2026-09-20] ////
def derive_small_image(data: bytes) -> tuple[bytes, tuple[int, int], tuple[int, int]]:
    with Image.open(io.BytesIO(standard_png(data))) as medium:
        medium.load()
        if medium.format != "PNG" or getattr(medium, "n_frames", 1) != 1:
            raise ValueError("Medium 资源必须是单帧 PNG")
        medium_size = medium.size
        small_size = tuple(max(1, (length * 7 + 5) // 10) for length in medium_size)
        with medium.convert("RGBA") as rgba:
            with rgba.resize(small_size, Image.Resampling.LANCZOS) as small:
                output = io.BytesIO()
                small.save(output, format="PNG")
    return pseudo_png(output.getvalue()), medium_size, small_size
# //// /按原包尺寸规则派生 CN Small 图片 ////


# //// 流式写入 Small 差分并记录解码结果 [@x380kkm 2026-09-20] ////
def write_small_archives(
    cdn_root: Path,
    directory: Path,
    records: dict[str, EntityRecord],
    workers: int,
    report: dict[str, Any],
) -> tuple[list[EntityRecord], list[Path]]:
    missing = set(records)
    entities: list[EntityRecord] = []
    jobs: deque[tuple[EntityRecord, Future]] = deque()

    def consume(archive: ResourceArchiveWriter) -> None:
        record, future = jobs.popleft()
        try:
            data, medium_size, small_size = future.result()
        except (OSError, ValueError, SyntaxError, RuntimeError) as error:
            report["decode_failures"].append({"entry_path": record.entry_path, "error": str(error)})
            return
        entry_path = SMALL_PREFIX + record.entry_path.removeprefix(MEDIUM_PREFIX)
        archive.write(entry_path, data)
        entity = EntityRecord(entry_path, len(data), encode_entity_digest(data), "common")
        entities.append(entity)
        report["image_byte_length"] += len(data)
        if len(report["samples"]) < 20:
            report["samples"].append({
                "medium_entry": record.entry_path,
                "small_entry": entry_path,
                "medium_size": medium_size,
                "small_size": small_size,
            })
        if len(entities) % 500 == 0:
            print(json.dumps({"generated": len(entities), "expected": len(records)}), flush=True)

    with ResourceArchiveWriter(directory) as archive:
        with ThreadPoolExecutor(max_workers=workers) as executor:
            for record, data in read_current_images(cdn_root, records, missing):
                jobs.append((record, executor.submit(derive_small_image, data)))
                if len(jobs) >= workers * 2:
                    consume(archive)
            while jobs:
                consume(archive)
    report["missing_medium_entries"] = sorted(missing)
    report["generated_count"] = len(entities)
    if missing or report["decode_failures"]:
        raise RuntimeError("Small 资源生成未覆盖全部当前 Medium 图片")
    return entities, archive.paths
# //// /流式写入 Small 差分并记录解码结果 ////


# //// 将独立 common 差分接入资源版本链 [@x380kkm 2026-09-20] ////
def highest_version(manifest: dict[str, Any]) -> str:
    versions = [current_target_version(manifest)]
    for group in archive_groups(manifest):
        versions.append(group["version"])
        if group.get("original_version"):
            versions.append(group["original_version"])
    return max(versions, key=version_key)


def existing_small_archives(
    cdn_root: Path, manifest: dict[str, Any], proposed: list[dict], entities: list[EntityRecord]
) -> list[dict] | None:
    for group in archive_groups(manifest):
        parts = []
        for archive in group["archive"]:
            location = archive.get("location", "")
            relative = "/".join(location.rsplit("/", 2)[-2:])
            if not Path(relative).name.startswith(ARCHIVE_NAME_PREFIX):
                continue
            parts.append({"relative_path": relative, "byte_length": archive["size"], "sha256": archive["sha256"]})
        if [(p["byte_length"], p["sha256"]) for p in parts] != [(p["byte_length"], p["sha256"]) for p in proposed]:
            continue
        if not all((cdn_root / p["relative_path"]).is_file()
                   and (cdn_root / p["relative_path"]).stat().st_size == p["byte_length"] for p in parts):
            continue
        if all(all(catalog.records.get(entity.entry_path) == entity for entity in entities)
               for catalog in (EntityCatalog.load(cdn_root, path) for path in entity_manifest_paths(cdn_root))):
            return parts
    return None


def manifest_archive_records(manifest: dict, parts: list[dict]) -> list[dict]:
    return [{"location": build_archive_location(manifest, part["relative_path"]),
             "size": part["byte_length"], "sha256": part["sha256"]} for part in parts]


def record_archive_parts(report: dict, parts: list[dict]) -> None:
    report.pop("archive", None)
    report.pop("archive_sha256", None)
    report.update({"archives": parts, "archive_count": len(parts),
                   "archive_byte_length": sum(part["byte_length"] for part in parts),
                   "maximum_archive_byte_length": max(part["byte_length"] for part in parts),
                   "archive_size_limit": MAX_ARCHIVE_BYTES})


def publish_small_archives(
    cdn_root: Path, archive_paths: list[Path], entities: list[EntityRecord], report: dict[str, Any]
) -> None:
    manifest_path = cdn_root / "path"
    original_path = manifest_path.read_bytes()
    manifest = read_json_object(manifest_path)
    original_version = highest_version(manifest)
    target_version = increment_version(original_version)
    parts = archive_part_records(archive_paths, original_version, target_version, ARCHIVE_NAME_PREFIX)
    existing = existing_small_archives(cdn_root, manifest, parts, entities)
    if existing is not None:
        record_archive_parts(report, existing)
        report.update({"status": "unchanged", "target_version": current_target_version(manifest)})
        return
    manifest["diff"].append({"version": target_version, "original_version": original_version,
                             "archive": manifest_archive_records(manifest, parts)})
    manifest["info"]["target_asset_version"] = target_version
    manifest["info"]["eventual_target_asset_version"] = target_version
    manifests = {
        path: render_entity_manifest(path, target_version, entities)
        for path in entity_manifest_paths(cdn_root)
    }
    if manifest_path.read_bytes() != original_path:
        raise RuntimeError("资源版本链在 Small 发布期间发生变化")
    install_archive_parts(cdn_root, archive_paths, parts)
    for path, data in manifests.items():
        atomic_write(path, data)
    atomic_write(manifest_path, json.dumps(manifest, ensure_ascii=False, separators=(",", ":")).encode("utf-8"))
    record_archive_parts(report, parts)
    report.update({
        "status": "installed",
        "original_version": original_version,
        "target_version": target_version,
        "entity_manifests": [str(path.relative_to(cdn_root)) for path in manifests],
    })
# //// /将独立 common 差分接入资源版本链 ////


# //// 将现有 Small 图片流式重分档并保持版本和实体内容 [@x380kkm 2026-09-20] ////
def repack_small_archives(cdn_root: Path, directory: Path, report: dict) -> None:
    source_relative = report["archive"]
    source_path = (cdn_root / source_relative).resolve(strict=True)
    if not source_path.is_relative_to(cdn_root) or not source_path.name.startswith(ARCHIVE_NAME_PREFIX):
        raise ValueError("Small 源归档路径超出 CDN 范围")
    manifest_path = cdn_root / "path"
    original_path = manifest_path.read_bytes()
    manifest = read_json_object(manifest_path)
    groups = [group for group in manifest["diff"] if group["version"] == report["target_version"]
              and group["original_version"] == report["original_version"]]
    if len(groups) != 1:
        raise ValueError("Small 源归档版本组无法唯一定位")
    group = groups[0]
    matches = [entry for entry in group["archive"] if entry["location"].endswith("/" + source_relative)]
    if len(matches) != 1 or source_path.stat().st_size != matches[0]["size"]:
        raise ValueError("Small 源归档与版本组不一致")
    expected = {path: record for path, record in EntityCatalog.load(cdn_root).records.items()
                if path.startswith(SMALL_PREFIX)}
    count = 0
    with ResourceArchiveWriter(directory) as output, zipfile.ZipFile(source_path) as source:
        for entry in source.infolist():
            record = expected.pop(entry.filename, None)
            if record is None or entry.file_size != record.byte_length:
                raise ValueError(f"Small 源归档与 EntityLists 不一致: {entry.filename}")
            data = source.read(entry)
            if encode_entity_digest(data) != record.digest:
                raise ValueError(f"Small 图片内容与 EntityLists 不一致: {entry.filename}")
            output.write(entry.filename, data)
            count += 1
            if count % 1000 == 0:
                print(json.dumps({"repacked": count, "expected": report["generated_count"]}), flush=True)
    if expected or count != report["generated_count"]:
        raise ValueError("Small 重分档未覆盖全部现有图片")
    parts = archive_part_records(output.paths, report["original_version"], report["target_version"], ARCHIVE_NAME_PREFIX)
    replacement = manifest_archive_records(manifest, parts)
    group["archive"] = [item for entry in group["archive"] for item in (replacement if entry is matches[0] else [entry])]
    if manifest_path.read_bytes() != original_path:
        raise RuntimeError("资源版本链在 Small 重分档期间发生变化")
    install_archive_parts(cdn_root, output.paths, parts)
    atomic_write(manifest_path, json.dumps(manifest, ensure_ascii=False, separators=(",", ":")).encode("utf-8"))
    record_archive_parts(report, parts)
    report.update({"status": "installed", "source_archive": source_relative, "repacked_count": count})
# //// /将现有 Small 图片流式重分档并保持版本和实体内容 ////


# //// 执行 Small 图片生成并输出审计结果 [@x380kkm 2026-09-20] ////
def main() -> int:
    parser = argparse.ArgumentParser(description="从当前 CN Medium 图片生成同源 Small 资源")
    parser.add_argument("--cdn-root", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--repack-report", type=Path)
    parser.add_argument("--workers", type=int, default=min(8, os.cpu_count() or 1))
    args = parser.parse_args()
    if args.workers < 1:
        parser.error("workers 必须大于 0")
    cdn_root = args.cdn_root.resolve(strict=True)
    report: dict[str, Any] = {
        "status": "generating",
        "source": "current-cn-medium-resolution-derivative",
        "scale_numerator": 7,
        "scale_denominator": 10,
        "rounding": "nearest-half-up",
        "image_byte_length": 0,
        "decode_failures": [],
        "samples": [],
    }
    if args.repack_report is not None:
        report = read_json_object(args.repack_report)
    try:
        archive_root = cdn_root / ARCHIVE_DIRECTORY
        archive_root.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix=".small-", dir=archive_root) as directory:
            if args.repack_report is not None:
                repack_small_archives(cdn_root, Path(directory), report)
            else:
                records = load_medium_records(cdn_root)
                report["medium_count"] = len(records)
                entities, archive_paths = write_small_archives(cdn_root, Path(directory), records, args.workers, report)
                if load_medium_records(cdn_root) != records:
                    raise RuntimeError("Medium 资源在 Small 生成期间发生变化")
                publish_small_archives(cdn_root, archive_paths, entities, report)
    except (OSError, ValueError, RuntimeError, zipfile.BadZipFile, zipfile.LargeZipFile) as error:
        report.update({"status": "failed", "error": str(error)})
    atomic_write(args.report, json.dumps(report, ensure_ascii=False, indent=2).encode("utf-8"))
    print(json.dumps({key: value for key, value in report.items() if key not in {"samples", "archives"}}, ensure_ascii=False), flush=True)
    return int(report["status"] == "failed")


if __name__ == "__main__":
    raise SystemExit(main())
# //// /执行 Small 图片生成并输出审计结果 ////
