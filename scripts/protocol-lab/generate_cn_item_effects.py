# audience: internal
# # generate-cn-item-effects
# 体力道具效果和使用期限来自客户端主表, 日期按国服 UTC+8 解析.
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import argparse
import json
from pathlib import Path

from cn_packaged_resource_reader import PackagedResourceReader
from generate_cn_shop_selection_fixture import master, timestamp


# //// 从客户端主表提取体力恢复效果和使用期限 [@x380kkm 2026-09-28] ////
def effect_catalog(reader: PackagedResourceReader) -> dict:
    items = master(reader, "master/item/item.orderedmap")
    result = {}
    for item_id, row in sorted(items.items(), key=lambda entry: int(entry[0])):
        if row[6] not in ("2", "3"):
            continue
        if len(row) != 23:
            raise ValueError(f"体力道具 {item_id} 字段数异常")
        effect = {"effectKind": int(row[6]), "effectValue": int(row[7])}
        if effect["effectValue"] <= 0:
            raise ValueError(f"体力道具 {item_id} 恢复量异常")
        for field, value in (("start_time", row[19]), ("end_time", row[20])):
            if value not in ("", "(None)"):
                effect[field] = timestamp(value)
        result[item_id] = effect
    return result
# //// /从客户端主表提取体力恢复效果和使用期限 ////


# //// 按客户端包和资源目录生成体力道具目录 [@x380kkm 2026-09-28] ////
def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--cdn-root", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    with PackagedResourceReader(args.package, cdn_root=args.cdn_root) as reader:
        catalog = effect_catalog(reader)
    args.output.write_text(json.dumps(catalog, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"items": len(catalog)}))
# //// /按客户端包和资源目录生成体力道具目录 ////


if __name__ == "__main__":
    main()
