# audience: internal
# # generate-cn-ranking-fixture
# 此脚本从 CN 客户端资源清单读取竞速活动, 关卡和排名奖励主表.
# 奖励 kind 使用 GeneralRewardKind 编码, rank_percentage 使用百分数.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import argparse
from datetime import datetime, timedelta, timezone
import json
from pathlib import Path

from cn_gacha_banner_assets import EntityCatalog, decode_ordered_map, read_logical_assets

MASTER_NAMES = ("ranking_event", "ranking_event_single_quest", "ranking_event_ranking_reward")
REFERENCE_TIMES = {1: 54410, 2: 25800, 3: 18880, 4: 31720, 5: 6540, 1000: 54410, 1001: 25800}


# //// 读取主表时间和排名档位 [@x380kkm 2026-09-21] ////
def timestamp(value):
    return int(datetime.strptime(value, "%Y-%m-%d %H:%M:%S").replace(
        tzinfo=timezone(timedelta(hours=8))).timestamp() * 1000)


def reward_tier(row):
    rewards = []
    for index in range(3, len(row), 3):
        if row[index] in ("", "(None)"):
            continue
        kind, amount = int(row[index]), int(row[index + 2])
        target = int(row[index + 1]) if row[index + 1] not in ("", "(None)") else None
        if kind not in range(8) or amount <= 0:
            raise ValueError(f"Invalid ranking reward: {row[0]}")
        rewards.append({"kind": kind, "id": target, "amount": amount})
    return {"id": int(row[0]), "border_percentage": float(row[1]) * 100,
            "reason_id": int(row[2]), "rewards": rewards}


# //// 按同一资源清单生成活动定义和奖励 [@x380kkm 2026-09-21] ////
def generate(cdn_root):
    catalog = EntityCatalog.load(cdn_root, cdn_root / "entities/PathFile.csv")
    paths = [f"master/quest/event/{name}.orderedmap" for name in MASTER_NAMES]
    records = {}
    for logical in paths:
        record = catalog.find(logical)
        if record is None:
            raise ValueError(f"Missing ranking master: {logical}")
        records[logical] = record
    raw, missing = read_logical_assets(cdn_root, records)
    if missing:
        raise ValueError(f"Missing ranking archives: {missing}")
    events, quests, rewards = [decode_ordered_map(raw[logical]) for logical in paths]
    result = {}
    for event_id, row in events.items():
        tiers = sorted((reward_tier(value) for value in rewards[event_id].values()),
                       key=lambda value: value["border_percentage"])
        if not tiers or tiers[-1]["border_percentage"] != 100:
            raise ValueError(f"Incomplete ranking rewards: {event_id}")
        result[event_id] = {
            "name": row[2], "quest_ids": [int(value[0]) for value in quests[event_id].values()],
            "reference_time_ms": REFERENCE_TIMES[int(event_id)],
            "start_at_ms": timestamp(row[18]), "play_end_at_ms": timestamp(row[19]),
            "aggregation_end_at_ms": timestamp(row[11]), "reward_end_at_ms": timestamp(row[12]),
            "tiers": tiers,
        }
    return result


# //// 写入个人服务使用的竞速定义 [@x380kkm 2026-09-21] ////
def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cdn-root", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    result = generate(args.cdn_root)
    args.output.write_text(json.dumps(result, ensure_ascii=False, separators=(",", ":")) + "\n", encoding="utf-8")
    print(json.dumps({"events": len(result), "tiers": sum(len(event["tiers"]) for event in result.values()),
                      "output": str(args.output)}, ensure_ascii=False))


if __name__ == "__main__":
    main()
