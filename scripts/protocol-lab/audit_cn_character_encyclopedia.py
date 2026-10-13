# audience: internal
# # cn-character-encyclopedia-audit
# 此脚本从最终客户端包的角色, 剧情加入和百科主表生成角色百科映射, 并核对服务端领取依赖.
# 多个平台的映射保持一致时, --write-map 输出服务端使用的角色主百科记录.

from __future__ import annotations

import argparse
import json
from pathlib import Path

from cn_gacha_banner_assets import decode_ordered_map
from cn_packaged_resource_reader import PackagedResourceReader


# //// 读取包内主表并保留缺失路径 [@x380kkm 2026-09-21] ////
def read_master(reader: PackagedResourceReader, logical: str) -> dict:
    payload = reader.read(logical)
    if payload is None:
        raise ValueError(f"包内缺少主表: {logical}")
    return decode_ordered_map(payload)


# //// 核对全部角色百科身份和剧情加入候选 [@x380kkm 2026-09-21] ////
def audit_package(path: Path, server_characters: dict, mana_nodes: dict) -> tuple[dict, dict]:
    with PackagedResourceReader(path) as reader:
        characters = read_master(reader, "master/character/character.orderedmap")
        encyclopedia = read_master(reader, "master/encyclopedia/encyclopedia.orderedmap")
        story = read_master(reader, "master/character/story_join_character.orderedmap")
    mapping = {}
    issues = []
    for keyword in encyclopedia.values():
        main = keyword.get("1")
        if main is None or main[4] not in ("0", "1"):
            continue
        character_id = main[5]
        if character_id not in characters:
            issues.append({"kind": "missing_encyclopedia_character", "characterId": int(character_id)})
            continue
        encyclopedia_id = int(main[0])
        if character_id in mapping:
            issues.append({"kind": "ambiguous_character_encyclopedia", "characterId": int(character_id)})
        if encyclopedia_id != (1_000_000 + int(character_id)) * 100 + 1:
            issues.append({"kind": "unexpected_encyclopedia_id", "characterId": int(character_id),
                           "encyclopediaId": encyclopedia_id})
        mapping[character_id] = encyclopedia_id
    missing_encyclopedia = sorted(int(character_id) for character_id in characters if character_id not in mapping)
    candidates = []
    for character_id, row in story.items():
        character = characters.get(character_id)
        server = server_characters.get(character_id)
        boards = mana_nodes.get(character_id, {})
        if character is None or server is None or not boards:
            issues.append({"kind": "missing_character_dependency", "characterId": int(character_id),
                           "client": character is not None, "server": server is not None,
                           "manaBoards": len(boards)})
        elif int(character[2]) != server["rarity"] or int(character[3]) != server["element"]:
            issues.append({"kind": "character_attribute_mismatch", "characterId": int(character_id)})
        candidates.append({"characterId": int(character_id), "asset": character[0] if character else None,
                           "joinType": row[6], "quest": int(row[4]),
                           "manaBoardCount": len(boards), "encyclopediaId": mapping.get(character_id)})
    return mapping, {"package": str(path), "characterCount": len(characters),
                     "mappedCount": len(mapping), "withoutEncyclopedia": sorted(missing_encyclopedia),
                     "storyCharacters": candidates, "issues": issues}


# //// 输出双平台核对结果和可复用的角色百科映射 [@x380kkm 2026-09-21] ////
def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", type=Path, action="append", required=True)
    parser.add_argument("--character-asset", type=Path, required=True)
    parser.add_argument("--mana-asset", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--write-map", type=Path)
    args = parser.parse_args()
    characters = json.loads(args.character_asset.read_text(encoding="utf-8-sig"))
    mana_nodes = json.loads(args.mana_asset.read_text(encoding="utf-8-sig"))
    reports = []
    mapping = None
    for package in args.package:
        candidate, report = audit_package(package, characters, mana_nodes)
        if mapping is not None and candidate != mapping:
            report["issues"].append({"kind": "platform_mapping_mismatch"})
        mapping = candidate
        reports.append(report)
    result = {"packages": reports, "valid": all(not report["issues"] for report in reports)}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    if not result["valid"]:
        raise SystemExit(f"角色领取依赖存在差异: {args.output}")
    if args.write_map:
        args.write_map.parent.mkdir(parents=True, exist_ok=True)
        ordered = dict(sorted(mapping.items(), key=lambda item: int(item[0])))
        args.write_map.write_text(json.dumps(ordered, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"packages": len(reports), "mappedCharacters": len(mapping),
                      "storyCharacters": [len(report["storyCharacters"]) for report in reports],
                      "report": str(args.output)}))


if __name__ == "__main__":
    main()
