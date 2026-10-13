# audience: internal
# # test-stage-cn-character-full-shots
# 此测试使用合成 CDN 验证 full-shot 过滤, 映射, archive 改写和原子发布.

# /// script
# requires-python = ">=3.12"
# dependencies = [
#   "Pillow",
# ]
# ///

from __future__ import annotations

import base64
import contextlib
import hashlib
import importlib.util
import io
import json
import struct
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest import mock

import cn_character_full_shot_assets as assets
import cn_character_full_shot_plan as plan
from PIL import Image

MODULE_PATH = Path(__file__).with_name("stage-cn-character-full-shots.py")
MODULE_SPEC = importlib.util.spec_from_file_location(
    "stage_cn_character_full_shots", MODULE_PATH
)
if MODULE_SPEC is None or MODULE_SPEC.loader is None:
    raise RuntimeError("full-shot staging module could not be loaded")
staging = importlib.util.module_from_spec(MODULE_SPEC)
sys.modules[MODULE_SPEC.name] = staging
MODULE_SPEC.loader.exec_module(staging)

SOURCE_LOGICAL_PATH = "character/global_hero/ui/full_shot_1440_1920_0.png"
TARGET_LOGICAL_PATH = "character/cn_hero/ui/full_shot_1440_1920_0.png"
LEGACY_ZIP64_EXTRA = b"\x01\x00\x10\x00" + struct.pack("<QQ", 1, 1)
PRESERVED_EXTRA = b"\xfe\xca\x03\x00abc"


# //// 生成带实体记录和 archive 的合成 CN CDN [@x380kkm 2026-08-21] ////
def png_bytes(
    size: tuple[int, int],
    color: tuple[int, int, int, int],
    *,
    pseudo: bool,
) -> bytes:
    output = io.BytesIO()
    Image.new("RGBA", size, color).save(output, format="PNG")
    data = bytearray(output.getvalue())
    if pseudo:
        data[1:4] = b"png"
    return bytes(data)


def cn_base64_sha256(data: bytes) -> str:
    return plan._sha256_cn_base64(data)


def write_spec(path: Path, entries: list[dict[str, str]]) -> None:
    path.write_text(
        json.dumps({"schema_version": 1, "entries": entries}),
        encoding="utf-8",
    )


def zip_info(filename: str) -> zipfile.ZipInfo:
    info = zipfile.ZipInfo(filename, date_time=(2024, 6, 7, 8, 9, 10))
    info.compress_type = zipfile.ZIP_DEFLATED
    info.extract_version = 45
    info.external_attr = 0o640 << 16
    info.extra = LEGACY_ZIP64_EXTRA + PRESERVED_EXTRA
    return info


def extra_field_ids(extra: bytes) -> list[int]:
    output: list[int] = []
    offset = 0
    while offset + 4 <= len(extra):
        field_id, field_size = struct.unpack_from("<HH", extra, offset)
        field_end = offset + 4 + field_size
        if field_end > len(extra):
            break
        output.append(field_id)
        offset = field_end
    return output


def write_fixture(root: Path) -> dict[str, object]:
    cdn_root = root / "cdn"
    source_root = root / "source"
    output_root = root / "output"
    report_path = root / "report.json"
    spec_path = root / "mappings.json"
    entity_root = cdn_root / "entities"
    archive_root = cdn_root / "archive-medium-full"
    entity_root.mkdir(parents=True)
    archive_root.mkdir(parents=True)

    target_hash = assets.hash_cn_asset_path(TARGET_LOGICAL_PATH)
    target_asset_path = f"production/medium_upload/{target_hash[:2]}/{target_hash[2:]}"
    unrelated_asset_path = (
        "production/medium_upload/ff/ffffffffffffffffffffffffffffffffffffff"
    )
    target_original = png_bytes((5, 7), (30, 40, 50, 255), pseudo=True)
    unrelated_original = png_bytes((3, 4), (1, 2, 3, 255), pseudo=True)
    archive_path = archive_root / "pinball-1.4.0-portrait-fixture.zip"
    with zipfile.ZipFile(
        archive_path, "w", compression=zipfile.ZIP_DEFLATED
    ) as archive:
        archive.writestr(zip_info(target_asset_path), target_original)
        archive.writestr(zip_info(unrelated_asset_path), unrelated_original)
    (cdn_root / "path").write_text(
        json.dumps(
            {
                "info": {"target_asset_version": "1.4.0", "eventual_target_asset_version": "1.4.0"},
                "full": {
                    "version": "1.4.0",
                    "archive": [
                        {
                            "location": (
                                "{$cdnAddress}/archive-medium-full/"
                                f"{archive_path.name}"
                            ),
                            "size": archive_path.stat().st_size,
                            "sha256": base64.b64encode(
                                hashlib.sha256(archive_path.read_bytes()).digest()
                            ).decode("ascii"),
                        }
                    ]
                },
                "diff": [],
            },
            separators=(",", ":"),
        ),
        encoding="utf-8",
    )

    target_row = (
        f"{target_asset_path},1.4.0,{len(target_original)},"
        f"{cn_base64_sha256(target_original)},medium\r\n"
    )
    unrelated_row = (
        f"{unrelated_asset_path},1.4.0,{len(unrelated_original)},unchanged,medium\r\n"
    )
    for file_name in (
        "PathFile.csv",
        "10939-ios_medium.csv",
        "10939-android_medium.csv",
    ):
        (entity_root / file_name).write_text(
            target_row + unrelated_row, encoding="utf-8", newline=""
        )
    (cdn_root / ".starpoint-cn-cdn.json").write_text(
        "pristine marker", encoding="utf-8"
    )

    source_path = source_root.joinpath(*Path(SOURCE_LOGICAL_PATH).parts)
    source_path.parent.mkdir(parents=True)
    source_data = png_bytes((10, 14), (220, 20, 40, 255), pseudo=False)
    source_path.write_bytes(source_data)
    write_spec(
        spec_path, [{"source": SOURCE_LOGICAL_PATH, "target": TARGET_LOGICAL_PATH}]
    )
    return {
        "archive_path": archive_path,
        "cdn_root": cdn_root,
        "output_root": output_root,
        "report_path": report_path,
        "source_data": source_data,
        "source_root": source_root,
        "spec_path": spec_path,
        "target_asset_path": target_asset_path,
        "target_original": target_original,
        "target_row": target_row,
        "unrelated_asset_path": unrelated_asset_path,
        "unrelated_original": unrelated_original,
        "unrelated_row": unrelated_row,
    }


# //// /生成带实体记录和 archive 的合成 CN CDN ////


# //// 验证 full-shot 过滤和一一映射 [@x380kkm 2026-08-21] ////
class MappingValidationTests(unittest.TestCase):
    def test_hash_matches_the_cn_client_contract(self) -> None:
        self.assertEqual(
            assets.hash_cn_asset_path(
                "character/white_tiger/ui/full_shot_1440_1920_0.png"
            ),
            "1e09b768505991302311ea653236d18157094cb1",
        )
        self.assertEqual(
            plan._sha256_cn_base64(b"test"),
            "n4bQgYhMfWWaL_qgxVrQFaO-TxsrC4Is0V1sFbDwCgg",
        )

    def test_rejects_non_full_shot_asset_families(self) -> None:
        invalid_paths = (
            "character/hero/pixelart/full_shot_1440_1920_0.png",
            "character/hero/ui/full_shot_illustration_setting_0.png",
            "event/summer/ui/full_shot_1440_1920_0.png",
            "character/hero/ui/full_shot_1440_1920_0.png/extra",
        )
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for index, invalid_path in enumerate(invalid_paths):
                spec = root / f"invalid-{index}.json"
                write_spec(
                    spec, [{"source": invalid_path, "target": TARGET_LOGICAL_PATH}]
                )
                with (
                    self.subTest(path=invalid_path),
                    self.assertRaisesRegex(
                        assets.PortraitStagingError,
                        "full_shot_1440_1920",
                    ),
                ):
                    assets.load_mapping_spec(spec)

    def test_rejects_duplicate_source_or_target_mappings(self) -> None:
        alternate = "character/other_hero/ui/full_shot_1440_1920_1.png"
        cases = (
            [
                {"source": SOURCE_LOGICAL_PATH, "target": TARGET_LOGICAL_PATH},
                {"source": SOURCE_LOGICAL_PATH, "target": alternate},
            ],
            [
                {"source": SOURCE_LOGICAL_PATH, "target": TARGET_LOGICAL_PATH},
                {"source": alternate, "target": TARGET_LOGICAL_PATH},
            ],
        )
        with tempfile.TemporaryDirectory() as temporary:
            for index, entries in enumerate(cases):
                spec = Path(temporary) / f"duplicate-{index}.json"
                write_spec(spec, entries)
                with (
                    self.subTest(index=index),
                    self.assertRaises(assets.PortraitStagingError),
                ):
                    assets.load_mapping_spec(spec)


# //// /验证 full-shot 过滤和一一映射 ////


# //// 验证可逆改写和不误替换 [@x380kkm 2026-08-21] ////
class PortraitStagingTests(unittest.TestCase):
    def test_stages_only_the_mapped_full_shot_and_preserves_the_base_cdn(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = write_fixture(Path(temporary))
            base_archive_sha = hashlib.sha256(
                fixture["archive_path"].read_bytes()
            ).hexdigest()
            base_csv = (fixture["cdn_root"] / "entities" / "PathFile.csv").read_bytes()

            manifest = staging.stage_portrait_cdn(
                fixture["cdn_root"],
                fixture["source_root"],
                fixture["spec_path"],
                fixture["output_root"],
            )

            self.assertEqual(manifest["status"], "ready")
            self.assertEqual(len(manifest["entries"]), 1)
            self.assertFalse(
                (fixture["output_root"] / ".starpoint-cn-cdn.json").exists()
            )
            self.assertTrue(
                (fixture["output_root"] / assets.OUTPUT_MANIFEST_NAME).is_file()
            )
            self.assertEqual(
                hashlib.sha256(fixture["archive_path"].read_bytes()).hexdigest(),
                base_archive_sha,
            )
            self.assertEqual(
                (fixture["cdn_root"] / "entities" / "PathFile.csv").read_bytes(),
                base_csv,
            )

            staged_archive = fixture["output_root"] / fixture[
                "archive_path"
            ].relative_to(fixture["cdn_root"])
            archive_bytes = staged_archive.read_bytes()
            self.assertNotIn(b"PK\x06\x06", archive_bytes)
            self.assertNotIn(b"PK\x06\x07", archive_bytes)
            with zipfile.ZipFile(staged_archive, "r") as archive:
                replacement = archive.read(fixture["target_asset_path"])
                unrelated = archive.read(fixture["unrelated_asset_path"])
                archive_infos = archive.infolist()
                for archive_info in archive_infos:
                    self.assertLessEqual(archive_info.extract_version, 20)
                    self.assertNotIn(0x0001, extra_field_ids(archive_info.extra))
                    self.assertIn(0xCAFE, extra_field_ids(archive_info.extra))
                with staged_archive.open("rb") as archive_file:
                    for archive_info in archive_infos:
                        archive_file.seek(archive_info.header_offset)
                        local_header = archive_file.read(30)
                        self.assertEqual(local_header[:4], b"PK\x03\x04")
                        filename_size, extra_size = struct.unpack_from(
                            "<HH", local_header, 26
                        )
                        archive_file.seek(filename_size, io.SEEK_CUR)
                        local_extra = archive_file.read(extra_size)
                        self.assertNotIn(0x0001, extra_field_ids(local_extra))
                        self.assertIn(0xCAFE, extra_field_ids(local_extra))
            info = assets.inspect_png(replacement, TARGET_LOGICAL_PATH)
            self.assertEqual(info, assets.PngInfo(5, 7, "RGBA", "pseudo_png"))
            self.assertEqual(unrelated, fixture["unrelated_original"])
            path_manifest = json.loads(
                (fixture["output_root"] / "path").read_text(encoding="utf-8")
            )
            path_entry = path_manifest["full"]["archive"][0]
            update = path_manifest["diff"][-1]
            self.assertEqual(update["original_version"], "1.4.0")
            self.assertEqual(update["version"], "1.4.1")
            self.assertEqual(path_manifest["info"]["target_asset_version"], "1.4.1")
            relative_diff = "/".join(update["archive"][0]["location"].rsplit("/", 2)[-2:])
            with zipfile.ZipFile(fixture["output_root"] / relative_diff) as diff_archive:
                self.assertEqual(diff_archive.namelist(), [fixture["target_asset_path"]])
                self.assertEqual(diff_archive.read(fixture["target_asset_path"]), replacement)
            self.assertEqual(path_entry["size"], staged_archive.stat().st_size)
            self.assertEqual(
                path_entry["sha256"],
                base64.b64encode(
                    hashlib.sha256(staged_archive.read_bytes()).digest()
                ).decode("ascii"),
            )
            standard, _ = assets._standard_png(replacement)
            with Image.open(io.BytesIO(standard)) as image:
                self.assertGreater(image.getpixel((2, 3))[0], 180)

            for csv_name in (
                "PathFile.csv",
                "10939-ios_medium.csv",
                "10939-android_medium.csv",
            ):
                staged_csv = (fixture["output_root"] / "entities" / csv_name).read_text(
                    encoding="utf-8"
                )
                self.assertIn(cn_base64_sha256(replacement), staged_csv)
                self.assertIn(fixture["unrelated_row"].strip(), staged_csv)

    def test_two_runs_produce_identical_manifests_and_changed_archives(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = write_fixture(Path(temporary))
            second_output = Path(temporary) / "output-second"
            first_manifest = staging.stage_portrait_cdn(
                fixture["cdn_root"],
                fixture["source_root"],
                fixture["spec_path"],
                fixture["output_root"],
            )
            second_manifest = staging.stage_portrait_cdn(
                fixture["cdn_root"],
                fixture["source_root"],
                fixture["spec_path"],
                second_output,
            )
            self.assertEqual(first_manifest, second_manifest)
            relative_archive = fixture["archive_path"].relative_to(fixture["cdn_root"])
            self.assertEqual(
                hashlib.sha256(
                    (fixture["output_root"] / relative_archive).read_bytes()
                ).hexdigest(),
                hashlib.sha256(
                    (second_output / relative_archive).read_bytes()
                ).hexdigest(),
            )

    def test_failure_does_not_publish_a_partial_output(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = write_fixture(Path(temporary))
            base_archive_sha = hashlib.sha256(
                fixture["archive_path"].read_bytes()
            ).hexdigest()
            with (
                mock.patch.object(
                    staging, "_rewrite_zip", side_effect=OSError("synthetic failure")
                ),
                self.assertRaisesRegex(OSError, "synthetic failure"),
            ):
                staging.stage_portrait_cdn(
                    fixture["cdn_root"],
                    fixture["source_root"],
                    fixture["spec_path"],
                    fixture["output_root"],
                )
            self.assertFalse(fixture["output_root"].exists())
            leftovers = list(Path(temporary).glob(".output.portrait-staging-*"))
            self.assertEqual(leftovers, [])
            self.assertEqual(
                hashlib.sha256(fixture["archive_path"].read_bytes()).hexdigest(),
                base_archive_sha,
            )

    def test_cli_writes_a_reason_when_the_source_portrait_is_missing(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = write_fixture(Path(temporary))
            source_path = fixture["source_root"].joinpath(
                *Path(SOURCE_LOGICAL_PATH).parts
            )
            source_path.unlink()
            with contextlib.redirect_stderr(io.StringIO()):
                exit_code = staging.main(
                    [
                        "--cdn-root",
                        str(fixture["cdn_root"]),
                        "--source-root",
                        str(fixture["source_root"]),
                        "--spec",
                        str(fixture["spec_path"]),
                        "--output-root",
                        str(fixture["output_root"]),
                        "--report",
                        str(fixture["report_path"]),
                    ]
                )
            self.assertEqual(exit_code, 1)
            self.assertFalse(fixture["output_root"].exists())
            report = json.loads(fixture["report_path"].read_text(encoding="utf-8"))
            self.assertEqual(report["status"], "failed")
            self.assertEqual(report["errors"][0]["code"], "missing_source_portrait")


# //// /验证可逆改写和不误替换 ////


if __name__ == "__main__":
    unittest.main()
