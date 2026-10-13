# audience: internal
# # cn-regional-shop-projection-tests
# 商店原始分段、追加商品的末段窗口和跨活动商品身份在同一投影入口核对.

import unittest

from cn_regional_shop_projection import project_shop


def item(event, start, end, reward):
    row = [""] * 50
    row[1], row[2] = str(event), "0"
    row[18], row[19] = "101", "2"
    row[26], row[27] = str(start), str(end)
    row[32:35] = ["0", str(reward), "1"]
    return row


def period(row, start, end):
    left, right = int(row[start]), int(row[end])
    return (left, right) if left < right else None


def project(local, regional, activity="advent:1"):
    server, owners = {}, {}
    for item_id, row in local.items():
        server.setdefault(row[2], {}).setdefault(row[1], {})[item_id] = {}
        owners[item_id] = {"eventType": int(row[2]), "eventId": int(row[1])}
    return project_shop(activity, local, regional, server, owners,
                        {0: {"101", "201", "202", "203"}, 3: set(), 4: set()}, period)


# //// 核对分段上架和 CN 追加商品的同组时间边界 [@x380kkm 2026-09-29] ////
class AdditionalItemWindowTests(unittest.TestCase):
    def test_preserves_source_windows_and_places_additions_in_final_phase(self):
        local = {"1": item(1, 100, 200, 201), "2": item(1, 100, 200, 202),
                 "3": item(1, 100, 200, 203)}
        regional = {"1": item(1, 10, 30, 201), "2": item(1, 20, 30, 202)}
        windows, audit = project(local, regional)
        self.assertEqual(windows, {"1": (10, 30), "2": (20, 30), "3": (20, 30)})
        self.assertEqual(audit["issues"], [])
        self.assertEqual(audit["item_mappings"]["3"]["method"], "cn_addition_final_shop_phase")
        self.assertEqual(audit["item_mappings"]["3"]["regional_item_ids"], ["1", "2"])

    def test_preserves_early_close_for_exact_item_and_final_exchange_for_addition(self):
        local = {"1": item(1, 100, 200, 201), "2": item(1, 100, 200, 202),
                 "3": item(1, 100, 200, 203)}
        regional = {"1": item(1, 10, 20, 201), "2": item(1, 10, 30, 202)}
        windows, _ = project(local, regional)
        self.assertEqual(windows, {"1": (10, 20), "2": (10, 30), "3": (10, 30)})

    def test_keeps_unrelated_item_group_and_disjoint_source_periods_unresolved(self):
        for addition, source in [(item(1, 150, 200, 203), item(1, 20, 30, 202)),
                                 (item(1, 100, 200, 203), item(1, 40, 50, 202))]:
            with self.subTest(addition=addition[26:28], source=source[26:28]):
                local = {"1": item(1, 100, 200, 201), "2": item(1, 100, 200, 202), "3": addition}
                regional = {"1": item(1, 10, 30, 201), "2": source}
                windows, audit = project(local, regional)
                self.assertNotIn("3", windows)
                self.assertEqual(audit["issues"], [{"kind": "unresolved_item_window", "item_id": "3"}])
# //// /核对分段上架和 CN 追加商品的同组时间边界 ////


# //// 核对完整交易内容对应的唯一活动商店身份 [@x380kkm 2026-09-29] ////
class CanonicalShopTests(unittest.TestCase):
    def test_reuses_complete_same_contract_catalog_under_its_local_owner(self):
        local = {"1": item(9, 100, 200, 201), "2": item(9, 100, 200, 202)}
        regional = {"1": item(1, 10, 30, 201), "2": item(1, 10, 30, 202)}
        windows, audit = project(local, regional)
        self.assertEqual(windows, {"1": (10, 30), "2": (10, 30)})
        self.assertEqual(audit["canonical_activity_id"], "advent:9")
        self.assertEqual(audit["category"], "canonical_activity_alias")
        self.assertEqual(audit["issues"], [])
        self.assertEqual(local["1"][1], "9")

    def test_rejects_different_trade_or_multiple_owners_for_same_regional_ids(self):
        for changed in [item(9, 100, 200, 203), item(8, 100, 200, 202)]:
            with self.subTest(owner=changed[1], reward=changed[33]):
                local = {"1": item(9, 100, 200, 201), "2": changed}
                regional = {"1": item(1, 10, 30, 201), "2": item(1, 10, 30, 202)}
                windows, audit = project(local, regional)
                self.assertEqual(windows, {})
                self.assertEqual(audit["canonical_activity_id"], "advent:1")
                self.assertEqual(audit["issues"][0]["kind"], "missing_cn_shop")
# //// /核对完整交易内容对应的唯一活动商店身份 ////


if __name__ == "__main__":
    unittest.main()
