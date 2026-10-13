# audience: internal
# # generate-cn-equipment-dissolve-fixture
# 装备分解目录取自最终资源包的装备, 物品和分解倍率主表.
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import argparse
import json
from pathlib import Path

from cn_packaged_resource_reader import PackagedResourceReader
from generate_cn_shop_selection_fixture import master


# //// 关联装备分解产出与物品持有上限 [@x380kkm 2026-09-23] ////
def catalog(reader: PackagedResourceReader) -> tuple[dict, dict]:
    equipment = master(reader, "master/item/equipment.orderedmap")
    items = master(reader, "master/item/item.orderedmap")
    craft = master(reader, "master/item/equipment_craft_point_exchange.orderedmap")
    grains = master(reader, "master/item/equipment_dissolve_rate.orderedmap")
    if any(len(row) != 23 for row in items.values()):
        raise ValueError("物品原始主表的字段数量与目录映射不符")
    definitions = {}
    for key, row in equipment.items():
        if len(row) != 16:
            raise ValueError(f"装备 {key} 的主表字段数量与目录映射不符")
        soul_id = row[10]
        generates_soul = row[9].lower() == "true"
        definitions[key] = {
            "ability_soul_id": int(soul_id),
            "obtain_source": int(row[15]),
            "generate_ability_soul": generates_soul,
            "max_level": int(row[8]),
            "sellable": row[13].lower() == "true",
            "craft_points": int(craft[row[11]][0]),
            "star_grains": int(grains[row[11]][0]) if row[15] == "0" else 0,
            "ability_soul_max_count": int(items[soul_id][18]) if generates_soul else 0,
        }
    rules = {
        key: {"max_count": int(items[key][18]), "sale_price": int(items[key][16])}
        for key in ("100000", "990008")
    }
    return definitions, rules
# //// /关联装备分解产出与物品持有上限 ////


# //// 生成服务使用的装备分解数据 [@x380kkm 2026-09-23] ////
def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--cdn-root", type=Path)
    parser.add_argument("--output-root", type=Path, default=Path.cwd())
    args = parser.parse_args()
    with PackagedResourceReader(args.package, cdn_root=args.cdn_root) as reader:
        definitions, rules = catalog(reader)
    for relative, document in (
        ("assets/equipment_dissolve.json", definitions),
        ("core/personal-service/assets/cn-equipment-dissolve-rules.json", rules),
    ):
        (args.output_root / relative).write_text(
            json.dumps(document, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )
    print(json.dumps({"equipment": len(definitions), "items": rules}))
# //// /生成服务使用的装备分解数据 ////


if __name__ == "__main__":
    main()
