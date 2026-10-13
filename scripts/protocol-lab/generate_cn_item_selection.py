# audience: internal
# # generate-cn-item-selection
# 自选道具目录保留客户端主表的选项顺序和国服 UTC+8 使用期限.
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import argparse
import json
from pathlib import Path

from cn_packaged_resource_reader import PackagedResourceReader
from cn_gacha_banner_atlas import ITEM_ATLAS_LOGICAL_PATH, ITEM_SPRITE_SHEET_LOGICAL_PATH, extract_item_atlas_icons
from generate_cn_shop_selection_fixture import master, timestamp


# //// 从道具主表关联可选素材和使用期限 [@x380kkm 2026-09-28] ////
def selection_catalog(reader: PackagedResourceReader) -> dict:
    items = master(reader, "master/item/item.orderedmap")
    bonuses = master(reader, "master/item/item_bonus_select.orderedmap")
    result = {}
    for item_id, row in sorted(items.items(), key=lambda entry: int(entry[0])):
        if row[6] != "22":
            continue
        if len(row) != 23:
            raise ValueError(f"自选道具 {item_id} 字段数异常")
        bonus = bonuses[row[22]]
        if len(bonus) != 20:
            raise ValueError(f"自选道具 {item_id} 奖励字段数异常")
        choices = []
        for offset in range(1, 19, 3):
            kind, amount, reward_id = bonus[offset:offset + 3]
            if kind == "0":
                continue
            if kind != "1" or reward_id not in items or int(amount) <= 0:
                raise ValueError(f"自选道具 {item_id} 素材选项异常")
            choices.append({"item_id": int(reward_id), "amount": int(amount)})
        if not choices:
            raise ValueError(f"自选道具 {item_id} 缺少素材选项")
        result[item_id] = {
            "name": row[2],
            "start_time": timestamp(row[19]),
            "end_time": timestamp(row[20]),
            "choices": choices,
        }
    validate_selection_resources(reader, items, result)
    return result
# //// /从道具主表关联可选素材和使用期限 ////


# //// 核对背包和素材选择界面引用的实际图集纹理 [@x380kkm 2026-09-28] ////
def validate_selection_resources(reader: PackagedResourceReader, items: dict, catalog: dict) -> None:
    item_ids = set(catalog)
    item_ids.update(str(choice["item_id"]) for item in catalog.values() for choice in item["choices"])
    names = {items[item_id][3] for item_id in item_ids}
    atlas = reader.read(ITEM_ATLAS_LOGICAL_PATH)
    texture = reader.read(ITEM_SPRITE_SHEET_LOGICAL_PATH)
    if atlas is None or texture is None:
        raise ValueError("素材选择界面缺少道具图集")
    _, missing = extract_item_atlas_icons(atlas, texture, names)
    if missing:
        raise ValueError(f"素材选择界面缺少图标: {missing}")
# //// /核对背包和素材选择界面引用的实际图集纹理 ////


# //// 按客户端包和资源目录生成自选道具目录 [@x380kkm 2026-09-28] ////
def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--cdn-root", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    with PackagedResourceReader(args.package, cdn_root=args.cdn_root) as reader:
        catalog = selection_catalog(reader)
    content = json.dumps(catalog, ensure_ascii=False, indent=2) + "\n"
    if args.check:
        if args.output.read_text(encoding="utf-8") != content:
            raise ValueError("自选道具目录与客户端主表存在差异")
    else:
        args.output.write_text(content, encoding="utf-8")
    print(json.dumps({"items": len(catalog),
                      "choices": sum(len(item["choices"]) for item in catalog.values()),
                      "resource_items": len(set(catalog) | {str(choice["item_id"]) for item in catalog.values() for choice in item["choices"]})}))
# //// /按客户端包和资源目录生成自选道具目录 ////


if __name__ == "__main__":
    main()
