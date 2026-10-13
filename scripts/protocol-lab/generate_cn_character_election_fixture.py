# audience: internal
# # generate-cn-character-election-fixture
# 投票候选沿用百科的角色身份, 保密开放日期和每期排除表, 主表日期使用国服 UTC+8.
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


# //// 读取投票及候选的客户端主表 [@x380kkm 2026-09-23] ////
def read_definitions(reader: PackagedResourceReader) -> dict:
    definitions = {}
    for name, directory in (
        ("character_election", "character_election"),
        ("character_election_exclude", "character_election"),
        ("encyclopedia", "encyclopedia"),
        ("character", "character"),
    ):
        content = reader.read(f"master/{directory}/{name}.orderedmap")
        if content is None:
            raise ValueError(f"缺少投票主表: {name}")
        definitions[name] = decode_multirow_ordered_map(content)
    return definitions


def timestamp(value: str) -> int:
    return int(datetime.strptime(value, "%Y-%m-%d %H:%M:%S")
               .replace(tzinfo=timezone(timedelta(hours=8))).timestamp())


# //// 按百科角色身份和保密日期生成候选 [@x380kkm 2026-09-23] ////
def candidates(definitions: dict) -> dict:
    result = {}
    for keyword_id, entries in definitions["encyclopedia"].items():
        row = entries["1"]
        if row[4] == "0":
            character_id = row[5]
            character = definitions["character"][character_id]
            if character[27] not in ("(None)", character_id):
                continue
        elif row[4] != "2":
            continue
        result[keyword_id] = {
            "available_at": timestamp(row[3]) if row[2].lower() == "true" else None,
        }
    return result


# //// 生成投票活动窗口和候选排除配置 [@x380kkm 2026-09-23] ////
def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--cdn-root", type=Path)
    parser.add_argument("--output", type=Path, default=Path("core/personal-service/assets/cn-character-election.json"))
    args = parser.parse_args()
    with PackagedResourceReader(args.package, cdn_root=args.cdn_root) as reader:
        definitions = read_definitions(reader)
    elections = {}
    for election_id, row in definitions["character_election"].items():
        excluded = definitions["character_election_exclude"].get(election_id, [])
        if excluded and isinstance(excluded[0], str):
            excluded = [excluded]
        elections[election_id] = {
            "start_at": timestamp(row[2]), "end_at": timestamp(row[3]),
            "excluded": [int(entry[0]) for entry in excluded],
        }
    fixture = {"elections": elections, "candidates": candidates(definitions)}
    args.output.write_text(json.dumps(fixture, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"elections": len(elections), "candidates": len(fixture["candidates"])}))


if __name__ == "__main__":
    main()
