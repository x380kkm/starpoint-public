# audience: internal
# # generate-cn-gacha-campaigns
# 免费扭蛋活动目录读取最终客户端 master, 活动与卡池的关联共享原始次数类型和开放时间.
# 临时入口沿用客户端提供的活动 ID, 其余入口 ID 由原活动和真实卡池共同确定.
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import argparse
import json
from datetime import datetime, timedelta, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CN_ZONE = timezone(timedelta(hours=8))
TEMPORARY_ID_BASE = 2_000_000
CAMPAIGN_ID_STRIDE = 1_000_000


# //// 转换客户端中国时间为 Unix 秒 [@x380kkm 2026-09-28] ////
def timestamp(value: str) -> int:
    return int(datetime.strptime(value, "%Y-%m-%d %H:%M:%S").replace(tzinfo=CN_ZONE).timestamp())


# //// 保存完整活动关系并连接临时卡池入口 [@x380kkm 2026-09-28] ////
def catalog(rows: dict, temporary_aliases: dict) -> dict:
    aliases_by_string = {row[0]: int(key) for key, row in rows.items() if "_temporary_" in row[0]}
    canonical_to_temporary = {int(canonical): int(alias) for alias, canonical in temporary_aliases.items()}
    official_rows = [(key, row) for key, row in sorted(rows.items(), key=lambda entry: int(entry[0]))
                     if "_temporary_" not in row[0]]
    latest_by_pool = {int(pool): int(key) for key, row in official_rows for pool in row[5].split(",") if pool}
    result = {}
    for key, row in official_rows:
        campaign_id = int(key)
        pools = sorted({int(value) for value in row[5].split(",") if value})
        temporary_campaign_ids = {}
        for pool_id in pools:
            alias_id = aliases_by_string.get(f"{row[0]}_temporary_{pool_id}")
            if alias_id is None:
                alias_id = (canonical_to_temporary[pool_id] if pool_id in canonical_to_temporary and latest_by_pool[pool_id] == campaign_id
                            else TEMPORARY_ID_BASE + campaign_id * CAMPAIGN_ID_STRIDE + pool_id)
            if pool_id >= CAMPAIGN_ID_STRIDE or alias_id > 2_147_483_647:
                raise ValueError(f"临时活动 ID 超出客户端整数范围: {campaign_id}/{pool_id}")
            temporary_campaign_ids[str(pool_id)] = alias_id
        result[key] = {
            "name": row[1], "kind": int(row[2]),
            "startTime": timestamp(row[3]), "endTime": timestamp(row[4]),
            "gachaIds": pools, "temporaryCampaignIds": temporary_campaign_ids,
        }
    return result


# //// 从客户端包生成免费扭蛋活动目录 [@x380kkm 2026-09-28] ////
def main() -> None:
    from cn_packaged_resource_reader import PackagedResourceReader
    from generate_cn_shop_selection_fixture import master

    parser = argparse.ArgumentParser()
    parser.add_argument("--package", required=True, type=Path)
    parser.add_argument("--cdn-root", type=Path)
    parser.add_argument("--output", type=Path, default=ROOT / "assets/gacha_campaign.json")
    args = parser.parse_args()
    policy = json.loads((ROOT / "assets/gacha-region-policy.json").read_text(encoding="utf-8"))
    with PackagedResourceReader(args.package, cdn_root=args.cdn_root) as reader:
        rows = master(reader, "master/gacha/gacha_campaign.orderedmap")
    result = catalog(rows, policy["temporaryAliases"])
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"campaigns": len(result), "relations": sum(len(row["gachaIds"]) for row in result.values())}))


if __name__ == "__main__":
    main()
