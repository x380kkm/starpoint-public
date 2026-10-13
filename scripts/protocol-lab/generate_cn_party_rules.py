# audience: internal
# # generate-cn-party-rules
# 编队规则取自最终资源包的角色属性, 魂珠适配属性和分组颜色主表.
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import argparse
import json
from pathlib import Path

from cn_packaged_resource_reader import PackagedResourceReader
from generate_cn_shop_selection_fixture import master


# //// 读取编队持久化使用的角色和魂珠规则 [@x380kkm 2026-09-23] ////
def party_rules(reader: PackagedResourceReader) -> dict:
    characters = master(reader, "master/character/character.orderedmap")
    items = master(reader, "master/item/item.orderedmap")
    colors = master(reader, "master/party/party_group_color.orderedmap")
    config = master(reader, "master/party/party_config.orderedmap")["1"]
    souls = {}
    for item_id, row in items.items():
        if row[6] != "11":
            continue
        elements = sorted({int(value) for value in row[12].split(",")})
        if row[14] != "5" or not elements or any(value not in range(6) for value in elements):
            raise ValueError(f"魂珠 {item_id} 的属性适配规则无法直接解析")
        souls[item_id] = elements
    return {
        "character_elements": {key: int(row[3]) for key, row in characters.items()},
        "ability_soul_elements": souls,
        "group_colors": sorted(int(key) for key in colors),
        "initial_group_color": int(config[0]),
    }
# //// /读取编队持久化使用的角色和魂珠规则 ////


# //// 生成服务使用的编队规则目录 [@x380kkm 2026-09-23] ////
def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--cdn-root", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    with PackagedResourceReader(args.package, cdn_root=args.cdn_root) as reader:
        rules = party_rules(reader)
    args.output.write_text(json.dumps(rules, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({key: len(value) if isinstance(value, (list, dict)) else value for key, value in rules.items()}))
# //// /生成服务使用的编队规则目录 ////


if __name__ == "__main__":
    main()
