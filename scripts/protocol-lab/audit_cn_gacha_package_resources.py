# audience: internal
# # audit-cn-gacha-package-resources
# 此脚本从最终 IPA 和 APK 的资源版本链核对全部卡池引用, 动画子纹理和 PNG 可见内容.
# 图集帧名按实际动画及 UI 引用检查, 图像尺寸按两种 FileReader 规格检查.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

from __future__ import annotations

import argparse
import io
import json
import zlib
from collections import Counter, defaultdict
from pathlib import Path

from PIL import Image

from cn_gacha_banner_assets import standard_png
from cn_gacha_banner_atlas import _Amf3Decoder, _atlas_index, _decode_atlas
from cn_gacha_resource_inventory import GachaResourceInventory, collect_scene_resources
from cn_packaged_resource_reader import PackagedResourceReader


# //// 提取 UI 和动画引用的纹理名 [@x380kkm 2026-09-21] ////
def texture_names(value):
    if isinstance(value, dict):
        images = value.get("i")
        if isinstance(images, list):
            for image in images:
                if isinstance(image, dict) and isinstance(image.get("p"), str):
                    yield image["p"]
        for bitmap in value.get("bitmaps", []):
            path = bitmap.get("attributes", {}).get("path")
            if isinstance(path, str):
                yield path
        for item in value.values():
            yield from texture_names(item)
    elif isinstance(value, list):
        for item in value:
            yield from texture_names(item)


# //// 检查图像解码, 有效像素区域和原始尺寸 [@x380kkm 2026-09-21] ////
def inspect_image(data: bytes) -> dict:
    if not data.startswith((b"\x89PNG", b"\x89png")):
        inflated = zlib.decompress(data, -15)
        if not inflated.startswith(b"ATF"):
            raise ValueError("平台纹理解压后缺少 ATF 标识")
        header = 12 if inflated[6] == 255 else 6
        return {"format": "atf", "bytes": len(data), "size": [2 ** inflated[header + 1], 2 ** inflated[header + 2]]}
    with Image.open(io.BytesIO(standard_png(data))) as source:
        source.load()
        alpha = source.convert("RGBA").getchannel("A")
        bounds = alpha.getbbox()
        return {"format": "png", "size": list(source.size), "visibleBounds": bounds,
                "visible": bounds is not None, "bytes": len(data)}


# //// 核对包内卡池全部资源及嵌套纹理 [@x380kkm 2026-09-21] ////
def audit_package(path: Path, sources: list[Path], previous: dict | None = None, overlays: list[Path] | None = None) -> dict:
    with PackagedResourceReader(path, overlays) as reader:
        inventory = GachaResourceInventory(reader)
        inventory.collect_masters()
        collect_scene_resources(inventory, sources)
        pending = list(inventory.references)
        visited = set()
        gaps = inventory.gaps
        texture_owners = defaultdict(set)
        movie_textures = {}
        atlases = {}
        frame_sources = {}
        file_records = {}
        while pending:
            logical = pending.pop()
            if logical in visited or logical.endswith(".png"):
                continue
            visited.add(logical)
            location = reader.locate(logical)
            if location is None:
                gaps.append({"kind": "missing_file", "logicalPath": logical, "owners": sorted(inventory.references[logical])})
                continue
            file_records[logical] = location
            payload = reader.read_location(location)
            try:
                if logical.endswith(".atlas.amf3.deflate"):
                    atlases[logical] = _atlas_index(_decode_atlas(payload))
                elif logical.endswith(".html.deflate"):
                    zlib.decompress(payload, -15).decode("utf-8")
                elif logical.endswith(".amf3.deflate"):
                    data = _Amf3Decoder(zlib.decompress(payload, -15)).decode()
                    if logical.endswith(".frame.amf3.deflate"):
                        frame_sources[logical] = data
                    if logical.startswith("gacha/feature_movie/") and logical.endswith(".movie.amf3.deflate"):
                        movie_textures[logical] = set(texture_names(data))
                    for name in texture_names(data):
                        texture_owners[name].add(logical)
                        if "/sprite_sheet/" in name:
                            sheet = name.split("/sprite_sheet/", 1)[0] + "/sprite_sheet"
                        elif "/.gen/" in name:
                            sheet = name.split("/.gen/", 1)[0] + "/sprite_sheet"
                        else:
                            continue
                        inventory.add_sheet(sheet, logical)
                        pending.append(sheet + ".atlas.amf3.deflate")
            except (ValueError, RuntimeError, zlib.error, UnicodeError) as error:
                gaps.append({"kind": "invalid_resource", "logicalPath": logical, "error": str(error)})
        atlas_names = defaultdict(list)
        for logical, entries in atlases.items():
            for name in entries:
                atlas_names[name].append(logical)
        for logical, names in movie_textures.items():
            sheet = logical.rsplit("/", 1)[0] + "/sprite_sheet.atlas.amf3.deflate"
            absent = sorted(names - set(atlases.get(sheet, {})))
            if absent:
                gaps.append({"kind": "movie_atlas_missing_frames", "logicalPath": logical, "atlas": sheet, "frames": absent})
        for item, name in inventory.equipment_keys.items():
            texture_owners[name].add(f"equipment:{item}")
        direct_textures = []
        for name, owners in texture_owners.items():
            if name not in atlas_names:
                logical = name if name.endswith(".png") else name + ".png"
                if reader.locate(logical) is not None:
                    direct_textures.append(logical)
                    inventory.references[logical].update(owners)
                else:
                    gaps.append({"kind": "missing_texture_key", "logicalPath": name, "owners": sorted(owners)})
        image_records = {}
        checked_images = {
            (entry.get("archive"), entry["entry"], entry["bytes"], entry["crc"]):
                {key: entry[key] for key in ("format", "size", "visibleBounds", "visible", "bytes") if key in entry}
            for image in (previous or {}).get("images", {}).values() for entry in image["resolutions"].values()
        }
        for number, logical in enumerate(sorted(path for path in inventory.references if path.endswith(".png")), 1):
            resolutions = {}
            for resolution in ("medium", "small"):
                location = reader.locate(logical, resolution)
                if location is None:
                    gaps.append({"kind": "missing_image", "logicalPath": logical, "resolution": resolution,
                                 "owners": sorted(inventory.references[logical])})
                    continue
                identity = (location.get("archive"), location["entry"], location["bytes"], location["crc"])
                try:
                    if identity not in checked_images:
                        checked_images[identity] = inspect_image(reader.read_location(location))
                    content = checked_images[identity]
                    resolutions[resolution] = {**location, **content}
                    if content.get("visible") is False:
                        gaps.append({"kind": "empty_image", "logicalPath": logical, "resolution": resolution})
                except (ValueError, OSError, zlib.error) as error:
                    gaps.append({"kind": "invalid_image", "logicalPath": logical, "resolution": resolution, "error": str(error)})
            image_records[logical] = {"owners": sorted(inventory.references[logical]), "resolutions": resolutions}
            if number % 250 == 0:
                print(json.dumps({"platform": reader.bundle.platform, "images": number, "gaps": len(gaps)}), flush=True)
        for logical, entries in atlases.items():
            image = logical.removesuffix(".atlas.amf3.deflate") + ".png"
            for resolution, content in image_records.get(image, {}).get("resolutions", {}).items():
                if content.get("format") != "png":
                    continue
                width, height = content["size"]
                invalid = [entry.name for entry in entries.values() if entry.x + entry.width > width or entry.y + entry.height > height]
                if invalid:
                    gaps.append({"kind": "atlas_outside_image", "logicalPath": logical, "resolution": resolution,
                                 "imageSize": [width, height], "frames": invalid})
        for logical, data in frame_sources.items():
            if isinstance(data, dict) and isinstance(data.get("name"), str):
                if not any(name.startswith(data["name"]) for name in atlas_names):
                    gaps.append({"kind": "missing_frame_prefix", "logicalPath": logical, "prefix": data["name"]})
        return {"package": str(path), "platform": reader.bundle.platform, "versions": reader.versions,
                "overlayArchives": [str(path) for path in overlays or []],
                "scope": {**inventory.master_report["scope"], "characters": len(inventory.characters),
                          "equipment": len(inventory.equipment), "files": len(file_records), "images": len(image_records),
                          "atlases": len(atlases), "textureKeys": len(texture_owners), "directTextures": len(direct_textures)},
                "masterIssues": {key: inventory.master_report[key] for key in ("missingFeatureGroups", "invalidRows")},
                "gapsByKind": dict(Counter(item["kind"] for item in gaps)), "gaps": gaps,
                "images": image_records, "files": file_records,
                "atlasCounts": {name: len(entries) for name, entries in atlases.items()}}
# //// /核对包内卡池全部资源及嵌套纹理 ////


# //// 输出最终客户端卡池资源报告 [@x380kkm 2026-09-21] ////
def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--client-package", type=Path, action="append", required=True)
    parser.add_argument("--client-source", type=Path, action="append", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--reuse-report", type=Path)
    parser.add_argument("--overlay-archive", type=Path, action="append")
    args = parser.parse_args()
    prior = json.loads(args.reuse_report.read_text(encoding="utf-8")) if args.reuse_report else {}
    prior_packages = {item["package"]: item for item in prior.get("packages", [])}
    reports = [audit_package(path, args.client_source, prior_packages.get(str(path)), args.overlay_archive) for path in args.client_package]
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps({"packages": reports}, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"output": str(args.output), "packages": [{"platform": report["platform"], "scope": report["scope"],
                       "gapsByKind": report["gapsByKind"]} for report in reports]}, ensure_ascii=False))


if __name__ == "__main__":
    main()
# //// /输出最终客户端卡池资源报告 ////
