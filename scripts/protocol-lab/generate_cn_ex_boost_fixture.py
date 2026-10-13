# audience: internal
# # generate-cn-ex-boost-fixture
# EX 材料和各级抽取权重来自最终资源包的组合, 状态和能力主表.
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import argparse
import json
from pathlib import Path

from cn_packaged_resource_reader import PackagedResourceReader
from generate_cn_shop_selection_fixture import master


# //// 关联材料和组合引用的状态与能力池 [@x380kkm 2026-09-23] ////
def catalog(reader: PackagedResourceReader) -> dict:
    result = {"materials": {}, "combinations": {}, "status_pools": {}, "ability_pools": {}}
    for item_id, row in master(reader, "master/ex_boost/ex_boost.orderedmap").items():
        if len(row) != 3 or int(row[0]) <= 0:
            raise ValueError(f"EX 材料 {item_id} 的字段与目录映射不符")
        result["materials"][item_id] = {
            "cost_item_count": int(row[0]),
            "elements": [int(element) for element in row[2].split(",")],
            "combination_pool": row[1],
        }
        if row[1] not in result["combinations"]:
            result["combinations"][row[1]] = combinations(reader, row[1])
    for entries in result["combinations"].values():
        for entry in entries:
            for kind, names in (("status", [entry["status_pool"]]), ("ability", entry["ability_pools"])):
                pools = result[f"{kind}_pools"]
                for name in names:
                    if name not in pools:
                        pools[name] = weighted_pool(reader, kind, name)
    return result
# //// /关联材料和组合引用的状态与能力池 ////


# //// 按组合主表保留状态池和能力槽顺序 [@x380kkm 2026-09-23] ////
def combinations(reader: PackagedResourceReader, name: str) -> list[dict]:
    rows = master(reader, f"master/ex_boost/odds/lots_combination/{name}.orderedmap")[name]
    result = []
    for key in sorted(rows, key=int):
        row = rows[key]
        if len(row) != 21 or int(row[0]) <= 0:
            raise ValueError(f"EX 组合 {name}:{key} 的字段与目录映射不符")
        status_pools = []
        ability_pools = []
        for index in range(1, 21, 2):
            kind, pool = row[index:index + 2]
            if kind == "0":
                status_pools.append(pool)
            elif kind == "1":
                ability_pools.append(pool)
            elif kind != "(None)":
                raise ValueError(f"EX 组合 {name}:{key} 包含未知抽取类型 {kind}")
        if len(status_pools) != 1:
            raise ValueError(f"EX 组合 {name}:{key} 的状态槽数量与响应契约不符")
        result.append({"weight": int(row[0]), "status_pool": status_pools[0], "ability_pools": ability_pools})
    return result
# //// /按组合主表保留状态池和能力槽顺序 ////


# //// 读取各个状态或能力 ID 的独立权重 [@x380kkm 2026-09-23] ////
def weighted_pool(reader: PackagedResourceReader, kind: str, name: str) -> list[dict]:
    rows = master(reader, f"master/ex_boost/odds/{kind}/{name}.orderedmap")[name]
    result = []
    for key in sorted(rows, key=int):
        row = rows[key]
        if len(row) != 2 or int(row[0]) <= 0 or int(row[1]) <= 0:
            raise ValueError(f"EX 抽取池 {kind}:{name}:{key} 的字段与目录映射不符")
        result.append({"id": int(row[0]), "weight": int(row[1])})
    return result
# //// /读取各个状态或能力 ID 的独立权重 ////


# //// 输出服务使用的 EX 抽取目录 [@x380kkm 2026-09-23] ////
def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--cdn-root", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    with PackagedResourceReader(args.package, cdn_root=args.cdn_root) as reader:
        result = catalog(reader)
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({key: len(value) for key, value in result.items()}))
# //// /输出服务使用的 EX 抽取目录 ////


if __name__ == "__main__":
    main()
