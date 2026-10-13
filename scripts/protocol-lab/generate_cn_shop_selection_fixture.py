# audience: internal
# # generate-cn-shop-selection-fixture
# 自选商店目录按最终客户端主表关联活动, 选项和商品, 日期使用国服 UTC+8.
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


def master(reader: PackagedResourceReader, logical: str) -> dict:
    content = reader.read(logical)
    if content is None:
        raise ValueError(f"缺少主表: {logical}")
    return decode_multirow_ordered_map(content)


def timestamp(value: str) -> int:
    return int(datetime.strptime(value, "%Y-%m-%d %H:%M:%S")
               .replace(tzinfo=timezone(timedelta(hours=8))).timestamp())


# //// 关联活动商店的选择条件与实际商品 [@x380kkm 2026-09-23] ////
def selection_catalog(reader: PackagedResourceReader) -> dict:
    prefix = "master/quest/event/event_shop_select_item_campaign"
    campaigns = master(reader, prefix + ".orderedmap")
    lineups = master(reader, prefix + "_lineup.orderedmap")
    boss_campaigns = master(reader, "master/shop/boss_coin_shop_select_item_campaign.orderedmap")
    boss_lineups = master(reader, "master/shop/boss_coin_shop_select_item_campaign_lineup.orderedmap")
    if boss_campaigns or boss_lineups:
        raise ValueError("Boss 币自选主表包含活动, 需要匹配其字段契约")
    result = {"campaigns": {}, "products": {}}
    for key, row in campaigns.items():
        result["campaigns"][f"4:{key}"] = {
            "activity_id": f"event-shop-select-item-campaign:{key}",
            "event_type": int(row[1]), "event_id": int(row[2]),
            "start_time": timestamp(row[6]), "end_time": timestamp(row[7]),
            "exchange_end_time": timestamp(row[8]),
            "lineups": sorted(int(lineup_id) for lineup_id, values in lineups.items()
                              if int(values[0]) == int(key)),
        }
    for shop_type, name, campaign_index, lineup_index in (
        (4, "event_item_shop", 4, 5), (7, "boss_coin_shop", 3, 4)
    ):
        for item_id, row in master(reader, f"master/shop/{name}.orderedmap").items():
            if row[campaign_index] == "(None)":
                continue
            campaign_key = f"{shop_type}:{row[campaign_index]}"
            campaign = result["campaigns"].get(campaign_key)
            if campaign is None:
                raise ValueError(f"商品 {shop_type}:{item_id} 缺少活动 {campaign_key}")
            lineup_id = None if row[lineup_index] == "(None)" else int(row[lineup_index])
            if lineup_id is not None and lineup_id not in campaign["lineups"]:
                raise ValueError(f"商品 {shop_type}:{item_id} 的选项不属于活动")
            result["products"][f"{shop_type}:{item_id}"] = {
                "campaign": campaign_key, "lineup_id": lineup_id,
            }
    for campaign_key, campaign in result["campaigns"].items():
        for lineup_id in campaign["lineups"]:
            if not any(product["campaign"] == campaign_key and product["lineup_id"] == lineup_id
                       for product in result["products"].values()):
                raise ValueError(f"选项 {lineup_id} 缺少实际商品")
    return result


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--cdn-root", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    with PackagedResourceReader(args.package, cdn_root=args.cdn_root) as reader:
        result = selection_catalog(reader)
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"campaigns": len(result["campaigns"]),
                      "lineups": sum(len(campaign["lineups"]) for campaign in result["campaigns"].values()),
                      "products": len(result["products"]), "boss_campaigns": 0}))


if __name__ == "__main__":
    main()
