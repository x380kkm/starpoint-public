# audience: internal
# # generate-cn-character-town
#
# 此脚本从 CN 最终客户端包的 StoryJoinCharacter 主表生成城镇角色领取资格.
# 生成结果保留主表字段, 并为主线条件附上单人战斗分类.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

from __future__ import annotations

import argparse
import json
from pathlib import Path

from cn_gacha_banner_assets import decode_ordered_map
from cn_packaged_resource_reader import PackagedResourceReader


SOURCE = "master/character/story_join_character.orderedmap"


# //// 读取最终包内的剧情加入角色主表 [@x380kkm 2026-09-22] ////
def read_story_join_character(package: Path, cdn_root: Path) -> dict[str, list[str]]:
    with PackagedResourceReader(package, cdn_root=cdn_root) as reader:
        payload = reader.read(SOURCE)
    if payload is None:
        raise ValueError(f"包内缺少主表: {SOURCE}")
    rows = decode_ordered_map(payload)
    if len(rows) != 3:
        raise ValueError(f"StoryJoinCharacter 主表行数异常: {len(rows)}")
    return rows


def integer(value: str, label: str) -> int:
    try:
        result = int(value)
    except ValueError as error:
        raise ValueError(f"{label} 不是整数: {value}") from error
    if result <= 0:
        raise ValueError(f"{label} 必须为正数: {value}")
    return result


# //// 转换主表行并绑定主线战斗分类 [@x380kkm 2026-09-22] ////
def convert(rows: dict[str, list[str]]) -> dict:
    result = []
    for raw_character_id, row in rows.items():
        if len(row) != 8:
            raise ValueError(f"角色 {raw_character_id} 的主表字段数异常: {len(row)}")
        character_id = integer(raw_character_id, "character_id")
        if row[0] != "0":
            raise ValueError(f"角色 {character_id} 使用了非主线条件: {row[0]}")
        if row[7].lower() != "false":
            raise ValueError(f"角色 {character_id} 使用了临时加入条件")
        chapter = integer(row[1], f"角色 {character_id} chapter")
        stage = integer(row[2], f"角色 {character_id} stage")
        node = integer(row[3], f"角色 {character_id} node")
        quest_id = integer(row[4], f"角色 {character_id} quest_id")
        result.append({
            "character_id": character_id,
            "emergence_kind": 0,
            "chapter": chapter,
            "stage": stage,
            "node": node,
            "quest_id": quest_id,
            "priority": int(row[5]),
            "join_type": row[6],
            "join_temporary": False,
            "quest_category": 1,
        })
    return {"source": SOURCE, "rows": result}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", required=True, type=Path)
    parser.add_argument("--cdn-root", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    generated = convert(read_story_join_character(args.package, args.cdn_root))
    encoded = json.dumps(generated, ensure_ascii=False, indent=2) + "\n"
    if args.check:
        current = args.output.read_text(encoding="utf-8")
        if current != encoded:
            raise SystemExit(f"城镇角色 fixture 与包内主表不一致: {args.output}")
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded, encoding="utf-8")
    print(json.dumps({"rows": len(generated["rows"]), "output": str(args.output)}, ensure_ascii=False))


if __name__ == "__main__":
    main()
