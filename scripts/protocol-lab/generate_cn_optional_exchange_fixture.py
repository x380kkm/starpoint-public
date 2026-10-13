# audience: internal
# # generate-cn-optional-exchange-fixture
# 自选活动目录和候选来自客户端最终资源, 日期沿用国服 UTC+8.
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import argparse
import json
from datetime import datetime, timedelta, timezone
from pathlib import Path

from audit_cn_gacha_candidate_assets import decode_multirow_ordered_map
from cn_packaged_resource_reader import PackagedResourceReader


# //// 读取客户端有序主表 [@x380kkm 2026-09-23] ////
def master(reader: PackagedResourceReader, logical: str) -> dict:
    content = reader.read(logical)
    if content is None:
        raise ValueError(f"缺少主表: {logical}")
    return decode_multirow_ordered_map(content)


def timestamp(value: str) -> int | None:
    if value == "(None)":
        return None
    return int(datetime.strptime(value, "%Y-%m-%d %H:%M:%S")
               .replace(tzinfo=timezone(timedelta(hours=8))).timestamp())


def rows(value: list) -> list[list[str]]:
    return [value] if value and isinstance(value[0], str) else value


# //// 将普通自选和宝库的票券与完整候选关联 [@x380kkm 2026-09-23] ////
def exchange_campaigns(reader: PackagedResourceReader, family: str, tickets: dict) -> dict:
    prefix = f"master/campaign/{family}/{family}_campaign"
    campaigns = master(reader, prefix + ".orderedmap")
    equipment = master(reader, prefix + "_equipment.orderedmap")
    characters = master(reader, prefix + "_character.orderedmap") if family == "special_exchange" else {}
    result = {}
    for key, row in campaigns.items():
        kind = int(row[3])
        candidates = [int(value[0]) for value in rows((characters if kind == 0 else equipment)[key])]
        if not candidates:
            raise ValueError(f"活动 {family}:{key} 缺少候选")
        ordinary = family == "special_exchange"
        source = int(row[10]) if ordinary else 1
        result[key] = {
            "name": row[1], "kind": kind,
            "start_at": timestamp(row[4]), "end_at": timestamp(row[5]),
            "exchange_start_at": timestamp(row[6]), "exchange_end_at": timestamp(row[7]),
            "ticket_id": int(row[9]), "candidates": candidates,
            "product_source": source, "price": int(row[11]) if ordinary and row[11] != "(None)" else 0,
            "gacha_ticket_id": tickets[str(kind)] if ordinary and source == 0 else None,
            "reward_count": 1 if ordinary else 5,
        }
    return result


# //// 保留新手自选的活动期限, 档期和角色排除表 [@x380kkm 2026-09-23] ////
def start_dash_campaigns(reader: PackagedResourceReader, tickets: dict) -> dict:
    prefix = "master/campaign/start_dash_exchange/start_dash_exchange_campaign"
    campaigns = master(reader, prefix + ".orderedmap")
    terms = master(reader, prefix + "_term.orderedmap")
    excluded = master(reader, prefix + "_exclude_character.orderedmap")
    result = {}
    for key, row in campaigns.items():
        result[key] = {
            "name": row[1], "start_at": timestamp(row[2]), "end_at": timestamp(row[3]),
            "period_days": int(row[4]), "ticket_id": int(row[6]), "price": int(row[7]),
            "gacha_ticket_id": tickets["0"],
            "terms": {index: timestamp(value[0]) for index, value in terms[key].items()},
            "excluded": {index: [int(value[0]) for value in rows(values)]
                         for index, values in excluded.get(key, {}).items()},
        }
    return result


# //// 按客户端稀有度和限定标记展开新手自选卡池 [@x380kkm 2026-09-23] ////
def start_dash_gachas(reader: PackagedResourceReader) -> dict:
    result = {}
    odds_cache = {}
    for key, row in master(reader, "master/gacha/gacha.orderedmap").items():
        if row[13] != "0" or row[21].lower() != "true":
            continue
        candidates = set()
        for field in (14, 15, 16):
            odds_id = row[field]
            if odds_id not in odds_cache:
                path = f"master/gacha_odds/{odds_id}.orderedmap"
                odds_cache[odds_id] = master(reader, path)[odds_id]
            for entry in odds_cache[odds_id].values():
                if int(entry[1]) in (4, 5) and entry[4].lower() == "false":
                    candidates.add(int(entry[0]))
        if not candidates:
            raise ValueError(f"新手自选卡池 {key} 缺少候选")
        result[key] = {
            "start_at": timestamp(row[29]), "end_at": timestamp(row[30]),
            "list_order": int(row[2]), "candidates": sorted(candidates),
        }
    return result


# //// 核对所有候选和消耗票券的实体主表归属 [@x380kkm 2026-09-23] ////
def validate_entities(reader: PackagedResourceReader, fixture: dict) -> dict:
    characters = set(master(reader, "master/character/character.orderedmap"))
    equipment = set(master(reader, "master/item/equipment.orderedmap"))
    items = set(master(reader, "master/item/item.orderedmap"))
    references = {"characters": set(), "equipment": set(), "items": set()}
    for family in ("special", "treasure"):
        for campaign in fixture[family].values():
            references["characters" if campaign["kind"] == 0 else "equipment"].update(map(str, campaign["candidates"]))
            references["items"].add(str(campaign["ticket_id"]))
            if campaign["gacha_ticket_id"] is not None:
                references["items"].add(str(campaign["gacha_ticket_id"]))
    for campaign in fixture["start_dash"].values():
        references["items"].update(map(str, (campaign["ticket_id"], campaign["gacha_ticket_id"])))
    for gacha in fixture["gachas"].values():
        references["characters"].update(map(str, gacha["candidates"]))
    for kind, definitions in (("characters", characters), ("equipment", equipment), ("items", items)):
        missing = references[kind] - definitions
        if missing:
            raise ValueError(f"自选目录的 {kind} 缺少实体: {sorted(missing)}")
    return {kind: len(values) for kind, values in references.items()}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--cdn-root", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    with PackagedResourceReader(args.package, cdn_root=args.cdn_root) as reader:
        tickets = {"0": 999001, "1": 999004}
        fixture = {
            "special": exchange_campaigns(reader, "special_exchange", tickets),
            "treasure": exchange_campaigns(reader, "treasure_vault_exchange", tickets),
            "start_dash": start_dash_campaigns(reader, tickets),
            "gachas": start_dash_gachas(reader),
        }
        references = validate_entities(reader, fixture)
    args.output.write_text(json.dumps(fixture, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"catalogs": {key: len(value) for key, value in fixture.items()}, "entities": references}))


if __name__ == "__main__":
    main()
