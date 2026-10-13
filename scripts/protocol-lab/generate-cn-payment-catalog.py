# audience: internal
# # generate-cn-payment-catalog
#
# 商品和 PremiumLoginBonus 内容读取最终客户端包.
# CN 功能权益按商品主表描述投影, 客户端主表日期使用 UTC+8.
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


SOURCES = (
    "master/payment/android_payment.orderedmap",
    "master/payment/ios_payment.orderedmap",
    "master/bonus/premium_login_bonus.orderedmap",
    "master/bonus/premium_login_bonus_config.orderedmap",
)
CN_TIME_ZONE = timezone(timedelta(hours=8))
FUND_FEATURES = {
    "LaiteFund": "laite", "LaiteFund2": "laite2", "LaiteFund3": "laite3",
    "MainQuestFund": "main_quest", "EXQuestFund": "ex_quest",
}
SUBSCRIPTION_FEATURES = {
    "MonthCard": "month_card", "MiniMonthCard": "mini_month_card",
    "WeeklyBonus": "weekly_bonus", "WeeklyBonus2": "weekly_bonus2",
}
DISPLAY_FEATURES = {None, "SalePack", "morethanstones", "Comeback"}
RESTRICTION_PERIODS = (
    "permanently", "monthly", "daily", "periodically", "monthly0", "weekly", "comeback",
)


def optional(value: str) -> str | None:
    return None if value in ("", "(None)") else value


def optional_integer(value: str) -> int | None:
    return int(value) if optional(value) is not None else None


def timestamp(value: str) -> int:
    return int(datetime.strptime(value, "%Y-%m-%d %H:%M:%S").replace(tzinfo=CN_TIME_ZONE).timestamp())


# //// 读取两平台商品及持续奖励主表 [@x380kkm 2026-09-22] ////
def read_masters(package: Path, cdn_root: Path) -> dict:
    masters = {}
    with PackagedResourceReader(package, cdn_root=cdn_root) as reader:
        for source in SOURCES:
            payload = reader.read(source)
            if payload is None:
                raise ValueError(f"包内缺少支付主表: {source}")
            masters[source] = decode_multirow_ordered_map(payload)
    return masters


# //// 转换即时物品与功能内容 [@x380kkm 2026-09-22] ////
def convert_contents(row: list[str]) -> tuple[list[dict], list[dict]]:
    rewards, entitlements = [], []
    reward_types = {"3": "experience", "4": "mana", "5": "pass_card_point", "7": "star_crumb"}
    for index in range(10, 31, 3):
        kind, raw_id, raw_amount = row[index:index + 3]
        if kind in ("(None)", "0"):
            continue
        if kind == "1":
            rewards.append({"kind": "item", "id": int(raw_id), "amount": int(raw_amount)})
        elif kind == "2":
            entitlements.append({"entitlement": "subscription", "id": raw_id})
        elif kind == "6":
            entitlements.append({"entitlement": "subscription", "id": "month_card"})
        elif kind in reward_types:
            rewards.append({"kind": reward_types[kind], "amount": int(raw_amount)})
        else:
            raise ValueError(f"未知支付内容类型: {kind}")
    return rewards, entitlements


# //// 转换支付功能对应的资格 [@x380kkm 2026-09-22] ////
def feature_entitlements(feature: str | None) -> list[dict]:
    if feature in FUND_FEATURES:
        return [{"entitlement": "growth_fund", "kind": FUND_FEATURES[feature]}]
    if feature in SUBSCRIPTION_FEATURES:
        return [{"entitlement": "subscription", "id": SUBSCRIPTION_FEATURES[feature]}]
    if feature == "PassCard":
        return [{"entitlement": "pass_card"}]
    if feature in DISPLAY_FEATURES:
        return []
    raise ValueError(f"未知支付功能: {feature}")


# //// 保存完整商品字段和平台资格条件 [@x380kkm 2026-09-22] ////
def convert_product(product_id: str, android: list[str], ios: list[str]) -> dict:
    if len(android) != 40 or len(ios) != 41:
        raise ValueError(f"支付主表列数异常: {product_id}")
    economic_fields = [8, 9, *range(10, 39)]
    if any(android[index] != ios[index] for index in economic_fields):
        raise ValueError(f"两平台支付权益存在差异: {product_id}")
    rewards, entitlements = convert_contents(android)
    feature = optional(android[38])
    entitlements.extend(feature_entitlements(feature))
    restriction = None
    if android[31] != "(None)":
        restriction = {"period": RESTRICTION_PERIODS[int(android[31])], "limit": int(android[32])}
    return {
        "store_product_id": product_id,
        "name": android[3],
        "charge_vmoney_num": int(android[33]),
        "free_vmoney_num": int(android[34]),
        "price": int(android[8]),
        "start_time": timestamp(android[35]),
        "end_time": timestamp(android[36]),
        "list_order": int(android[1]),
        "page_kind": int(android[37]),
        "feature": feature,
        "reason_id": optional_integer(android[9]),
        "required_main_quest_id": optional_integer(android[39]),
        "ios_required_main_quest_id": optional_integer(ios[39]),
        "direct_purchase": feature != "MiniMonthCard",
        "purchase_restriction": restriction,
        "rewards": rewards,
        "entitlements": entitlements,
    }


def subscription(kind: str, duration: int, rewards: list[dict], **extra) -> dict:
    return {
        "kind": kind, "duration_days": duration, "daily_rewards": rewards,
        "daily_quest_boost_amount": 0, "boss_battle_boost_amount": 0,
        "daily_challenge_amount": 0, "enable_auto_3x": False, "reason_id": None,
        "activation_payment_total": None, "repurchase_remaining_days": 0,
        "catch_up_rewards": True, **extra,
    }


# //// 投影 CN 商品描述中的日领和加成权益 [@x380kkm 2026-09-22] ////
def cn_subscriptions(products: dict[str, list[str]]) -> dict:
    descriptions = {
        "com.leiting.wf.stonepack_Daily": ("30天", "50", "体力回复药（中）"),
        "com.leiting.wf.stonepack_RewardPoints": ("7天", "通用装备单抽扭蛋券", "3天"),
        "com.leiting.wf.challenge_point_pack": ("7天", "单人挑战次数", "3天"),
        "worldflipper.daily_set_02": ("30元", "30天", "非可购买礼包"),
    }
    for product_id, terms in descriptions.items():
        if any(term not in products[product_id][7] for term in terms):
            raise ValueError(f"周期权益描述与转换规则不一致: {product_id}")
    return {
        "month_card": subscription("month_card", 30, [
            {"kind": "free_vmoney", "amount": 50}, {"kind": "item", "id": 102, "amount": 1},
        ], daily_quest_boost_amount=1, boss_battle_boost_amount=1),
        "weekly_bonus": subscription("weekly_boss_boost", 7, [
            {"kind": "item", "id": 999005, "amount": 1},
        ], daily_quest_boost_amount=1, boss_battle_boost_amount=1, repurchase_remaining_days=3),
        "weekly_bonus2": subscription("weekly_daily_challenge", 7, [],
            daily_challenge_amount=1, repurchase_remaining_days=3, catch_up_rewards=False),
        "mini_month_card": subscription("mini_month_card", 30, [
            {"kind": "free_vmoney", "amount": 50},
        ], activation_payment_total=30),
    }


# //// 转换付费登录奖励的完整日领内容 [@x380kkm 2026-09-22] ////
def premium_daily_rewards(row: list[str]) -> list[dict]:
    result = []
    names = {"0": "free_vmoney", "1": "item", "2": "character", "3": "mana", "4": "experience"}
    for index in range(3, 23, 4):
        kind, amount, item_id, character_id = row[index:index + 4]
        if kind == "(None)":
            continue
        reward = {"kind": names[kind], "amount": int(amount)}
        if kind in ("1", "2"):
            reward["id"] = int(item_id if kind == "1" else character_id)
        result.append(reward)
    return result


# //// 关联登录奖励的周期和每日能力加成 [@x380kkm 2026-09-22] ////
def premium_subscriptions(masters: dict) -> dict:
    bonuses = masters[SOURCES[2]]
    configurations = masters[SOURCES[3]]
    result = {}
    for bonus_id, config in configurations.items():
        days = bonuses[bonus_id]
        if set(days) != {str(day) for day in range(1, int(config[3]) + 1)}:
            raise ValueError(f"登录奖励日序缺失: {bonus_id}")
        daily = premium_daily_rewards(days["1"])
        if any(premium_daily_rewards(row) != daily for row in days.values()):
            raise ValueError(f"登录奖励每日内容不同: {bonus_id}")
        result[bonus_id] = subscription("premium_login_bonus", int(config[3]), daily,
            enable_auto_3x=config[0].lower() == "true",
            daily_quest_boost_amount=int(config[1]), boss_battle_boost_amount=int(config[2]),
            reason_id=int(config[4]))
    return result


# //// 生成所有商品并要求权益引用存在 [@x380kkm 2026-09-22] ////
def convert(masters: dict) -> dict:
    android, ios = masters[SOURCES[0]], masters[SOURCES[1]]
    if set(android) != set(ios):
        raise ValueError("两平台支付商品目录不同")
    products = {key: convert_product(key, row, ios[key]) for key, row in android.items()}
    subscriptions = cn_subscriptions(android) | premium_subscriptions(masters)
    for product in products.values():
        for entitlement in product["entitlements"]:
            if entitlement["entitlement"] == "subscription" and entitlement["id"] not in subscriptions:
                raise ValueError(f"商品权益缺少定义: {product['store_product_id']}")
    return {"sources": list(SOURCES), "products": products, "subscriptions": subscriptions}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", required=True, type=Path)
    parser.add_argument("--cdn-root", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    catalog = convert(read_masters(args.package, args.cdn_root))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(catalog, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"products": len(catalog["products"]), "subscriptions": len(catalog["subscriptions"]), "output": str(args.output)}, ensure_ascii=False))


if __name__ == "__main__":
    main()
