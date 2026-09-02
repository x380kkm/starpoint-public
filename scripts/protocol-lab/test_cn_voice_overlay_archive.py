# audience: internal
# # test-cn-voice-overlay-archive
#
# 此测试验证语音归档在包内 CDN 和可写覆盖目录使用同一资源版本契约.

from __future__ import annotations

import base64
import hashlib
import io
import json
import tempfile
import unittest
import zipfile
from pathlib import Path

from cn_voice_overlay_archive import (
    ARCHIVE_NAME_PREFIX,
    LEGACY_ARCHIVE_NAME_PREFIX,
    install_voice_archive,
)


MASTER_ENTRY = "production/upload/aa/" + "a" * 38
AUDIO_ENTRY = "production/upload/bb/" + "b" * 38


# //// 构造最小 CN 语音差分 [@x380kkm 2026-08-29] ////
def entity_digest(data: bytes) -> str:
    encoded = base64.b64encode(hashlib.sha256(data).digest()).decode("ascii")
    return encoded.rstrip("=").replace("+", "_").replace("/", "-")


def archive_bytes(entries: list[tuple[str, bytes]]) -> bytes:
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for name, data in entries:
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, data)
    return output.getvalue()


def write_cdn(root: Path) -> None:
    entity_root = root / "entities"
    entity_root.mkdir(parents=True)
    for name in ("PathFile.csv", "10939-ios_medium.csv"):
        (entity_root / name).write_text(
            "production/upload/00/" + "0" * 38 + ",1.4.0,1,fixture,common\n",
            encoding="utf-8",
        )
    (root / "path").write_text(
        json.dumps(
            {
                "info": {
                    "client_asset_version": "1.4.54",
                    "target_asset_version": "1.4.54",
                    "eventual_target_asset_version": "1.4.54",
                    "is_initial": True,
                    "latest_maj_first_version": "1.4.0",
                },
                "full": {
                    "version": "1.4.0",
                    "archive": [
                        {
                            "location": "https://assets.example/patch/cn/archive-ios-full/base.zip",
                            "size": 1,
                            "sha256": base64.b64encode(b"0" * 32).decode("ascii"),
                        }
                    ],
                },
                "diff": [],
                "asset_version_hash": "fixture",
            },
            separators=(",", ":"),
        ),
        encoding="utf-8",
    )


def write_report(root: Path) -> Path:
    master = b"master-body"
    audio = b"audio-body"
    data = archive_bytes([(MASTER_ENTRY, master), (AUDIO_ENTRY, audio)])
    archive_path = root / "archive-ios-diff" / "starpoint-cn-voice-overlay-ios.zip"
    archive_path.parent.mkdir(parents=True)
    archive_path.write_bytes(data)
    report_path = root / "voice-overlay-report.json"
    report_path.write_text(
        json.dumps(
            {
                "role_count": 17,
                "speech_mp3_count": 1,
                "battle_mp3_count": 0,
                "missing_count": 0,
                "archive": {
                    "relative_path": "archive-ios-diff/starpoint-cn-voice-overlay-ios.zip",
                    "byte_length": len(data),
                    "sha256": base64.b64encode(hashlib.sha256(data).digest()).decode("ascii"),
                    "zip64": False,
                    "entry_count": 2,
                },
                "masters": [
                    {
                        "logical_path": "master/character/character_text.orderedmap",
                        "entry_path": MASTER_ENTRY,
                        "byte_length": len(master),
                        "digest": entity_digest(master),
                    }
                ],
                "assets": [
                    {
                        "logical_path": "character/example/voice/ally/join.mp3",
                        "entry_path": AUDIO_ENTRY,
                        "byte_length": len(audio),
                        "digest": entity_digest(audio),
                    }
                ],
            },
            separators=(",", ":"),
        ),
        encoding="utf-8",
    )
    return report_path
# //// /构造最小 CN 语音差分 ////


# //// 验证包内 CDN 与可写覆盖安装 [@x380kkm 2026-08-29] ////
class VoiceOverlayArchiveTests(unittest.TestCase):
    def test_installs_idempotent_cdn_and_override_chains(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            report_path = write_report(root / "report")

            cdn_root = root / "cdn"
            write_cdn(cdn_root)
            first = install_voice_archive(cdn_root, report_path)
            second = install_voice_archive(cdn_root, report_path)
            self.assertEqual("1.4.55", first["target_version"])
            self.assertFalse(first["reused"])
            self.assertTrue(second["reused"])
            self.assertTrue(
                Path(first["archive"]["relative_path"]).name.startswith(
                    ARCHIVE_NAME_PREFIX
                )
            )
            self.assertEqual("1.4.55", json.loads((cdn_root / "path").read_text())["info"]["target_asset_version"])
            self.assertTrue((cdn_root / first["archive"]["relative_path"]).is_file())
            entity_text = (cdn_root / "entities" / "10939-ios_medium.csv").read_text()
            self.assertIn(f"{MASTER_ENTRY},1.4.55,", entity_text)
            self.assertIn(f"{AUDIO_ENTRY},1.4.55,", entity_text)

            manifest = json.loads((cdn_root / "path").read_text())
            manifest["info"]["target_asset_version"] = "1.4.56"
            manifest["info"]["eventual_target_asset_version"] = "1.4.56"
            manifest["diff"].append(
                {
                    "version": "1.4.56",
                    "original_version": "1.4.55",
                    "archive": [],
                }
            )
            (cdn_root / "path").write_text(
                json.dumps(manifest, separators=(",", ":")), encoding="utf-8"
            )
            path_with_later_diff = (cdn_root / "path").read_bytes()
            entities_with_later_diff = (
                cdn_root / "entities" / "10939-ios_medium.csv"
            ).read_bytes()
            after_later_diff = install_voice_archive(cdn_root, report_path)
            self.assertTrue(after_later_diff["reused"])
            self.assertEqual("1.4.55", after_later_diff["target_version"])
            self.assertEqual(path_with_later_diff, (cdn_root / "path").read_bytes())
            self.assertEqual(
                entities_with_later_diff,
                (cdn_root / "entities" / "10939-ios_medium.csv").read_bytes(),
            )

            manifest["diff"].append(
                {
                    "version": "1.4.57",
                    "original_version": "1.4.56",
                    "archive": [
                        {
                            "location": "https://assets.example/patch/cn/archive-ios-diff/starpoint-cn-voice-overlay-1.4.56-1.4.57.zip",
                            "size": len((cdn_root / first["archive"]["relative_path"]).read_bytes()),
                            "sha256": base64.b64encode(
                                hashlib.sha256(
                                    (cdn_root / first["archive"]["relative_path"]).read_bytes()
                                ).digest()
                            ).decode("ascii"),
                        }
                    ],
                }
            )
            manifest["info"]["target_asset_version"] = "1.4.57"
            manifest["info"]["eventual_target_asset_version"] = "1.4.57"
            (cdn_root / "archive-ios-diff" / "starpoint-cn-voice-overlay-1.4.56-1.4.57.zip").write_bytes(
                (cdn_root / first["archive"]["relative_path"]).read_bytes()
            )
            (cdn_root / "path").write_text(
                json.dumps(manifest, separators=(",", ":")), encoding="utf-8"
            )
            latest_match = install_voice_archive(cdn_root, report_path)
            self.assertTrue(latest_match["reused"])
            self.assertEqual("1.4.57", latest_match["target_version"])

            source_root = root / "source"
            override_root = root / "override"
            write_cdn(source_root)
            override_first = install_voice_archive(
                source_root, report_path, override_root=override_root
            )
            override_second = install_voice_archive(
                source_root, report_path, override_root=override_root
            )
            self.assertFalse(override_first["reused"])
            self.assertTrue(override_second["reused"])
            self.assertEqual(
                "1.4.54",
                json.loads((source_root / "path").read_text())["info"]["target_asset_version"],
            )
            self.assertEqual(
                "1.4.55",
                json.loads((override_root / "path").read_text())["info"]["target_asset_version"],
            )
            self.assertTrue(
                (override_root / override_first["archive"]["relative_path"]).is_file()
            )
            override_entities = (
                override_root / "entities" / "PathFile.csv"
            ).read_text()
            self.assertIn(f"{MASTER_ENTRY},1.4.55,", override_entities)
            self.assertIn(f"{AUDIO_ENTRY},1.4.55,", override_entities)

    def test_reuses_legacy_installed_archive(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            report_path = write_report(root / "report")
            archive_data = (
                report_path.parent
                / "archive-ios-diff"
                / "starpoint-cn-voice-overlay-ios.zip"
            ).read_bytes()
            cdn_root = root / "cdn"
            write_cdn(cdn_root)
            legacy_name = f"{LEGACY_ARCHIVE_NAME_PREFIX}1.4.54-1.4.55.zip"
            legacy_relative = f"archive-ios-diff/{legacy_name}"
            legacy_path = cdn_root / legacy_relative
            legacy_path.parent.mkdir(parents=True)
            legacy_path.write_bytes(archive_data)
            manifest = json.loads((cdn_root / "path").read_text(encoding="utf-8"))
            manifest["info"]["target_asset_version"] = "1.4.55"
            manifest["info"]["eventual_target_asset_version"] = "1.4.55"
            manifest["diff"].append(
                {
                    "version": "1.4.55",
                    "original_version": "1.4.54",
                    "archive": [
                        {
                            "location": f"https://assets.example/patch/cn/{legacy_relative}",
                            "size": len(archive_data),
                            "sha256": base64.b64encode(
                                hashlib.sha256(archive_data).digest()
                            ).decode("ascii"),
                        }
                    ],
                }
            )
            (cdn_root / "path").write_text(
                json.dumps(manifest, separators=(",", ":")), encoding="utf-8"
            )

            audit = install_voice_archive(cdn_root, report_path)

            self.assertTrue(audit["reused"])
            self.assertEqual(legacy_relative, audit["archive"]["relative_path"])
            self.assertFalse(
                any(
                    path.name.startswith(ARCHIVE_NAME_PREFIX)
                    for path in (cdn_root / "archive-ios-diff").glob("*.zip")
                )
            )
# //// /验证包内 CDN 与可写覆盖安装 ////


if __name__ == "__main__":
    unittest.main()
