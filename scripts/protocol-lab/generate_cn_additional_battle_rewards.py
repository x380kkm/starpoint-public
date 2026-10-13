# audience: internal
# # generate-cn-additional-battle-rewards
# 追加掉落目录关联最终客户端主表中的活动, 关卡范围, 敌人等级门槛和加权奖励组.
# 活动时间使用 UTC+8, 奖励位置沿用主表键. 日常关卡的等级来自抽选表中等级一致的候选.
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import argparse
import json
from pathlib import Path

from cn_packaged_resource_reader import PackagedResourceReader
from cn_collect_reward_relations import complete_collect_quest_relations
from generate_cn_battle_reward_rules import BATTLE_MASTERS, quest_rows
from generate_cn_shop_selection_fixture import master, timestamp

REPOSITORY = Path(__file__).resolve().parents[2]
REWARD_MASTER_ROOT = "master/reward/event/"
ENEMY_LEVEL_COLUMNS = {
    1: 106, 2: 106, 4: 106, 7: 112, 10: 108, 11: 89, 13: 107,
    15: 107, 18: 107, 19: 106, 21: 109, 22: 95, 23: 96, 24: 95,
    25: 98, 26: 107, 27: 99,
}
DAILY_ODDS_COLUMNS = {6: 71, 14: 72}
ODDS_ENEMY_LEVEL_COLUMN = 11
QUEST_RANGE_PARTS = {
    0: (2, 3, 4), 1: (2, 3, 4), 2: (2, 3, 4), 11: (None, None, 4),
    12: (None, None, None),
    **{kind: (2, None, 4) for kind in (*range(3, 11), *range(13, 21))},
}
QUEST_REFERENCE_CATEGORIES = {
    0: 1, 1: 4, 2: 2, 3: 6, 4: 11, 5: 10, 6: 7, 7: 13, 8: 14, 9: 18,
    10: 19, 11: 20, 12: 21, 13: 22, 14: 3, 15: 23, 16: 24, 17: 25, 18: 26, 19: 27,
}


# //// 读取范围字段并区分全集与空集合 [@x380kkm 2026-09-28] ////
def quest_part(value: str) -> list[int] | None:
    if value == "(None)":
        return None
    return [int(part) for part in value.split(",")] if value else []


# //// 按客户端范围类型提取有效维度 [@x380kkm 2026-09-28] ////
def quest_relation(row: list[str]) -> dict:
    kind = None if row[1] == "(None)" else int(row[1])
    columns = (None, None, None) if kind is None else QUEST_RANGE_PARTS[kind]
    return {
        "quest_kind": kind,
        "quest_parts": [quest_part(row[column]) if column is not None else None for column in columns],
        "reward_relation_id": int(row[5]),
    }


# //// 按关卡引用字段解析活动解锁条件 [@x380kkm 2026-09-28] ////
def required_quest(row: list[str], quest_tables: dict) -> dict | None:
    if row[23] == "(None)":
        return None
    kind = int(row[23])
    category = QUEST_REFERENCE_CATEGORIES[kind]
    columns = (24, 25, 26) if kind in (0, 1, 2) else (24, 26)
    if kind == 14:
        columns = (26,)
    quest_row = quest_tables[category]
    for column in columns:
        quest_row = quest_row[row[column]]
    quest_id = int(row[26]) if kind == 14 else int(quest_row[0])
    return {"category": category, "quest_id": quest_id}


# //// 关联活动时间, 解锁关卡与掉落范围 [@x380kkm 2026-09-28] ////
def collect_events(reader: PackagedResourceReader, events: dict, relations: dict) -> list[dict]:
    required_categories = {QUEST_REFERENCE_CATEGORIES[int(row[23])]
                           for row in events.values() if row[23] != "(None)"}
    quest_tables = {category: master(reader, path) for category, path, _, _ in BATTLE_MASTERS
                    if category in required_categories}
    missing_events = set(relations) - set(events)
    if missing_events:
        raise ValueError(f"关卡范围引用缺失活动: {sorted(missing_events)}")
    result = []
    for event_id, row in events.items():
        event = {
            "id": int(event_id), "start_time": timestamp(row[20]), "end_time": timestamp(row[21]),
            "relations": [quest_relation(relation) for relation in relations.get(event_id, {}).values()],
        }
        requirement = required_quest(row, quest_tables)
        if requirement is not None:
            event["required_quest"] = requirement
        result.append(event)
    return result


# //// 保留每个敌人等级门槛对应的奖励组 [@x380kkm 2026-09-28] ////
def reward_relations(reader: PackagedResourceReader) -> dict:
    table = master(reader, REWARD_MASTER_ROOT + "collect_item_event_reward_relation.orderedmap")
    return {relation_id: [
        {"minimum_enemy_level": int(level), "group_id": int(group[level][2])}
        for level in sorted(group, key=int)
    ] for relation_id, group in table.items()}


# //// 保留加权奖励的主表位置和元素稀有度 [@x380kkm 2026-09-28] ////
def weighted_reward(position: str, row: list[str]) -> dict:
    kind = int(row[1])
    if kind not in range(8):
        raise ValueError(f"追加奖励类型超出客户端定义: {kind}")
    reward_id = None if row[2] in ("", "(None)") else int(row[2])
    result = {
        "position": int(position), "weight": int(row[4]),
        "reward": {"type": kind, "id": reward_id, "count": 1 if kind == 2 else int(row[3])},
    }
    if kind in (6, 7):
        result["element_rarity"] = reward_id
    return result


# //// 读取完整追加奖励组 [@x380kkm 2026-09-28] ////
def reward_groups(reader: PackagedResourceReader) -> dict:
    table = master(reader, REWARD_MASTER_ROOT + "additional_reward.orderedmap")
    return {group_id: [weighted_reward(position, row) for position, row in group.items()]
            for group_id, group in table.items()}


# //// 归并日常关卡抽选候选中的同一敌人等级 [@x380kkm 2026-09-28] ////
def daily_enemy_levels(reader: PackagedResourceReader, quest_path: str) -> dict[str, int]:
    odds_path = quest_path.removesuffix(".orderedmap") + "_odds.orderedmap"
    result = {}
    for group_id, group in master(reader, odds_path).items():
        levels = {int(row[ODDS_ENEMY_LEVEL_COLUMN]) for row in group.values()}
        if len(levels) != 1:
            raise ValueError(f"日常关卡抽选组包含多个敌人等级: {odds_path}:{group_id}:{sorted(levels)}")
        result[group_id] = levels.pop()
    return result


# //// 从客户端字段提取服务关卡的实际敌人等级 [@x380kkm 2026-09-28] ////
def quest_enemy_levels(reader: PackagedResourceReader, quests: dict) -> dict[str, int]:
    result = {}
    for category, path, _, _ in BATTLE_MASTERS:
        if category not in ENEMY_LEVEL_COLUMNS and category not in DAILY_ODDS_COLUMNS:
            continue
        odds_levels = daily_enemy_levels(reader, path) if category in DAILY_ODDS_COLUMNS else None
        for quest_id, row in quest_rows(category, master(reader, path)):
            key = f"{category}:{quest_id}"
            if key not in quests:
                continue
            if odds_levels is not None:
                result[key] = odds_levels[row[DAILY_ODDS_COLUMNS[category]]]
            else:
                value = row[ENEMY_LEVEL_COLUMNS[category]]
                if value not in ("", "(None)"):
                    result[key] = int(value)
    return result


# //// 校验追加掉落引用并生成服务目录 [@x380kkm 2026-09-28] ////
def additional_battle_rewards(reader: PackagedResourceReader, quests: dict) -> dict:
    relations = reward_relations(reader)
    groups = reward_groups(reader)
    event_master = master(reader, REWARD_MASTER_ROOT + "collect_item_event.orderedmap")
    relation_path = REWARD_MASTER_ROOT + "collect_item_event_quest_relation.orderedmap"
    quest_relations = complete_collect_quest_relations(
        event_master, master(reader, relation_path), relations, groups)
    events = collect_events(reader, event_master, quest_relations)
    for event in events:
        for relation in event["relations"]:
            if str(relation["reward_relation_id"]) not in relations:
                raise ValueError(f"活动缺少奖励门槛: {event['id']}:{relation['reward_relation_id']}")
    for relation_id, thresholds in relations.items():
        for threshold in thresholds:
            if str(threshold["group_id"]) not in groups:
                raise ValueError(f"等级门槛缺少奖励组: {relation_id}:{threshold['group_id']}")
    return {
        "events": events, "reward_relations": relations, "reward_groups": groups,
        "quest_enemy_levels": quest_enemy_levels(reader, quests),
        "client_quest_relation_master": {"entry_path": reader.locate(relation_path)["entry"],
                                         "master": [(event_id, list(rows.items()))
                                                    for event_id, rows in quest_relations.items()]},
    }


# //// 从分发包写出追加掉落目录 [@x380kkm 2026-09-28] ////
def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--cdn-root", type=Path)
    parser.add_argument("--output", type=Path,
                        default=REPOSITORY / "core/personal-service/assets/cn-additional-battle-rewards.json")
    args = parser.parse_args()
    quests = json.loads((REPOSITORY / "core/personal-service/assets/cn-single-battle.json")
                       .read_text(encoding="utf-8"))["quests"]
    with PackagedResourceReader(args.package, cdn_root=args.cdn_root) as reader:
        result = additional_battle_rewards(reader, quests)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n",
                           encoding="utf-8")
    print(json.dumps({"events": len(result["events"]), "reward_relations": len(result["reward_relations"]),
                      "reward_groups": len(result["reward_groups"]), "quests": len(result["quest_enemy_levels"])}))


if __name__ == "__main__":
    main()
