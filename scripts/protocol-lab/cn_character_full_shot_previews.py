# audience: internal
# # cn-character-full-shot-previews
# 立绘选择页使用图集内的普通和觉醒缩图.
# 预览保持当前图集矩形和裁剪坐标, 相同几何信息的日服纹理按目标分辨率使用.

from __future__ import annotations

import hashlib
import io
import re
import zlib
from pathlib import Path

from PIL import Image

from cn_character_full_shot_assets import (
    PSEUDO_PNG_SIGNATURE, PortraitStagingError, _standard_png, _target_asset_paths, inspect_png,
)
from cn_character_full_shot_plan import (
    PayloadArtifact, _sha256_cn_base64, find_archive_occurrences,
)
from cn_gacha_banner_assets import EntityCatalog, read_logical_assets
from cn_gacha_banner_atlas import _Amf3Decoder

PREVIEW_NAME = re.compile(r"^(character/[^/]+/ui/)full_shot_illustration_setting_(\d+)$")


def decode_atlas(data: bytes):
    return _Amf3Decoder(zlib.decompress(data, -15)).decode()


# //// 按当前矩形生成角色立绘选择页缩图 [@x380kkm 2026-09-27] ////
def render_preview_sheet(sheet: bytes, frames, replacements, reference_size=None):
    with Image.open(io.BytesIO(_standard_png(sheet)[0])) as original:
        texture = original.convert("RGBA")
    reference_width, reference_height = reference_size or texture.size
    scale_x, scale_y = texture.width / reference_width, texture.height / reference_height
    count = 0
    for frame in frames:
        match = PREVIEW_NAME.fullmatch(frame["n"])
        if match is None:
            continue
        logical = f"{match[1]}full_shot_1440_1920_{match[2]}.png"
        paths = sorted(_target_asset_paths(logical).intersection(replacements))
        if not paths:
            raise PortraitStagingError("missing_preview_portrait", "预览缩图缺少对应日服立绘.",
                                      target=logical)
        data = replacements[paths[0]].payload_path.read_bytes()
        left, top = round(frame["x"] * scale_x), round(frame["y"] * scale_y)
        right, bottom = round((frame["x"] + frame["w"]) * scale_x), round((frame["y"] + frame["h"]) * scale_y)
        with Image.open(io.BytesIO(_standard_png(data)[0])) as image:
            portrait = image.convert("RGBA").resize((right - left, bottom - top), Image.Resampling.LANCZOS)
        if min(left, top) < 0 or left + portrait.width > texture.width or top + portrait.height > texture.height:
            raise PortraitStagingError("invalid_preview_frame", "预览缩图矩形超出图集.")
        texture.paste(portrait, (left, top))
        count += 1
    if count == 0:
        raise PortraitStagingError("empty_portrait_preview", "角色预览图集缺少立绘矩形.")
    output = io.BytesIO()
    texture.save(output, format="PNG")
    data = output.getvalue()
    if sheet.startswith(PSEUDO_PNG_SIGNATURE):
        data = PSEUDO_PNG_SIGNATURE + data[8:]
    return data, count
# //// /按当前矩形生成角色立绘选择页缩图 ////


# //// 按目标分辨率和像素格式适配完整预览图集 [@x380kkm 2026-09-27] ////
def adapt_preview_sheet(data: bytes, target):
    with Image.open(io.BytesIO(_standard_png(data)[0])) as source:
        image = source.convert(target.mode)
        if image.size != (target.width, target.height):
            image = image.resize((target.width, target.height), Image.Resampling.LANCZOS)
        output = io.BytesIO()
        image.save(output, format="PNG")
    data = output.getvalue()
    if target.resource_format == "pseudo_png":
        data = PSEUDO_PNG_SIGNATURE + data[8:]
    return data
# //// /按目标分辨率和像素格式适配完整预览图集 ////


# //// 优先采用几何信息相同的日服预览图集 [@x380kkm 2026-09-27] ////
def select_preview_sheet(source_root: Path, logical: str, sheet: bytes, atlas: bytes, replacements,
                         reference_size=None):
    frames = decode_atlas(atlas)
    source_sheet = source_root / logical
    source_atlas = source_root / logical.replace(".png", ".atlas.amf3.deflate")
    if source_sheet.is_file() and source_atlas.is_file():
        source_data = source_sheet.read_bytes()
        source_info = inspect_png(source_data, logical)
        target_info = inspect_png(sheet, logical)
        if decode_atlas(source_atlas.read_bytes()) == frames:
            count = sum(PREVIEW_NAME.fullmatch(frame["n"]) is not None for frame in frames)
            if source_info == target_info:
                return source_data, "jp_sheet", count
            data = adapt_preview_sheet(source_data, target_info)
            return data, "jp_sheet_adapted", count
    data, count = render_preview_sheet(sheet, frames, replacements, reference_size)
    return data, "jp_portrait", count
# //// /优先采用几何信息相同的日服预览图集 ////


# //// 将日服立绘预览写入同一资源替换集合 [@x380kkm 2026-09-27] ////
def add_illustration_previews(cdn_root, source_root, mappings, spool_root,
                             archive_replacements, asset_replacements):
    catalog = EntityCatalog.load(cdn_root, cdn_root / "entities/PathFile.csv")
    records = {}
    sheets = {}
    for prefix in sorted({mapping.target.rsplit("/ui/", 1)[0] for mapping in mappings}):
        logical = prefix + "/ui/illustration_setting_sprite_sheet.png"
        atlas = logical.replace(".png", ".atlas.amf3.deflate")
        image_record, atlas_record = catalog.find(logical), catalog.find(atlas)
        if image_record is None and atlas_record is None:
            continue
        if image_record is None or atlas_record is None:
            raise PortraitStagingError("incomplete_portrait_preview", "角色预览图集与坐标资源未成对.",
                                      target=prefix)
        image_records = [catalog.records[path]
                         for path in sorted(_target_asset_paths(logical).intersection(catalog.records))]
        sheets[logical] = (atlas, image_record, image_records)
        records[atlas] = atlas_record
        records.update((record.entry_path, record) for record in image_records)
    if not sheets:
        return []
    raw, missing = read_logical_assets(cdn_root, records)
    if missing:
        raise PortraitStagingError("missing_portrait_preview", "角色预览图集资源缺失.", targets=missing)
    occurrences = find_archive_occurrences(
        cdn_root, {record.entry_path for _, _, image_records in sheets.values() for record in image_records},
        spool_root / "preview-originals",
    )
    entries = []
    for logical, (atlas, image_record, image_records) in sheets.items():
        reference = inspect_png(raw[image_record.entry_path], logical)
        variants = []
        for record in image_records:
            data, source, count = select_preview_sheet(
                source_root, logical, raw[record.entry_path], raw[atlas], asset_replacements,
                (reference.width, reference.height),
            )
            digest = hashlib.sha256(data).hexdigest()
            payload_path = spool_root / "replacements" / digest
            payload_path.write_bytes(data)
            payload = PayloadArtifact(payload_path, digest, _sha256_cn_base64(data), len(data))
            matches = occurrences.get(record.entry_path, [])
            if not any(match.payload.sha256_encoded == record.digest for match in matches):
                raise PortraitStagingError("preview_archive_mismatch", "角色预览归档与实体记录不一致.",
                                          target=logical, asset_path=record.entry_path)
            for occurrence in matches:
                relative = occurrence.archive_path.relative_to(cdn_root)
                archive_replacements.setdefault(relative, {})[record.entry_path] = payload
            asset_replacements[record.entry_path] = payload
            variants.append({"asset_path": record.entry_path, "source": source, "frame_count": count})
        entries.append({"target": logical, "variants": variants})
    return entries
# //// /将日服立绘预览写入同一资源替换集合 ////
