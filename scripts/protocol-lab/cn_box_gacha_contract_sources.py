# audience: internal
# # cn-box-gacha-contract-sources
# 客户端字段和奖励编号从生成的 Values 类读取, 服务目录路径和分派动作从对应源码读取.

from __future__ import annotations

import json
import re
from pathlib import Path


# //// 从客户端生成类读取字段位置与奖励构造函数 [@x380kkm 2026-09-28] ////
def column(source: str, field: str) -> int:
    aliases = {}
    for line in source.splitlines():
        assignment = re.search(r"(?:var )?(_loc\d+_)(?::[\w*]+)? = param1\[(\d+)\]", line)
        if assignment:
            aliases[assignment[1]] = int(assignment[2])
        assignment = re.search(rf"^\s*{re.escape(field)} = (.+);", line)
        if assignment:
            indexes = {int(value) for value in re.findall(r"param1\[(\d+)\]", assignment[1])}
            indexes.update(aliases[value] for value in re.findall(r"_loc\d+_", assignment[1]) if value in aliases)
            if len(indexes) == 1:
                return indexes.pop()
    raise ValueError(f"客户端字段位置无法唯一确定: {field}")


def kind_schema(source: str, enum: str) -> dict:
    branches = list(re.finditer(
        rf'if\(\s*(_loc\d+_)\s*[!=]=\s*"(\d+)"\s*\)[\s\S]*?{enum}\.(\w+)', source))
    aliases = {match[1] for match in branches}
    if len(aliases) != 1:
        raise ValueError(f"客户端类型分派无法唯一确定: {enum}")
    alias = aliases.pop()
    indexes = re.findall(rf"{alias}(?::\w+)? = param1\[(\d+)\]", source[:branches[0].start()])
    targets = {int(value) for value in re.findall(rf"{enum}\.\w+\(int\(Std\.parseInt\(param1\[(\d+)\]", source)}
    if not indexes or len(targets) != 1:
        raise ValueError(f"客户端类型或引用字段无法唯一确定: {enum}")
    return {"type": int(indexes[-1]), "id": targets.pop(), "names": {int(match[2]): match[3] for match in branches}}


def client_schema(source_root: Path) -> dict:
    directory = source_root / "pinball/master/generated"
    pool = (directory / "BoxGachaValues.as").read_text(encoding="utf-8")
    box = (directory / "BoxValues.as").read_text(encoding="utf-8")
    reward = (directory / "BoxRewardValues.as").read_text(encoding="utf-8")
    return {
        "pool": {**kind_schema(pool, "TradeCostKind"), "count": column(pool, "cost_number")},
        "box": {name: column(box, field) for name, field in (
            ("resetKind", "reset_kind"), ("previous", "required_box_id"))},
        "reward": {**kind_schema(reward, "GeneralRewardKind"), **{
            name: column(reward, field) for name, field in (
                ("key", "multiplied_reward_id"), ("count", "number_per_capsule"),
                ("available", "capsule_number"), ("tier", "feature_kind"))}},
    }


# //// 读取实际编译目录与 TypeScript 枚举 [@x380kkm 2026-09-28] ////
def included_json(source: Path, constant: str) -> tuple[Path, dict]:
    text = source.read_text(encoding="utf-8")
    matches = re.findall(rf'const {constant}:.*?include_str!\(\s*"([^"]+)"\s*\)', text)
    if len(matches) != 1:
        raise ValueError(f"服务目录引用无法唯一确定: {source}:{constant}")
    return read_reference(source, matches[0])


def function_json(source: Path, function: str) -> tuple[Path, dict]:
    text = source.read_text(encoding="utf-8")
    section = re.search(rf"fn {function}\b([\s\S]*?)(?=\n(?:pub|fn)|\Z)", text)
    matches = re.findall(r'include_str!\(\s*"([^"]+)"\s*\)', section[0] if section else "")
    if len(matches) != 1:
        raise ValueError(f"服务定义引用无法唯一确定: {source}:{function}")
    return read_reference(source, matches[0])


def read_reference(source: Path, relative: str) -> tuple[Path, dict]:
    path = (source.parent / relative).resolve()
    document = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(document, dict):
        raise ValueError(f"服务定义格式无效: {path}")
    return path, document


def typescript_enum(source: str, name: str) -> dict[str, int]:
    body = re.search(rf"export enum {name}\s*\{{([^}}]+)\}}", source)
    if body is None:
        raise ValueError(f"TypeScript 枚举缺失: {name}")
    entries = re.sub(r"/\*[\s\S]*?\*/|//[^\n]*", "", body[1]).split(',')
    values = {}
    next_value = 0
    for entry in filter(str.strip, entries):
        match = re.fullmatch(r"\s*(\w+)\s*(?:=\s*(\d+))?\s*", entry)
        if match is None:
            raise ValueError(f"TypeScript 枚举表达式无法确定: {name}:{entry}")
        value = int(match[2]) if match[2] is not None else next_value
        values[match[1]] = value
        next_value = value + 1
    return values


def source_branches(source: str, marker: str, pattern: str) -> dict[str, dict]:
    if source.count(marker) != 1:
        raise ValueError(f"奖励分派入口无法唯一确定: {marker}")
    offset = source.index(marker)
    matches = list(re.finditer(pattern, source[offset:], re.MULTILINE))
    if not matches:
        raise ValueError(f"奖励分派分支缺失: {marker}")
    indent = min(len(match[1]) for match in matches)
    matches = [match for match in matches if len(match[1]) == indent]
    return {match[2]: {
        "body": source[offset + match.end():offset + matches[index + 1].start() if index + 1 < len(matches) else len(source)],
        "line": source.count('\n', 0, offset + match.start()) + 1,
    } for index, match in enumerate(matches)}


# //// 对照客户端语义核验两个后端的实际奖励动作 [@x380kkm 2026-09-28] ////
def audit_dispatch(repository: Path, kinds: dict[int, str], used: dict, issues: list) -> list:
    rust_path = repository / "core/personal-service/src/cn_box_gacha/reward.rs"
    ts_path = repository / "src/lib/gacha.ts"
    rust = source_branches(rust_path.read_text(encoding="utf-8").split("#[cfg(test)]")[0],
        "match reward_type {", r"^([ \t]+)(\d+|_)\s*=>")
    types = typescript_enum((repository / "src/lib/types.ts").read_text(encoding="utf-8"), "BoxGachaRewardType")
    ts_source = ts_path.read_text(encoding="utf-8")
    start = ts_source.index("export function drawBoxGachaSync")
    end = ts_source.index("export function rewardPlayerBoxGachaResultSync", start)
    ts = source_branches(ts_source[start:end], "switch (reward.type) {",
        r"^([ \t]+)case BoxGachaRewardType\.(\w+):")
    for branch in ts.values():
        branch["line"] += ts_source.count('\n', 0, start)
    effects = {
        "Item": (r"\badd_item\s*\(", "ITEM", r"\bdrawnItems\.set\s*\("),
        "Equipment": (r"\bgrant_equipment\s*\(", "EQUIPMENT", r"\bdrawnEquipment\.set\s*\("),
        "Mana": (r'\badd_user_value\(root,\s*"free_mana"', "MANA", r"\bdrawnMana\s*\+="),
        "PooledExp": (r'\badd_user_value\(root,\s*"exp_pool"', "EXP", r"\bdrawnExp\s*\+="),
        "Character": (r"\bgrant_character\s*\(", "CHARACTER", r"\bdrawnCharacters\.set\s*\("),
    }
    result = []
    for kind, locations in sorted(used.items()):
        name = kinds.get(kind)
        effect = effects.get(name)
        for backend, path, branch, expression, number in (
            ("rust", rust_path, rust.get(str(kind)), effect[0] if effect else None, kind),
            ("typescript", ts_path, ts.get(effect[1]) if effect else None,
                effect[2] if effect else None, types.get(effect[1]) if effect else None),
        ):
            valid = number == kind and branch is not None and expression is not None and re.search(expression, branch["body"]) is not None
            evidence = {"backend": backend, "type": kind, "semantic": name,
                "actualType": number, "source": str(path), "line": branch["line"] if branch else None}
            result.append({**evidence, "supported": valid})
            if not valid:
                issues.append({"kind": "reward_dispatch_mismatch", **evidence, "rewards": locations})
    return result
