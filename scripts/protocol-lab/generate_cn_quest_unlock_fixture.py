# audience: internal
# # generate-cn-quest-unlock-fixture
# 一次性解锁费用沿用战斗主表的分类和消耗模式字段, 输入包与 CDN 覆盖共同构成资源视图.
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import argparse
import json
from pathlib import Path

from cn_packaged_resource_reader import PackagedResourceReader
from generate_cn_battle_reward_rules import BATTLE_MASTERS, ENTRY_MODE_COLUMNS, quest_rows
from generate_cn_shop_selection_fixture import master


# //// 按分类和关卡提取一次性解锁费用 [@x380kkm 2026-09-23] ////
def unlock_costs(reader: PackagedResourceReader) -> dict:
    costs = {}
    for category, logical_path, _, _ in BATTLE_MASTERS:
        column = ENTRY_MODE_COLUMNS[category]
        for quest_id, row in quest_rows(category, master(reader, logical_path)):
            if row[column] != "0":
                continue
            item_ids = [int(value) for value in row[column + 1].split(",")]
            item_counts = [int(value) for value in row[column + 2].split(",")]
            if len(item_ids) != len(item_counts) or min(item_ids + item_counts) <= 0:
                raise ValueError(f"关卡解锁材料无效: {category}:{quest_id}")
            costs[f"{category}:{quest_id}"] = {"itemIds": item_ids, "itemCounts": item_counts}
    return costs
# //// /按分类和关卡提取一次性解锁费用 ////


# //// 从客户端资源生成服务端解锁目录 [@x380kkm 2026-09-23] ////
def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--cdn-root", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    with PackagedResourceReader(args.package, cdn_root=args.cdn_root) as reader:
        costs = unlock_costs(reader)
    args.output.write_text(json.dumps(costs, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"quests": len(costs), "categories": sorted({key.split(":")[0] for key in costs})}))
# //// /从客户端资源生成服务端解锁目录 ////


if __name__ == "__main__":
    main()
