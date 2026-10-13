# audience: internal
# # generate-cn-battle-reward-rules
# 此脚本从 CN 主数据提取关卡收益, 评级, 抽取次数, 掉落权重, 竞速阶段奖励和剧情败北战关卡.
# 奖励索引沿用主表位置, 元素奖励按关卡推荐属性解析.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import argparse
import json
from pathlib import Path

from cn_gacha_banner_assets import EntityCatalog, decode_ordered_map, read_logical_assets
from cn_battle_quest_fields import quest_fields
from cn_event_currency_rewards import project_event_currencies
from cn_packaged_resource_reader import PackagedResourceReader
from generate_cn_shop_selection_fixture import master

QUEST_MASTERS = (
    ("master/quest/main_quest.orderedmap", 1, 49),
    ("master/quest/ex_quest.orderedmap", 4, 49),
    ("master/quest/character_quest.orderedmap", 3, 51),
    ("master/quest/event/story_event_single_quest.orderedmap", 10, 51),
    ("master/quest/event/world_story_event_quest.orderedmap", 18, 50),
    ("master/quest/event/expert_single_event_quest.orderedmap", 21, 52),
)
RANKING_MASTER = "master/quest/event/ranking_event_single_quest.orderedmap"
PHASE_MASTER = "master/reward/phase_quest_reward.orderedmap"
SCORE_MASTER = "master/reward/score_reward.orderedmap"
RARE_MASTER = "master/reward/rare_score_reward.orderedmap"
ELEMENT_MASTER = "master/reward/reward_element_map.orderedmap"
CONFIG_MASTERS = ("master/config/config_iosbundled.orderedmap", "master/config/config.orderedmap")
ENEMY_ELEMENTS = {0: 3, 1: 0, 2: 1, 3: 2, 4: 5, 5: 4}
BATTLE_MASTERS = (
    (1, "master/quest/main_quest.orderedmap", (88, 89, 90, 91, 92), 72),
    (2, "master/quest/boss_battle_quest.orderedmap", (88, 89, 90, 91, 92), 72),
    (3, "master/quest/character_quest.orderedmap", (90, 91, 92, 93, 94), 74),
    (4, "master/quest/ex_quest.orderedmap", (88, 89, 90, 91, 92), 72),
    (6, "master/quest/event/daily_week_event_quest.orderedmap", (66,) * 5, None),
    (7, "master/quest/event/advent_event_quest.orderedmap", (94, 95, 96, 97, 98), 78),
    (10, "master/quest/event/story_event_single_quest.orderedmap", (90, 91, 92, 93, 94), 74),
    (11, RANKING_MASTER, (), 68),
    (13, "master/quest/event/challenge_dungeon_event_quest.orderedmap", (89, 90, 91, 92, 93), 73),
    (14, "master/quest/event/daily_exp_mana_event_quest.orderedmap", (67,) * 5, None),
    (15, "master/quest/practice/practice_quest.orderedmap", (89, 90, 91, 92, 93), 73),
    (18, "master/quest/event/world_story_event_quest.orderedmap", (89, 90, 91, 92, 93), 73),
    (19, "master/quest/event/world_story_event_boss_battle_quest.orderedmap", (88, 89, 90, 91, 92), 72),
    (20, "master/quest/event/tower_dungeon_event_quest.orderedmap", (82,) * 5, 70),
    (21, "master/quest/event/expert_single_event_quest.orderedmap", (91, 92, 93, 94, 95), 75),
    (22, "master/quest/event/carnival_event_quest.orderedmap", (81,) * 5, 69),
    (23, "master/quest/event/raid_event_quest.orderedmap", (82,) * 5, 70),
    (24, "master/quest/event/rush_event_quest.orderedmap", (81,) * 5, 69),
    (25, "master/quest/event/solo_time_attack_event_quest.orderedmap", (84,) * 5, 72),
    (26, "master/quest/event/hard_multi_event_quest.orderedmap", (89, 90, 91, 92, 93), 73),
    (27, "master/quest/event/score_attack_event_quest.orderedmap", (85,) * 5, 73),
)
SCORE_GROUP_COLUMNS = {
    1: 70, 2: 70, 3: 72, 4: 70, 6: 65, 7: 76, 10: 72, 13: 71, 14: 66,
    15: 71, 18: 71, 19: 70, 20: 69, 21: 73, 22: 68, 23: 69, 24: 68, 25: 71, 26: 71, 27: 72,
}
ENTRY_MODE_COLUMNS = {
    1: 55, 2: 55, 3: 57, 4: 55, 6: 50, 7: 61, 10: 57, 11: 52, 13: 56,
    14: 51, 15: 56, 18: 56, 19: 55, 20: 54, 21: 58, 22: 53, 23: 54,
    24: 53, 25: 56, 26: 56, 27: 57,
}


# //// 读取任务与奖励主表 [@x380kkm 2026-09-21] ////
def read_masters(cdn_root):
    catalog = EntityCatalog.load(cdn_root, cdn_root / "entities/PathFile.csv")
    paths = [path for path, _, _ in QUEST_MASTERS]
    paths.extend(path for _, path, _, _ in BATTLE_MASTERS)
    paths.extend((RANKING_MASTER, PHASE_MASTER, SCORE_MASTER, RARE_MASTER, ELEMENT_MASTER))
    paths.extend(CONFIG_MASTERS)
    records = {}
    for path in paths:
        record = catalog.find(path)
        if record is None:
            if path in CONFIG_MASTERS:
                continue
            raise ValueError(f"Missing battle reward master: {path}")
        records[path] = record
    raw, missing = read_logical_assets(cdn_root, records)
    if missing:
        raise ValueError(f"Missing battle reward archives: {missing}")
    return {path: decode_ordered_map(data) for path, data in raw.items()}


def rows(value):
    if isinstance(value, dict):
        for child in value.values():
            yield from rows(child)
    else:
        yield value


def quest_rows(category, master):
    if category in (3, 15):
        return master.items()
    return ((row[0], row) for row in rows(master))


# //// 解析每次入场道具和体力消耗 [@x380kkm 2026-09-22] ////
def entry_cost(category, row):
    mode_column = ENTRY_MODE_COLUMNS[category]
    mode = row[mode_column]
    if mode not in ("", "(None)", "0", "1"):
        raise ValueError(f"Unknown entry item mode: {category}:{row[0]}:{mode}")
    item_id = None
    item_count = 0
    if mode == "1":
        item_ids = row[mode_column + 1].split(",")
        item_counts = row[mode_column + 2].split(",")
        if len(item_ids) != 1 or len(item_counts) != 1:
            raise ValueError(f"Multiple entry item kinds: {category}:{row[0]}")
        item_id, item_count = int(item_ids[0]), int(item_counts[0])
        if item_id <= 0 or item_count <= 0:
            raise ValueError(f"Invalid entry item: {category}:{row[0]}")
    stamina = row[mode_column + 14]
    return {"entry_item_id": item_id, "entry_item_count": item_count,
            "stamina_cost": int(stamina) if stamina not in ("", "(None)") else 0}


# //// 保留奖励位置并解析普通与稀有掉落 [@x380kkm 2026-09-21] ////
def reward_id(kind, value, element, element_map):
    if value in ("", "(None)"):
        return None
    if kind in (6, 7) and element is not None:
        map_kind = "1" if kind == 6 else "2"
        return int(element_map[map_kind][value][str(ENEMY_ELEMENTS[element])][0])
    return int(value)


def score_rewards(group, element=None, element_map=None):
    result = []
    for position, row in group.items():
        kind = int(row[1])
        if kind == 0:
            reward_type = int(row[2])
            if reward_type not in range(8):
                raise ValueError(f"Unsupported phase reward kind: {reward_type}")
            result.append({
                "type": kind, "reward_type": reward_type,
                "id": reward_id(reward_type, row[3], element, element_map),
                "count": 1 if reward_type == 2 else int(row[4]), "field5": int(row[5]), "position": int(position),
            })
        elif kind == 1:
            result.append({"type": kind, "id": int(row[6]),
                           "rarity": float(row[7]), "position": int(position)})
        else:
            raise ValueError(f"Unsupported phase lottery kind: {kind}")
    return result


def rare_rewards(group, element=None, element_map=None):
    result = []
    for position, row in group.items():
        kind = int(row[1])
        if kind not in range(8):
            raise ValueError(f"Unsupported phase rare reward kind: {kind}")
        result.append({"type": kind, "position": int(position),
                       "element_rarity": int(row[2]) if kind in (6, 7) and element is None else None,
                       "id": reward_id(kind, row[2], element, element_map),
                       "count": 1 if kind == 2 else int(row[3]), "rarity": float(row[4])})
    return result


# //// 从下载配置或包内配置读取多人普通掉落次数倍率 [@x380kkm 2026-09-28] ////
def multiplayer_common_reward_multiplier(masters, cdn_root, package):
    configs = {key: row for path in CONFIG_MASTERS for key, row in masters.get(path, {}).items()}
    if "1" not in configs:
        if package is None:
            raise ValueError("CDN 缺少普通掉落配置, 请通过 --package 提供包含 bundled 配置的客户端包")
        with PackagedResourceReader(package, cdn_root=cdn_root) as reader:
            configs = {key: row for path in CONFIG_MASTERS for key, row in master(reader, path).items()}
    return float(configs["1"][19])


# //// 生成阶段奖励和剧情败北战配置 [@x380kkm 2026-09-21] ////
def generate(cdn_root, package=None):
    masters = read_masters(cdn_root)
    assets = Path(__file__).resolve().parents[2] / "assets"
    currency_projection = project_event_currencies(
        masters, json.loads((assets / "cn_item_catalog.json").read_text(encoding="utf-8")),
        json.loads((assets / "event_item_shop.json").read_text(encoding="utf-8")),
    )
    masters.update(currency_projection["masters"])
    common_reward_multiplier = multiplayer_common_reward_multiplier(masters, cdn_root, package)
    phase_quests = {}
    for quest in rows(masters[RANKING_MASTER]):
        group = masters[PHASE_MASTER][quest[67]]
        element = int(quest[68])
        phases = []
        rare_groups = {}
        for phase_id, row in group.items():
            score_group_id = int(row[2]) if row[2] not in ("", "(None)") else None
            rewards = score_rewards(masters[SCORE_MASTER][str(score_group_id)], element,
                                    masters[ELEMENT_MASTER]) if score_group_id else []
            for reward in rewards:
                if reward["type"] == 1:
                    group_id = str(reward["id"])
                    group_rewards = rare_rewards(masters[RARE_MASTER][group_id], element,
                                                 masters[ELEMENT_MASTER])
                    rare_groups[group_id] = group_rewards
            phases.append({
                "phase": int(phase_id), "score_reward_group_id": score_group_id,
                "score_reward_count": int(row[3]), "rank_point_reward": int(row[4]),
                "character_exp_reward": int(row[5]), "mana_reward": int(row[6]),
                "pool_exp_reward": int(row[7]), "score_rewards": rewards,
            })
        phase_quests[f"11:{quest[0]}"] = {
            "phases": sorted(phases, key=lambda phase: phase["phase"]),
            "rare_reward_groups": dict(sorted(rare_groups.items(), key=lambda pair: int(pair[0]))),
        }
    lose_quests = [f"{category}:{quest_id}" for path, category, kind_column in QUEST_MASTERS
                   for quest_id, row in quest_rows(category, masters[path]) if row[kind_column] == "2"]
    quest_rewards = {}
    for category, path, count_columns, element_column in BATTLE_MASTERS:
        for quest_id, row in quest_rows(category, masters[path]):
            counts = [int(row[index]) if row[index] not in ("", "(None)") else 0 for index in count_columns]
            if any(count < 0 for count in counts):
                raise ValueError(f"Negative reward count: {path}:{row[0]}")
            score_group = row[SCORE_GROUP_COLUMNS[category]] if category in SCORE_GROUP_COLUMNS else "(None)"
            quest_rewards[f"{category}:{quest_id}"] = {
                "master_fields": quest_fields(category, row),
                **entry_cost(category, row),
                "score_reward_group_id": int(score_group) if score_group not in ("", "(None)") else None,
                "score_reward_counts": counts or [0] * 5,
                "element": int(row[element_column]) if element_column is not None and row[element_column] not in ("", "(None)") else None,
            }
    return {
        "common_reward_multiplier_by_multi_play_mode": common_reward_multiplier,
        "currency_projection": {"score_group_count": len(currency_projection["score_group_origins"]),
                                "rare_group_count": len(currency_projection["rare_group_origins"])},
        "phase_quests": phase_quests, "lose_quests": sorted(lose_quests),
        "quest_rewards": quest_rewards,
        "reward_element_map": {kind: {rarity: {element: [int(value) for value in row]
                                                for element, row in elements.items()}
                                      for rarity, elements in rarities.items()}
                               for kind, rarities in masters[ELEMENT_MASTER].items()},
        "score_reward_groups": {key: score_rewards(group) for key, group in masters[SCORE_MASTER].items()},
        "rare_reward_groups": {key: rare_rewards(group) for key, group in masters[RARE_MASTER].items()},
    }


# //// 写入个人服务战斗奖励配置 [@x380kkm 2026-09-21] ////
def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cdn-root", required=True, type=Path)
    parser.add_argument("--package", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    result = generate(args.cdn_root, args.package)
    args.output.write_text(json.dumps(result, ensure_ascii=False, separators=(",", ":")) + "\n", encoding="utf-8")
    print(json.dumps({"phase_quests": len(result["phase_quests"]),
                      "lose_quests": result["lose_quests"],
                      "quest_rewards": len(result["quest_rewards"]),
                      "phase_count": sum(len(quest["phases"]) for quest in result["phase_quests"].values()),
                      "output": str(args.output)}, ensure_ascii=False))


if __name__ == "__main__":
    main()
