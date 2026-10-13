# audience: internal
# # generate-cn-rescue-quests
# 协战目录使用客户端建房分类, 名称和难度来自关卡与难度主表.
# 关卡与所属活动分别保留国服 UTC+8 原始窗口, 时间输出为 Unix 秒.
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import argparse
import json
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

from cn_boss_release_dates import apply_boss_row_release, boss_release_dates
from cn_packaged_resource_reader import PackagedResourceReader
from generate_cn_battle_reward_rules import quest_rows
from generate_cn_shop_selection_fixture import master, timestamp


@dataclass(frozen=True)
class ActivitySource:
    name: str
    prefix: str
    start_index: int
    end_index: int


@dataclass(frozen=True)
class QuestSource:
    category_id: int
    category_name: str
    logical_path: str
    difficulty_index: int
    activity: ActivitySource | None = None
    kind_index: int | None = None
    play_kind_index: int | None = None
    hidden_index: int | None = None


QUEST_SOURCES = (
    QuestSource(2, "领主战", "master/quest/boss_battle_quest.orderedmap", 107,
                hidden_index=123),
    QuestSource(7, "讨伐活动", "master/quest/event/advent_event_quest.orderedmap", 113,
                ActivitySource("advent_event", "advent", 24, 25),
                kind_index=52, play_kind_index=53),
    QuestSource(19, "剧情活动领主战",
                "master/quest/event/world_story_event_boss_battle_quest.orderedmap", 107,
                ActivitySource("world_story_event", "world-story", 22, 23)),
    QuestSource(26, "共同决战", "master/quest/event/hard_multi_event_quest.orderedmap", 108,
                ActivitySource("hard_multi_event", "hard-multi", 23, 24), kind_index=50),
)


# //// 按客户端入口条件选择协战关卡 [@x380kkm 2026-09-23] ////
def is_cooperative_battle(source: QuestSource, row: list[str]) -> bool:
    if source.kind_index is not None and row[source.kind_index] != "1":
        return False
    if source.play_kind_index is not None and row[source.play_kind_index] not in ("1", "2"):
        return False
    return source.hidden_index is None or row[source.hidden_index].lower() == "false"


# //// 转换可选主表时间 [@x380kkm 2026-09-23] ////
def parse_optional_timestamp(value: str) -> int | None:
    return None if value in ("", "(None)") else timestamp(value)


# //// 关联协战关卡的活动名称与原始窗口 [@x380kkm 2026-09-23] ////
def get_activity_fields(source: ActivitySource | None, activities: dict, quest_id: int) -> dict:
    if source is None:
        return {
            "activity_id": None,
            "activity_name": None,
            "activity_start_time": None,
            "activity_end_time": None,
        }
    activity_id = str(quest_id // 1000)
    row = activities[activity_id]
    return {
        "activity_id": f"{source.prefix}:{activity_id}",
        "activity_name": row[2],
        "activity_start_time": parse_optional_timestamp(row[source.start_index]),
        "activity_end_time": parse_optional_timestamp(row[source.end_index]),
    }


# //// 从最终资源主表生成可选协战目录 [@x380kkm 2026-09-23] ////
def build_rescue_catalog(reader: PackagedResourceReader) -> list[dict]:
    difficulties = master(reader, "master/quest/quest_rank.orderedmap")
    releases = boss_release_dates()
    result = []
    for source in QUEST_SOURCES:
        activities = master(
            reader, f"master/quest/event/{source.activity.name}.orderedmap"
        ) if source.activity else {}
        for key, row in quest_rows(source.category_id, master(reader, source.logical_path)):
            if source.category_id == 2:
                apply_boss_row_release(row, releases)
            if not is_cooperative_battle(source, row):
                continue
            difficulty_id = row[source.difficulty_index]
            difficulty = difficulties[difficulty_id][1]
            quest_id = int(key)
            result.append({
                "category_id": source.category_id,
                "quest_id": quest_id,
                "name": row[2].replace("::quest_rank::", difficulty),
                "difficulty_id": int(difficulty_id),
                "difficulty": difficulty,
                "category_name": source.category_name,
                **get_activity_fields(source.activity, activities, quest_id),
                "start_time": parse_optional_timestamp(row[5]),
                "end_time": parse_optional_timestamp(row[6]),
            })
    return sorted(result, key=lambda entry: (entry["category_id"], entry["quest_id"]))


# //// 从救援系统解锁主表读取通关条件 [@x380kkm 2026-09-25] ////
def rescue_unlock_quests(reader: PackagedResourceReader) -> list[dict]:
    systems = master(reader, "master/game_system_unlock/game_system_unlock.orderedmap")
    conditions = master(reader, "master/game_system_unlock/game_system_unlock_condition.orderedmap")
    result = []
    for row in conditions[systems["attention"][0]].values():
        if row[0] != "0" or any(row[index] != "(None)" for index in (5, 10, 15)):
            raise ValueError("救援系统包含尚未映射的解锁条件.")
        result.append({"category_id": 1, "quest_id": int(row[4])})
    return result


# //// 保存协战目录并汇总分类数量 [@x380kkm 2026-09-23] ////
def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--cdn-root", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    with PackagedResourceReader(args.package, cdn_root=args.cdn_root) as reader:
        result = build_rescue_catalog(reader)
        unlock_quests = rescue_unlock_quests(reader)
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    args.output.with_name("cn-rescue-unlock.json").write_text(
        json.dumps(unlock_quests, indent=2) + "\n", encoding="utf-8"
    )
    print(json.dumps({
        "quests": len(result),
        "categories": dict(Counter(entry["category_id"] for entry in result)),
        "activities": len({entry["activity_id"] for entry in result if entry["activity_id"]}),
    }))


if __name__ == "__main__":
    main()
