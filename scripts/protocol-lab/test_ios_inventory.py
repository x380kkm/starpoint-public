# audience: internal
# # test-ios-inventory
# 此测试验证 arm64 Mach-O 的最低系统, SDK, FairPlay cryptid 和代码签名段读取.

from __future__ import annotations

import struct
import unittest

from ios_inventory import (
    LC_BUILD_VERSION,
    LC_CODE_SIGNATURE,
    LC_ENCRYPTION_INFO_64,
    MH_MAGIC_64,
    analyze_macho,
)


# //// 验证 Mach-O 加载命令决定 IPA 是否可直接重签名 [@x380kkm 2026-07-20] ////
class IosInventoryTest(unittest.TestCase):
    def test_reads_encryption_and_build_version(self) -> None:
        encryption = struct.pack("<IIIIII", LC_ENCRYPTION_INFO_64, 24, 4096, 8192, 1, 0)
        code_signature = struct.pack("<IIII", LC_CODE_SIGNATURE, 16, 12000, 512)
        build_version = struct.pack("<IIIIII", LC_BUILD_VERSION, 24, 2, 0x000C0000, 0x00120200, 0)
        commands = encryption + code_signature + build_version
        header = struct.pack(
            "<IiiIIIII",
            MH_MAGIC_64,
            0x0100000C,
            0,
            2,
            3,
            len(commands),
            0,
            0,
        )

        result = analyze_macho(header + commands)

        self.assertTrue(result["fairplay_encrypted"])
        self.assertTrue(result["has_code_signature_command"])
        self.assertEqual("arm64", result["slices"][0]["architecture"])
        self.assertEqual("12.0.0", result["slices"][0]["build_version"]["minimum_os"])
        self.assertEqual("18.2.0", result["slices"][0]["build_version"]["sdk"])


# //// /验证 Mach-O 加载命令决定 IPA 是否可直接重签名 ////


if __name__ == "__main__":
    unittest.main()
