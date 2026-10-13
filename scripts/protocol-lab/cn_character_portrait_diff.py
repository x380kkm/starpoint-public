# audience: internal
# # cn-character-portrait-diff
# 角色立绘变更作为独立图片差分接续当前资源版本链.
# 各资源类别保留原 EntityLists 分类, 每个差分归档使用经典 ZIP 分档.

from __future__ import annotations

import json

from cn_gacha_banner_assets import EntityCatalog
from cn_resource_archive_parts import ResourceArchiveWriter, archive_part_records, install_archive_parts
from cn_voice_overlay_archive import atomic_write, build_archive_location, increment_version
from prepare_cn_small_resources import highest_version


# //// 将完整立绘替换集合加入图片增量归档 [@x380kkm 2026-09-27] ////
def publish_portrait_diff(cdn_root, replacements, spool_root):
    manifest_path = cdn_root / "path"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8-sig"))
    original_version = highest_version(manifest)
    target_version = increment_version(original_version)
    catalog = EntityCatalog.load(cdn_root, cdn_root / "entities/PathFile.csv")
    categories = {}
    for path, payload in replacements.items():
        category = catalog.records[path].asset_kind
        categories.setdefault(category, {})[path] = payload
    parts = []
    for category, payloads in sorted(categories.items()):
        directory = spool_root / ("diff-" + category)
        directory.mkdir()
        with ResourceArchiveWriter(directory) as writer:
            for path, payload in sorted(payloads.items()):
                writer.write(path, payload.payload_path.read_bytes())
        records = archive_part_records(
            writer.paths, original_version, target_version, "starpoint-jp-portraits-",
        )
        for record in records:
            record["relative_path"] = record["relative_path"].replace(
                "archive-common-diff/", f"archive-{category}-diff/", 1,
            )
        (cdn_root / f"archive-{category}-diff").mkdir(exist_ok=True)
        install_archive_parts(cdn_root, writer.paths, records)
        parts.extend(records)
    manifest["diff"].append({
        "version": target_version, "original_version": original_version,
        "archive": [{"location": build_archive_location(manifest, part["relative_path"]),
                     "size": part["byte_length"], "sha256": part["sha256"]} for part in parts],
    })
    manifest["info"]["target_asset_version"] = target_version
    manifest["info"]["eventual_target_asset_version"] = target_version
    atomic_write(manifest_path, json.dumps(manifest, ensure_ascii=False, separators=(",", ":")).encode("utf-8"))
    return {"original_version": original_version, "target_version": target_version,
            "asset_count": len(replacements), "archives": parts}
# //// /将完整立绘替换集合加入图片增量归档 ////
