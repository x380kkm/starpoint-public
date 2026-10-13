# audience: internal
# # generate-cn-periodic-battle-rewards
# 周期奖励目录关联降临战关卡, 奖励组和奖励次数. 国服主表时间使用 UTC+8.
# 每日恢复时刻来自客户端显示文案, 奖励位置沿用主表键.
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import argparse
import json
import re
from pathlib import Path

from cn_packaged_resource_reader import PackagedResourceReader
from generate_cn_shop_selection_fixture import master, timestamp


# //// 读取周期奖励的客户端位置与道具数量 [@x380kkm 2026-09-28] ////
def periodic_reward(position: str, row: list[str]) -> dict:
    kind = int(row[1])
    reward_id = None if row[2] in ("", "(None)") else int(row[2])
    result = {
        "position": int(position),
        "probability": float(row[4]),
        "reward": {"type": kind, "id": reward_id, "count": 1 if kind == 2 else int(row[3])},
    }
    if kind in (6, 7):
        result["element_rarity"] = reward_id
    return result


# //// 关联降临战周期次数与主表奖励组 [@x380kkm 2026-09-28] ////
def periodic_battle_rewards(reader: PackagedResourceReader) -> dict:
    points = master(reader, "master/reward/periodic_reward_point.orderedmap")
    events = master(reader, "master/quest/event/hard_multi_event.orderedmap")
    quests = master(reader, "master/quest/event/hard_multi_event_quest.orderedmap")
    groups = master(reader, "master/reward/periodic_reward.orderedmap")
    strings = master(reader, "master/string/ui_string.orderedmap")
    recovery_message = strings["featured_reward_recovery_cycle_daily_message"][0]
    recovery_hour = re.fullmatch(r"每天(\d+):00 追加", recovery_message)
    if recovery_hour is None:
        raise ValueError(f"每日奖励恢复时刻格式无法识别: {recovery_message}")
    result = {
        "daily_recovery_hour": int(recovery_hour[1]),
        "points": {},
        "quests": {},
        "reward_groups": {
            group_id: [periodic_reward(position, row) for position, row in group.items()]
            for group_id, group in groups.items()
        },
    }
    for event_id, event in events.items():
        point_id = None if event[14] == "(None)" else int(event[14])
        if point_id is not None:
            point = points[str(point_id)]
            result["points"][str(point_id)] = {
                "activity_id": f"hard-multi:{event_id}",
                "max_point": int(point[1]),
                "recovery_point": int(point[2]),
                "recovery_cycle": int(point[3]),
                "start_time": timestamp(point[7]),
                "end_time": timestamp(point[8]),
            }
        for row in quests.get(event_id, {}).values():
            group_id = None if row[123] == "(None)" else int(row[123])
            if group_id is None:
                continue
            if str(group_id) not in result["reward_groups"]:
                raise ValueError(f"降临战 {row[0]} 缺少周期奖励组 {group_id}")
            result["quests"][f"26:{row[0]}"] = {
                "point_id": point_id, "group_id": group_id, "slots": int(row[124]),
            }
    return result


# //// 从客户端包生成周期奖励目录 [@x380kkm 2026-09-28] ////
def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--cdn-root", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    with PackagedResourceReader(args.package, cdn_root=args.cdn_root) as reader:
        result = periodic_battle_rewards(reader)
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({key: len(result[key]) for key in ("points", "quests", "reward_groups")}))


if __name__ == "__main__":
    main()

