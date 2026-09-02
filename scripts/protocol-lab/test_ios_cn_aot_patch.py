# audience: internal
# # test-ios-cn-aot-patch
# 此测试验证 CN iOS AOT 原始与代理端点的等长替换, 唯一性检查和证据.

from __future__ import annotations

import unittest
from urllib.parse import urlsplit

from ios_cn_aot_patch import (
    CN_API_STRING_POOL_PATTERN,
    CN_BACKUP_VERSION_CANDIDATES,
    CN_PRIMARY_VERSION_CANDIDATES,
    EXPECTED_SOBOT_AUTHORITY_COUNTS,
    EXPECTED_SDK_URL_COUNT,
    ORIGINAL_SDK_AUTHORITY_PATTERN,
    PORT_ONE_SOBOT_PROXY_PATTERN,
    SOBOT_AUTHORITY_PATTERN,
    SOBOT_LOOPBACK_PATTERN,
    build_loopback_authority_prefix,
    build_sobot_loopback_authority_prefix,
    patch_cn_aot_endpoints,
)


# //// 构造 Emulator 代理 SDK 地址 [@x380kkm 2026-07-23] ////
def create_emulator_proxy_sdk_urls() -> bytes:
    return b"|".join(
        f"http://{'0' * (1 + index % 8)}@10.0.2.2:8001/sdk/{index}".encode(
            "ascii"
        )
        for index in range(EXPECTED_SDK_URL_COUNT)
    )
# //// /构造 Emulator 代理 SDK 地址 ////


# //// 构造原始雷霆 SDK 地址 [@x380kkm 2026-07-23] ////
def create_original_sdk_urls() -> bytes:
    authorities = (
        "https://id.leiting.com",
        "https://login.leiting.com",
        "https://loginslave1.roguelike.com",
        "http://logmonitor.leiting.com",
    )
    return b"|".join(
        f"{authorities[index % len(authorities)]}/sdk/{index}".encode("ascii")
        for index in range(EXPECTED_SDK_URL_COUNT)
    )
# //// /构造原始雷霆 SDK 地址 ////


# //// 构造原始 Sobot 地址 [@x380kkm 2026-08-22] ////
def create_original_sobot_urls() -> bytes:
    return b"|".join(
        (
            b"https://api.sobot.com/chat-sdk/sdk/user/v2/config.action",
            b"https://img.sobot.com/chatres/common/face.png",
            b"https://img.sobot.com/chatres/common/logo.png",
            b"https://www.sobot.com/chat/index.html",
        )
    )
# //// /构造原始 Sobot 地址 ////


# //// 构造端口 1 Sobot 代理地址 [@x380kkm 2026-08-22] ////
def create_port_one_sobot_urls() -> bytes:
    return b"|".join(
        (
            b"http://00@127.0.0.1:1/chat-sdk/sdk/user/v2/config.action",
            b"http://00@127.0.0.1:1/chatres/common/face.png",
            b"http://00@127.0.0.1:1/chatres/common/logo.png",
            b"http://00@127.0.0.1:1/chat/index.html",
        )
    )
# //// /构造端口 1 Sobot 代理地址 ////


# //// 构造包含五类 CN 端点的 AOT fixture [@x380kkm 2026-08-22] ////
def create_aot_fixture(
    sdk_urls: bytes,
    candidate_index: int = 1,
    sobot_urls: bytes | None = None,
) -> bytes:
    return (
        b"prefix"
        + b"|".join(
            (
                CN_API_STRING_POOL_PATTERN,
                CN_PRIMARY_VERSION_CANDIDATES[candidate_index],
                CN_BACKUP_VERSION_CANDIDATES[candidate_index],
                sdk_urls,
                sobot_urls or create_original_sobot_urls(),
            )
        )
        + b"suffix"
    )
# //// /构造包含五类 CN 端点的 AOT fixture ////


# //// 验证五类 CN 端点保持二进制长度 [@x380kkm 2026-08-22] ////
class IosCnAotPatchTest(unittest.TestCase):
    def test_patches_unique_endpoints_without_moving_binary_data(self) -> None:
        source = create_aot_fixture(create_emulator_proxy_sdk_urls())

        patched, evidence = patch_cn_aot_endpoints(source)

        self.assertEqual(len(source), len(patched))
        self.assertEqual(5, len(evidence))
        self.assertEqual(
            [
                "api_server",
                "primary_version",
                "backup_version",
                "sdk_urls",
                "observed_third_party_urls",
            ],
            [item["endpoint"] for item in evidence],
        )
        for item in evidence[:3]:
            self.assertGreater(item["bytes"], 0)
            self.assertIn("127.0.0.1:17171", item["target"])
            target = urlsplit(item["target"])
            self.assertEqual("http", target.scheme)
            self.assertEqual("127.0.0.1", target.hostname)
            self.assertEqual(17171, target.port)
        self.assertEqual(EXPECTED_SDK_URL_COUNT, evidence[3]["count"])
        self.assertEqual("emulator_proxy", evidence[3]["source_mode"])
        self.assertEqual(4, evidence[4]["count"])
        self.assertEqual(
            EXPECTED_SOBOT_AUTHORITY_COUNTS,
            evidence[4]["authority_counts"],
        )
        self.assertEqual("original_authorities", evidence[4]["source_mode"])
        self.assertNotIn(b"10.0.2.2:8001", patched)
        self.assertIsNone(SOBOT_AUTHORITY_PATTERN.search(patched))
        self.assertEqual(4, len(tuple(SOBOT_LOOPBACK_PATTERN.finditer(patched))))

    def test_rejects_a_missing_endpoint(self) -> None:
        with self.assertRaisesRegex(ValueError, "CN API 端点数量不正确"):
            patch_cn_aot_endpoints(b"missing")

    def test_accepts_original_https_version_endpoints(self) -> None:
        source = create_aot_fixture(create_original_sdk_urls(), candidate_index=0)

        patched, evidence = patch_cn_aot_endpoints(source)

        self.assertEqual(len(source), len(patched))
        self.assertTrue(evidence[1]["source"].startswith("https://update.leiting.com"))
        self.assertTrue(
            evidence[2]["source"].startswith("https://update.roguelike.com")
        )
        self.assertEqual("original_authorities", evidence[3]["source_mode"])
        self.assertIsNone(ORIGINAL_SDK_AUTHORITY_PATTERN.search(patched))

    def test_builds_valid_loopback_authorities_for_supported_lengths(self) -> None:
        for source_length in range(22, 40):
            target = build_loopback_authority_prefix(source_length)
            parsed = urlsplit(target.decode("ascii"))

            self.assertEqual(source_length, len(target))
            self.assertEqual("127.0.0.1", parsed.hostname)
            self.assertEqual(17171, parsed.port)

        for source_length in (20, 21):
            target = build_sobot_loopback_authority_prefix(source_length)
            parsed = urlsplit(target.decode("ascii"))

            self.assertEqual(source_length, len(target))
            self.assertEqual("127.1", parsed.hostname)
            self.assertEqual(17171, parsed.port)

    def test_rewrites_port_one_sobot_proxies(self) -> None:
        source = create_aot_fixture(
            create_emulator_proxy_sdk_urls(),
            sobot_urls=create_port_one_sobot_urls(),
        )

        patched, evidence = patch_cn_aot_endpoints(source)

        self.assertEqual(len(source), len(patched))
        self.assertEqual("port_one_loopback", evidence[4]["source_mode"])
        self.assertIsNone(PORT_ONE_SOBOT_PROXY_PATTERN.search(patched))
        self.assertEqual(4, len(tuple(SOBOT_LOOPBACK_PATTERN.finditer(patched))))

    def test_rejects_mixed_sdk_sources(self) -> None:
        source = create_aot_fixture(
            create_emulator_proxy_sdk_urls()
        ) + b"|https://id.leiting.com/sdk/mixed"

        with self.assertRaisesRegex(ValueError, "CN SDK URL 数量不正确"):
            patch_cn_aot_endpoints(source)

    def test_rejects_a_duplicate_endpoint(self) -> None:
        source = (
            create_aot_fixture(create_emulator_proxy_sdk_urls())
            + CN_API_STRING_POOL_PATTERN
        )

        with self.assertRaisesRegex(ValueError, "CN API 端点数量不正确"):
            patch_cn_aot_endpoints(source)

    def test_rejects_an_incomplete_sobot_authority_set(self) -> None:
        source = create_aot_fixture(
            create_emulator_proxy_sdk_urls(),
            sobot_urls=b"https://api.sobot.com/chat-sdk",
        )

        with self.assertRaisesRegex(ValueError, "Sobot authority 数量不正确"):
            patch_cn_aot_endpoints(source)


# //// /验证五类 CN 端点保持二进制长度 ////


if __name__ == "__main__":
    unittest.main()
