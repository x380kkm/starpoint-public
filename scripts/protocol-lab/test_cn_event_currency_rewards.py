# audience: internal
# # cn-event-currency-rewards-tests
# 共用奖励组的活动货币投影同时覆盖普通掉落、稀有掉落和关卡引用.

import unittest
from copy import deepcopy

from cn_event_currency_rewards import QUEST_MASTER, SCORE_MASTER, RARE_MASTER, project_event_currencies


def quest(quest_id):
    row = [""] * 77
    row[0], row[76] = str(quest_id), "10"
    return row


def inputs():
    masters = {QUEST_MASTER: {"1": {"1": quest(1001)}, "2": {"1": quest(2001)}},
               SCORE_MASTER: {"10": {"1": ["silver", "0", "0", "100", "4", "500", "", ""],
                                     "3": ["rare", "1", "", "", "", "", "20", "0.25"]}},
               RARE_MASTER: {"20": {"2": ["silver", "0", "100", "50", "0.4", "true"],
                                     "4": ["weapon", "1", "301", "1", "0.6", "false"]}}}
    items = {"items": [{"id": str(item_id), "name": "银币", "thumbnail_id": "coin/silver",
                        "kind": "exchange", "effect_kind": 15, "category": 6, "group": 2}
                       for item_id in (100, 200)]}
    shops = {"0": {str(event): {"1": {"costs": [{"id": item_id, "amount": 5}]}}
                    for event, item_id in [(1, 100), (2, 200)]}}
    return masters, items, shops


# //// 验证同源奖励组按活动复制并保留抽取参数 [@x380kkm 2026-09-29] ////
class EventCurrencyRewardsTests(unittest.TestCase):
    def test_changes_only_target_currency_and_keeps_original_groups(self):
        masters, items, shops = inputs()
        original = deepcopy(masters)
        result = project_event_currencies(masters, items, shops)
        projected = result["masters"]
        self.assertEqual(masters, original)
        self.assertEqual(result["quest_score_groups"], {"2001": 11})
        self.assertEqual(projected[SCORE_MASTER]["10"], masters[SCORE_MASTER]["10"])
        self.assertEqual(projected[RARE_MASTER]["20"], masters[RARE_MASTER]["20"])
        self.assertEqual(projected[QUEST_MASTER]["1"], masters[QUEST_MASTER]["1"])
        self.assertEqual(projected[SCORE_MASTER]["11"]["1"], ["silver", "0", "0", "200", "4", "500", "", ""])
        self.assertEqual(projected[SCORE_MASTER]["11"]["3"], ["rare", "1", "", "", "", "", "21", "0.25"])
        self.assertEqual(projected[RARE_MASTER]["21"]["2"], ["silver", "0", "200", "50", "0.4", "true"])
        self.assertEqual(projected[RARE_MASTER]["21"]["4"], masters[RARE_MASTER]["20"]["4"])
        self.assertEqual(project_event_currencies(masters, items, shops), result)

    def test_keeps_currency_without_same_item_identity_unchanged(self):
        masters, items, shops = inputs()
        items["items"][1]["thumbnail_id"] = "coin/gold"
        result = project_event_currencies(masters, items, shops)
        self.assertEqual(result["masters"], masters)
        self.assertEqual(result["quest_score_groups"], {})

    def test_requires_unique_currency_within_the_activity_shop(self):
        masters, items, shops = inputs()
        third = dict(items["items"][1], id="300")
        items["items"].append(third)
        shops["0"]["2"]["2"] = {"costs": [{"id": 300, "amount": 5}]}
        with self.assertRaisesRegex(ValueError, "multiple matching currencies"):
            project_event_currencies(masters, items, shops)
# //// /验证同源奖励组按活动复制并保留抽取参数 ////


if __name__ == "__main__":
    unittest.main()
