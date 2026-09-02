# audience: internal
# # test-prepare-ios-signing
# 此测试验证设备 profile 的有效期, UDID 范围, bundle identifier 匹配和 wildcard 展开.

from __future__ import annotations

from datetime import UTC, datetime
import unittest

from prepare_ios_signing import SigningProfileError, prepare_signing_entitlements


# //// 构造签名 profile 测试数据 [@x380kkm 2026-07-23] ////
def profile(application_identifier: str) -> dict:
    return {
        "Name": "Starpoint Development",
        "UUID": "00000000-0000-0000-0000-000000000001",
        "ExpirationDate": datetime(2027, 1, 1),
        "ApplicationIdentifierPrefix": ["TEAM123456"],
        "TeamIdentifier": ["TEAM123456"],
        "ProvisionedDevices": ["device-a", "device-b"],
        "Entitlements": {
            "application-identifier": application_identifier,
            "com.apple.developer.team-identifier": "TEAM123456",
            "get-task-allow": True,
            "keychain-access-groups": ["TEAM123456.*"],
        },
    }
# //// /构造签名 profile 测试数据 ////


# //// 验证签名 entitlements 只覆盖 profile 允许的设备 App [@x380kkm 2026-07-23] ////
class PrepareIosSigningTest(unittest.TestCase):
    def test_expands_wildcard_application_and_keychain_identifiers(self) -> None:
        entitlements, summary = prepare_signing_entitlements(
            profile("TEAM123456.dev.starpoint.*"),
            "dev.starpoint.PersonalServiceDiagnostic",
            now=datetime(2026, 1, 1, tzinfo=UTC),
        )

        expected_identifier = "TEAM123456.dev.starpoint.PersonalServiceDiagnostic"
        self.assertEqual(expected_identifier, entitlements["application-identifier"])
        self.assertEqual([expected_identifier], entitlements["keychain-access-groups"])
        self.assertEqual(2, summary["provisioned_device_count"])
        self.assertTrue(summary["get_task_allow"])

    def test_accepts_exact_application_identifier(self) -> None:
        entitlements, _ = prepare_signing_entitlements(
            profile("TEAM123456.dev.starpoint.PersonalServiceDiagnostic"),
            "dev.starpoint.PersonalServiceDiagnostic",
            now=datetime(2026, 1, 1, tzinfo=UTC),
        )

        self.assertEqual(
            "TEAM123456.dev.starpoint.PersonalServiceDiagnostic",
            entitlements["application-identifier"],
        )

    def test_rejects_bundle_identifier_outside_profile(self) -> None:
        with self.assertRaisesRegex(SigningProfileError, "not covered"):
            prepare_signing_entitlements(
                profile("TEAM123456.dev.starpoint.PersonalServiceDiagnostic"),
                "dev.starpoint.Other",
                now=datetime(2026, 1, 1, tzinfo=UTC),
            )

    def test_rejects_expired_profile(self) -> None:
        with self.assertRaisesRegex(SigningProfileError, "expired"):
            prepare_signing_entitlements(
                profile("TEAM123456.*"),
                "dev.starpoint.PersonalServiceDiagnostic",
                now=datetime(2028, 1, 1, tzinfo=UTC),
            )

    def test_rejects_profile_without_registered_devices(self) -> None:
        value = profile("TEAM123456.*")
        value["ProvisionedDevices"] = []

        with self.assertRaisesRegex(SigningProfileError, "no registered devices"):
            prepare_signing_entitlements(
                value,
                "dev.starpoint.PersonalServiceDiagnostic",
                now=datetime(2026, 1, 1, tzinfo=UTC),
            )
# //// /验证签名 entitlements 只覆盖 profile 允许的设备 App ////


if __name__ == "__main__":
    unittest.main()
