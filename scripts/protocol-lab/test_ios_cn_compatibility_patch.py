# audience: internal
# # test-ios-cn-compatibility-patch
# 此测试验证 CN iOS 1.8.4 兼容补丁的唯一上下文, 幂等性和失败边界.

from __future__ import annotations

import unittest

from ios_cn_compatibility_patch import (
    CN_1_8_4_COMPATIBILITY_PATCHES,
    apply_binary_patch,
    patch_cn_1_8_4_compatibility,
)


# //// 构造包含全部原始补丁窗口的紧凑 fixture [@x380kkm 2026-07-23] ////
def create_compatibility_fixture() -> bytes:
    return b"|fixture-boundary|".join(
        patch.source_window for patch in CN_1_8_4_COMPATIBILITY_PATCHES
    )
# //// /构造包含全部原始补丁窗口的紧凑 fixture ////


# //// 验证兼容补丁集只接受唯一原始或目标上下文 [@x380kkm 2026-07-23] ////
class IosCnCompatibilityPatchTest(unittest.TestCase):
    def test_preloads_odds_for_every_open_equipment_gacha(self) -> None:
        patch = next(
            patch
            for patch in CN_1_8_4_COMPATIBILITY_PATCHES
            if patch.name == "gacha_equipment_odds_preload"
        )

        self.assertEqual(0x3E9864C, patch.preferred_offset)
        self.assertEqual(bytes.fromhex("e4030032"), patch.source_window[16:20])
        self.assertEqual(bytes.fromhex("e4031f2a"), patch.target_window[16:20])

    def test_reapplies_adjacent_air_guards(self) -> None:
        patches = tuple(
            patch
            for patch in CN_1_8_4_COMPATIBILITY_PATCHES
            if patch.name
            in {
                "air_safe_fallthrough_guard_00533240",
                "air_safe_fallthrough_guard_00533254",
            }
        )
        window_start = min(
            patch.preferred_offset - patch.change_offset for patch in patches
        )
        window_end = max(
            patch.preferred_offset
            - patch.change_offset
            + len(patch.source_window)
            for patch in patches
        )
        source = bytearray(window_end - window_start)
        assigned = bytearray(window_end - window_start)
        for patch in patches:
            patch_start = (
                patch.preferred_offset - patch.change_offset - window_start
            )
            for index, value in enumerate(patch.source_window, patch_start):
                if assigned[index]:
                    self.assertEqual(source[index], value)
                source[index] = value
                assigned[index] = 1

        patched = bytes(source)
        for patch in patches:
            patched, evidence = apply_binary_patch(patched, patch)
            self.assertEqual("applied", evidence["status"])
        for patch in patches:
            repeated, evidence = apply_binary_patch(patched, patch)
            self.assertEqual(patched, repeated)
            self.assertEqual("already_applied", evidence["status"])

    def test_applies_every_patch_without_changing_binary_length(self) -> None:
        source = create_compatibility_fixture()

        patched, evidence = patch_cn_1_8_4_compatibility(source)

        self.assertEqual(len(source), len(patched))
        self.assertEqual(len(CN_1_8_4_COMPATIBILITY_PATCHES), len(evidence))
        self.assertTrue(all(item["status"] == "applied" for item in evidence))
        for patch in CN_1_8_4_COMPATIBILITY_PATCHES:
            self.assertNotIn(patch.source_window, patched)
            self.assertEqual(1, patched.count(patch.target_window))

    def test_accepts_an_already_patched_binary(self) -> None:
        patched, _ = patch_cn_1_8_4_compatibility(create_compatibility_fixture())

        repeated, evidence = patch_cn_1_8_4_compatibility(patched)

        self.assertEqual(patched, repeated)
        self.assertTrue(
            all(item["status"] == "already_applied" for item in evidence)
        )

    def test_rejects_a_missing_patch_context(self) -> None:
        source = b"|fixture-boundary|".join(
            patch.source_window for patch in CN_1_8_4_COMPATIBILITY_PATCHES[1:]
        )

        with self.assertRaisesRegex(
            ValueError, "patch=air_packaging_guard, source=0, target=0"
        ):
            patch_cn_1_8_4_compatibility(source)

    def test_rejects_a_duplicate_patch_context(self) -> None:
        source = create_compatibility_fixture()
        source += CN_1_8_4_COMPATIBILITY_PATCHES[0].source_window

        with self.assertRaisesRegex(
            ValueError, "patch=air_packaging_guard, source=2, target=0"
        ):
            patch_cn_1_8_4_compatibility(source)
# //// /验证兼容补丁集只接受唯一原始或目标上下文 ////


if __name__ == "__main__":
    unittest.main()
