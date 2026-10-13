# audience: internal
# # cn-box-reset-kinds
#
# 该脚本从 CN 客户端 box 主表导出箱体重置类型.
# 运行环境读取 CDN 实体目录和对应资源归档.

from __future__ import annotations

import argparse
import json
from pathlib import Path

from audit_cn_gacha_candidate_assets import decode_multirow_ordered_map
from cn_gacha_banner_assets import EntityCatalog, read_logical_assets


MASTER_PATH = "master/box_gacha/box.orderedmap"


# //// 从箱体主表提取重置类型 [@x380kkm 2026-09-27] ////
def read_box_reset_kinds(cdn_root: Path, entity_path: Path) -> dict[str, dict[str, int]]:
    catalog = EntityCatalog.load(cdn_root, entity_path)
    record = catalog.find(MASTER_PATH)
    if record is None:
        raise ValueError(f"CN master is missing: {MASTER_PATH}")
    assets, missing = read_logical_assets(cdn_root, {MASTER_PATH: record})
    if missing:
        raise ValueError(f"CN master archive is missing: {MASTER_PATH}")
    boxes = decode_multirow_ordered_map(assets[MASTER_PATH])
    result = {}
    for pool_id, pool in sorted(boxes.items(), key=lambda entry: int(entry[0])):
        result[pool_id] = {}
        for box_id, row in sorted(pool.items(), key=lambda entry: int(entry[0])):
            reset_kind = int(row[11])
            if reset_kind not in (0, 1, 2):
                raise ValueError(f"CN box reset kind is invalid: {pool_id}/{box_id}")
            result[pool_id][box_id] = reset_kind
    return result
# //// /从箱体主表提取重置类型 ////


# //// 写入服务端箱体重置目录 [@x380kkm 2026-09-27] ////
def main() -> None:
    parser = argparse.ArgumentParser(description="导出 CN 箱体重置类型.")
    parser.add_argument("--cdn-root", required=True, type=Path)
    parser.add_argument("--entity", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    reset_kinds = read_box_reset_kinds(args.cdn_root, args.entity)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(reset_kinds, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"pools": len(reset_kinds), "boxes": sum(map(len, reset_kinds.values()))}))
# //// /写入服务端箱体重置目录 ////


if __name__ == "__main__":
    main()
