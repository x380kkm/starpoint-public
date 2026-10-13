# audience: internal
# # generate-cn-raid-rush-rewards
# 此脚本从 CN 主数据提取团队战总讨伐奖励和狂热激战排名奖励.
# 奖励 kind 沿用 GeneralRewardKind 编码, 时间按 UTC+8 转换.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import argparse
from datetime import datetime, timedelta, timezone
import json
from pathlib import Path

from cn_gacha_banner_assets import EntityCatalog, asset_entry_paths, decode_ordered_map, read_logical_assets
from cn_packaged_resource_reader import PackagedResourceReader

MASTER_NAMES = (
    "raid_event", "raid_event_quest", "raid_event_overall_reward",
    "rush_event", "rush_event_ranking_reward",
)


# //// 读取奖励种类和活动时间 [@x380kkm 2026-09-21] ////
def timestamp(value):
    if value in ("", "(None)"):
        return None
    return int(datetime.strptime(value, "%Y-%m-%d %H:%M:%S").replace(
        tzinfo=timezone(timedelta(hours=8))).timestamp() * 1000)


def optional_integer(value):
    return None if value in ("", "(None)") else int(value)


def rewards(row, offset):
    result = []
    for index in range(offset, len(row), 3):
        if row[index] in ("", "(None)"):
            continue
        kind, amount = int(row[index]), int(row[index + 2])
        target = optional_integer(row[index + 1])
        if kind not in (0, 1, 2, 3, 4, 6, 7) or amount <= 0:
            raise ValueError(f"Unsupported event reward: {row}")
        if kind in (0, 1, 6, 7) and (target is None or target <= 0):
            raise ValueError(f"Missing event reward id: {row}")
        result.append({"kind": kind, "id": target, "amount": amount})
    return result


# //// 从资源清单读取活动与奖励主表 [@x380kkm 2026-09-21] ////
def read_masters(cdn_root, package):
    if package is not None:
        with PackagedResourceReader(package) as reader:
            return {name: decode_ordered_map(reader.read(f"master/quest/event/{name}.orderedmap"))
                    for name in MASTER_NAMES}
    catalog = EntityCatalog.load(cdn_root, cdn_root / "entities/PathFile.csv")
    paths = [f"master/quest/event/{name}.orderedmap" for name in MASTER_NAMES]
    records = {}
    for logical in paths:
        record = catalog.find(logical)
        if record is None:
            raise ValueError(f"Missing event master: {logical}")
        records[logical] = record
    raw, missing = read_logical_assets(cdn_root, records)
    if missing:
        raise ValueError(f"Missing event archives: {missing}")
    return {name: decode_ordered_map(raw[path]) for name, path in zip(MASTER_NAMES, paths)}


# //// 首档排名奖励包含榜首且保留各档奖励内容 [@x380kkm 2026-09-28] ////
def include_first_rank(master):
    for event in master.values():
        rows = sorted(event.values(), key=lambda row: int(row[0]))
        if rows:
            rows[0][0] = "1"


# //// 生成服务奖励和客户端共用的完整排名档位 [@x380kkm 2026-09-28] ////
def generate(masters):
    raids = {}
    for event_id, row in masters["raid_event"].items():
        raids[event_id] = {
            "start_at_ms": timestamp(row[22]), "play_end_at_ms": timestamp(row[23]),
            "reward_end_at_ms": timestamp(row[19]), "auto_start_point": optional_integer(row[20]) or 0,
            "quest_ids": [int(quest[0]) for quest in masters["raid_event_quest"][event_id].values()],
            "overall_rewards": [],
        }
    for reward_id, row in masters["raid_event_overall_reward"].items():
        requirement = int(row[2])
        first = optional_integer(row[3])
        interval = optional_integer(row[4])
        if requirement not in (0, 1) or (requirement == 1 and (interval is None or interval <= 0)):
            raise ValueError(f"Invalid raid reward requirement: {reward_id}")
        raids[row[0]]["overall_rewards"].append({
            "id": int(reward_id), "requirement": requirement, "first_kill": first,
            "interval": interval, "rewards": rewards(row, 7),
        })
    rushes = {}
    for event_id, row in masters["rush_event"].items():
        banner_times = row[2].split(",")
        rushes[event_id] = {
            "start_at_ms": timestamp(row[15]), "play_end_at_ms": timestamp(row[16]),
            "aggregation_end_at_ms": timestamp(banner_times[2]) if len(banner_times) > 3 else timestamp(row[16]),
            "reward_end_at_ms": timestamp(row[17]),
            "ranking_rewards": [{"rank_start": int(reward[0]), "rank_end": int(reward[1]),
                                 "rewards": rewards(reward, 3)}
                                for reward in masters["rush_event_ranking_reward"].get(event_id, {}).values()],
        }
    return {"raid": raids, "rush": rushes}


# //// 生成个人服务奖励配置 [@x380kkm 2026-09-21] ////
def main():
    parser = argparse.ArgumentParser()
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--cdn-root", type=Path)
    source.add_argument("--package", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--rush-master-output", type=Path, default=Path(__file__).resolve().parents[2] / "core/personal-service/assets/cn-rush-ranking-master.json")
    args = parser.parse_args()
    masters = read_masters(args.cdn_root, args.package)
    ranking_master = masters["rush_event_ranking_reward"]
    include_first_rank(ranking_master)
    result = generate(masters)
    args.output.write_text(json.dumps(result, ensure_ascii=False, separators=(",", ":")) + "\n", encoding="utf-8")
    args.rush_master_output.write_text(json.dumps({
        "entry_path": asset_entry_paths("master/quest/event/rush_event_ranking_reward.orderedmap")[0],
        "master": ranking_master,
    }, ensure_ascii=False, separators=(",", ":")) + "\n", encoding="utf-8")
    print(json.dumps({"raids": len(result["raid"]), "rushes": len(result["rush"]),
                      "overall_rewards": sum(len(event["overall_rewards"]) for event in result["raid"].values()),
                      "output": str(args.output)}, ensure_ascii=False))


if __name__ == "__main__":
    main()
