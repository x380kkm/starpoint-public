# audience: internal
# # cn-challenge-catalog
# 此脚本从移动端最终资源提取挑战活动所属关系, UTC+8 时间窗口和剧情关卡引用的挑战点.
# 剧情和世界剧情关卡主表分别有 129 和 130 列, 第 128 列都是挑战点编号; 挑战点主表第 1 列是次数上限, 第 3 列是是否每日恢复.
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import argparse
import json
from datetime import datetime, timedelta, timezone
from pathlib import Path

from cn_gacha_banner_assets import decode_ordered_map
from cn_packaged_resource_reader import PackagedResourceReader

QUEST_MASTER_COLUMNS = {"story_event_single_quest": 129, "world_story_event_quest": 130}
QUEST_CHALLENGE_POINT_INDEX = 128


# //// 提取挑战活动所属关系并核对两端来源 [@x380kkm 2026-09-27] ////
def read_campaigns(points, campaigns):
    parents = {}
    windows = {}
    for campaign_id in sorted(campaigns, key=int):
        row = campaigns[campaign_id]
        point_id = row[1]
        if point_id not in points:
            raise ValueError(f"Campaign {campaign_id} references unknown challenge {point_id}")
        parents[campaign_id] = int(point_id)
        windows[campaign_id] = {
            "point_id": int(point_id),
            "start_at": int(datetime.strptime(row[3], "%Y-%m-%d %H:%M:%S")
                .replace(tzinfo=timezone(timedelta(hours=8))).timestamp()),
            "end_at": int(datetime.strptime(row[4], "%Y-%m-%d %H:%M:%S")
                .replace(tzinfo=timezone(timedelta(hours=8))).timestamp()),
        }
    return parents, windows


# //// 提取剧情关卡引用的挑战点及其次数规则 [@x380kkm 2026-10-04] ////
def read_quest_points(points, quest_masters):
    quests = {}
    for name, master in quest_masters.items():
        for event_rows in master.values():
            for row in event_rows.values():
                if len(row) != QUEST_MASTER_COLUMNS[name]:
                    raise ValueError(f"{name} quest {row[0]} has {len(row)} columns")
                point_id = row[QUEST_CHALLENGE_POINT_INDEX]
                if point_id == "(None)":
                    continue
                if point_id not in points:
                    raise ValueError(f"{name} quest {row[0]} references unknown challenge {point_id}")
                quests[row[0]] = int(point_id)
    referenced = sorted(set(quests.values()))
    return {
        "points": {str(point_id): {
            "max": int(points[str(point_id)][1]),
            "recovery": points[str(point_id)][3] == "true",
        } for point_id in referenced},
        "quests": dict(sorted(quests.items(), key=lambda item: int(item[0]))),
    }


def read_catalog(package):
    with PackagedResourceReader(package) as reader:
        points = decode_ordered_map(reader.read("master/quest/event/daily_challenge_point.orderedmap"))
        campaigns = decode_ordered_map(reader.read("master/campaign/daily_challenge_point_campaign.orderedmap"))
        quest_masters = {name: decode_ordered_map(reader.read(f"master/quest/event/{name}.orderedmap"))
                         for name in QUEST_MASTER_COLUMNS}
    return (*read_campaigns(points, campaigns), read_quest_points(points, quest_masters))


ASSETS = Path(__file__).resolve().parents[2] / "core/personal-service/assets"
parser = argparse.ArgumentParser()
parser.add_argument("packages", nargs="+", type=Path)
parser.add_argument("--output", type=Path, default=ASSETS / "cn-daily-challenge-campaigns.json")
parser.add_argument("--windows-output", type=Path, default=ASSETS / "cn-daily-challenge-windows.json")
parser.add_argument("--quests-output", type=Path, default=ASSETS / "cn-quest-challenge-points.json")
args = parser.parse_args()
catalog = read_catalog(args.packages[0])
for package in args.packages[1:]:
    if read_catalog(package) != catalog:
        raise ValueError(f"Daily challenge identities differ in {package}")
parents, windows, quest_points = catalog
args.output.write_text(json.dumps(parents, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
args.windows_output.write_text(json.dumps(windows, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
args.quests_output.write_text(json.dumps(quest_points, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
print(json.dumps({"campaigns": len(parents), "quest_points": len(quest_points["points"]), "quests": len(quest_points["quests"]),
                  "packages": len(args.packages), "output": str(args.output), "windows_output": str(args.windows_output),
                  "quests_output": str(args.quests_output)}, ensure_ascii=False))
