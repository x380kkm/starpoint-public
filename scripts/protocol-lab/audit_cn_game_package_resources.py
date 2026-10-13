# audience: internal
# # audit-cn-game-package-resources
# 此脚本按实际资源版本链检查客户端主表, 加载代码, 嵌套纹理和分发目录.
# 图像报告区分文件可达, 内容可解析, 可见像素与原图来源, 动态路径保留未解析范围.
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
import zipfile
import zlib
from collections import Counter, defaultdict
from pathlib import Path

from PIL import Image

from audit_cn_client_resource_resolution import is_scaled_asset
from audit_cn_gacha_package_resources import inspect_image, texture_names
from cn_game_resource_inventory import collect_game_references
from cn_gacha_banner_atlas import _Amf3Decoder, _atlas_index, _decode_atlas
from cn_gacha_resource_inventory import GachaResourceInventory
from cn_packaged_resource_reader import PackagedResourceReader


# //// 记录按内容标识复用的解码结果 [@x380kkm 2026-09-21] ////
def content_identity(location):
    return (location["entry"], location["bytes"], location["crc"])


def image_cache(reports):
    cache = {}
    for report in reports:
        for package in report.get("packages", []):
            for image in package.get("images", {}).values():
                for content in image.get("resolutions", {}).values():
                    cache[content_identity(content)] = {
                        key: content[key] for key in ("format", "size", "visibleBounds", "visible", "bytes") if key in content}
    return cache


def checked_file_entries(reports):
    return {content_identity(content) for report in reports for package in report.get("packages", [])
            for content in package.get("files", {}).values()}
# //// /记录按内容标识复用的解码结果 ////


# //// 按最终覆盖结果核对实体清单与分发归档 [@x380kkm 2026-09-21] ////
def audit_distribution(reader):
    gaps, manifests = [], {}
    for name in reader.cdn.entries:
        if not name.startswith("entities/") or not (name.endswith(f"-{reader.bundle.platform}_medium.csv") or name == "entities/PathFile.csv"):
            continue
        required = Counter()
        count = 0
        for row in csv.reader(io.StringIO(reader.cdn.read(name).decode("utf-8-sig"))):
            if not row:
                continue
            entry, _, size, _, kind = row
            if kind in {"ios", "android"} and kind != reader.bundle.platform:
                continue
            count += 1
            required[kind] += 1
            found = reader.downloaded.get(entry)
            if found is None:
                gaps.append({"kind": "missing_declared_entity", "manifest": name, "entry": entry})
            elif found[1].file_size != int(size):
                gaps.append({"kind": "entity_size_mismatch", "manifest": name, "entry": entry,
                             "expected": int(size), "actual": found[1].file_size})
        manifests[name] = {"entryCount": count, "kinds": dict(required)}
    for group in [reader.document["full"], *reader.document.get("diff", [])]:
        for item in group["archive"]:
            name = "/".join(item["location"].rsplit("/", 2)[-2:])
            if name.split("/", 1)[0].split("-")[1] not in ("common", "medium", reader.bundle.platform):
                continue
            actual = reader.cdn.entries.get(name)
            if actual is None or actual["size"] != item["size"]:
                gaps.append({"kind": "download_archive_mismatch", "archive": name,
                             "expected": item["size"], "actual": actual})
    target = reader.document.get("info", {}).get("eventual_target_asset_version")
    if target and target != reader.versions[-1]:
        gaps.append({"kind": "incomplete_version_chain", "target": target, "reached": reader.versions[-1]})
    return {"manifests": manifests, "archives": len(reader.archives), "downloadedEntries": len(reader.downloaded),
            "bundleEntries": len(reader.bundle.entries), "gaps": gaps}
# //// /按最终覆盖结果核对实体清单与分发归档 ////


# //// 读取必需的服务图片目录并记录缺口 [@x380kkm 2026-09-21] ////
def read_service_catalog(cdn, path, fields):
    if path not in cdn.entries:
        return [], [{"kind": "missing_service_catalog", "path": path}]
    try:
        entries = cdn.read_json(path)
    except (OSError, ValueError, KeyError, zipfile.BadZipFile) as error:
        return [], [{"kind": "invalid_service_catalog", "path": path, "error": str(error)}]
    for field in fields:
        if not isinstance(entries, dict) or field not in entries:
            return [], [{"kind": "invalid_service_catalog", "path": path, "field": field,
                         "error": "目录缺少条目数组"}]
        entries = entries[field]
    if not isinstance(entries, list):
        return [], [{"kind": "invalid_service_catalog", "path": path, "error": "目录条目需要数组"}]
    if not entries:
        return [], [{"kind": "empty_service_catalog", "path": path}]
    return entries, []


# //// 检查服务端漫画与活动图片的实际内容和来源 [@x380kkm 2026-09-21] ////
def audit_service_images(reader):
    comics, gaps = read_service_catalog(reader.cdn, "comic/index.json", ())
    activities, activity_gaps = read_service_catalog(reader.cdn, "activity-catalog.json", ("activities",))
    gaps.extend(activity_gaps)
    images, substitutions = {}, []
    for index, comic in enumerate(comics):
        if (not isinstance(comic, dict) or type(comic.get("kind")) is not int or comic["kind"] not in (0, 1)
                or type(comic.get("episode")) is not int or comic["episode"] <= 0):
            gaps.append({"kind": "invalid_service_catalog_entry", "path": "comic/index.json", "entry": index,
                         "error": "漫画条目需要有效的 kind 和 episode"})
            continue
        if not isinstance(comic.get("title"), str):
            gaps.append({"kind": "invalid_service_catalog_entry", "path": "comic/index.json", "entry": index,
                         "error": "漫画条目需要字符串标题"})
        owner = f"comic:{comic['kind']}:{comic['episode']}"
        if comic.get("availability") == "unavailable":
            gaps.append({"kind": "original_image_unavailable", "owner": owner, "notice": comic.get("notice")})
        elif comic.get("availability") == "substitute":
            substitutions.append({"owner": owner, "source": comic.get("source"), "notice": comic.get("notice")})
        for size in ("main", "thumbnail_l", "thumbnail_s"):
            extension = "png" if size == "main" else "jpg"
            images[f"comic/{comic['kind']}/{size}/{comic['episode']}.{extension}"] = owner
    for index, activity in enumerate(activities):
        if (not isinstance(activity, dict) or not isinstance(activity.get("activity_id"), str)
                or not activity["activity_id"].strip()):
            gaps.append({"kind": "invalid_service_catalog_entry", "path": "activity-catalog.json", "entry": index,
                         "error": "活动条目需要字符串 activity_id"})
            continue
        candidates = activity.get("image_candidates", [])
        if not isinstance(candidates, list):
            gaps.append({"kind": "invalid_service_catalog_entry", "path": "activity-catalog.json", "entry": index,
                         "error": "image_candidates 需要数组"})
            continue
        for candidate_index, image in enumerate(candidates):
            if (not isinstance(image, dict)
                    or not all(isinstance(image.get(field), str) and image[field].strip() for field in ("source_type", "key"))):
                gaps.append({"kind": "invalid_service_catalog_entry", "path": "activity-catalog.json", "entry": index,
                             "imageCandidate": candidate_index, "error": "图片候选需要字符串 source_type 和 key"})
                continue
            if image.get("source_type") == "activity_banner":
                images["activity-banners/" + image["key"]] = activity["activity_id"]
    checked = 0
    for name, owner in images.items():
        if name not in reader.cdn.entries:
            gaps.append({"kind": "missing_service_image", "path": name, "owner": owner})
            continue
        try:
            with Image.open(io.BytesIO(reader.cdn.read(name))) as image:
                image.load()
                if image.convert("RGBA").getchannel("A").getbbox() is None:
                    gaps.append({"kind": "empty_service_image", "path": name, "owner": owner})
            checked += 1
        except (ValueError, OSError) as error:
            gaps.append({"kind": "invalid_service_image", "path": name, "error": str(error)})
    return {"comicEntries": len(comics), "activities": len(activities),
            "imageCount": len(images), "decodedImages": checked, "substitutions": substitutions, "gaps": gaps}
# //// /检查服务端漫画与活动图片的实际内容和来源 ////


# //// 解码引用资源并沿实际纹理名展开图集 [@x380kkm 2026-09-21] ////
def audit_references(reader, references, cache, checked_files):
    gaps, observations, records, images, atlases = list(references.issues), [], {}, {}, {}
    frame_sources = {}
    texture_owners = defaultdict(set)
    pending = list(references.resources.values())
    visited = set()
    visited_groups = set()
    while pending or len(visited_groups) < len(references.resources):
        if not pending:
            pending = [item for key, item in references.resources.items() if key not in visited_groups]
        reference = pending.pop()
        candidates = [reference["logicalPath"], *reference["alternativePaths"]]
        visited_groups.add(tuple(candidates))
        logical = next((path for path in candidates if reader.locate(path) is not None), candidates[0])
        if logical in visited:
            continue
        visited.add(logical)
        location = reader.locate(logical)
        owners = sorted(reference["owners"])
        if location is None:
            gaps.append({"kind": "missing_reference", "logicalPath": logical, "alternativePaths": candidates[1:],
                         "owners": owners, "evidence": sorted(reference["evidence"])})
            continue
        if logical.endswith((".png", ".atf.deflate")):
            resolutions = {}
            for resolution in ("medium", "small"):
                found = reader.locate(logical, resolution)
                if found is None:
                    gaps.append({"kind": "missing_image_resolution", "logicalPath": logical, "resolution": resolution})
                    continue
                try:
                    key = content_identity(found)
                    if key not in cache:
                        cache[key] = inspect_image(reader.read_location(found))
                    content = cache[key]
                    resolutions[resolution] = {**found, **content}
                    if content.get("visible") is False:
                        observations.append({"kind": "transparent_image", "logicalPath": logical, "resolution": resolution})
                except (ValueError, OSError, zlib.error) as error:
                    gaps.append({"kind": "invalid_image", "logicalPath": logical, "error": str(error)})
            images[logical] = {"owners": owners, "resolutions": resolutions}
        else:
            try:
                if logical.endswith(".mp3") and content_identity(location) in checked_files:
                    payload = None
                else:
                    payload = reader.read_location(location)
                if logical.endswith(".atlas.amf3.deflate"):
                    atlases[logical] = _atlas_index(_decode_atlas(payload))
                    references.add([logical.removesuffix(".atlas.amf3.deflate") + ".png"], logical, logical, "texture")
                elif logical.endswith(".amf3.deflate"):
                    data = _Amf3Decoder(zlib.decompress(payload, -15)).decode()
                    if logical.endswith(".frame.amf3.deflate"):
                        if not isinstance(data, dict) or not isinstance(data.get("name"), str) or not data["name"]:
                            raise ValueError("逐帧动画缺少纹理名称前缀.")
                        frame_sources[logical] = (data["name"], add_texture_sheet_references(reader, references, data["name"], logical))
                    for texture in texture_names(data):
                        texture_owners[texture].add(logical)
                        add_texture_sheet_references(reader, references, texture, logical)
                        direct = direct_texture_path(texture)
                        if reader.locate(direct) is not None:
                            references.add([direct], logical, logical, "texture")
                elif logical.endswith((".html.deflate", ".css.deflate", ".xml.deflate")):
                    zlib.decompress(payload, -15).decode("utf-8")
                records[logical] = {**location, "owners": owners}
            except (ValueError, RuntimeError, IndexError, zlib.error, UnicodeError) as error:
                gaps.append({"kind": "invalid_resource", "logicalPath": logical, "error": str(error)})
        if len(visited) % 1000 == 0:
            print(json.dumps({"platform": reader.bundle.platform, "references": len(visited), "gaps": len(gaps)}), flush=True)
    atlas_names = {name for entries in atlases.values() for name in entries}
    for logical, (prefix, sheets) in frame_sources.items():
        if not sheets:
            gaps.append({"kind": "missing_frame_atlas", "logicalPath": logical, "prefix": prefix,
                         "candidates": [sheet + ".atlas.amf3.deflate" for sheet in texture_sheets(prefix)]})
        elif not any(name.startswith(prefix) for sheet in sheets for name in atlases.get(sheet, {})):
            gaps.append({"kind": "missing_frame_prefix", "logicalPath": logical, "prefix": prefix,
                         "atlases": sorted(sheets)})
    for (sheet, member), owners in references.members.items():
        if member not in atlases.get(sheet + ".atlas.amf3.deflate", {}):
            gaps.append({"kind": "missing_atlas_member", "atlas": sheet, "member": member, "owners": sorted(owners)})
    for texture, owners in texture_owners.items():
        if texture not in atlas_names and reader.locate(direct_texture_path(texture)) is None:
            gaps.append({"kind": "unresolved_texture_key", "texture": texture, "owners": sorted(owners)})
    for logical, entries in atlases.items():
        image = logical.removesuffix(".atlas.amf3.deflate") + ".png"
        for resolution, content in images.get(image, {}).get("resolutions", {}).items():
            scale = 0.7 if resolution == "small" and is_scaled_asset(image) else 1
            width, height = content["size"]
            outside = [frame.name for frame in entries.values()
                       if round((frame.x + frame.width) * scale) > width or round((frame.y + frame.height) * scale) > height]
            if outside:
                gaps.append({"kind": "atlas_outside_image", "logicalPath": logical, "resolution": resolution, "frames": outside})
    return {"references": len(visited), "files": records, "images": images, "atlasCount": len(atlases),
            "textureKeys": len(texture_owners), "observations": observations, "gaps": gaps}


# //// 沿客户端纹理名称登记实际图集和图像 [@x380kkm 2026-09-28] ////
def add_texture_sheet_references(reader, references, texture, owner):
    sheets = set()
    for sheet in texture_sheets(texture):
        logical = sheet + ".atlas.amf3.deflate"
        if reader.locate(logical) is not None:
            references.add([logical], owner, owner, "atlas")
            references.add([sheet + ".png"], owner, owner, "texture")
            sheets.add(logical)
    return sheets


def direct_texture_path(texture):
    if texture.startswith("scene/title_bundled/key_visual/.gen/"):
        texture = texture.replace("/.gen/", "/separate/")
    return texture if texture.endswith(".png") else texture + ".png"


def texture_sheets(texture):
    if "/sprite_sheet/" in texture:
        yield texture.split("/sprite_sheet/", 1)[0] + "/sprite_sheet"
    path = texture.split(".gen/", 1)[0]
    parts = path.split("/")
    if path.startswith("character/"):
        yield "/".join(parts[:-1]) + ("/special_sprite_sheet" if parts[-1] == "special" else "/sprite_sheet")
    elif path.startswith("ability_node/"):
        yield "ability_node/sprite_sheet"
    elif path.startswith("scene/"):
        yield "/".join(parts[:2]) + "/sprite_sheet"
    elif path.startswith(("battle/common/layer0", "battle/common/layer1")):
        yield "/".join(parts[:3])
    elif path.startswith("town/particle/"):
        yield "/".join(parts[:-1]) + "/sprite_sheet"
    elif len(parts) > 1:
        yield "/".join([*parts[:-1], parts[-2]])
    if "/.gen/" in texture:
        yield texture.split("/.gen/", 1)[0] + "/sprite_sheet"
# //// /解码引用资源并沿实际纹理名展开图集 ////


# //// 汇总最终客户端的资源覆盖与边界 [@x380kkm 2026-09-21] ////
def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--client-package", type=Path, required=True)
    parser.add_argument("--client-source-root", type=Path, required=True)
    parser.add_argument("--activity-projection", type=Path, required=True)
    parser.add_argument("--path-constants", type=Path)
    parser.add_argument("--client-pcode-root", type=Path)
    parser.add_argument("--cdn-root", type=Path)
    parser.add_argument("--reuse-report", type=Path, action="append", default=[])
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    prior = [json.loads(path.read_text(encoding="utf-8-sig")) for path in args.reuse_report]
    with PackagedResourceReader(args.client_package, cdn_root=args.cdn_root) as reader:
        references, scope = collect_game_references(reader, args.client_source_root, args.activity_projection,
                                                    args.path_constants, args.client_pcode_root)
        gacha = GachaResourceInventory(reader)
        gacha.collect_masters()
        for logical, owners in gacha.references.items():
            for owner in owners:
                references.add([logical], owner, "current_gacha_master", "gacha")
        scope["gacha"] = gacha.master_report["scope"]
        distribution = audit_distribution(reader)
        service = audit_service_images(reader)
        resources = audit_references(reader, references, image_cache(prior), checked_file_entries(prior))
        all_gaps = [*distribution["gaps"], *service["gaps"], *resources["gaps"], *gacha.gaps]
        report = {"package": str(args.client_package), "cdnRoot": str(args.cdn_root) if args.cdn_root else None,
                  "platform": reader.bundle.platform, "versions": reader.versions, "scope": scope,
                  "distribution": distribution, "serviceImages": service, **resources,
                  "gaps": all_gaps, "gapsByKind": dict(Counter(gap["kind"] for gap in all_gaps))}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps({"packages": [report]}, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"output": str(args.output), "references": report["references"], "images": len(report["images"]),
                      "gapsByKind": report["gapsByKind"]}, ensure_ascii=False))
    if all_gaps:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
# //// /汇总最终客户端的资源覆盖与边界 ////
