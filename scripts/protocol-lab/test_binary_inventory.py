# audience: internal
# # test-binary-inventory
# 此测试验证 DEX, ELF 和 SWF 识别, CN 8003 与历史 18888 常量定位, 网络字符串提取和 ELF 架构读取.

from __future__ import annotations

import struct
import tempfile
import unittest
from pathlib import Path

from binary_inventory import build_inventory


# //// 验证客户端二进制清单保留协议证据 [@x380kkm 2026-07-20] ////
class BinaryInventoryTest(unittest.TestCase):
    def test_builds_protocol_inventory(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory) / "unpacked"
            root.mkdir()
            elf_header = bytearray(20)
            elf_header[:6] = b"\x7fELF\x02\x01"
            elf_header[18:20] = struct.pack("<H", 183)
            (root / "libclient.so").write_bytes(
                bytes(elf_header)
                + b"connect room_sequence 8003 18888 shijtswygamegf.leiting.com\x00"
            )
            (root / "classes.dex").write_bytes(
                b"dex\n035\x00" + struct.pack("<H", 8003) + struct.pack("<H", 18888)
            )
            (root / "game.dat").write_bytes(b"CWS" + b"socket msgpack")

            output_path = Path(temporary_directory) / "inventory.json"
            strings_directory = Path(temporary_directory) / "strings"
            inventory = build_inventory(root, output_path, strings_directory)

            files = {file["path"]: file for file in inventory["files"]}
            self.assertEqual("aarch64", files["libclient.so"]["elf"]["architecture"])
            indicators = {item["value"] for item in files["libclient.so"]["indicator_strings"]}
            self.assertIn("connect room_sequence 8003 18888 shijtswygamegf.leiting.com", indicators)
            self.assertTrue(files["classes.dex"]["protocol_port_hits"]["8003"])
            self.assertTrue(files["classes.dex"]["port_18888_hits"])
            self.assertEqual("swf", files["game.dat"]["kind"])
            self.assertEqual(1, inventory["summary"]["swf_count"])
            self.assertEqual(2, inventory["summary"]["files_with_protocol_ports"])
            self.assertTrue(Path(files["libclient.so"]["strings_path"]).is_file())


# //// /验证客户端二进制清单保留协议证据 ////


if __name__ == "__main__":
    unittest.main()
