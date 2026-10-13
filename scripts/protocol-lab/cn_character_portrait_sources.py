# audience: internal
# # cn-character-portrait-sources
# 本地日服立绘按目标路径合并, 后列出的原图集合覆盖同名目标.
# 当前主表和 EntityLists 共同确定需要进入 CDN 的角色立绘.

from __future__ import annotations

import json
import os
import shutil
from dataclasses import asdict, replace
from pathlib import Path

from cn_character_full_shot_assets import PortraitStagingError, load_mapping_spec
from cn_gacha_banner_assets import EntityCatalog, decode_ordered_map, read_logical_assets

CHARACTER_MASTER = "master/character/character.orderedmap"


# //// 合并本地日服原图并核对当前角色引用 [@x380kkm 2026-09-27] ////
def collect_portrait_sources(cdn_root: Path, sources_root: Path, config_path: Path):
    config = json.loads(config_path.read_text(encoding="utf-8-sig"))
    selected = {}
    for source in config["sources"]:
        root = (sources_root / source["root"]).resolve(strict=True)
        for mapping in load_mapping_spec(root / source["mappings"]):
            if mapping.source_region != "jp" or mapping.replacement_status != "selected":
                raise PortraitStagingError(
                    "invalid_jp_source", "角色立绘原图集合需要使用日服资源.",
                    target=mapping.target,
                )
            original = (root / "source" / mapping.source).resolve(strict=True)
            if not original.is_relative_to(root) or not original.is_file():
                raise PortraitStagingError("invalid_jp_source", "角色立绘原图路径无效.")
            selected[mapping.target] = (mapping, original)

    catalog = EntityCatalog.load(cdn_root, cdn_root / "entities/PathFile.csv")
    master_record = catalog.find(CHARACTER_MASTER)
    if master_record is None:
        raise PortraitStagingError("missing_character_master", "角色主表缺失.")
    raw, missing = read_logical_assets(cdn_root, {CHARACTER_MASTER: master_record})
    if missing:
        raise PortraitStagingError("missing_character_master", "角色主表归档缺失.")
    characters = decode_ordered_map(raw[CHARACTER_MASTER])
    required = {}
    absent = []
    for character_id, row in characters.items():
        for evolution in range(min(int(row[29]), 1) + 1):
            target = f"character/{row[0]}/ui/full_shot_1440_1920_{evolution}.png"
            if catalog.find(target) is not None:
                required[target] = character_id
            else:
                absent.append({"character_id": character_id, "target": target})
    missing_sources = sorted(set(required) - set(selected))
    if missing_sources:
        raise PortraitStagingError(
            "missing_jp_portraits", "当前角色立绘缺少日服原图.",
            targets=missing_sources,
        )
    selected = {target: value for target, value in selected.items()
                if catalog.find(target) is not None}
    return selected, {
        "character_count": len(characters),
        "referenced_portrait_count": len(required),
        "additional_portrait_count": len(set(selected) - set(required)),
        "characters_without_entity_portraits": absent,
    }
# //// /合并本地日服原图并核对当前角色引用 ////


# //// 将选定原图汇集为立绘生成器的输入 [@x380kkm 2026-09-27] ////
def prepare_portrait_sources(selected, output_root: Path) -> Path:
    entries = []
    for target, (mapping, original) in sorted(selected.items()):
        destination = output_root / "source" / target
        link_source(original, destination)
        entries.append(asdict(replace(mapping, source=target)))
    spec = output_root / "mappings.json"
    spec.write_text(
        json.dumps({"schema_version": 2, "entries": entries}, ensure_ascii=False),
        encoding="utf-8",
    )
    return spec
# //// /将选定原图汇集为立绘生成器的输入 ////


# //// 汇集与日服原图成对的预览纹理和坐标 [@x380kkm 2026-09-27] ////
def prepare_preview_sources(selected, sources_root: Path, config_path: Path, output_root: Path):
    config = json.loads(config_path.read_text(encoding="utf-8-sig"))
    preview_root = sources_root / config["preview_root"]
    copied = 0
    for prefix in sorted({target.rsplit("/ui/", 1)[0] for target in selected}):
        paths = [prefix + "/ui/illustration_setting_sprite_sheet" + suffix
                 for suffix in (".png", ".atlas.amf3.deflate")]
        if all((preview_root / path).is_file() for path in paths):
            for path in paths:
                link_source(preview_root / path, output_root / "source" / path)
            copied += 1
    return copied


def link_source(original: Path, destination: Path):
    destination.parent.mkdir(parents=True, exist_ok=True)
    try:
        os.link(original, destination)
    except OSError:
        shutil.copy2(original, destination)
# //// /汇集与日服原图成对的预览纹理和坐标 ////
