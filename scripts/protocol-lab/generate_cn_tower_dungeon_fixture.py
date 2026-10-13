# audience: internal
# # generate-cn-tower-dungeon-fixture
# 幽玄域关卡使用独立的无评级战斗字段. 每期目录保留主表开始时间和完整十关.
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import argparse
import json
from collections import defaultdict
from pathlib import Path

from cn_packaged_resource_reader import PackagedResourceReader
from generate_cn_shop_selection_fixture import master, timestamp


# //// 从幽玄域主表生成十关期别和战斗收益 [@x380kkm 2026-09-23] ////
def catalog(reader: PackagedResourceReader) -> tuple[dict, dict]:
    events = master(reader, "master/quest/event/tower_dungeon_event_quest.orderedmap")
    periods = {}
    quests = {}
    for event_id, rows in events.items():
        grouped = defaultdict(list)
        for row in rows.values():
            if len(row) != 116 or row[52] != "1":
                raise ValueError(f"幽玄域 {event_id} 的关卡字段与战斗映射不符")
            quest_id = int(row[0])
            grouped[(row[5], row[6])].append(quest_id)
            quest = {
                "name": row[2],
                "scoreRewardGroupId": int(row[69]),
                "bRankTime": 0,
                "aRankTime": 0,
                "sRankTime": 0,
                "sPlusRankTime": 0,
                "rankPointReward": int(row[83]),
                "characterExpReward": int(row[84]),
                "manaReward": int(row[85]),
                "poolExpReward": int(row[86]),
            }
            if row[4] != "(None)":
                quest["clearRewardId"] = int(row[4])
            quests[str(quest_id)] = quest
        periods[event_id] = []
        for (start, end), quest_ids in sorted(grouped.items()):
            if len(quest_ids) != 10 or timestamp(start) >= timestamp(end):
                raise ValueError(f"幽玄域 {event_id} 的 {start} 期与十关窗口契约不符")
            periods[event_id].append({
                "starts_at_ms": timestamp(start) * 1000,
                "quest_ids": sorted(quest_ids),
            })
    return quests, periods
# //// /从幽玄域主表生成十关期别和战斗收益 ////


# //// 输出幽玄域源资产和服务共享期别目录 [@x380kkm 2026-09-23] ////
def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--cdn-root", type=Path)
    parser.add_argument("--output-root", type=Path, default=Path.cwd())
    args = parser.parse_args()
    with PackagedResourceReader(args.package, cdn_root=args.cdn_root) as reader:
        quests, periods = catalog(reader)
    for relative, document in (
        ("assets/tower_dungeon_event_quest.json", quests),
        ("core/personal-service/assets/cn-tower-dungeon-periods.json", periods),
    ):
        (args.output_root / relative).write_text(json.dumps(document, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"events": len(periods), "periods": sum(map(len, periods.values())), "quests": len(quests)}))
# //// /输出幽玄域源资产和服务共享期别目录 ////


if __name__ == "__main__":
    main()
