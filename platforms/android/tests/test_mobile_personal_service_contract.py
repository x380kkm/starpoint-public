# audience: internal
# # mobile-personal-service-contract-tests
#
# 该测试保持 iOS Framework 和 Android 诊断宿主使用同一套 loopback 管理契约.

from __future__ import annotations

import unittest
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[3]
IOS_HEADER = REPOSITORY_ROOT / "platforms" / "ios" / "PersonalServiceBootstrap" / "StarpointPersonalServiceBootstrap.h"
IOS_BOOTSTRAP = REPOSITORY_ROOT / "platforms" / "ios" / "PersonalServiceBootstrap" / "StarpointPersonalServiceBootstrap.m"
IOS_HARNESS = REPOSITORY_ROOT / "platforms" / "ios" / "DiagnosticHarness" / "main.m"
ANDROID_ACTIVITY = REPOSITORY_ROOT / "platforms" / "android" / "DiagnosticHarness" / "src" / "dev" / "starpoint" / "personalservice" / "DiagnosticActivity.java"
ANDROID_BOOTSTRAP = REPOSITORY_ROOT / "platforms" / "android" / "PersonalServiceBootstrap" / "src" / "dev" / "starpoint" / "personalservice" / "PersonalServiceBootstrap.java"
ANDROID_JNI = REPOSITORY_ROOT / "platforms" / "android" / "DiagnosticHarness" / "native" / "starpoint_android_bridge.c"


# //// 验证移动端个人服务的端口, token 和生命周期契约 [@x380kkm 2026-07-24] ////
class MobilePersonalServiceContractTests(unittest.TestCase):
    def test_ios_exposes_the_android_equivalent_lifecycle_surface(self) -> None:
        header = IOS_HEADER.read_text(encoding="utf-8")
        for declaration in (
            "starpoint_personal_service_bootstrap_start",
            "starpoint_personal_service_bootstrap_port",
            "starpoint_personal_service_bootstrap_is_running",
            "starpoint_personal_service_bootstrap_copy_management_token",
            "starpoint_personal_service_bootstrap_flush",
            "starpoint_personal_service_bootstrap_stop",
        ):
            self.assertIn(declaration, header)

    def test_both_hosts_use_service_discovered_loopback_and_fragment_token(self) -> None:
        ios_harness = IOS_HARNESS.read_text(encoding="utf-8")
        android_activity = ANDROID_ACTIVITY.read_text(encoding="utf-8")
        self.assertIn("starpoint_personal_service_bootstrap_port()", ios_harness)
        self.assertIn("nativeGetPort(serviceHandle)", android_activity)
        self.assertIn("components.fragment", ios_harness)
        self.assertIn("/manage/#token=%s", android_activity)

    def test_both_hosts_flush_and_stop_before_release(self) -> None:
        ios_bootstrap = IOS_BOOTSTRAP.read_text(encoding="utf-8")
        android_activity = ANDROID_ACTIVITY.read_text(encoding="utf-8")
        self.assertIn("starpoint_personal_service_flush(personalService)", ios_bootstrap)
        self.assertIn("starpoint_personal_service_stop(personalService)", ios_bootstrap)
        self.assertIn("checkpointCompleted = nativeFlush(handle)", android_activity)
        self.assertIn("nativeStop(handle)", android_activity)

    def test_android_formal_bootstrap_matches_ios_lifecycle_surface(self) -> None:
        bootstrap = ANDROID_BOOTSTRAP.read_text(encoding="utf-8")
        jni = ANDROID_JNI.read_text(encoding="utf-8")
        for method in ("start", "isRunning", "flush", "stop", "endpoint", "managementUrl"):
            self.assertIn(f" {method}(", bootstrap)
        self.assertIn("getNoBackupFilesDir()", bootstrap)
        self.assertIn("/manage/#token=%s", bootstrap)
        for native_name in (
            "nativeStart",
            "nativeGetPort",
            "nativeIsRunning",
            "nativeCopyManagementToken",
            "nativeFlush",
            "nativeStop",
        ):
            self.assertIn(
                f"Java_dev_starpoint_personalservice_PersonalServiceBootstrap_{native_name}",
                jni,
            )


# //// /验证移动端个人服务的端口, token 和生命周期契约 ////


if __name__ == "__main__":
    unittest.main()
