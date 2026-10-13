# audience: internal
# # cn-regional-activity-periods
# 区域活动共享 CN 的身份与奖励定义, 原始活动和子记录时间由区域 master 提供.

# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

from __future__ import annotations

import argparse
import json
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta, timezone
from pathlib import Path

from cn_gacha_banner_assets import decode_ordered_map, hash_cn_asset_path
from regional_zip_range import RemoteZip64Archive
from cn_regional_shop_projection import project_shop


ROOT = Path(__file__).resolve().parents[2]
ARCHIVE_URL = "https://ia601507.us.archive.org/21/items/gacha-archive/Online/World%20Flipper/air.jp.co.cygames.worldflipper-1.780.0.1780000.zip"
EVENT_PREFIXES = {
    "advent:", "ranking:", "story:", "challenge-dungeon:", "world-story:",
    "tower-dungeon:", "expert-single:", "carnival:", "raid:", "rush:",
    "solo-time-attack:", "hard-multi:", "score-attack:", "collect-item:",
}


# //// 读取有身份路径的 master 行和客户端时区 [@x380kkm 2026-09-27] ////
def rows(value, prefix=()):
    if isinstance(value, list):
        yield prefix, value
    else:
        for key, child in value.items():
            yield from rows(child, prefix + (key,))


def timestamp(value):
    try:
        return int(datetime.strptime(value, "%Y-%m-%d %H:%M:%S")
                   .replace(tzinfo=timezone(timedelta(hours=8))).timestamp() * 1000)
    except (ValueError, TypeError):
        return None


def period(row, start, end):
    start_time = timestamp(row[start]) if start is not None and start < len(row) else None
    end_time = timestamp(row[end]) if end is not None and end < len(row) else None
    return (start_time, end_time) if start_time is not None and end_time is not None and start_time < end_time else None


def field_indexes(master):
    indexes = master["synchronized_field_indexes"]
    if master["name"] in {"collect_item_event_mission", "event_mission"}:
        return {index: index - 5 for index in indexes}
    return {index: index for index in indexes}


def time_fields(master, row):
    return {str(target): row[source] for target, source in field_indexes(master).items()
            if source < len(row) and (timestamp(row[source]) is not None
                                     or row[source] in {"", "(None)"})}


def associated_rows(master, source, activity_key):
    index = master.get("activity_id_index")
    condition = master.get("activity_id_filter")
    return {"/".join(key): row for key, row in rows(source)
            if condition is None or row[condition[0]] == condition[1]
            if (row[index] == activity_key if index is not None else key[0] == activity_key)}
# //// /读取有身份路径的 master 行和客户端时区 ////


# //// 将错位商品日期映射到所属活动的本地期别 [@x380kkm 2026-09-29] ////
def canonical_item_windows(items, local_shop, canonical_period, regional_period):
    corrected = {}
    if canonical_period is None:
        return corrected
    for item_id, (start, end) in items.items():
        original = period(local_shop[item_id], 26, 27)
        if original is None or max(original[0], canonical_period[0]) <= min(original[1], canonical_period[1]):
            continue
        projected_start = canonical_period[0] + max(0, start - regional_period[0])
        projected_end = (canonical_period[1] + end - regional_period[1]
                         if end >= regional_period[1]
                         else canonical_period[0] + max(0, end - regional_period[0]))
        corrected[item_id] = (projected_start, projected_end)
    return corrected
# //// /将错位商品日期映射到所属活动的本地期别 ////


# //// 从同身份完整记录集合生成区域时间 [@x380kkm 2026-09-27] ////
def generate_periods(masters, local, regional, quests, server_shop, shop_item_map, references):
    accepted = {}
    excluded = {}
    audit = {}
    roots = [master for master in masters if master.get("quest_category") is None
             and master.get("activity_id_index") is None
             and master["activity_id_prefix"] in EVENT_PREFIXES]
    for master in roots:
        name, prefix = master["name"], master["activity_id_prefix"]
        children = [child for child in masters if child["activity_id_prefix"] == prefix and child != master]
        for key, source in regional.get(name, {}).items():
            canonical = local[name].get(key)
            source_period = period(source, master["start_index"], master["end_index"])
            if not isinstance(canonical, list) or not isinstance(source, list) or source[0] != canonical[0] or source_period is None:
                continue
            canonical_period = period(canonical, master["start_index"], master["end_index"])
            activity_id = prefix + key
            if canonical_period and canonical_period != source_period and max(canonical_period[0], source_period[0]) < min(canonical_period[1], source_period[1]):
                excluded[activity_id] = "overlapping-period"
                continue
            reward_end = timestamp(source[12]) if name == "ranking_event" else max(
                [source_period[1]] + [timestamp(source[index]) for index in master["synchronized_field_indexes"]
                                     if timestamp(source[index]) is not None])
            aggregation_end = timestamp(source[11]) if name == "ranking_event" else source_period[1]
            if reward_end is None or aggregation_end is None:
                excluded[activity_id] = "reward-window"
                continue
            record = {"start_at_ms": source_period[0], "end_at_ms": source_period[1],
                      "aggregation_end_at_ms": aggregation_end,
                      "reward_end_at_ms": reward_end,
                      "masters": {name: {key: time_fields(master, source)}}, "quests": {}, "items": {}}
            reason = None
            for child in children:
                child_name = child["name"]
                left = associated_rows(child, local.get(child_name, {}), key)
                right = associated_rows(child, regional.get(child_name, {}), key)
                if child_name == "event_mission" and left.keys() <= right.keys():
                    right = {row_key: right[row_key] for row_key in left}
                if left.keys() != right.keys():
                    reason = f"{child_name}:record-set"
                    break
                patches = {}
                for row_key, row in right.items():
                    if child_name == "event_mission" and row[0] != left[row_key][0]:
                        reason = f"{child_name}:mission-identity"
                        break
                    if child.get("quest_category") is not None and child.get("start_index") is not None:
                        quest_key = f"{child['quest_category']}:{row[0]}"
                        if row[0] != left[row_key][0] or quest_key not in quests:
                            reason = f"{child_name}:quest-identity"
                            break
                        record["quests"][quest_key] = (
                            timestamp(row[child["start_index"]]) or source_period[0],
                            timestamp(row[child["end_index"]]) or source_period[1],
                        )
                        continue
                    fields = time_fields(child, row)
                    if fields:
                        patches[row_key] = fields
                if reason:
                    break
                if patches:
                    record["masters"][child_name] = patches
            if reason:
                excluded[activity_id] = reason
                continue
            item_windows, shop_audit = project_shop(
                activity_id, local["event_item_shop"], regional.get("event_item_shop", {}),
                server_shop, shop_item_map, references, period,
            )
            audit[activity_id] = shop_audit
            if shop_audit["issues"]:
                excluded[activity_id] = "event_item_shop:reference-or-window-conflict"
                continue
            record["items"] = item_windows
            canonical_activity_id = shop_audit["canonical_activity_id"]
            if canonical_activity_id != activity_id:
                record["canonical_activity_id"] = canonical_activity_id
            owner_key = canonical_activity_id.split(":", 1)[1]
            owner = local[name].get(owner_key)
            owner_period = period(owner, master["start_index"], master["end_index"]) if isinstance(owner, list) else None
            corrected_items = canonical_item_windows(item_windows, local["event_item_shop"], owner_period, source_period)
            if corrected_items:
                record["canonical_items"] = corrected_items
                shop_audit["canonical_item_windows"] = corrected_items
            for times in item_windows.values():
                record["reward_end_at_ms"] = max(record["reward_end_at_ms"], times[1])
            accepted[activity_id] = record
    return dict(sorted(accepted.items())), dict(sorted(excluded.items())), dict(sorted(audit.items()))
# //// /从同身份完整记录集合生成区域时间 ////


# //// 读取区域归档并生成服务端时间资产 [@x380kkm 2026-09-27] ////
def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cache-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, default=ROOT / "core/personal-service/assets/cn-regional-activity-periods.json")
    parser.add_argument("--audit-output", type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    manifest = json.loads((ROOT / "core/personal-service/assets/cn-activity-master-projection.json").read_text(encoding="utf-8"))
    masters = [master for master in manifest["masters"]
               if master["activity_id_prefix"] in EVENT_PREFIXES or master["name"] == "event_item_shop"]
    local = {master["name"]: decode_ordered_map((ROOT / "core/personal-service/assets/cn-activity-masters" / (master["name"] + ".orderedmap")).read_bytes()) for master in masters}
    archive = RemoteZip64Archive(ARCHIVE_URL, args.cache_root)
    def load(master):
        logical = "master/shop/event_item_shop.orderedmap" if master["name"] == "event_item_shop" else (
            "master/reward/event/collect_item_event.orderedmap" if master["name"] == "collect_item_event" else (
                f"master/mission/{master['name']}.orderedmap" if master["name"] in {"collect_item_event_mission", "event_mission"} else f"master/quest/event/{master['name']}.orderedmap"))
        entry = archive.find_hashed_asset(hash_cn_asset_path(logical))
        return master["name"], decode_ordered_map(archive.read(entry)) if entry else {}
    with ThreadPoolExecutor(max_workers=4) as workers:
        regional = dict(workers.map(load, masters))
    quests = json.loads((ROOT / "core/personal-service/assets/cn-single-battle.json").read_text(encoding="utf-8"))["quests"]
    server_shop = json.loads((ROOT / "assets/event_item_shop.json").read_text(encoding="utf-8"))
    shop_item_map = json.loads((ROOT / "assets/event_item_shop_id_map.json").read_text(encoding="utf-8"))
    references = {
        0: {str(item["id"]) for item in json.loads((ROOT / "assets/cn_item_catalog.json").read_text(encoding="utf-8"))["items"]},
        3: set(json.loads((ROOT / "assets/character.json").read_text(encoding="utf-8"))),
        4: set(json.loads((ROOT / "assets/equipment_dissolve.json").read_text(encoding="utf-8"))),
    }
    records, excluded, audit = generate_periods(masters, local, regional, quests, server_shop, shop_item_map, references)
    serialized = json.dumps(records, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n"
    if args.check:
        try:
            artifact_matches = args.output.read_text(encoding="utf-8") == serialized
        except (OSError, ValueError):
            artifact_matches = False
    else:
        args.output.write_text(serialized, encoding="utf-8")
        artifact_matches = True
    summary = {"output": str(args.output), "period_count": len(records),
               "quest_count": sum(len(record["quests"]) for record in records.values()),
               "item_count": sum(len(record["items"]) for record in records.values()),
               "shop_categories": dict(Counter(item["category"] for item in audit.values())),
               "shop_issue_count": sum(len(item["issues"]) for item in audit.values()),
               "excluded": excluded, "artifact_matches": artifact_matches}
    if args.audit_output:
        report = {"summary": summary, "events": audit, "regional_archive": ARCHIVE_URL,
                  "cn_reference_assets": ["assets/cn_item_catalog.json", "assets/character.json", "assets/equipment_dissolve.json"]}
        args.audit_output.parent.mkdir(parents=True, exist_ok=True)
        args.audit_output.write_text(json.dumps(report, ensure_ascii=False, sort_keys=True, indent=2) + "\n", encoding="utf-8")
        summary["audit_output"] = str(args.audit_output)
    print(json.dumps(summary, ensure_ascii=False))
    return 1 if args.check and (not artifact_matches or excluded) else 0


if __name__ == "__main__":
    raise SystemExit(main())
# //// /读取区域归档并生成服务端时间资产 ////
