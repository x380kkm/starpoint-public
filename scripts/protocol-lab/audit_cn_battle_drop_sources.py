# audience: internal
# # cn-battle-drop-sources
# 当前客户端资源主表, 共用收益规则和打包关卡保持奖励位置与引用一致, 无普通掉落的关卡单独列出首通收益.
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import argparse
import json
from collections import Counter
from pathlib import Path

from generate_cn_battle_reward_rules import ENEMY_ELEMENTS, generate


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def normalized_score_reward(reward, quest, source):
    normalized = {key: value for key, value in reward.items() if value is not None}
    kind = reward.get("reward_type")
    if kind in (6, 7):
        rarity = str(reward["id"])
        normalized["element_rarity"] = reward["id"]
        normalized["id"] = source["reward_element_map"]["1" if kind == 6 else "2"]\
            [rarity][str(ENEMY_ELEMENTS[quest.get("element", 0)])][0]
    return normalized


# //// 从客户端主表重新生成收益规则并核对打包掉落引用 [@x380kkm 2026-09-28] ////
def audit(cdn_root, repository, package=None):
    assets = repository / "core/personal-service/assets"
    source = generate(cdn_root, package)
    stored = read_json(assets / "cn-battle-reward-rules.json")
    if source != stored:
        changed = [key for key in set(source) | set(stored) if source.get(key) != stored.get(key)]
        raise ValueError(f"Battle reward rules differ from current client masters: {changed}")
    fixture = read_json(assets / "cn-single-battle.json")
    capacities = read_json(assets / "cn-shop-capacities.json")["items"]
    categories = Counter()
    rare_groups = set()
    no_base_drops = []
    for key, quest in fixture["quests"].items():
        rule = source["quest_rewards"][key]
        group_id = None if quest["category"] == 27 else rule["score_reward_group_id"]
        expected_counts = rule["score_reward_counts"] if group_id is not None else [0] * 5
        if quest.get("score_reward_group_id") != group_id or quest["score_reward_counts"] != expected_counts:
            raise ValueError(f"Quest drop group or ranked counts differ from source: {key}")
        if group_id is None:
            if quest["category"] == 18 and quest["stamina_cost"] > 0:
                no_base_drops.append({"quest": key, "name": quest["name"],
                                      "clear_reward": quest.get("clear_reward"),
                                      "s_plus_reward": quest.get("s_plus_reward")})
            continue
        source_group = source["score_reward_groups"][str(group_id)]
        expected_group = [normalized_score_reward(entry, quest, source) for entry in source_group]
        if not source_group or quest["score_rewards"] != expected_group:
            raise ValueError(f"Quest rewards or positions differ from source: {key}")
        categories[quest["category"]] += 1
        for reward in quest["score_rewards"]:
            if reward["type"] == 1:
                rare_groups.add(str(reward["id"]))
            else:
                if reward["count"] <= 0 or reward["field5"] <= 0:
                    raise ValueError(f"Common reward has empty amount or weight: {key}:{reward['position']}")
                if reward["reward_type"] in (0, 6, 7) and str(reward["id"]) not in capacities:
                    raise ValueError(f"Common reward item is absent from client master: {key}:{reward['id']}")
        for reward in source_group:
            if reward["type"] != 1:
                continue
            group = source["rare_reward_groups"][str(reward["id"])]
            if not group or abs(sum(entry["rarity"] for entry in group) - 1) > 1e-8:
                raise ValueError(f"Rare reward probabilities are incomplete: {reward['id']}")
            for entry in group:
                item = entry["id"]
                kind = entry["type"]
                if kind in (6, 7):
                    item = source["reward_element_map"]["1" if kind == 6 else "2"]\
                        [str(entry["element_rarity"])][str(ENEMY_ELEMENTS[quest.get("element", 0)])][0]
                if kind in (0, 6, 7) and str(item) not in capacities:
                    raise ValueError(f"Rare reward item is absent from client master: {key}:{item}")
    if rare_groups != set(fixture["rare_reward_groups"]):
        raise ValueError("Packaged rare reward groups differ from quest references")
    for group_id in rare_groups:
        expected = [{key: value for key, value in entry.items() if value is not None}
                    for entry in source["rare_reward_groups"][group_id]]
        if fixture["rare_reward_groups"][group_id] != expected:
            raise ValueError(f"Rare reward records differ from current client master: {group_id}")
    return {"source_quests": len(source["quest_rewards"]), "packaged_quests": len(fixture["quests"]),
            "reward_bearing_categories": dict(sorted(categories.items())), "rare_reward_groups": len(rare_groups),
            "world_story_without_base_drops": sorted(no_base_drops, key=lambda row: row["quest"])}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cdn-root", type=Path, required=True)
    parser.add_argument("--package", type=Path)
    parser.add_argument("--repository", type=Path, default=Path(__file__).resolve().parents[2])
    args = parser.parse_args()
    print(json.dumps(audit(args.cdn_root, args.repository, args.package), ensure_ascii=False))


if __name__ == "__main__":
    main()
