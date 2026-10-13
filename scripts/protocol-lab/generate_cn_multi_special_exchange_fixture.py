# audience: internal
# # generate-cn-multi-special-exchange-fixture
# 活动和票券候选来自客户端最终资源, master 时间使用 UTC+8.
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

MASTER_ROOT = "master/campaign/multi_special_exchange"


# //// 将活动时窗转换为服务端时间戳 [@x380kkm 2026-09-22] ////
def timestamp(value: str) -> int:
    return int(datetime.strptime(value, "%Y-%m-%d %H:%M:%S")
               .replace(tzinfo=timezone(timedelta(hours=8))).timestamp() * 1000)
# //// /将活动时窗转换为服务端时间戳 ////


# //// 读取完整活动和票券角色主表并生成服务端定义 [@x380kkm 2026-09-22] ////
def generate(package: Path, cdn_root: Path | None) -> dict:
    with PackagedResourceReader(package, cdn_root=cdn_root) as reader:
        definitions = {}
        for name in ("multi_special_exchange_campaign", "multi_special_exchange_campaign_character"):
            logical = f"{MASTER_ROOT}/{name}.orderedmap"
            content = reader.read(logical)
            if content is None:
                raise ValueError(f"缺少活动主表: {logical}")
            definitions[name] = decode_multirow_ordered_map(content)
    tickets = {key: [int(row[0]) for row in rows]
               for key, rows in definitions["multi_special_exchange_campaign_character"].items()}
    campaigns = {}
    for key, row in definitions["multi_special_exchange_campaign"].items():
        if row[2] != "0":
            raise ValueError(f"活动 {key} 的奖励类型无法识别: {row[2]}")
        ticket_ids = [int(row[index]) for index in (10, 12, 14) if row[index] != "(None)"]
        for ticket in ticket_ids:
            if not tickets.get(str(ticket)):
                raise ValueError(f"活动 {key} 的票券 {ticket} 缺少候选角色")
        campaigns[key] = {
            "name": row[1],
            "start_at_ms": timestamp(row[3]),
            "end_at_ms": timestamp(row[4]),
            "exchange_end_at_ms": timestamp(row[5]),
            "ticket_ids": ticket_ids,
        }
    return {"campaigns": campaigns, "tickets": tickets}
# //// /读取完整活动和票券角色主表并生成服务端定义 ////


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--cdn-root", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    fixture = generate(args.package, args.cdn_root)
    args.output.write_text(json.dumps(fixture, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"campaigns": len(fixture["campaigns"]), "tickets": len(fixture["tickets"]),
                      "candidates": sum(map(len, fixture["tickets"].values()))}))


if __name__ == "__main__":
    main()
