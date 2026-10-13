# audience: internal
# # restore-cn-startup-images
# 此脚本从区域客户端归档按相同逻辑路径读取 PNG, 并接入两平台共用的 CN 差分资源链.
# discover 根据客户端缺图审计匹配区域归档, fetch 保存原图和来源, install 按 FileReader 规则发布 20 MiB 分档.
# 每张图片携带 regional-original 或 generated-substitute 来源, 同批图片共用差分版本.
# raw_entries 中的原始客户端文件与图片产物共同发布, payload_file 相对于准备清单.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

from __future__ import annotations

import argparse
import binascii
import json
from itertools import chain
import re
import tempfile
import zipfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from audit_cn_client_resource_resolution import is_scaled_asset
from cn_gacha_banner_assets import (
    EntityCatalog, EntityRecord, encode_entity_digest, hash_cn_asset_path, inspect_png, pseudo_png, standard_png,
)
from cn_voice_overlay_archive import (
    archive_groups, atomic_write, increment_version, read_json_object, render_entity_manifest,
)
from cn_regional_startup_images import discover_images, indexed_archive_members
from cn_resource_archive_parts import ResourceArchiveWriter, archive_part_records, install_archive_parts
from prepare_cn_small_resources import derive_small_image, highest_version, manifest_archive_records
from regional_zip_range import RemoteZip64Archive
from restore_cn_gacha_feature_images import entity_manifest_paths


# //// 缓存同逻辑路径的区域 PNG 和预览 [@x380kkm 2026-09-20] ////
def fetch_images(candidate_path, cache_root, output_directory):
    candidates = read_json_object(candidate_path)
    archive = RemoteZip64Archive(candidates["source"]["url"], cache_root)
    if candidates["source"].get("manifest_entry") or candidates.get("manifest_entry"):
        members = indexed_archive_members(archive, candidates["matches"], cache_root)
    else:
        entries = {entry.name: entry for entry in archive.entries}
        members = {item["logicalPath"]: {"archive": archive, "entry": entries[item["entry"]],
                                        "source_entry": item["entry"], "record": None}
                   for item in candidates["matches"]}
    output_directory.mkdir(parents=True, exist_ok=True)

    def fetch_image(candidate):
        logical = candidate["logicalPath"]
        digest = hash_cn_asset_path(logical)
        suffix = "/" + digest[:2] + "/" + digest[2:]
        member = members[logical]
        entry = member["entry"]
        if not entry.name.endswith(suffix):
            raise ValueError(f"区域资源与逻辑路径不一致: {logical}")
        path = output_directory / (digest + ".png")
        if path.is_file():
            data = path.read_bytes()
            payload = pseudo_png(data)
        else:
            payload = member["archive"].read(entry)
            data = standard_png(payload)
        record = member["record"]
        if record is not None and (len(payload) != int(record[2]) or encode_entity_digest(payload) != record[3]):
            raise ValueError(f"原图内容与 EntityList 不符: {logical}")
        dimensions = inspect_png(data)
        if not path.is_file():
            atomic_write(path, data)
        print(json.dumps({"fetched": logical, "dimensions": dimensions}, ensure_ascii=False), flush=True)
        return {"logical_path": logical, "preview": str(path.resolve()), "dimensions": dimensions,
                "source_entry": member["source_entry"], "owners": candidate["owners"],
                "provenance": "regional-original"}

    with ThreadPoolExecutor(max_workers=4) as executor:
        assets = list(executor.map(fetch_image, candidates["matches"]))
    report = {"source": candidates["source"], "provenance": "regional-original", "assets": assets,
              "remaining_missing": candidates["missing"], "candidate_report": str(candidate_path)}
    path = output_directory / "prepared.json"
    atomic_write(path, (json.dumps(report, ensure_ascii=False, indent=2) + "\n").encode("utf-8"))
    return {"prepared": str(path), "asset_count": len(assets)}
# //// /缓存同逻辑路径的区域 PNG 和预览 ////


# //// 按客户端规格生成图像资源 [@x380kkm 2026-09-20] ////
def image_payloads(asset):
    logical = asset["logical_path"]
    data = pseudo_png(Path(asset["preview"]).read_bytes())
    if list(inspect_png(data)) != asset["dimensions"]:
        raise ValueError(f"PNG 尺寸与准备记录不一致: {logical}")
    digest = hash_cn_asset_path(logical)
    suffix = digest[:2] + "/" + digest[2:]
    if is_scaled_asset(logical):
        small, _, _ = derive_small_image(data)
        return [("production/medium_upload/" + suffix, data), ("production/small_upload/" + suffix, small)]
    return [("production/upload/" + suffix, data)]


# //// 按未压缩成员识别已安装的同内容归档 [@x380kkm 2026-09-20] ////
def find_installed_archives(cdn_root, manifest, entities, contents):
    if not entities:
        return None
    for path in entity_manifest_paths(cdn_root):
        catalog = EntityCatalog.load(cdn_root, path)
        if any(catalog.records.get(entity.entry_path) != entity for entity in entities):
            return None
    for group in archive_groups(manifest):
        found = {}
        parts = []
        for archive in group["archive"]:
            relative = "/".join(archive["location"].rsplit("/", 2)[-2:])
            path = cdn_root / relative
            if not path.name.startswith("starpoint-startup-") or not path.is_file():
                continue
            with zipfile.ZipFile(path) as existing:
                found.update({item.filename: (item.file_size, item.CRC) for item in existing.infolist()})
            parts.append({"relative_path": relative, "byte_length": path.stat().st_size, "sha256": archive["sha256"]})
        if contents and all(found.get(path) == content for path, content in contents.items()):
            return group["original_version"], group["version"], parts
    return None


# //// 解析图片级来源与整批默认来源 [@x380kkm 2026-09-21] ////
def prepared_records(prepared, name):
    records = [{**record, "provenance": record.get("provenance", prepared.get("provenance", "regional-original"))}
               for record in prepared.get(name, [])]
    for record in records:
        if record["provenance"] not in ("regional-original", "derived-substitute", "generated-substitute"):
            raise ValueError(f"资源来源必须是 regional-original, derived-substitute 或 generated-substitute: {record.get('logical_path', record.get('entry_path'))}")
    return records
# //// /解析图片级来源与整批默认来源 ////


# //// 读取客户端散列路径下的原始文件 [@x380kkm 2026-09-21] ////
def raw_payload(record, directory):
    entry = record["entry_path"]
    if not isinstance(entry, str) or re.fullmatch(r"production/(?:upload|medium_upload|small_upload|android_upload|ios_upload)/[0-9a-f]{2}/[0-9a-f]{38}", entry) is None:
        raise ValueError(f"原始资源路径必须是 production 上传目录下的散列路径: {entry}")
    return entry, (directory / record["payload_file"]).read_bytes()
# //// /读取客户端散列路径下的原始文件 ////


# //// 合并图片与原始文件并拒绝同路径内容冲突 [@x380kkm 2026-09-21] ////
def merge_prepared_payloads(assets, raw_entries, directory):
    images = (entry for asset in assets for entry in image_payloads(asset))
    originals = (raw_payload(record, directory) for record in raw_entries)
    payloads = {}
    for entry, data in chain(images, originals):
        if entry in payloads and payloads[entry] != data:
            raise ValueError(f"同一客户端资源路径包含不同内容: {entry}")
        payloads[entry] = data
    return sorted(payloads.items())
# //// /合并图片与原始文件并拒绝同路径内容冲突 ////


# //// 将准备图片接入共用差分版本链 [@x380kkm 2026-09-20] ////
def install_images(cdn_root, prepared_path, report_path):
    prepared = read_json_object(prepared_path)
    assets = prepared_records(prepared, "assets")
    raw_entries = prepared_records(prepared, "raw_entries")
    manifest_path = cdn_root / "path"
    original_path = manifest_path.read_bytes()
    manifest = read_json_object(manifest_path)
    payloads = merge_prepared_payloads(assets, raw_entries, prepared_path.parent)
    entities = [EntityRecord(entry, len(data), encode_entity_digest(data), "common") for entry, data in payloads]
    contents = {entry: (len(data), binascii.crc32(data) & 0xFFFFFFFF) for entry, data in payloads}
    original_version = highest_version(manifest)
    target_version = original_version
    parts = []
    existing = find_installed_archives(cdn_root, manifest, entities, contents)
    if existing is not None:
        original_version, target_version, parts = existing
    manifest_paths = entity_manifest_paths(cdn_root)
    if existing is None and payloads:
        target_version = increment_version(original_version)
        with tempfile.TemporaryDirectory(prefix="startup-images-") as directory:
            with ResourceArchiveWriter(Path(directory)) as archive:
                for entry, data in payloads:
                    archive.write(entry, data)
            parts = archive_part_records(archive.paths, original_version, target_version, "starpoint-startup-images-")
            manifest["diff"].append({"version": target_version, "original_version": original_version,
                                     "archive": manifest_archive_records(manifest, parts)})
            manifest["info"]["target_asset_version"] = target_version
            manifest["info"]["eventual_target_asset_version"] = target_version
            manifests = {path: render_entity_manifest(path, target_version, entities) for path in manifest_paths}
            if manifest_path.read_bytes() != original_path:
                raise RuntimeError("资源版本链在图片发布期间发生变化")
            install_archive_parts(cdn_root, archive.paths, parts)
            for path, data in manifests.items():
                atomic_write(path, data)
            atomic_write(manifest_path, json.dumps(manifest, ensure_ascii=False, separators=(",", ":")).encode("utf-8"))
    provenances = {record["provenance"] for record in [*assets, *raw_entries]}
    report = {
        "source": next(iter(provenances)) if len(provenances) == 1 else "mixed-resource-batch",
        "source_archive": prepared["source"], "status": "installed",
        "original_version": original_version, "target_version": target_version,
        "archives": parts, "archive_count": len(parts), "entry_count": len(entities),
        "archive_byte_length": sum(part["byte_length"] for part in parts),
        "assets": [{**asset, "entry_paths": [entry for entry, _ in image_payloads(asset)]}
                   for asset in assets],
        "raw_entries": raw_entries, "raw_entry_count": len(raw_entries),
        "resolutions": ["medium", "small"], "platforms": ["ios", "android"],
        "entity_manifests": [str(path.relative_to(cdn_root)) for path in manifest_paths],
        "remaining_missing": prepared["remaining_missing"],
        "missing_dynamic_png_fallback": "dynamic/error/not_found.png",
        "reused": existing is not None or not payloads,
    }
    atomic_write(report_path, (json.dumps(report, ensure_ascii=False, indent=2) + "\n").encode("utf-8"))
    return {"report": str(report_path), "asset_count": len(assets), "raw_entry_count": len(raw_entries),
            "original_version": original_version, "target_version": target_version, "archives": parts,
            "reused": report["reused"]}


# //// 执行图片准备或资源安装 [@x380kkm 2026-09-20] ////
def main():
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest="command", required=True)
    discover = commands.add_parser("discover", help="从客户端缺图审计和区域归档生成恢复候选")
    discover.add_argument("--reference-report", type=Path, required=True)
    discover.add_argument("--archive-url", required=True)
    discover.add_argument("--cache-root", type=Path, required=True)
    discover.add_argument("--output", type=Path, required=True)
    discover.add_argument("--entity-entry", help="归档包含多份 EntityList 时指定清单路径")
    fetch = commands.add_parser("fetch")
    fetch.add_argument("--candidates", type=Path, required=True)
    fetch.add_argument("--cache-root", type=Path, required=True)
    fetch.add_argument("--output", type=Path, required=True)
    install = commands.add_parser("install")
    install.add_argument("--cdn-root", type=Path, required=True)
    install.add_argument("--prepared", type=Path, required=True)
    install.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "discover":
        result = discover_images(args.reference_report, args.archive_url, args.cache_root, args.output, args.entity_entry)
    elif args.command == "fetch":
        result = fetch_images(args.candidates, args.cache_root, args.output)
    else:
        result = install_images(args.cdn_root.resolve(strict=True), args.prepared, args.report)
    print(json.dumps(result, ensure_ascii=False))


if __name__ == "__main__":
    main()
# //// /执行图片准备或资源安装 ////
