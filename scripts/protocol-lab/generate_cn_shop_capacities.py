# audience: internal
# # generate-cn-shop-capacities
# 奖励的持有上限与经验增长参数来自最终资源中的物品和全局配置主表.
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import argparse
import json
from pathlib import Path

from cn_packaged_resource_reader import PackagedResourceReader
from generate_cn_shop_selection_fixture import master


# //// 提取奖励的客户端持有上限 [@x380kkm 2026-09-25] ////
def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--cdn-root", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    with PackagedResourceReader(args.package, cdn_root=args.cdn_root) as reader:
        items = master(reader, "master/item/item.orderedmap")
        configs = master(reader, "master/config/config_iosbundled.orderedmap")
        configs.update(master(reader, "master/config/config.orderedmap"))
        config = configs["1"]
        limits = {
            "virtual_money": int(config[5]),
            "mana": int(config[6]),
            "star_crumb": int(config[7]),
            "exp_pool": int(config[10]),
            "exp_pool_gain_value": int(config[8]),
            "exp_pool_gain_seconds": int(config[9]),
            "equipment_stack": int(config[25]),
            "bond_token": int(config[26]),
            "boss_boost_point": int(config[30]),
            "boost_point": int(config[32]),
            "items": {key: int(row[18]) for key, row in items.items()},
            "overflow_receivable_items": [
                int(key) for key, row in items.items()
                if int(row[14]) == 6 and row[21].lower() == "true"
            ],
        }
    args.output.write_text(json.dumps(limits, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({key: len(value) if isinstance(value, (dict, list)) else value
                      for key, value in limits.items()}))
# //// /提取奖励的客户端持有上限 ////


if __name__ == "__main__":
    main()
