# audience: internal
# # generate-cn-exchange-item-limits
# 星屑兑换物品的持有上限来自客户端物品主表.
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import argparse
import json
from pathlib import Path

from generate_cn_shop_selection_fixture import master
from cn_packaged_resource_reader import PackagedResourceReader


# //// 提取星屑兑换物品的主表持有上限 [@x380kkm 2026-09-23] ////
def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--cdn-root", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    with PackagedResourceReader(args.package, cdn_root=args.cdn_root) as reader:
        offers = master(reader, "master/shop/star_crumb_exchange.orderedmap")
        items = master(reader, "master/item/item.orderedmap")
        exchange_items = {row[1]: items[row[1]]
                          for row in offers.values() if row[0] == "1"}
        if any(len(row) != 23 for row in exchange_items.values()):
            raise ValueError("星屑兑换物品主表字段数与当前解析契约不一致")
        limits = {key: int(row[18]) for key, row in exchange_items.items()}
    if any(count <= 0 for count in limits.values()):
        raise ValueError("星屑兑换物品上限必须为正数")
    args.output.write_text(json.dumps(limits, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(limits))
# //// /提取星屑兑换物品的主表持有上限 ////


if __name__ == "__main__":
    main()
