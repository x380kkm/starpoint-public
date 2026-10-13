# audience: internal
# # prepare-cn-gacha-atlas-recovery
# 此脚本按图像内容相同的区域缓存定位匹配图集, 并核对当前动画引用的全部帧名.
# 输出保留原始 atlas 字节和来源, CDN 发布由构建调用方执行.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

from __future__ import annotations

import argparse
import csv
import io
import json
import zlib
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from cn_gacha_banner_assets import hash_cn_asset_path
from cn_gacha_banner_atlas import _Amf3Decoder, _atlas_index, _decode_atlas
from cn_packaged_resource_reader import PackagedResourceReader
from cn_regional_startup_images import indexed_archive_members
from regional_zip_range import RemoteZip64Archive


# //// 按实际图片内容选择同源图集 [@x380kkm 2026-09-21] ////
def prepare(audit_path: Path, package: Path, archive_url: str, cache_root: Path, output: Path, entity_list: Path | None = None) -> dict:
    audit = json.loads(audit_path.read_text(encoding="utf-8"))
    report = next(item for item in audit["packages"] if Path(item["package"]).resolve() == package.resolve())
    source_records = {}
    if entity_list is not None:
        with entity_list.open(encoding="utf-8-sig") as source:
            source_records = {row[0]: row for row in csv.reader(source) if len(row) == 5}
    remote = None if source_records else RemoteZip64Archive(archive_url, cache_root)
    entries = {entry.name.rsplit("/", 2)[-2] + "/" + entry.name.rsplit("/", 1)[-1]: entry
               for entry in remote.entries if "/production/upload/" in entry.name} if remote else {}
    candidates = []
    unmatched = []
    checked = 0
    with PackagedResourceReader(package) as reader:
        manifest_name = f"entities/10939-{reader.bundle.platform}_medium.csv"
        current_records = {row[0]: row for row in csv.reader(io.StringIO(reader.cdn.read(manifest_name).decode("utf-8-sig"))) if len(row) == 5}
        for logical, current in report["files"].items():
            if not logical.startswith("gacha/feature_movie/") or not logical.endswith(".atlas.amf3.deflate"):
                continue
            checked += 1
            image = logical.removesuffix(".atlas.amf3.deflate") + ".png"
            current_image = report["images"][image]["resolutions"]["medium"]
            source_identity = {}
            if source_records:
                image_row = source_records.get(current_image["entry"])
                atlas_row = source_records.get(current["entry"])
                if image_row is None or atlas_row is None:
                    unmatched.append({"logicalPath": logical, "kind": "missing_source_pair"})
                    continue
                if image_row[2:4] != current_records[current_image["entry"]][2:4]:
                    unmatched.append({"logicalPath": logical, "kind": "different_source_image"})
                    continue
                if atlas_row[2:4] == current_records[current["entry"]][2:4]:
                    continue
                source_identity = {"record": atlas_row, "source_image": image_row[0]}
            else:
                source_identity = cached_source_pair(entries, logical, image, current, current_image)
                if source_identity.get("kind"):
                    unmatched.append({"logicalPath": logical, **source_identity})
                    continue
                if not source_identity:
                    continue
            owners = report["images"][image]["owners"]
            movies = [owner for owner in owners if owner.endswith(".movie.amf3.deflate")]
            required = set()
            for movie in movies:
                data = _Amf3Decoder(zlib.decompress(reader.read(movie), -15)).decode()
                required.update(item["p"] for item in data["i"])
            candidates.append({"logicalPath": logical, "current": current, "image": image,
                               "imageSize": current_image["size"], "requiredNames": sorted(required),
                               "owners": owners, **source_identity})
    output.mkdir(parents=True, exist_ok=True)
    if source_records and candidates:
        remote = RemoteZip64Archive(archive_url, cache_root)
        members = indexed_archive_members(remote, candidates, cache_root)
    else:
        indexed = {entry.name: entry for entry in remote.entries} if remote else {}
        members = {item["logicalPath"]: {"archive": remote, "entry": indexed[item["source_entry"]], "source_entry": item["source_entry"]} for item in candidates}

    def prepare_atlas(candidate):
        logical = candidate["logicalPath"]
        destination = output / (hash_cn_asset_path(logical) + ".atlas.amf3.deflate")
        member = members[logical]
        candidate = {**candidate, "source_entry": member["source_entry"]}
        data = destination.read_bytes() if destination.is_file() else member["archive"].read(member["entry"])
        atlas = _atlas_index(_decode_atlas(data))
        missing = sorted(set(candidate["requiredNames"]) - set(atlas))
        width, height = candidate["imageSize"]
        outside = [entry.name for entry in atlas.values() if entry.x + entry.width > width or entry.y + entry.height > height]
        if missing or outside:
            return {**candidate, "status": "source_contract_mismatch", "missingNames": missing, "outside": outside}
        if not destination.is_file():
            destination.write_bytes(data)
        print(json.dumps({"prepared": logical, "frames": len(atlas)}), flush=True)
        return {**candidate, "status": "prepared", "file": str(destination.resolve()), "frameCount": len(atlas)}

    with ThreadPoolExecutor(max_workers=4) as executor:
        prepared = list(executor.map(prepare_atlas, candidates))
    result = {"source": {"url": archive_url}, "package": str(package), "checkedAtlasCount": checked,
              "prepared": prepared, "unmatched": unmatched}
    (output / "prepared.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return {"output": str(output / "prepared.json"), "checked": checked, "candidates": len(candidates),
            "prepared": sum(item["status"] == "prepared" for item in prepared), "unmatched": len(unmatched)}
# //// /按实际图片内容选择同源图集 ////


# //// 从区域缓存筛选图像一致而索引不同的原始图集 [@x380kkm 2026-09-21] ////
def cached_source_pair(entries, logical, image, current, current_image):
    digest = hash_cn_asset_path(image)
    remote_image = entries.get(digest[:2] + "/" + digest[2:])
    digest = hash_cn_asset_path(logical)
    remote_atlas = entries.get(digest[:2] + "/" + digest[2:])
    if remote_image is None or remote_atlas is None:
        return {"kind": "missing_source_pair"}
    if (remote_image.uncompressed_size, remote_image.crc32) != (current_image["bytes"], current_image["crc"]):
        return {"kind": "different_source_image"}
    if (remote_atlas.uncompressed_size, remote_atlas.crc32) == (current["bytes"], current["crc"]):
        return {}
    return {"source_entry": remote_atlas.name, "source_image": remote_image.name}


# //// 生成可发布的同源图集候选 [@x380kkm 2026-09-21] ////
def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--audit", type=Path, required=True)
    parser.add_argument("--client-package", type=Path, required=True)
    parser.add_argument("--archive-url", required=True)
    parser.add_argument("--cache-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--entity-list", type=Path)
    args = parser.parse_args()
    print(json.dumps(prepare(args.audit, args.client_package, args.archive_url, args.cache_root, args.output, args.entity_list), ensure_ascii=False))


if __name__ == "__main__":
    main()
# //// /生成可发布的同源图集候选 ////
