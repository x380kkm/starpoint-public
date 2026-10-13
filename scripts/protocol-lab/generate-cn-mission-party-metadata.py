# audience: internal
# # generate-cn-mission-party-metadata
#
# 任务条件使用 CN 角色技能的 DSL 构造节点. 技能进化档和切换技能共用角色的技能进化编号.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

from __future__ import annotations

import argparse
import json
import zlib
from pathlib import Path
from typing import Any

from cn_gacha_banner_assets import decode_ordered_map
from cn_gacha_banner_atlas import _Amf3Decoder
from cn_packaged_resource_reader import PackagedResourceReader
from cn_mission_equipment_metadata import generate_equipment


MASTER_PATHS = {
    "characters": "master/character/character.orderedmap",
    "skills": "master/skill/action_skill.orderedmap",
    "switched_skills": "master/skill/switched_action_skill.orderedmap",
}


# //// 按当前 Entity 目录批量读取主表与技能程序 [@x380kkm 2026-09-21] ////
def read_assets(reader: PackagedResourceReader, paths: set[str]) -> dict[str, bytes]:
    values = {path: reader.read(path) for path in paths}
    missing = [path for path, data in values.items() if data is None]
    if missing:
        raise ValueError(f"CN mission source entries are missing: {missing}")
    return values


def action_path(row: list[str], column: int) -> str:
    return row[column] + ".action.dsl.amf3.deflate"


def effect_constructors(value: Any, wanted: set[str]) -> set[str]:
    found = set()
    if isinstance(value, list):
        if value and isinstance(value[0], str) and value[0] in wanted:
            found.add(value[0])
        if value and value[0] == "ACToleranceOfElement" and "ACToleranceOfElement_Down" in wanted:
            strength = value[3]
            if any(isinstance(amount, (int, float)) and amount < 0 for level in strength for amount in level.values()):
                found.add("ACToleranceOfElement_Down")
        for child in value:
            found.update(effect_constructors(child, wanted))
    elif isinstance(value, dict):
        for child in value.values():
            found.update(effect_constructors(child, wanted))
    return found
# //// /按当前 Entity 目录批量读取主表与技能程序 ////


# //// 为每个角色的技能进化档生成任务效果集合 [@x380kkm 2026-09-21] ////
def generate_characters(reader: PackagedResourceReader, missions: dict) -> dict:
    data = read_assets(reader, set(MASTER_PATHS.values()))
    masters = {name: decode_ordered_map(data[path]) for name, path in MASTER_PATHS.items()}
    wanted = {effect for mission in missions.values() for effect in mission["action_effects"]}
    paths = set()
    for character in masters["characters"].values():
        paths.update(action_path(row, 7) for row in masters["skills"][character[8]].values())
        if character[9] not in ("", "(None)"):
            paths.update(action_path(row, 0) for row in masters["switched_skills"][character[14]].values())
    programs = read_assets(reader, paths)
    effects = {}
    for path, data in programs.items():
        tree = _Amf3Decoder(zlib.decompress(data, -15)).decode()
        if not isinstance(tree, list) or not tree or tree[0] != "ActionDsl":
            raise ValueError(f"CN skill is not an action DSL: {path}")
        effects[path] = effect_constructors(tree, wanted)
    characters = {}
    for character_id, row in masters["characters"].items():
        skills = masters["skills"][row[8]]
        switched = masters["switched_skills"].get(row[14], {}) if row[9] not in ("", "(None)") else {}
        definitions = {}
        for evolution, skill in skills.items():
            combined = set(effects[action_path(skill, 7)])
            if evolution in switched:
                combined.update(effects[action_path(switched[evolution], 0)])
            definitions[evolution] = {"effects": sorted(combined), "unisonable": skill[3].lower() == "true"}
        characters[character_id] = {
            "string_id": row[0], "element": int(row[3]) + 1, "skills": definitions,
        }
    return characters
# //// /为每个角色的技能进化档生成任务效果集合 ////


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--client-scripts", type=Path, required=True)
    parser.add_argument("--cdn-root", type=Path, required=True)
    parser.add_argument("--missions", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    missions = json.loads(args.missions.read_text(encoding="utf-8"))["missions"]
    with PackagedResourceReader(args.package, cdn_root=args.cdn_root) as reader:
        characters = generate_characters(reader, missions)
        equipment = generate_equipment(reader, args.client_scripts)
    args.output.write_text(json.dumps({"characters": characters, "equipment": equipment}, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"characters": len(characters), "equipment": len(equipment), "output": str(args.output)}))


if __name__ == "__main__":
    main()
