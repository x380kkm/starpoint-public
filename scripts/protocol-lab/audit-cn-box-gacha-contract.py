# audience: internal
# # audit-cn-box-gacha-contract
# 箱池成本, 箱序, 奖励内容和重置类型与客户端 CN 主表逐项核对.
# 引用验证读取角色, 装备和物品主表, 奖励类型验证读取客户端 Values 类和两个后端分派源码.
# 运行需要 Pillow, CN CDN 实体目录, 对应资源归档和客户端生成类源码. 报告中的任一问题使进程返回 1.

# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

from __future__ import annotations

import argparse
import json
from collections import defaultdict
from pathlib import Path

from audit_cn_gacha_candidate_assets import decode_multirow_ordered_map
from cn_box_gacha_contract_sources import audit_dispatch, client_schema, function_json, included_json
from cn_gacha_banner_assets import EntityCatalog, read_logical_assets


REPOSITORY = Path(__file__).resolve().parents[2]
MASTER_PATHS = {
    "pools": "master/box_gacha/box_gacha.orderedmap",
    "boxes": "master/box_gacha/box.orderedmap",
    "rewards": "master/box_gacha/box_reward.orderedmap",
    "Item": "master/item/item.orderedmap",
    "Equipment": "master/item/equipment.orderedmap",
    "Character": "master/character/character.orderedmap",
}


# //// 按当前实体目录读取完整箱池和引用主表 [@x380kkm 2026-09-28] ////
def read_masters(cdn: Path, entity: Path | None, issues: list) -> tuple[dict, str]:
    catalog = EntityCatalog.load(cdn, entity)
    records = {}
    for path in MASTER_PATHS.values():
        record = catalog.find(path)
        if record is None:
            issues.append({"kind": "missing_master_entity", "path": path})
        else:
            records[path] = record
    assets, missing = read_logical_assets(cdn, records)
    issues.extend({"kind": "missing_master_archive", "path": path} for path in missing)
    return ({name: decode_multirow_ordered_map(assets[path]) for name, path in MASTER_PATHS.items() if path in assets},
        str(catalog.manifest_path))


def compare_keys(expected: dict, actual: dict, location: str, issues: list) -> None:
    for key in sorted(expected.keys() - actual.keys()):
        issues.append({"kind": "missing_server_record", "path": f"{location}/{key}"})
    for key in sorted(actual.keys() - expected.keys()):
        issues.append({"kind": "server_only_record", "path": f"{location}/{key}"})


def compare_value(expected, actual, location: str, issues: list) -> None:
    if type(expected) is not type(actual) or expected != actual:
        issues.append({"kind": "value_mismatch", "path": location, "expected": expected, "actual": actual})


def mapping(value, location: str, issues: list) -> dict:
    if isinstance(value, dict):
        return value
    issue = {"kind": "invalid_record_map", "path": location, "actualType": type(value).__name__}
    if issue not in issues:
        issues.append(issue)
    return {}


def read_number(row: list, index: int) -> int:
    return int(row[index])


# //// 校验奖励和抽取费用的真实主表引用 [@x380kkm 2026-09-28] ////
def check_reference(masters: dict, semantic: str, target: int, location: str, references: dict, issues: list) -> None:
    references[semantic].setdefault(target, []).append(location)
    if str(target) not in masters.get(semantic, {}):
        issues.append({"kind": "missing_referenced_definition", "path": location,
            "master": MASTER_PATHS[semantic], "id": target})


def reward_rows(rows: dict, fields: dict, location: str, issues: list) -> dict:
    result = {}
    for key, row in rows.items():
        try:
            reward_id = str(read_number(row, fields["key"]))
            value = {name: read_number(row, fields[name]) for name in ("type", "count", "available", "tier")}
            if row[fields["id"]] not in ("", "(None)"):
                value["id"] = read_number(row, fields["id"])
            if reward_id in result:
                issues.append({"kind": "duplicate_master_reward", "path": f"{location}/{reward_id}"})
            result[reward_id] = value
        except (IndexError, TypeError, ValueError) as error:
            issues.append({"kind": "invalid_master_reward", "path": f"{location}/{key}", "error": str(error)})
    return result


# //// 对照箱序与全部奖励字段并汇总真实引用 [@x380kkm 2026-09-28] ////
def audit_boxes(pool_id: str, masters: dict, service: dict, schema: dict, references: dict, used: dict, issues: list) -> tuple[int, int]:
    boxes = masters["boxes"].get(pool_id, {})
    source_rewards = masters["rewards"].get(pool_id, {})
    server_rewards = mapping(service["rewards"].get(pool_id, {}), f"{pool_id}/rewards", issues)
    pool = mapping(service["pools"].get(pool_id, {}), f"{pool_id}/pool", issues)
    available = mapping(pool.get("availableCounts", {}), f"{pool_id}/availableCounts", issues)
    reset = mapping(service["reset"].get(pool_id, {}), f"{pool_id}/reset", issues)
    for name, actual in (("master-rewards", source_rewards), ("rewards", server_rewards), ("availableCounts", available), ("reset", reset)):
        compare_keys(boxes, actual, f"{pool_id}/{name}", issues)
    order = sorted(server_rewards, key=lambda key: (not key.isdecimal(), int(key) if key.isdecimal() else key))
    rewards_count = 0
    for box_id, box in boxes.items():
        location = f"{pool_id}/{box_id}"
        try:
            previous = box[schema["box"]["previous"]]
            if previous not in ("", "(None)") and (previous not in order or box_id not in order or order.index(previous) >= order.index(box_id)):
                issues.append({"kind": "box_order_mismatch", "path": location, "requiredBox": previous, "serverOrder": order})
            compare_value(read_number(box, schema["box"]["resetKind"]), reset.get(box_id), f"{location}/resetKind", issues)
        except (IndexError, TypeError, ValueError) as error:
            issues.append({"kind": "invalid_master_box", "path": location, "error": str(error)})
        expected = reward_rows(source_rewards.get(box_id, {}), schema["reward"], location, issues)
        actual = mapping(server_rewards.get(box_id, {}), f"{location}/rewards", issues)
        compare_keys(expected, actual, location, issues)
        compare_value(sum(row["available"] for row in expected.values()), available.get(box_id), f"{location}/availableCounts", issues)
        rewards_count += len(expected)
        for reward_id, reward in expected.items():
            path = f"{location}/{reward_id}"
            actual_reward = mapping(actual.get(reward_id, {}), path, issues)
            for field in ("type", "id", "count", "available", "tier"):
                compare_value(reward.get(field), actual_reward.get(field), f"{path}/{field}", issues)
            used[reward["type"]].append(path)
            semantic = schema["reward"]["names"].get(reward["type"])
            if semantic in ("Item", "Equipment", "Character"):
                check_reference(masters, semantic, reward.get("id", 0), path, references, issues)
            if reward["count"] <= 0 or reward["available"] < 0:
                issues.append({"kind": "invalid_reward_quantity", "path": path, "reward": reward})
    return len(boxes), rewards_count


# //// 核对全部池的费用和服务分派支持范围 [@x380kkm 2026-09-28] ////
def audit(args) -> dict:
    issues = []
    schema = client_schema(args.client_source_root)
    masters, manifest = read_masters(args.cdn_root, args.entity, issues)
    if not all(name in masters for name in MASTER_PATHS):
        return {"issues": issues, "entity": manifest}
    service_root = args.repository_root / "core/personal-service/src/cn_box_gacha"
    inputs = {
        "pools": included_json(service_root / "catalog.rs", "BOX_GACHA_ASSET"),
        "rewards": included_json(service_root / "catalog.rs", "BOX_REWARD_ASSET"),
        "reset": included_json(service_root / "rules.rs", "RESET_KINDS_ASSET"),
    }
    service = {key: value[1] for key, value in inputs.items()}
    for name, document in service.items():
        compare_keys(masters["pools"], document, name, issues)
    for name in ("boxes", "rewards"):
        compare_keys(masters["pools"], masters[name], f"master/{name}", issues)
    ordering_source = (service_root / "catalog.rs").read_text(encoding="utf-8")
    if "box_ids.sort_unstable();" not in ordering_source:
        issues.append({"kind": "unresolved_service_box_order", "source": str(service_root / "catalog.rs")})
    references = defaultdict(dict)
    used = defaultdict(list)
    boxes_count = rewards_count = 0
    for pool_id, row in masters["pools"].items():
        fields = schema["pool"]
        try:
            cost_kind = read_number(row, fields["type"])
            semantic = fields["names"].get(cost_kind)
            item_id, cost = read_number(row, fields["id"]), read_number(row, fields["count"])
            actual = mapping(service["pools"].get(pool_id, {}), f"{pool_id}/pool", issues)
            compare_value(item_id, actual.get("itemId"), f"{pool_id}/itemId", issues)
            compare_value(cost, actual.get("count"), f"{pool_id}/cost", issues)
            if semantic != "Item" or cost <= 0:
                issues.append({"kind": "unsupported_box_cost", "path": pool_id, "type": cost_kind, "semantic": semantic, "count": cost})
            else:
                check_reference(masters, "Item", item_id, f"{pool_id}/cost", references, issues)
        except (IndexError, TypeError, ValueError) as error:
            issues.append({"kind": "invalid_master_pool", "path": pool_id, "error": str(error)})
        count, rewards = audit_boxes(pool_id, masters, service, schema, references, used, issues)
        boxes_count += count
        rewards_count += rewards
    dispatch = audit_dispatch(args.repository_root, schema["reward"]["names"], used, issues)
    source_root = args.repository_root / "core/personal-service/src"
    service_definitions = {
        "Character": included_json(source_root / "cn_character.rs", "CHARACTER_ASSET"),
        "Equipment": function_json(source_root / "cn_equipment/dissolve/catalog.rs", "equipment_definitions"),
    }
    for semantic, (path, definitions) in service_definitions.items():
        for target in sorted(references[semantic]):
            if str(target) not in definitions:
                issues.append({"kind": "missing_service_definition", "asset": str(path), "id": target,
                    "rewards": references[semantic][target]})
    return {
        "entity": manifest, "clientSourceRoot": str(args.client_source_root),
        "serviceAssets": {name: str(value[0]) for name, value in inputs.items()},
        "serviceReferenceAssets": {name: str(value[0]) for name, value in service_definitions.items()},
        "counts": {"pools": len(masters["pools"]), "boxes": boxes_count, "rewards": rewards_count,
            "rewardTypes": {str(kind): len(rows) for kind, rows in sorted(used.items())},
            "referencedDefinitions": {name: len(ids) for name, ids in references.items()}},
        "dispatch": dispatch, "issues": issues,
    }


# //// 输出完整不一致清单并使用失败退出码 [@x380kkm 2026-09-28] ////
def main() -> int:
    parser = argparse.ArgumentParser(description="全量核对 CN 箱池主表, 服务目录和奖励分派.")
    parser.add_argument("--cdn-root", type=Path, required=True)
    parser.add_argument("--client-source-root", type=Path, required=True)
    parser.add_argument("--entity", type=Path)
    parser.add_argument("--repository-root", type=Path, default=REPOSITORY)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        report = audit(args)
    except (OSError, ValueError, KeyError, TypeError, RuntimeError) as error:
        report = {"issues": [{"kind": "audit_input_error", "error": str(error)}]}
    report["ok"] = not report["issues"]
    serialized = json.dumps(report, ensure_ascii=False, indent=2) + '\n'
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(serialized, encoding="utf-8")
        print(json.dumps({"ok": report["ok"], "counts": report.get("counts"), "issues": len(report["issues"]), "report": str(args.output)}, ensure_ascii=False))
    else:
        print(serialized, end="")
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
