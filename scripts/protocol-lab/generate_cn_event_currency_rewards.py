# audience: internal
# # generate-cn-event-currency-rewards
# 客户端完整奖励主表和活动关卡引用由同一份原始奖励投影生成.
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import argparse
import json
from pathlib import Path

from cn_event_currency_rewards import SCORE_MASTER, RARE_MASTER, project_event_currencies
from cn_gacha_banner_assets import hash_cn_asset_path
from generate_cn_battle_reward_rules import read_masters


ROOT = Path(__file__).resolve().parents[2]


# //// 生成保留记录顺序的客户端奖励主表 [@x380kkm 2026-09-29] ////
def client_projection(projection):
    masters = []
    for logical_path in (SCORE_MASTER, RARE_MASTER):
        digest = hash_cn_asset_path(logical_path)
        masters.append({"entry_path": f"production/upload/{digest[:2]}/{digest[2:]}",
                        "groups": [[group_id, list(group.items())]
                                   for group_id, group in projection["masters"][logical_path].items()]})
    return {key: value for key, value in projection.items() if key != "masters"} | {"masters": masters}
# //// /生成保留记录顺序的客户端奖励主表 ////


# //// 从原始 CN 主表生成活动货币客户端投影 [@x380kkm 2026-09-29] ////
def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cdn-root", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    masters = read_masters(args.cdn_root)
    items = json.loads((ROOT / "assets/cn_item_catalog.json").read_text(encoding="utf-8"))
    shops = json.loads((ROOT / "assets/event_item_shop.json").read_text(encoding="utf-8"))
    projection = project_event_currencies(masters, items, shops)
    result = client_projection(projection)
    args.output.write_text(json.dumps(result, ensure_ascii=False, separators=(",", ":")) + "\n", encoding="utf-8")
    print(json.dumps({"events": projection["events"], "quest_count": len(projection["quest_score_groups"]),
                      "score_group_count": len(projection["score_group_origins"]),
                      "rare_group_count": len(projection["rare_group_origins"]), "output": str(args.output)},
                     ensure_ascii=False))
# //// /从原始 CN 主表生成活动货币客户端投影 ////


if __name__ == "__main__":
    main()
