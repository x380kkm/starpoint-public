# audience: internal
# # prepare-cn-atlas-aliases
# 此脚本在 CN 原图集末尾追加纹理别名, 保留原始成员与配套 PNG.
# 战斗变体使用同图集中 medium 动画的对应图像槽, 来源记录区分精确别名和派生替代.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

from __future__ import annotations

import argparse
import json
import zlib
from pathlib import Path

from audit_cn_client_resource_resolution import ResourceReference, hashed_suffix, is_platform_dependent, resource_paths
from audit_cn_gacha_package_resources import inspect_image, texture_names
from cn_gacha_banner_atlas import _Amf3Decoder, _atlas_index, _decode_atlas
from cn_packaged_resource_reader import PackagedResourceReader


UI_ATLAS = "scene/select_item_campaign/sprite_sheet.atlas.amf3.deflate"
BATTLE_ATLAS = "battle/common/layer1.atlas.amf3.deflate"
BATTLE_ANIMATION = "battle/common/layer1/zone_progress_eliminate_zakoes_breaking_"


# //// 编码 AMF3 图集的整数和字符串 [@x380kkm 2026-09-21] ////
def encode_u29(value):
    if not 0 <= value <= 0x1FFFFFFF:
        raise ValueError(f"AMF3 U29 超出范围: {value}")
    if value < 0x80:
        return bytes([value])
    if value < 0x4000:
        return bytes([(value >> 7) | 0x80, value & 0x7F])
    if value < 0x200000:
        return bytes([(value >> 14) | 0x80, ((value >> 7) & 0x7F) | 0x80, value & 0x7F])
    return bytes([(value >> 22) | 0x80, ((value >> 15) & 0x7F) | 0x80,
                  ((value >> 8) & 0x7F) | 0x80, value & 0xFF])


def encode_string(value):
    data = value.encode("utf-8")
    return encode_u29((len(data) << 1) | 1) + data


def encode_frame(frame):
    result = bytearray(b"\x0a\x0b\x01")
    for field, value in frame.items():
        result.extend(encode_string(field))
        if field == "n":
            result.extend(b"\x06" + encode_string(value))
        elif isinstance(value, bool):
            result.append(3 if value else 2)
        elif isinstance(value, int):
            result.extend(b"\x04" + encode_u29(value & 0x1FFFFFFF))
        else:
            raise ValueError(f"图集字段需要字符串, 布尔或整数: {field}: {value}")
    result.append(1)
    return bytes(result)
# //// /编码 AMF3 图集的整数和字符串 ////


# //// 在原始 AMF3 数组末尾追加别名 [@x380kkm 2026-09-21] ////
def append_aliases(payload, aliases):
    original = _decode_atlas(payload)
    frames = {frame["n"]: frame for frame in original}
    additions = []
    for alias in aliases:
        name, source = alias["name"], alias["source"]
        if name in frames:
            raise ValueError(f"目标图集已包含别名: {name}")
        additions.append({**frames[source], "n": name})
    inflated = zlib.decompress(payload, -15)
    cursor = _Amf3Decoder(inflated)
    if cursor._read_byte() != 9:
        raise ValueError("图集根节点需要 AMF3 数组")
    header = cursor._read_u29()
    if header != (len(original) << 1) | 1:
        raise ValueError("图集数组长度与解码成员数不一致")
    updated = b"\x09" + encode_u29(((len(original) + len(additions)) << 1) | 1)
    updated += inflated[cursor.position:] + b"".join(map(encode_frame, additions))
    result = zlib.compress(updated, wbits=-15)
    decoded = _decode_atlas(result)
    if decoded[:len(original)] != original or decoded[len(original):] != additions:
        raise ValueError("图集追加结果改变了原始成员或目标别名")
    return result, additions, len(original)
# //// /在原始 AMF3 数组末尾追加别名 ////


# //// 从实际 UI 和动画图像槽构建别名来源 [@x380kkm 2026-09-21] ////
def ui_aliases():
    return [{"name": f"scene/select_item_campaign/sprite_sheet/select_item_campaign-assets/select_item_campaign_lineup_{rarity}",
             "source": f"scene/campaign/special_exchange/sprite_sheet/select_item_campaign-assets/select_item_campaign_lineup_{rarity}",
             "derivation": "exact_rectangle_alias", "owners": ["scene/select_item_campaign/event_shop_select_item_campaign_lineup_list.ui.amf3.deflate"]}
            for rarity in ("silver", "gold", "rainbow")]


def battle_aliases(reader):
    animations = {size: _Amf3Decoder(zlib.decompress(reader.read(BATTLE_ANIMATION + size + ".parts.amf3.deflate"), -15)).decode()
                  for size in ("small", "medium", "large")}
    expected_names = {"small": ["icon0101", "effect_powerflip_shot01", "effect_powerflip_shot03", "effect_powerflip_shot02"],
                      "large": ["icon0104", "effect_powerflip_shot01", "effect_powerflip_shot03", "effect_powerflip_shot02"]}
    medium = animations["medium"]["i"]
    if len(medium) != 4:
        raise ValueError("medium 动画图像槽需要包含图标和三种闪光")
    aliases = []
    for size in ("small", "large"):
        images = animations[size]["i"]
        if [image["p"].rsplit("/", 1)[1] for image in images] != expected_names[size]:
            raise ValueError(f"战斗变体的图像槽与已定位的资源角色不一致: {size}")
        if animations[size]["a"] != animations["medium"]["a"]:
            raise ValueError(f"战斗变体的图像混合模式不同: {size}")
        for index, image in enumerate(images):
            aliases.append({"name": image["p"], "source": medium[index]["p"],
                            "derivation": "medium_effect_image_slot_substitute", "imageSlot": index,
                            "owners": [BATTLE_ANIMATION + size + ".parts.amf3.deflate"],
                            "limitation": "small 和 large 复用 medium 的图标与闪光像素, 动画保留各自的时间轴和变换."})
    return aliases
# //// /从实际 UI 和动画图像槽构建别名来源 ////


# //// 核对原图范围及实际资源引用 [@x380kkm 2026-09-21] ////
def validate_aliases(reader, logical, payload, aliases):
    frames = _atlas_index(_decode_atlas(payload))
    image = logical.removesuffix(".atlas.amf3.deflate") + ".png"
    textures = {}
    for resolution in ("medium", "small"):
        content = inspect_image(reader.read(image, resolution))
        width, height = content["size"]
        outside = [frame.name for frame in frames.values() if frame.x + frame.width > width or frame.y + frame.height > height]
        if outside:
            raise ValueError(f"图集矩形超出配套图像: {logical}: {resolution}: {outside}")
        textures[resolution] = {**reader.locate(image, resolution), **content}
    for alias in aliases:
        if frames[alias["name"]].__dict__ | {"name": alias["source"]} != frames[alias["source"]].__dict__:
            raise ValueError(f"别名与原始矩形存在差异: {alias['name']}")
        for owner in alias["owners"]:
            data = _Amf3Decoder(zlib.decompress(reader.read(owner), -15)).decode()
            if alias["name"] not in set(texture_names(data)):
                raise ValueError(f"使用方缺少目标纹理引用: {owner}: {alias['name']}")
    return textures
# //// /核对原图范围及实际资源引用 ////


# //// 输出具有来源信息的原始图集准备文件 [@x380kkm 2026-09-21] ////
def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--client-package", action="append", type=Path, required=True)
    parser.add_argument("--output-directory", type=Path, required=True)
    args = parser.parse_args()
    prepared = {"source": "cn_atlas_rectangle_aliases", "provenance": "derived-substitute", "assets": [], "raw_entries": []}
    args.output_directory.mkdir(parents=True, exist_ok=True)
    canonical = {}
    for package in args.client_package:
        with PackagedResourceReader(package) as reader:
            for logical, aliases in ((UI_ATLAS, ui_aliases()), (BATTLE_ATLAS, battle_aliases(reader))):
                original = reader.read(logical)
                payload, additions, original_count = append_aliases(original, aliases)
                textures = validate_aliases(reader, logical, payload, aliases)
                reference = ResourceReference(logical, hashed_suffix(logical), False, is_platform_dependent(logical))
                entry, _ = resource_paths(reference, reader.bundle, "medium")
                if entry in canonical:
                    record, previous = canonical[entry]
                    if payload != previous:
                        raise ValueError(f"两平台原图集成员不同, 需要独立准备: {logical}")
                    record["platforms"].append(reader.bundle.platform)
                    record["validation"][reader.bundle.platform] = {"textures": textures, "sourcePackage": str(package)}
                    continue
                filename = entry.split("/")[1] + "-" + entry.rsplit("/", 2)[1] + entry.rsplit("/", 1)[1] + ".atlas.amf3.deflate"
                (args.output_directory / filename).write_bytes(payload)
                record = {"entry_path": entry, "payload_file": filename, "logical_path": logical,
                          "provenance": "derived-substitute", "owners": sorted({owner for alias in aliases for owner in alias["owners"]}),
                          "platforms": [reader.bundle.platform], "resolutions": ["medium", "small"],
                          "aliases": aliases, "originalMemberCount": original_count, "addedMemberCount": len(additions),
                          "originalMembersPreserved": True, "pairedImagePreserved": True,
                          "validation": {reader.bundle.platform: {"textures": textures, "sourcePackage": str(package)}}}
                prepared["raw_entries"].append(record)
                canonical[entry] = (record, payload)
    output = args.output_directory / "prepared.json"
    output.write_text(json.dumps(prepared, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"prepared": str(output), "atlases": len(prepared["raw_entries"]),
                      "aliases": sum(item["addedMemberCount"] for item in prepared["raw_entries"])}))


if __name__ == "__main__":
    main()
# //// /输出具有来源信息的原始图集准备文件 ////
