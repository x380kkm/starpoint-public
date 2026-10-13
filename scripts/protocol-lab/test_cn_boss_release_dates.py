# audience: internal
# # test-cn-boss-release-dates
# 测试覆盖日服时区换算, 源日期匹配与同名关卡的独立窗口.

import json
import unittest
from datetime import datetime
from pathlib import Path

from cn_boss_release_dates import (
    CN_TIMEZONE, SOURCE_PATH, apply_boss_row_release, boss_release_dates, cn_release_time, ordered_map_codec,
)


# //// 验证公告时间与主表字段代表同一时刻 [@x380kkm 2026-09-26] ////
class BossReleaseDateTests(unittest.TestCase):
    def test_converts_japanese_noon_to_chinese_eleven(self):
        release = boss_release_dates()[1006003]
        self.assertEqual(cn_release_time(release), "2020-01-15 11:00:00")

    def test_matches_the_original_date_and_accepts_reapplication(self):
        releases = boss_release_dates()
        row = ["1019004", "4", "妖狐", "thumbnail", "1", releases[1019004]["master_start_at"], "(None)"]
        apply_boss_row_release(row, releases)
        apply_boss_row_release(row, releases)
        self.assertEqual(row[5], "2021-06-14 11:00:00")
        self.assertEqual(row[6], "(None)")

    def test_distinct_releases_keep_their_own_window(self):
        releases = boss_release_dates()
        sources = {entry["quest_id"]: entry for entry in json.loads(SOURCE_PATH.read_text(encoding="utf-8"))["quests"]}
        for quest_id, expected in (
            (1035004, "2023-12-14 11:00:00"),
            (1022004, "2025-02-20 12:00:00"),
            (1036004, "2023-07-21 12:00:00"),
            (1071004, "2025-06-26 12:00:00"),
        ):
            source = sources[quest_id]
            row = [str(quest_id), "4", source["name"], "thumbnail", "1", source["master_start_at"], source["master_end_at"]]
            with self.subTest(quest_id=quest_id):
                apply_boss_row_release(row, releases)
                self.assertEqual(row[5], expected)
                self.assertEqual(row[6], source["master_end_at"])

    def test_a_changed_source_window_requires_an_explicit_mapping(self):
        releases = boss_release_dates()
        row = ["1019004", "4", "妖狐", "thumbnail", "1", "2025-07-03 12:00:00", "(None)"]
        with self.assertRaisesRegex(ValueError, "1019004"):
            apply_boss_row_release(row, releases)

    def test_client_service_and_rescue_windows_cover_visible_super_quests(self):
        assets = Path(__file__).resolve().parents[2] / "core/personal-service/assets"
        windows = json.loads((assets / "cn-quest-windows.json").read_text(encoding="utf-8"))["quests"]
        rescue = {entry["quest_id"]: entry for entry in json.loads(
            (assets / "cn-rescue-quests.json").read_text(encoding="utf-8")
        ) if entry["category_id"] == 2}
        sources = json.loads(SOURCE_PATH.read_text(encoding="utf-8"))["quests"]
        client_rows = {}

        def collect_rows(value):
            if isinstance(value, dict):
                for child in value.values():
                    collect_rows(child)
            else:
                for row in value:
                    if row[107] == "4" and row[123] == "false":
                        client_rows[int(row[0])] = row

        collect_rows(ordered_map_codec().decode_ordered_map(
            (assets / "cn-activity-masters/boss_battle_quest.orderedmap").read_bytes()
        ))
        self.assertEqual(set(client_rows), {entry["quest_id"] for entry in sources})
        for release in sources:
            quest_id = release["quest_id"]
            start = release["master_start_at"]
            if release["jp_super_release_at"] and (
                release["master_end_at"] == "(None)" or release["schedule_basis"] == "official_jp_super_release"
            ):
                start = cn_release_time(release)
            instant = int(datetime.fromisoformat(start).replace(tzinfo=CN_TIMEZONE).timestamp())
            end = release["master_end_at"]
            end_instant = None if end == "(None)" else int(
                datetime.fromisoformat(end).replace(tzinfo=CN_TIMEZONE).timestamp()
            )
            with self.subTest(quest_id=quest_id):
                self.assertEqual(client_rows[quest_id][5:7], [start, end])
                self.assertEqual(windows[f"2:{quest_id}"]["start_at_ms"], instant * 1000)
                self.assertEqual(rescue[quest_id]["start_time"], instant)
                self.assertEqual(windows[f"2:{quest_id}"]["end_at_ms"], None if end_instant is None else end_instant * 1000)
                self.assertEqual(rescue[quest_id]["end_time"], end_instant)


if __name__ == "__main__":
    unittest.main()
