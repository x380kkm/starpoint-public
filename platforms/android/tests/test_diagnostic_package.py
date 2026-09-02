# audience: internal
# # android-diagnostic-package-tests
#
# 该文件使用最小 DEX, AArch64 ELF 和二进制清单验证 APK 结构,
# 并验证诊断清单限制及 Java native 声明与 JNI 导出名称一致.

from __future__ import annotations

import json
import re
import struct
import subprocess
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ElementTree
import zipfile
from pathlib import Path

ANDROID_ROOT = Path(__file__).resolve().parents[1]
PACKAGE_SCRIPT = ANDROID_ROOT / "package_diagnostic_apk.py"
VERIFY_SCRIPT = ANDROID_ROOT / "verify_diagnostic_apk.py"
ACTIVITY_SOURCE = (
    ANDROID_ROOT
    / "DiagnosticHarness"
    / "src"
    / "dev"
    / "starpoint"
    / "personalservice"
    / "DiagnosticActivity.java"
)
JNI_SOURCE = ANDROID_ROOT / "DiagnosticHarness" / "native" / "starpoint_android_bridge.c"
MANIFEST_SOURCE = ANDROID_ROOT / "DiagnosticHarness" / "AndroidManifest.xml"
NETWORK_SECURITY_SOURCE = (
    ANDROID_ROOT
    / "DiagnosticHarness"
    / "res"
    / "xml"
    / "network_security_config.xml"
)
ANDROID_ATTRIBUTE = "{http://schemas.android.com/apk/res/android}"


# //// 验证诊断 APK 组装和错误架构拒绝 [@x380kkm 2026-07-23] ////
class DiagnosticPackageTests(unittest.TestCase):
    def test_manifest_keeps_storage_and_cleartext_local(self) -> None:
        manifest = ElementTree.parse(MANIFEST_SOURCE).getroot()
        application = manifest.find("application")
        self.assertIsNotNone(application)
        self.assertEqual(application.get(f"{ANDROID_ATTRIBUTE}allowBackup"), "false")
        self.assertEqual(
            application.get(f"{ANDROID_ATTRIBUTE}usesCleartextTraffic"),
            "false",
        )
        self.assertEqual(
            application.get(f"{ANDROID_ATTRIBUTE}networkSecurityConfig"),
            "@xml/network_security_config",
        )
        activity = application.find("activity")
        self.assertIsNotNone(activity)
        self.assertEqual(activity.get(f"{ANDROID_ATTRIBUTE}exported"), "true")
        self.assertEqual(activity.get(f"{ANDROID_ATTRIBUTE}launchMode"), "singleTask")

        network_security = ElementTree.parse(NETWORK_SECURITY_SOURCE).getroot()
        base_config = network_security.find("base-config")
        self.assertIsNotNone(base_config)
        self.assertEqual(base_config.get("cleartextTrafficPermitted"), "false")
        loopback_domain = network_security.find("domain-config/domain")
        self.assertIsNotNone(loopback_domain)
        self.assertEqual(loopback_domain.get("includeSubdomains"), "false")
        self.assertEqual(loopback_domain.text, "127.0.0.1")

    def test_java_native_declarations_match_jni_exports(self) -> None:
        activity_source = ACTIVITY_SOURCE.read_text(encoding="utf-8")
        jni_source = JNI_SOURCE.read_text(encoding="utf-8")
        java_methods = set(
            re.findall(r"private static native \S+ (native\w+)\(", activity_source)
        )
        jni_methods = set(
            re.findall(
                r"Java_dev_starpoint_personalservice_DiagnosticActivity_(native\w+)\s*\(",
                jni_source,
            )
        )
        self.assertEqual(java_methods, jni_methods)

    def test_packages_and_verifies_arm64_payload(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            base, dex, native = self._write_inputs(root, machine=183)
            package = root / "diagnostic.apk"
            inventory = root / "inventory.json"
            subprocess.run(
                [
                    sys.executable,
                    str(PACKAGE_SCRIPT),
                    "--base",
                    str(base),
                    "--dex",
                    str(dex),
                    "--native-library",
                    str(native),
                    "--output",
                    str(package),
                ],
                check=True,
                capture_output=True,
                text=True,
            )
            subprocess.run(
                [
                    sys.executable,
                    str(VERIFY_SCRIPT),
                    "--apk",
                    str(package),
                    "--output",
                    str(inventory),
                ],
                check=True,
                capture_output=True,
                text=True,
            )
            report = json.loads(inventory.read_text(encoding="utf-8"))
            self.assertEqual(report["native_machine"], "AArch64")
            with zipfile.ZipFile(package) as archive:
                self.assertIn("classes.dex", archive.namelist())
                self.assertIn(
                    "lib/arm64-v8a/libstarpoint_android_bridge.so",
                    archive.namelist(),
                )

    def test_rejects_non_arm64_native_library(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            base, dex, native = self._write_inputs(root, machine=62)
            package = root / "diagnostic.apk"
            subprocess.run(
                [
                    sys.executable,
                    str(PACKAGE_SCRIPT),
                    "--base",
                    str(base),
                    "--dex",
                    str(dex),
                    "--native-library",
                    str(native),
                    "--output",
                    str(package),
                ],
                check=True,
                capture_output=True,
                text=True,
            )
            result = subprocess.run(
                [sys.executable, str(VERIFY_SCRIPT), "--apk", str(package)],
                check=False,
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("not AArch64", result.stderr)

    def _write_inputs(self, root: Path, machine: int) -> tuple[Path, Path, Path]:
        base = root / "base.apk"
        with zipfile.ZipFile(base, "w") as archive:
            archive.writestr("AndroidManifest.xml", b"\x03\x00\x08\x00" + bytes(12))
            archive.writestr("resources.arsc", b"\x02\x00\x0c\x00" + bytes(12))
        dex = root / "classes.dex"
        dex.write_bytes(b"dex\n035\x00" + bytes(32))
        native = root / "libstarpoint_android_bridge.so"
        elf = bytearray(64)
        elf[:6] = b"\x7fELF\x02\x01"
        struct.pack_into("<H", elf, 18, machine)
        native.write_bytes(elf)
        return base, dex, native


if __name__ == "__main__":
    unittest.main()
# //// /验证诊断 APK 组装和错误架构拒绝 ////
