# audience: internal
# # test-probe-ios-cn-simulator-offline
# 此测试在 Windows 上验证 Simulator 平台补丁, 启动守卫, 资源覆盖和观察报告收束.

from __future__ import annotations

import hashlib
import json
import struct
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from probe_ios_cn_simulator_offline import (
    LC_BUILD_VERSION,
    MACH_HEADER_64_SIZE,
    MH_MAGIC_64,
    PLATFORM_IOS,
    PLATFORM_IOS_SIMULATOR,
    STARTUP_GUARD_OFFSET,
    STARTUP_GUARD_SOURCE,
    STARTUP_GUARD_TARGET,
    export_http_observations,
    find_non_loopback_network_urls,
    grant_notification_permission,
    inject_cdn_overlay,
    patch_macho_platform,
    patch_startup_guard,
    record_http_observation_result,
    replace_personal_service_framework,
    summarize_http_observations,
    upgrade_legacy_cn_network_patches,
)


# //// 构造带一个 build-version 命令的 arm64 Mach-O [@x380kkm 2026-08-21] ////
def create_macho(platform: int) -> bytes:
    command = struct.pack(
        "<IIIIII", LC_BUILD_VERSION, 24, platform, 0x000C0000, 0x00120200, 0
    )
    header = struct.pack(
        "<IiiIIIII", MH_MAGIC_64, 0x0100000C, 0, 2, 1, len(command), 0, 0
    )
    return header + command + b"payload"
# //// /构造带一个 build-version 命令的 arm64 Mach-O ////


# //// 验证无头 Simulator 探针的纯函数边界 [@x380kkm 2026-08-21] ////
class IosCnSimulatorOfflineProbeTest(unittest.TestCase):
    def test_patches_exactly_one_ios_platform_command(self) -> None:
        source = create_macho(PLATFORM_IOS)

        patched, count = patch_macho_platform(source)

        self.assertEqual(1, count)
        self.assertEqual(len(source), len(patched))
        self.assertEqual(
            PLATFORM_IOS_SIMULATOR,
            struct.unpack_from("<I", patched, MACH_HEADER_64_SIZE + 8)[0],
        )

    def test_rejects_an_unknown_platform(self) -> None:
        with self.assertRaisesRegex(ValueError, "不支持的 Mach-O 平台"):
            patch_macho_platform(create_macho(99))

    def test_patches_only_the_locked_startup_guard(self) -> None:
        source = bytearray(STARTUP_GUARD_OFFSET + 64)
        source[
            STARTUP_GUARD_OFFSET : STARTUP_GUARD_OFFSET
            + len(STARTUP_GUARD_SOURCE)
        ] = STARTUP_GUARD_SOURCE

        patched, evidence = patch_startup_guard(bytes(source))

        self.assertEqual(
            STARTUP_GUARD_TARGET,
            patched[STARTUP_GUARD_OFFSET : STARTUP_GUARD_OFFSET + 4],
        )
        self.assertEqual(STARTUP_GUARD_OFFSET, evidence["offset"])
        self.assertEqual(len(source), len(patched))

    def test_accepts_only_loopback_urls_in_network_logs(self) -> None:
        logs = (
            "worldflipper [com.apple.network] url: http://127.0.0.1:17171/health\n"
            "worldflipper [com.apple.network] url: http://00@127.1:17171/chat-sdk/config\n"
            "worldflipper [com.apple.CFNetwork] url: http://000@127.0.0.1:17171/manage/\n"
        )

        self.assertEqual([], find_non_loopback_network_urls(logs))

    def test_reports_every_non_loopback_url(self) -> None:
        logs = (
            "worldflipper [com.apple.network] url: https://api.sobot.com/collect, definite\n"
            "worldflipper [com.apple.CFNetwork] url: http://example.com/report\n"
        )

        self.assertEqual(
            ["http://example.com/report", "https://api.sobot.com/collect"],
            find_non_loopback_network_urls(logs),
        )

    def test_grants_notifications_for_the_installed_bundle(self) -> None:
        command_result = {
            "arguments": [],
            "exit_code": 0,
            "stdout": "",
            "stderr": "",
        }
        with patch(
            "probe_ios_cn_simulator_offline.run_command",
            return_value=command_result,
        ) as run:
            result = grant_notification_permission("SIM-UDID", "dev.starpoint.game")

        self.assertIs(command_result, result)
        self.assertEqual("granted", result["outcome"])
        run.assert_called_once_with(
            [
                "xcrun",
                "simctl",
                "privacy",
                "SIM-UDID",
                "grant",
                "notifications",
                "dev.starpoint.game",
            ],
            check=False,
        )

    def test_continues_when_notifications_cannot_be_pregranted(self) -> None:
        command_result = {
            "arguments": [],
            "exit_code": 1,
            "stdout": "",
            "stderr": "Operation not permitted",
        }
        with patch(
            "probe_ios_cn_simulator_offline.run_command",
            return_value=command_result,
        ):
            result = grant_notification_permission("SIM-UDID", "dev.starpoint.game")

        self.assertEqual("unavailable", result["outcome"])

    def test_injects_nested_cdn_overlay_files(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            temporary_root = Path(temporary_directory)
            app_root = temporary_root / "worldflipper.app"
            overlay_root = temporary_root / "overlay"
            source = overlay_root / "wf" / "210009_config_20200415.json"
            source.parent.mkdir(parents=True)
            source.write_bytes(b'{"version":1}\n')
            app_root.mkdir()

            injected = inject_cdn_overlay(app_root, overlay_root)

            destination = (
                app_root
                / "StarpointCNCDN"
                / "wf"
                / "210009_config_20200415.json"
            )
            self.assertEqual(source.read_bytes(), destination.read_bytes())
            self.assertEqual(
                [
                    {
                        "path": "wf/210009_config_20200415.json",
                        "size": len(b'{"version":1}\n'),
                    }
                ],
                injected,
            )

    def test_replaces_the_complete_personal_service_framework(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            temporary_root = Path(temporary_directory)
            app_root = temporary_root / "worldflipper.app"
            destination = (
                app_root / "Frameworks" / "PersonalServiceBootstrap.framework"
            )
            destination.mkdir(parents=True)
            (destination / "obsolete.txt").write_text("old", encoding="utf-8")
            overlay = (
                temporary_root
                / "overlay"
                / "PersonalServiceBootstrap.framework"
            )
            (overlay / "Headers").mkdir(parents=True)
            binary_body = b"new-framework-binary"
            (overlay / "PersonalServiceBootstrap").write_bytes(binary_body)
            (overlay / "Info.plist").write_bytes(b"plist")
            (overlay / "Headers" / "service.h").write_bytes(b"header")

            evidence = replace_personal_service_framework(app_root, overlay)

            self.assertFalse((destination / "obsolete.txt").exists())
            self.assertEqual(
                binary_body,
                (destination / "PersonalServiceBootstrap").read_bytes(),
            )
            self.assertEqual(3, evidence["file_count"])
            self.assertEqual(
                hashlib.sha256(binary_body).hexdigest(),
                evidence["binary_sha256"],
            )

    def test_keeps_the_existing_framework_when_overlay_is_incomplete(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            temporary_root = Path(temporary_directory)
            app_root = temporary_root / "worldflipper.app"
            destination = (
                app_root / "Frameworks" / "PersonalServiceBootstrap.framework"
            )
            destination.mkdir(parents=True)
            marker = destination / "installed.txt"
            marker.write_text("installed", encoding="utf-8")
            overlay = (
                temporary_root
                / "overlay"
                / "PersonalServiceBootstrap.framework"
            )
            overlay.mkdir(parents=True)
            (overlay / "PersonalServiceBootstrap").write_bytes(b"binary")

            with self.assertRaisesRegex(ValueError, "缺少 Info.plist"):
                replace_personal_service_framework(app_root, overlay)

            self.assertEqual("installed", marker.read_text(encoding="utf-8"))

    def test_upgrades_four_sobot_authorities_and_one_cityjson_url(self) -> None:
        sobot_authority = b"http://00@127.0.0.1:1"
        cityjson_url = b"http://@127.0.0.1:1/cityjson?ie=utf-8"
        source = b"|".join([sobot_authority] * 4 + [cityjson_url])

        patched, evidence = upgrade_legacy_cn_network_patches(source)

        self.assertEqual(4, evidence["sobot"]["count"])
        self.assertEqual("port_one_loopback", evidence["sobot"]["source_mode"])
        self.assertEqual(1, evidence["cityjson"]["count"])
        self.assertEqual(4, patched.count(b"http://00@127.1:17171"))
        self.assertEqual(
            1,
            patched.count(b"http://127.0.0.1:17171/cityjson?ie=u8"),
        )
        self.assertNotIn(cityjson_url, patched)

    def test_rejects_more_than_one_legacy_cityjson_url(self) -> None:
        sobot_authority = b"http://00@127.0.0.1:1"
        cityjson_url = b"http://@127.0.0.1:1/cityjson?ie=utf-8"
        source = b"|".join([sobot_authority] * 4 + [cityjson_url] * 2)

        with self.assertRaisesRegex(ValueError, "旧 cityjson 地址数量不正确"):
            upgrade_legacy_cn_network_patches(source)

    def test_summarizes_core_errors_and_slim_probe_resources(self) -> None:
        observation_report = {
            "status": "failed",
            "error_code": "CORE_HTTP_RESPONSE_FAILED",
            "first_failure": {
                "method": "GET",
                "path": "/cdn/wf/config.json",
                "status": 404,
            },
            "core_observation_count": 3,
            "core_failure_count": 2,
            "missing_required_requests": [
                {"method": "POST", "path": "/sync_data"}
            ],
            "observations": [
                {
                    "method": "GET",
                    "path": "/cdn/wf/config.json",
                    "status": 404,
                    "count": 2,
                    "core": True,
                },
                {
                    "method": "POST",
                    "path": "/auth_login",
                    "status": 500,
                    "count": 1,
                    "core": True,
                },
            ],
        }

        summary = summarize_http_observations(observation_report, False)

        self.assertEqual(
            [{"method": "GET", "path": "/cdn/wf/config.json", "count": 2}],
            summary["core_404"],
        )
        self.assertEqual(
            [{"method": "POST", "path": "/auth_login", "count": 1}],
            summary["core_500"],
        )
        self.assertEqual(
            "probe_input_missing_bundled_cdn",
            summary["resource_failure_classification"],
        )
        self.assertEqual(
            [{"method": "POST", "path": "/sync_data"}],
            summary["missing_required_requests"],
        )

    def test_exports_observations_before_container_cleanup(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            output_root = Path(temporary_directory)
            observation_report = {
                "status": "failed",
                "error_code": "REQUIRED_OBSERVATIONS_MISSING",
                "first_failure": {
                    "reason": "required_request_missing",
                    "method": "POST",
                    "path": "/sync_data",
                },
                "core_observation_count": 2,
                "core_failure_count": 0,
                "required_failure_count": 0,
                "required_failures": [],
                "missing_required_requests": [
                    {"method": "POST", "path": "/sync_data"}
                ],
                "observations": [],
            }

            def run(arguments: list[str], **_: object) -> dict[str, object]:
                if arguments[:3] == ["xcrun", "simctl", "get_app_container"]:
                    return {
                        "arguments": arguments,
                        "exit_code": 0,
                        "stdout": "/sim/data\n",
                        "stderr": "",
                    }
                output_argument = Path(arguments[arguments.index("--output") + 1])
                output_argument.write_text(
                    json.dumps(observation_report), encoding="utf-8"
                )
                return {
                    "arguments": arguments,
                    "exit_code": 1,
                    "stdout": str(output_argument),
                    "stderr": "",
                }

            with patch(
                "probe_ios_cn_simulator_offline.run_command", side_effect=run
            ):
                result = export_http_observations(
                    "SIM-UDID",
                    "dev.starpoint.game",
                    "2026-08-22T00:00:00+00:00",
                    output_root,
                    False,
                )

            self.assertEqual("failed", result["status"])
            self.assertEqual(
                "REQUIRED_OBSERVATIONS_MISSING", result["error_code"]
            )
            self.assertEqual("/sim/data", result["data_container"])
            self.assertEqual(
                "sqlite_read_transaction_with_wal", result["consistency"]
            )
            self.assertEqual(
                [{"method": "POST", "path": "/sync_data"}],
                result["summary"]["missing_required_requests"],
            )
            observation_window = json.loads(
                (output_root / "http-observation-window.json").read_text(
                    encoding="utf-8"
                )
            )
            self.assertEqual(
                [
                    {"method": "POST", "path": "/sync_data"},
                    {
                        "method": "GET",
                        "path": "/chat-sdk/sdk/user/v2/config.action",
                    },
                    {
                        "method": "POST",
                        "path": "/chat-sdk/sdk/user/v2/appInit.action",
                    },
                    {
                        "method": "GET",
                        "path": "/wf/210009_config_20200415.json",
                    },
                ],
                observation_window["required_requests"],
            )

    def test_core_observation_failure_keeps_artifacts_and_fails_probe(self) -> None:
        report = {
            "status": "passed",
            "screenshots": {"app": "app.png", "management": "management.png"},
            "network_negative_assertion": {"log_path": "worldflipper.log"},
        }
        result = {
            "status": "failed",
            "error_code": "CORE_HTTP_RESPONSE_FAILED",
            "output_path": "http-observations.json",
        }

        record_http_observation_result(report, result)

        self.assertEqual("failed", report["status"])
        self.assertEqual("app.png", report["screenshots"]["app"])
        self.assertEqual(
            "worldflipper.log",
            report["network_negative_assertion"]["log_path"],
        )
        self.assertIs(result, report["http_observations"])
# //// /验证无头 Simulator 探针的纯函数边界 ////


if __name__ == "__main__":
    unittest.main()
