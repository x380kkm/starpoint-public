# audience: internal
# # install-cn-gacha-atlas-recovery
# 此脚本把已配对的区域 atlas 接入 CN 共用差分链, 保持图片与 master 内容原样.
# 来源报告包含当前图片身份和动画帧名, 发布前核对目标资源与报告的实际字节.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

from __future__ import annotations

import argparse
import binascii
import json
import tempfile
from pathlib import Path

from cn_gacha_banner_assets import EntityCatalog, EntityRecord, encode_entity_digest, hash_cn_asset_path, read_logical_assets
from cn_resource_archive_parts import ResourceArchiveWriter, archive_part_records, install_archive_parts
from cn_voice_overlay_archive import atomic_write, increment_version, render_entity_manifest
from prepare_cn_small_resources import highest_version, manifest_archive_records
from restore_cn_gacha_feature_images import entity_manifest_paths


# //// 读取经来源配对的 atlas 原始文件 [@x380kkm 2026-09-21] ////
def prepared_assets(reports: list[Path]) -> tuple[list[dict], dict[str, tuple[dict, bytes]]]:
    documents = [json.loads(path.read_text(encoding="utf-8")) for path in reports]
    assets = {}
    for document in documents:
        for record in document["prepared"]:
            if record["status"] != "prepared":
                raise ValueError(f"图集来源契约尚未匹配: {record['logicalPath']}")
            logical = record["logicalPath"]
            data = Path(record["file"]).read_bytes()
            if logical in assets and assets[logical][1] != data:
                raise ValueError(f"区域来源给出不同图集: {logical}")
            assets[logical] = (record, data)
    return documents, assets


# //// 将同源图集作为一个版本组发布 [@x380kkm 2026-09-21] ////
def install(cdn_root: Path, prepared: list[Path], report_path: Path) -> dict:
    documents, assets = prepared_assets(prepared)
    path_file = cdn_root / "path"
    before = path_file.read_bytes()
    manifest = json.loads(before)
    catalog = EntityCatalog.load(cdn_root)
    payloads = {}
    entities = []
    for logical, (_, data) in assets.items():
        digest = hash_cn_asset_path(logical)
        entry = "production/upload/" + digest[:2] + "/" + digest[2:]
        payloads[entry] = data
        entities.append(EntityRecord(entry, len(data), encode_entity_digest(data), "common"))
    reused = all(catalog.records.get(record.entry_path) == record for record in entities)
    original_version = highest_version(manifest)
    target_version = original_version
    parts = []
    if not reused:
        current, missing = read_logical_assets(cdn_root, {logical: catalog.find(logical) for logical in assets})
        if missing:
            raise ValueError(f"目标 CDN 缺少待修复图集: {missing}")
        for logical, (record, _) in assets.items():
            data = current[logical]
            if (len(data), binascii.crc32(data) & 0xFFFFFFFF) != (record["current"]["bytes"], record["current"]["crc"]):
                raise ValueError(f"目标图集与审计输入不同: {logical}")
        target_version = increment_version(original_version)
        with tempfile.TemporaryDirectory(prefix="gacha-atlas-") as temporary:
            with ResourceArchiveWriter(Path(temporary)) as archive:
                for entry, data in sorted(payloads.items()):
                    archive.write(entry, data)
            parts = archive_part_records(archive.paths, original_version, target_version, "starpoint-gacha-atlas-")
            manifest["diff"].append({"original_version": original_version, "version": target_version,
                                     "archive": manifest_archive_records(manifest, parts)})
            manifest["info"]["target_asset_version"] = target_version
            manifest["info"]["eventual_target_asset_version"] = target_version
            manifests = {path: render_entity_manifest(path, target_version, entities) for path in entity_manifest_paths(cdn_root)}
            if path_file.read_bytes() != before:
                raise RuntimeError("源 CDN 在图集发布期间发生变化")
            install_archive_parts(cdn_root, archive.paths, parts)
            for path, data in manifests.items():
                atomic_write(path, data)
            atomic_write(path_file, json.dumps(manifest, ensure_ascii=False, separators=(",", ":")).encode("utf-8"))
    report = {"originalVersion": original_version, "targetVersion": target_version, "assetCount": len(assets),
              "reused": reused, "archives": parts, "sources": [{"source": item["source"], "checked": item["checkedAtlasCount"],
                  "prepared": len(item["prepared"]), "unmatched": item["unmatched"]} for item in documents],
              "assets": [{"logicalPath": logical, "source": record["source_entry"], "bytes": len(data),
                          "owners": record["owners"]} for logical, (record, data) in assets.items()]}
    atomic_write(report_path, (json.dumps(report, ensure_ascii=False, indent=2) + "\n").encode("utf-8"))
    return {key: report[key] for key in ("originalVersion", "targetVersion", "assetCount", "reused", "archives")}
# //// /将同源图集作为一个版本组发布 ////


# //// 执行图集发布 [@x380kkm 2026-09-21] ////
def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cdn-root", type=Path, required=True)
    parser.add_argument("--prepared", type=Path, action="append", required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(install(args.cdn_root, args.prepared, args.report), ensure_ascii=False))


if __name__ == "__main__":
    main()
# //// /执行图集发布 ////
