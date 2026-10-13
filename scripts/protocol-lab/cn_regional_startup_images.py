# audience: internal
# # cn-regional-startup-images
# 此模块以客户端审计中的逻辑路径定位区域原图, 支持展开缓存和 EntityList 索引的嵌套资源 ZIP.
# 候选记录保留区域归档和 EntityList 来源, 图片内容由原始资源摘要约束.

from __future__ import annotations

import csv
import io
import json
import re
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

from cn_gacha_banner_assets import hash_cn_asset_path
from cn_voice_overlay_archive import atomic_write, read_json_object
from regional_zip_range import RemoteZip64Archive


# //// 收集按逻辑路径去重的缺失 PNG [@x380kkm 2026-09-20] ////
def missing_image_owners(report: dict) -> dict[str, list[str]]:
    owners: dict[str, set[str]] = {}
    for item in report["missing"]:
        logical = item["logicalPath"]
        if logical.endswith(".png"):
            owners.setdefault(logical, set()).update(item["owners"])
    return {logical: sorted(values) for logical, values in owners.items()}


# //// 从指定 EntityList 定位原图记录 [@x380kkm 2026-09-20] ////
def find_manifest_images(archive, manifest_entry, owners):
    entry = next(entry for entry in archive.entries if entry.name == manifest_entry)
    records = {}
    for row in csv.reader(io.StringIO(archive.read(entry).decode("utf-8-sig"))):
        if len(row) >= 5:
            records[row[0]] = row[:5]
    matches = []
    for logical, owner_list in owners.items():
        digest = hash_cn_asset_path(logical)
        record = records.get("production/upload/" + digest[:2] + "/" + digest[2:])
        if record is not None:
            matches.append({"logicalPath": logical, "record": record, "owners": owner_list})
    return matches


# //// 从展开缓存定位同路径原图 [@x380kkm 2026-09-20] ////
def find_cached_images(archive, owners):
    entries = {entry.name.rsplit("/", 2)[-2] + "/" + entry.name.rsplit("/", 1)[-1]: entry
               for entry in archive.entries if "/production/upload/" in entry.name}
    matches = []
    for logical, owner_list in owners.items():
        digest = hash_cn_asset_path(logical)
        entry = entries.get(digest[:2] + "/" + digest[2:])
        if entry is not None:
            matches.append({"logicalPath": logical, "entry": entry.name,
                            "size": entry.uncompressed_size, "owners": owner_list})
    return matches


# //// 生成缺图审计对应的区域恢复候选 [@x380kkm 2026-09-20] ////
def discover_images(reference_report, archive_url, cache_root, output, entity_entry=None):
    owners = missing_image_owners(read_json_object(reference_report))
    archive = RemoteZip64Archive(archive_url, cache_root)
    entity_entries = [entry.name for entry in archive.entries
                      if "/entities/" in entry.name and entry.name.endswith(".csv")]
    if entity_entry is None and len(entity_entries) > 1:
        raise ValueError("归档包含多份 EntityList, 请用 --entity-entry 选择: " + ", ".join(entity_entries))
    if entity_entry is None and entity_entries:
        entity_entry = entity_entries[0]
    source = {"url": archive.url, "archive_size": archive.metrics.archive_size,
              "directory_offset": archive.metrics.directory_offset, "directory_size": archive.metrics.directory_size}
    if entity_entry is not None:
        if entity_entry not in entity_entries:
            raise ValueError(f"归档内没有指定 EntityList: {entity_entry}")
        source["manifest_entry"] = entity_entry
        matches = find_manifest_images(archive, entity_entry, owners)
    else:
        matches = find_cached_images(archive, owners)
    matched = {item["logicalPath"] for item in matches}
    report = {"source": source, "matches": matches, "missing": sorted(set(owners) - matched),
              "reference_report": str(reference_report)}
    atomic_write(output, (json.dumps(report, ensure_ascii=False, indent=2) + "\n").encode("utf-8"))
    return {"report": str(output), "matched": len(matches), "missing": len(report["missing"])}


# //// 读取 EntityList 指定版本的资源 ZIP 中央目录 [@x380kkm 2026-09-20] ////
def indexed_archive_members(parent, matches, cache_root: Path):
    records = {item["record"][0]: item for item in matches}
    archive_specs = {(item["record"][1], item["record"][4]) for item in matches}
    entries = []
    for entry in parent.entries:
        filename = entry.name.rsplit("/", 1)[-1]
        version = re.fullmatch(r"(?:s_)?asset-[\d.]+-([\d.]+)-\d+-[a-f0-9]+\.zip", filename)
        group = re.search(r"/archive-([^/]+)-diff/", entry.name)
        if version and group and (version[1], group[1]) in archive_specs:
            entries.append((entry, version[1], group[1]))

    def index_archive(spec):
        entry, version, group = spec
        archive = RemoteZip64Archive.from_stored_member(parent, entry, cache_root)
        members = [(member, records[member.name]) for member in archive.entries
                   if member.name in records and records[member.name]["record"][1] == version
                   and records[member.name]["record"][4] == group]
        return entry, archive, members

    found = {}
    with ThreadPoolExecutor(max_workers=6) as executor:
        futures = [executor.submit(index_archive, entry) for entry in entries]
        for completed, future in enumerate(as_completed(futures), 1):
            outer, archive, members = future.result()
            for entry, match in members:
                if entry.uncompressed_size == int(match["record"][2]):
                    found.setdefault(match["logicalPath"], {"archive": archive, "entry": entry,
                                     "source_entry": outer.name + "!" + entry.name, "record": match["record"]})
            if completed % 10 == 0 or completed == len(entries):
                print(json.dumps({"indexed_archives": completed, "archive_count": len(entries),
                                  "matched_images": len(found)}), flush=True)
    missing = sorted({item["logicalPath"] for item in matches} - set(found))
    if missing:
        raise ValueError(f"归档缺少 EntityList 指定的原图: {missing}")
    return found
