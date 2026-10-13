# audience: internal
# # generate-cn-optional-rewards
#
# 成长基金, 修行等级奖励和修行任务均读取最终客户端主表.
# 任务原表保留现有任务生成器的多行数组形状.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

from __future__ import annotations

import argparse
import json
from datetime import datetime, timedelta, timezone
from pathlib import Path

from audit_cn_gacha_candidate_assets import decode_multirow_ordered_map
from cn_packaged_resource_reader import PackagedResourceReader


def timestamp(value: str) -> int:
    return int(datetime.strptime(value, "%Y-%m-%d %H:%M:%S").replace(tzinfo=timezone(timedelta(hours=8))).timestamp())


def optional_integer(value: str) -> int | None:
    return int(value) if value not in ("", "(None)") else None


def mission_reward(row: list[str]) -> dict:
    kind = int(row[0])
    return {
        "kind": kind, "amount": 1 if kind == 6 else int(row[1]),
        "item_id": optional_integer(row[2]), "character_id": optional_integer(row[3]),
        "equipment_id": optional_integer(row[4]), "degree_id": optional_integer(row[5]),
    }


# //// 将基金资格绑定到对应等级或关卡 [@x380kkm 2026-09-22] ////
def growth_fund(rows: dict) -> dict:
    result = []
    for row in rows.values():
        kind = int(row[4])
        rank = int(row[5]) if kind == 0 else None
        fund_kind = ("laite" if rank < 80 else "laite2" if rank < 180 else "laite3") if kind == 0 else ("main_quest" if kind == 1 else "ex_quest")
        rewards = []
        if int(row[14]) != 0:
            raise ValueError(f"基金包含付费货币奖励: {row[0]}")
        if int(row[15]) > 0:
            rewards.append(mission_reward(["0", row[15], "", "", "", ""]))
        for index in (8, 11):
            if row[index] == "1":
                rewards.append(mission_reward(["1", row[index + 2], row[index + 1], "", "", ""]))
            elif row[index] not in ("0", "(None)"):
                raise ValueError(f"基金奖励类型未知: {row[0]}")
        result.append({
            "bonus_id": int(row[0]), "fund_kind": fund_kind, "required_rank": rank,
            "quest_category": None if kind == 0 else (1 if kind == 1 else 4),
            "quest_id": None if kind == 0 else int(row[6] if kind == 1 else row[7]),
            "rewards": rewards,
        })
    return {"rows": result}


# //// 转换修行活动等级和双轨奖励 [@x380kkm 2026-09-22] ////
def pass_card(events: dict, rewards: dict) -> dict:
    return {
        "events": {key: {
            "id": int(key), "point_cap": int(row[4]), "level_threshold": int(row[5]),
            "start_time": timestamp(row[8]), "end_time": timestamp(row[9]),
            "force_end_time": timestamp(row[10]), "is_free": row[11].lower() == "true",
        } for key, row in events.items()},
        "rewards": {key: {
            "id": int(key), "event_id": int(row[0]), "level": int(row[1]),
            "reward1": mission_reward(row[2:8]), "reward2": mission_reward(row[8:14]),
            "is_payment_money": row[14].lower() == "true",
        } for key, row in rewards.items()},
    }


def write_json(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


# //// 输出领取目录和通用任务生成器的输入 [@x380kkm 2026-09-22] ////
def generate(package: Path, cdn_root: Path, output: Path, mission_assets: Path) -> None:
    with PackagedResourceReader(package, cdn_root=cdn_root) as reader:
        def read(suffix: str) -> dict:
            source = f"master/{suffix}.orderedmap"
            payload = reader.read(source)
            if payload is None:
                raise ValueError(f"包内缺少主表: {source}")
            return decode_multirow_ordered_map(payload)

        write_json(output / "cn-growth-fund.json", growth_fund(read("growth_fund/growth_fund")))
        write_json(output / "cn-pass-card.json", pass_card(read("pass_card/pass_card_event"), read("pass_card/pass_card_reward")))
        for period in ("daily", "week", "event"):
            name = f"pass_card_{period}_mission"
            definitions = {key: [row] for key, row in read(f"pass_card/{name}").items()}
            stages = {key: {stage: [row] for stage, row in rows.items()} for key, rows in read(f"pass_card/{name}_reward").items()}
            write_json(mission_assets / f"mission_{name}.json", definitions)
            write_json(mission_assets / f"mission_{name}_reward.json", stages)
    print(json.dumps({"output": str(output), "mission_assets": str(mission_assets)}, ensure_ascii=False))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--cdn-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--mission-assets", type=Path, required=True)
    args = parser.parse_args()
    generate(args.package, args.cdn_root, args.output, args.mission_assets)


if __name__ == "__main__":
    main()
