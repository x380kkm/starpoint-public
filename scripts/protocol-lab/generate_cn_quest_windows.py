# audience: internal
# # generate-cn-quest-windows
# 关卡窗口关联父活动与子关卡开放日期. 等价日常组保留相同奖励和入场条件的完整关卡数据.
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import argparse
import json
from collections import defaultdict
from pathlib import Path

from audit_cn_gacha_candidate_assets import decode_multirow_ordered_map
from cn_boss_release_dates import apply_boss_master_releases
from cn_packaged_resource_reader import PackagedResourceReader
from generate_cn_battle_reward_rules import quest_rows, rows
from generate_cn_shop_selection_fixture import timestamp

QUEST_NAME_COLUMNS = {6: 1, 21: 4, 22: 4, 23: 4, 24: 4, 25: 3, 27: 4}


def time_value(value):
    return None if value in ("", "(None)") else timestamp(value) * 1000


def daily_quest_signature(row, event_key):
    normalized = list(row)
    for kind_index, event_index, multiplied_index in ((6, 7, 10), (11, 12, 15)):
        if normalized[kind_index] == "3" and normalized[event_index] == event_key:
            normalized[event_index] = "self"
            normalized[multiplied_index] = str(int(normalized[multiplied_index]) % 1000)
    return json.dumps(normalized[1:4] + normalized[6:], ensure_ascii=False)


# //// 从父活动和嵌套关卡主表读取开放日期 [@x380kkm 2026-09-26] ////
def build_catalog(asset_root, reader):
    manifest = json.loads((asset_root / "cn-activity-master-projection.json").read_text(encoding="utf-8"))
    masters = {entry["name"]: entry for entry in manifest["masters"]}
    decoded = {}
    events = {}
    quests = {}
    daily_candidates = defaultdict(list)

    def load(name):
        if name not in decoded:
            decoded[name] = decode_multirow_ordered_map((asset_root / masters[name]["binary_path"]).read_bytes())
        return decoded[name]

    for definition in manifest["masters"]:
        category = definition.get("quest_category")
        parent_name = definition.get("parent_activity_master")
        if not category or not parent_name or not definition["name"].endswith("_quest"):
            continue
        parent_definition = masters[parent_name]
        parents = load(parent_name)
        for event_key, entries in load(definition["name"]).items():
            parent = parents[event_key]
            event_id = definition["activity_id_prefix"] + event_key
            events[event_id] = {
                "start_at_ms": time_value(parent[parent_definition["start_index"]]),
                "end_at_ms": time_value(parent[parent_definition["end_index"]]),
                "weekdays": [day == "true" for day in parent[8:15]] if category == 6 else [],
            }
            event_quests = list(rows(entries))
            for row in event_quests:
                quest_id = int(row[0])
                if row[definition["start_index"]] not in ("", "(None)") and "-" not in row[definition["start_index"]]:
                    raise ValueError(f"关卡开放字段不匹配: {definition['name']} {quest_id} {list(enumerate(row[:14]))}")
                quests[f"{category}:{quest_id}"] = {
                    "activity_id": event_id,
                    "name": row[QUEST_NAME_COLUMNS.get(category, 2)],
                    "start_at_ms": time_value(row[definition["start_index"]]),
                    "end_at_ms": time_value(row[definition["end_index"]]),
                    "manage": category in (6, 14),
                }
            if category == 6:
                signatures = {daily_quest_signature(row, event_key): row for row in event_quests}
                daily_candidates[json.dumps(parent[1:8], ensure_ascii=False)].append((event_id, signatures))

    logical = "master/quest/boss_battle_quest.orderedmap"
    boss_binary = apply_boss_master_releases(reader.read(logical))
    bosses = decode_multirow_ordered_map(boss_binary)
    difficulties = decode_multirow_ordered_map(reader.read("master/quest/quest_rank.orderedmap"))
    for key, row in quest_rows(2, bosses):
        quests[f"2:{key}"] = {
            "activity_id": None,
            "name": row[2].replace("::quest_rank::", difficulties[row[107]][1]),
            "start_at_ms": time_value(row[5]),
            "end_at_ms": time_value(row[6]),
            "manage": row[123].lower() == "false",
        }
    groups = []
    for candidates in daily_candidates.values():
        if len(candidates) < 2:
            continue
        candidates.sort(key=lambda pair: (len(pair[1]), events[pair[0]]["start_at_ms"], int(pair[0].split(":")[1])))
        canonical, target_rows = candidates[-1]
        if not all(set(candidate).issubset(target_rows) for _, candidate in candidates):
            continue
        groups.append([event_id for event_id, _ in candidates])
        events[canonical]["coverage_start_at_ms"] = min(events[event_id]["start_at_ms"] for event_id, _ in candidates)
        for signature, row in target_rows.items():
            starts = [time_value(candidate[signature][4]) for _, candidate in candidates if signature in candidate]
            quests[f"6:{row[0]}"]["coverage_start_at_ms"] = min(starts)
    return {
        "events": events,
        "quests": quests,
        "daily_groups": groups,
        "boss_master": {
            "name": "boss_battle_quest", "activity_id_prefix": "quest:2:",
            "entry_path": reader.locate(logical)["entry"],
            "binary_path": "cn-activity-masters/boss_battle_quest.orderedmap",
            "start_index": 5, "end_index": 6, "activity_id_index": 0,
            "synchronized_field_indexes": [5, 6], "quest_category": 2,
        },
    }, boss_binary


# //// 输出关卡窗口目录和原始领主主表 [@x380kkm 2026-09-24] ////
def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("package", type=Path)
    parser.add_argument("--asset-root", type=Path, default=Path("core/personal-service/assets"))
    args = parser.parse_args()
    with PackagedResourceReader(args.package) as reader:
        catalog, boss_binary = build_catalog(args.asset_root, reader)
    (args.asset_root / "cn-quest-windows.json").write_text(json.dumps(catalog, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    (args.asset_root / "cn-activity-masters/boss_battle_quest.orderedmap").write_bytes(boss_binary)
    print(json.dumps({"quests": len(catalog["quests"]), "events": len(catalog["events"]), "daily_groups": catalog["daily_groups"]}, ensure_ascii=False))


if __name__ == "__main__":
    main()
