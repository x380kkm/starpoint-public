# audience: internal
# # test-build-cn-voice-overlay
#
# 此测试验证 CN 语音排除表和战斗语音序号遵循客户端读取契约.

from __future__ import annotations

import hashlib
import importlib.util
import sys
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("build-cn-voice-overlay.py")
SPEC = importlib.util.spec_from_file_location("build_cn_voice_overlay", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"语音生成器无法加载: {MODULE_PATH}")
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


# //// 验证语音排除表产生可播放的客户端状态 [@x380kkm 2026-08-29] ////
class VoiceExclusionTests(unittest.TestCase):
    def test_writes_a_nonmatching_value_when_all_entries_are_restored(self) -> None:
        ui_string = {
            "character_voice_exclude": [["ruin_girl|dragon_slayer"]],
        }

        removed = MODULE.remove_voice_exclusions(
            ui_string, {"ruin_girl", "dragon_slayer"}
        )

        self.assertEqual(("ruin_girl", "dragon_slayer"), removed)
        self.assertEqual([["(None)"]], ui_string["character_voice_exclude"])

    def test_preserves_entries_outside_the_restored_character_set(self) -> None:
        ui_string = {
            "character_voice_exclude": [["ruin_girl|silent_character"]],
        }

        removed = MODULE.remove_voice_exclusions(ui_string, {"ruin_girl"})

        self.assertEqual(("ruin_girl",), removed)
        self.assertEqual(
            [["silent_character"]], ui_string["character_voice_exclude"]
        )
# //// /验证语音排除表产生可播放的客户端状态 ////


# //// 验证战斗语音序号按客户端规则连续枚举 [@x380kkm 2026-08-29] ////
class BattleVoicePathTests(unittest.TestCase):
    def test_stops_at_the_first_missing_index(self) -> None:
        string_id = "sample_character"
        paths = [
            f"character/{string_id}/voice/battle/battle_start_{index}.mp3"
            for index in (0, 2)
        ]
        remote_index = {
            hashlib.sha1(
                (path + MODULE.CN_ASSET_HASH_SALT).encode("utf-8")
            ).hexdigest(): None
            for path in paths
        }

        actual = MODULE.enumerate_battle_paths(string_id, remote_index, ())

        self.assertEqual((paths[0],), actual)
# //// /验证战斗语音序号按客户端规则连续枚举 ////


if __name__ == "__main__":
    unittest.main()
