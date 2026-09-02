# audience: internal
# # test-package-ios-personal-service
# 此测试验证 Mach-O 加载命令注入, 宿主 App 设备契约和 IPA 安全重打包.

from __future__ import annotations

import base64
import hashlib
import json
import plistlib
import stat
import struct
import tempfile
import unittest
import zipfile
from pathlib import Path

from ios_inventory import MH_MAGIC_64
from ios_cn_aot_patch import (
    CN_API_STRING_POOL_PATTERN,
    CN_BACKUP_VERSION_CANDIDATES,
    CN_PRIMARY_VERSION_CANDIDATES,
    EXPECTED_SOBOT_AUTHORITY_COUNTS,
    EXPECTED_SDK_URL_COUNT,
    SOBOT_LOOPBACK_PATTERN,
)
from ios_cn_compatibility_patch import CN_1_8_4_COMPATIBILITY_PATCHES
from package_ios_personal_service import (
    LC_SEGMENT_64,
    configure_ipad_multitasking_info,
    inject_load_dylib,
    package_cn_personal_service_ipa,
    package_personal_service_ipa,
    read_macho_layout,
    validate_gacha_banner_assets,
    write_report,
)


def create_arm64_macho(file_type: int, first_section_offset: int = 4096) -> bytes:
    section = struct.pack(
        "<16s16sQQIIIIIIII",
        b"__text",
        b"__TEXT",
        0x100000000 + first_section_offset,
        16,
        first_section_offset,
        2,
        0,
        0,
        0,
        0,
        0,
        0,
    )
    segment = (
        struct.pack(
            "<II16sQQQQiiII",
            LC_SEGMENT_64,
            72 + len(section),
            b"__TEXT",
            0x100000000,
            first_section_offset + 16,
            0,
            first_section_offset + 16,
            5,
            5,
            1,
            0,
        )
        + section
    )
    header = struct.pack(
        "<IiiIIIII",
        MH_MAGIC_64,
        0x0100000C,
        0,
        file_type,
        1,
        len(segment),
        0,
        0,
    )
    data = bytearray(first_section_offset + 16)
    data[: len(header + segment)] = header + segment
    data[first_section_offset : first_section_offset + 16] = b"personal-service"
    return bytes(data)


# //// 验证 Mach-O 头部空白区注入 [@x380kkm 2026-07-22] ////
class MachOLoadCommandInjectionTest(unittest.TestCase):
    def test_injects_an_idempotent_load_command(self) -> None:
        load_path = (
            "@executable_path/Frameworks/PersonalServiceBootstrap.framework/"
            "PersonalServiceBootstrap"
        )
        original = create_arm64_macho(2)

        patched, layout = inject_load_dylib(original, load_path)
        repeated, repeated_layout = inject_load_dylib(patched, load_path)

        self.assertEqual(2, layout.command_count)
        self.assertIn(load_path, layout.linked_dylibs)
        self.assertEqual(patched, repeated)
        self.assertEqual(layout, repeated_layout)

    def test_rejects_nonzero_header_padding(self) -> None:
        original = bytearray(create_arm64_macho(2))
        layout = read_macho_layout(original)
        original[layout.command_end] = 1

        with self.assertRaisesRegex(ValueError, "包含非零数据"):
            inject_load_dylib(
                bytes(original), "@executable_path/Frameworks/Test.framework/Test"
            )

    def test_rejects_insufficient_header_padding(self) -> None:
        command_end = 32 + 72 + 80
        original = create_arm64_macho(2, first_section_offset=command_end)

        with self.assertRaisesRegex(ValueError, "padding 不足"):
            inject_load_dylib(
                original, "@executable_path/Frameworks/Test.framework/Test"
            )


# //// /验证 Mach-O 头部空白区注入 ////


# //// 验证 IPA 重打包隔离 bundle 并移除旧签名 [@x380kkm 2026-07-24] ////
class PersonalServiceIpaPackagingTest(unittest.TestCase):
    def test_configures_ipad_multitasking_without_changing_iphone_orientation(
        self,
    ) -> None:
        info = {
            "UIDeviceFamily": [1],
            "UISupportedInterfaceOrientations": [
                "UIInterfaceOrientationLandscapeLeft",
            ],
            "UISupportedInterfaceOrientations~ipad": [
                "UIInterfaceOrientationPortrait"
            ],
            "UIRequiresFullScreen": True,
            "UILaunchScreen": "legacy-value",
        }

        configure_ipad_multitasking_info(info)
        first_result = dict(info)
        configure_ipad_multitasking_info(info)

        self.assertEqual([1, 2], info["UIDeviceFamily"])
        self.assertEqual(
            ["UIInterfaceOrientationLandscapeLeft"],
            info["UISupportedInterfaceOrientations"],
        )
        self.assertEqual(
            [
                "UIInterfaceOrientationPortrait",
                "UIInterfaceOrientationPortraitUpsideDown",
                "UIInterfaceOrientationLandscapeLeft",
                "UIInterfaceOrientationLandscapeRight",
            ],
            info["UISupportedInterfaceOrientations~ipad"],
        )
        self.assertFalse(info["UIRequiresFullScreen"])
        self.assertEqual({}, info["UILaunchScreen"])
        self.assertEqual(first_result, info)

    def test_packages_framework_and_marks_output_unsigned(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            input_ipa = root / "input.ipa"
            output_ipa = root / "output.ipa"
            framework = root / "PersonalServiceBootstrap.framework"
            framework.mkdir()
            framework_binary = framework / "PersonalServiceBootstrap"
            framework_binary.write_bytes(create_arm64_macho(6))
            framework_plist = {
                "CFBundlePackageType": "FMWK",
                "UIDeviceFamily": [9],
                "UISupportedInterfaceOrientations~ipad": ["framework-value"],
                "UIRequiresFullScreen": True,
            }
            (framework / "Info.plist").write_bytes(
                plistlib.dumps(framework_plist)
            )

            info = {
                "CFBundleExecutable": "worldflipper",
                "CFBundleIdentifier": "com.example.original",
                "CFBundleDisplayName": "Original",
                "UIDeviceFamily": [1],
                "UISupportedInterfaceOrientations": [
                    "UIInterfaceOrientationPortrait",
                    "UIInterfaceOrientationPortraitUpsideDown",
                ],
                "UISupportedInterfaceOrientations~ipad": [
                    "UIInterfaceOrientationPortrait"
                ],
                "UIRequiresFullScreen": True,
                "UILaunchScreen": "legacy-value",
            }
            with zipfile.ZipFile(input_ipa, "w") as archive:
                archive.writestr("Payload/Test.app/Info.plist", plistlib.dumps(info))
                sdk_urls = b"|".join(
                    f"https://id.leiting.com/sdk/{index}".encode("ascii")
                    for index in range(EXPECTED_SDK_URL_COUNT)
                )
                sobot_urls = b"|".join(
                    (
                        b"https://api.sobot.com/chat-sdk/sdk/user/v2/config.action",
                        b"https://img.sobot.com/chatres/common/face.png",
                        b"https://img.sobot.com/chatres/common/logo.png",
                        b"https://www.sobot.com/chat/index.html",
                    )
                )
                compatibility_windows = b"|".join(
                    patch.source_window
                    for patch in CN_1_8_4_COMPATIBILITY_PATCHES
                )
                executable = create_arm64_macho(2) + b"|".join(
                    (
                        CN_API_STRING_POOL_PATTERN,
                        CN_PRIMARY_VERSION_CANDIDATES[0],
                        CN_BACKUP_VERSION_CANDIDATES[0],
                        sdk_urls,
                        sobot_urls,
                        compatibility_windows,
                    )
                )
                archive.writestr("Payload/Test.app/worldflipper", executable)
                archive.writestr(
                    "Payload/Test.app/_CodeSignature/CodeResources", b"old"
                )
                archive.writestr("Payload/Test.app/embedded.mobileprovision", b"old")

            result = package_cn_personal_service_ipa(
                input_ipa,
                framework,
                output_ipa,
                "dev.starpoint.offline",
                "Starpoint Offline",
            )
            report_path = root / "package-report.json"
            write_report(report_path, result)

            with zipfile.ZipFile(output_ipa) as archive:
                names = archive.namelist()
                packaged_info = plistlib.loads(
                    archive.read("Payload/Test.app/Info.plist")
                )
                packaged_executable = archive.read("Payload/Test.app/worldflipper")
                framework_info = archive.getinfo(
                    "Payload/Test.app/Frameworks/PersonalServiceBootstrap.framework/"
                    "PersonalServiceBootstrap"
                )
                packaged_framework_plist = plistlib.loads(
                    archive.read(
                        "Payload/Test.app/Frameworks/"
                        "PersonalServiceBootstrap.framework/Info.plist"
                    )
                )

            self.assertNotIn("Payload/Test.app/_CodeSignature/CodeResources", names)
            self.assertNotIn("Payload/Test.app/embedded.mobileprovision", names)
            self.assertEqual(
                "dev.starpoint.offline", packaged_info["CFBundleIdentifier"]
            )
            self.assertEqual("Starpoint Offline", packaged_info["CFBundleDisplayName"])
            self.assertEqual([1, 2], packaged_info["UIDeviceFamily"])
            self.assertEqual(
                [
                    "UIInterfaceOrientationPortrait",
                    "UIInterfaceOrientationPortraitUpsideDown",
                ],
                packaged_info["UISupportedInterfaceOrientations"],
            )
            self.assertEqual(
                [
                    "UIInterfaceOrientationPortrait",
                    "UIInterfaceOrientationPortraitUpsideDown",
                    "UIInterfaceOrientationLandscapeLeft",
                    "UIInterfaceOrientationLandscapeRight",
                ],
                packaged_info["UISupportedInterfaceOrientations~ipad"],
            )
            self.assertFalse(packaged_info["UIRequiresFullScreen"])
            self.assertEqual({}, packaged_info["UILaunchScreen"])
            self.assertEqual(framework_plist, packaged_framework_plist)
            self.assertTrue(
                packaged_info["NSAppTransportSecurity"]["NSAllowsLocalNetworking"]
            )
            self.assertIn(
                result["load_path"],
                read_macho_layout(packaged_executable).linked_dylibs,
            )
            self.assertEqual(0o755, stat.S_IMODE(framework_info.external_attr >> 16))
            self.assertTrue(result["requires_resigning"])
            self.assertFalse(result["installable"])
            self.assertEqual(
                result, json.loads(report_path.read_text(encoding="utf-8"))
            )
            self.assertEqual(5, len(result["cn_endpoint_replacements"]))
            for replacement in result["cn_endpoint_replacements"]:
                if replacement["endpoint"] == "sdk_urls":
                    self.assertEqual(EXPECTED_SDK_URL_COUNT, replacement["count"])
                    self.assertEqual(
                        "original_authorities", replacement["source_mode"]
                    )
                    self.assertNotIn(b"https://id.leiting.com", packaged_executable)
                    continue
                if replacement["endpoint"] == "observed_third_party_urls":
                    self.assertEqual(4, replacement["count"])
                    self.assertEqual(
                        EXPECTED_SOBOT_AUTHORITY_COUNTS,
                        replacement["authority_counts"],
                    )
                    self.assertEqual(
                        4,
                        len(tuple(SOBOT_LOOPBACK_PATTERN.finditer(packaged_executable))),
                    )
                    continue
                expected = replacement["target"].encode("ascii")
                if replacement["endpoint"] == "api_server":
                    expected = b"\x04http\x1b" + expected.removeprefix(b"http://")
                self.assertIn(expected, packaged_executable)

            self.assertEqual(4, result["schema_version"])
            self.assertEqual(
                "three_finger_long_press", result["management_activation_method"]
            )
            self.assertEqual(
                len(CN_1_8_4_COMPATIBILITY_PATCHES),
                len(result["cn_compatibility_patches"]),
            )
            self.assertTrue(
                all(
                    item["status"] == "applied"
                    for item in result["cn_compatibility_patches"]
                )
            )
            self.assertIsNone(packaged_info.get("StarpointCNCDNBundlePath"))

    def test_embeds_a_non_empty_cn_cdn_bundle_and_declares_the_relative_path(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            input_ipa = root / "input.ipa"
            output_ipa = root / "output.ipa"
            framework = root / "PersonalServiceBootstrap.framework"
            framework.mkdir()
            (framework / "PersonalServiceBootstrap").write_bytes(create_arm64_macho(6))
            cdn_bundle = root / "cn"
            (cdn_bundle / "entities").mkdir(parents=True)
            (cdn_bundle / "entities" / "fixture.csv").write_bytes(b"id,name\n1,test\n")
            (cdn_bundle / "archive-common-full").mkdir()
            archive_bytes = b"zip"
            (cdn_bundle / "archive-common-full" / "fixture.zip").write_bytes(
                archive_bytes
            )
            banner_hash = "a" * 40
            standard_banner = (
                b"\x89PNG\r\n\x1a\n"
                + b"\x00\x00\x00\rIHDR"
                + struct.pack(">II", 510, 180)
                + bytes(16)
            )
            game_banner = b"\x89png\r\n\x1a\n" + standard_banner[8:]
            catalog_bytes = json.dumps(
                {
                    "activities": [
                        {
                            "activity_id": "gacha:80000",
                            "kind": "gacha",
                            "tags": ["banner:generated"],
                            "banner_key": f"{banner_hash}.png",
                            "image_candidates": [{"key": f"{banner_hash}.png"}],
                        }
                    ]
                },
                separators=(",", ":"),
            ).encode("utf-8")
            (cdn_bundle / "activity-catalog.json").write_bytes(catalog_bytes)
            management_banner_path = (
                cdn_bundle / "activity-banners" / f"{banner_hash}.png"
            )
            management_banner_path.parent.mkdir()
            management_banner_path.write_bytes(standard_banner)
            game_banner_path = (
                cdn_bundle
                / "production"
                / "bundle"
                / banner_hash[:2]
                / banner_hash[2:]
            )
            game_banner_path.parent.mkdir(parents=True)
            game_banner_path.write_bytes(game_banner)
            (cdn_bundle / "path").write_text(
                json.dumps(
                    {
                        "full": {
                            "archive": [
                                {
                                    "location": (
                                        "{$cdnAddress}/archive-common-full/fixture.zip"
                                    ),
                                    "size": 1,
                                    "sha256": base64.b64encode(bytes(32)).decode("ascii"),
                                }
                            ]
                        },
                        "diff": [],
                    },
                    separators=(",", ":"),
                ),
                encoding="utf-8",
            )

            with zipfile.ZipFile(input_ipa, "w") as archive:
                archive.writestr(
                    "Payload/Test.app/Info.plist",
                    plistlib.dumps(
                        {
                            "CFBundleExecutable": "worldflipper",
                            "CFBundleIdentifier": "com.example.original",
                        }
                    ),
                )
                archive.writestr(
                    "Payload/Test.app/worldflipper",
                    create_arm64_macho(2),
                )
                archive.writestr(
                    f"Payload/Test.app/asset/production/bundle/{banner_hash[:2]}/{banner_hash[2:]}",
                    b"stale-banner",
                )

            result = package_personal_service_ipa(
                input_ipa,
                framework,
                output_ipa,
                "dev.starpoint.offline",
                "Starpoint Offline",
                cdn_bundle,
            )

            with zipfile.ZipFile(output_ipa) as archive:
                names = archive.namelist()
                packaged_info = plistlib.loads(
                    archive.read("Payload/Test.app/Info.plist")
                )
                archive_fixture = archive.getinfo(
                    "Payload/Test.app/StarpointCNCDN/archive-common-full/fixture.zip"
                )
                packaged_path_bytes = archive.read(
                    "Payload/Test.app/StarpointCNCDN/path"
                )
                packaged_banner = archive.read(
                    f"Payload/Test.app/asset/production/bundle/{banner_hash[:2]}/{banner_hash[2:]}"
                )

            self.assertEqual(
                "StarpointCNCDN",
                packaged_info["StarpointCNCDNBundlePath"],
            )
            self.assertEqual("direct", packaged_info["StarpointCNCDNBundleMode"])
            self.assertIn(
                "Payload/Test.app/StarpointCNCDN/entities/fixture.csv",
                names,
            )
            self.assertIn(
                "Payload/Test.app/StarpointCNCDN/archive-common-full/fixture.zip",
                names,
            )
            self.assertEqual(zipfile.ZIP_STORED, archive_fixture.compress_type)
            self.assertEqual(game_banner, packaged_banner)
            path_entry = json.loads(packaged_path_bytes)["full"]["archive"][0]
            self.assertEqual(len(archive_bytes), path_entry["size"])
            self.assertEqual(
                base64.b64encode(hashlib.sha256(archive_bytes).digest()).decode("ascii"),
                path_entry["sha256"],
            )
            self.assertEqual(6, result["cn_cdn_file_count"])
            self.assertEqual(
                len(b"id,name\n1,test\n")
                + len(archive_bytes)
                + len(packaged_path_bytes)
                + len(catalog_bytes)
                + len(standard_banner)
                + len(game_banner),
                result["cn_cdn_total_size"],
            )
            self.assertEqual(1, result["embedded_gacha_banner_count"])
            self.assertEqual(len(game_banner), result["embedded_gacha_banner_size"])

    def test_rejects_an_empty_cn_cdn_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            empty_bundle = root / "empty-cn"
            empty_bundle.mkdir()

            with self.assertRaisesRegex(ValueError, "不能为空"):
                package_personal_service_ipa(
                    root / "missing.ipa",
                    root / "missing.framework",
                    root / "output.ipa",
                    "dev.starpoint.offline",
                    "Starpoint Offline",
                    empty_bundle,
                )

    def test_rejects_a_gacha_activity_without_a_banner(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            cdn_bundle = Path(temporary_directory)
            (cdn_bundle / "activity-catalog.json").write_text(
                json.dumps(
                    {
                        "activities": [
                            {
                                "activity_id": "gacha:80000",
                                "kind": "gacha",
                                "image_candidates": [],
                            }
                        ]
                    }
                ),
                encoding="utf-8",
            )

            with self.assertRaisesRegex(ValueError, "缺少有效 banner_key"):
                validate_gacha_banner_assets(cdn_bundle)

    def test_rejects_the_original_bundle_identifier(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            input_ipa = root / "input.ipa"
            framework = root / "PersonalServiceBootstrap.framework"
            framework.mkdir()
            (framework / "PersonalServiceBootstrap").write_bytes(create_arm64_macho(6))
            with zipfile.ZipFile(input_ipa, "w") as archive:
                archive.writestr(
                    "Payload/Test.app/Info.plist",
                    plistlib.dumps(
                        {
                            "CFBundleExecutable": "worldflipper",
                            "CFBundleIdentifier": "com.example.original",
                        }
                    ),
                )
                archive.writestr("Payload/Test.app/worldflipper", create_arm64_macho(2))

            with self.assertRaisesRegex(ValueError, "不同于原包"):
                package_personal_service_ipa(
                    input_ipa,
                    framework,
                    root / "output.ipa",
                    "com.example.original",
                    "Starpoint Offline",
                )


# //// /验证 IPA 重打包隔离 bundle 并移除旧签名 ////


if __name__ == "__main__":
    unittest.main()
