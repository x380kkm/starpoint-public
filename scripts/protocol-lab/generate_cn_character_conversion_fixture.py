# audience: internal
# # generate-cn-character-conversion-fixture
# 角色来源映射来自客户端角色主表, 副本转换使用该字段确定星粒产出.
# 输入包与对应的 CDN 覆盖共同构成客户端资源视图.
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import argparse
import json
from collections import Counter
from pathlib import Path

from cn_packaged_resource_reader import PackagedResourceReader
from generate_cn_shop_selection_fixture import master


# //// 提取角色主表的取得来源 [@x380kkm 2026-09-23] ////
def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--cdn-root", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    with PackagedResourceReader(args.package, cdn_root=args.cdn_root) as reader:
        characters = master(reader, "master/character/character.orderedmap")
        sources = {key: int(row[32]) for key, row in characters.items()}
    args.output.write_text(json.dumps(sources, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"characters": len(sources), "sources": dict(Counter(sources.values()))}))
# //// /提取角色主表的取得来源 ////


if __name__ == "__main__":
    main()
