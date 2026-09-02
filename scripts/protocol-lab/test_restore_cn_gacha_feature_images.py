# audience: internal
# # test-restore-cn-gacha-feature-images
# 此测试验证 gacha feature 图片按同族尺寸补齐, 并通过 medium 差分和 iOS EntityLists 提供给客户端.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

from __future__ import annotations

import base64
import csv
import hashlib
import io
import json
import struct
import tempfile
import unittest
import zipfile
import zlib
from pathlib import Path
from typing import Any

from PIL import Image

from cn_gacha_banner_assets import (
    encode_entity_digest,
    hash_cn_asset_path,
    inspect_png,
    pseudo_png,
    standard_png,
)
from restore_cn_gacha_feature_images import (
    FEATURE_ARCHIVE_DIRECTORY,
    FEATURE_ARCHIVE_NAME_PREFIX,
    FEATURE_MASTER_PATH,
    GACHA_MASTER_PATH,
    feature_entry_path,
    restore_feature_images,
)


# //// 生成 orderedmap, PNG 和 CDN 清单夹具 [@x380kkm 2026-08-28] ////
def encode_csv_row(row: list[str]) -> bytes:
    output = io.StringIO(newline="")
    csv.writer(output, lineterminator="").writerow(row)
    return output.getvalue().encode("utf-8")


def encode_ordered_map(entries: dict[str, Any]) -> bytes:
    keys: list[bytes] = []
    values: list[bytes] = []
    offsets: list[tuple[int, int]] = []
    key_length = 0
    data_length = 0
    for key, value in entries.items():
        key_bytes = key.encode("utf-8")
        value_bytes = (
            encode_ordered_map(value)
            if isinstance(value, dict)
            else zlib.compress(encode_csv_row(value))
        )
        keys.append(key_bytes)
        values.append(value_bytes)
        key_length += len(key_bytes)
        data_length += len(value_bytes)
        offsets.append((key_length, data_length))
    index = bytearray(struct.pack("<I", len(offsets)))
    for key_end, data_end in offsets:
        index.extend(struct.pack("<II", key_end, data_end))
    compressed_index = zlib.compress(bytes(index) + b"".join(keys))
    return struct.pack("<I", len(compressed_index)) + compressed_index + b"".join(
        values
    )


def png_bytes(size: tuple[int, int], color: tuple[int, int, int]) -> bytes:
    output = io.BytesIO()
    Image.new("RGB", size, color).save(output, format="PNG")
    return pseudo_png(output.getvalue())


def deterministic_archive(entries: dict[str, bytes]) -> bytes:
    output = io.BytesIO()
    with zipfile.ZipFile(
        output, "w", compression=zipfile.ZIP_DEFLATED, allowZip64=False
    ) as archive:
        for name, data in entries.items():
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, data)
    return output.getvalue()


def archive_digest(data: bytes) -> str:
    return base64.b64encode(hashlib.sha256(data).digest()).decode("ascii")


def entity_row(entry_path: str, data: bytes, asset_kind: str) -> str:
    return (
        f"{entry_path},1.4.54,{len(data)},{encode_entity_digest(data)},"
        f"{asset_kind}"
    )


def write_path_manifest(cdn_root: Path, archive_name: str, archive_data: bytes) -> None:
    (cdn_root / "path").write_text(
        json.dumps(
            {
                "info": {
                    "client_asset_version": "1.4.54",
                    "target_asset_version": "1.4.54",
                    "eventual_target_asset_version": "1.4.54",
                    "is_initial": True,
                    "latest_maj_first_version": "1.4.0",
                },
                "full": {"version": "1.4.0", "archive": []},
                "diff": [
                    {
                        "version": "1.4.54",
                        "original_version": "1.4.53",
                        "archive": [
                            {
                                "location": (
                                    "https://assets.example/patch/cn/"
                                    f"archive-common-diff/{archive_name}"
                                ),
                                "size": len(archive_data),
                                "sha256": archive_digest(archive_data),
                            }
                        ],
                    }
                ],
            },
            separators=(",", ":"),
        ),
        encoding="utf-8",
    )


def write_target_cdn(root: Path) -> tuple[Path, dict[str, bytes]]:
    cdn_root = root / "cdn"
    entity_root = cdn_root / "entities"
    common_root = cdn_root / "archive-common-diff"
    medium_root = cdn_root / "archive-medium-full"
    entity_root.mkdir(parents=True)
    common_root.mkdir()
    medium_root.mkdir()

    missing_landscape = "dynamic/gacha_banner/thunder_element_pickup_01.png"
    missing_portrait = "dynamic/gacha_banner/equipment_pickup_1.png"
    existing_landscape = "dynamic/gacha_banner/thunder_element_pickup_03.png"
    existing_portrait = "dynamic/gacha_banner/equipment_pickup_8.png"
    landscape_banner = "dynamic/gacha_list_banner/thunder_element_pickup_01.png"
    portrait_banner = "dynamic/gacha_list_banner/equipment_pickup_1.png"
    feature_master = encode_ordered_map(
        {
            "16": {"1": ["1", missing_landscape.removesuffix(".png"), "", "", "", "", "(None)", "", ""]},
            "32": {"1": ["1", existing_landscape.removesuffix(".png"), "", "", "", "", "(None)", "", ""]},
            "5000": {"1": ["1", missing_portrait.removesuffix(".png"), "", "", "", "", "(None)", "", ""]},
            "5008": {"1": ["1", existing_portrait.removesuffix(".png"), "", "", "", "", "(None)", "", ""]},
        }
    )
    gacha_master = encode_ordered_map(
        {
            "16": ["pool16", "Pool 16", "0", landscape_banner.removesuffix(".png")],
            "32": ["pool32", "Pool 32", "0", "dynamic/gacha_list_banner/thunder_element_pickup_03"],
            "5000": ["pool5000", "Pool 5000", "0", portrait_banner.removesuffix(".png")],
            "5008": ["pool5008", "Pool 5008", "0", "dynamic/gacha_list_banner/equipment_pickup_8"],
        }
    )
    masters = {
        feature_entry_path(FEATURE_MASTER_PATH).replace(
            "production/medium_upload/", "production/upload/"
        ): feature_master,
        feature_entry_path(GACHA_MASTER_PATH).replace(
            "production/medium_upload/", "production/upload/"
        ): gacha_master,
    }
    common_archive_name = "masters.zip"
    common_archive = deterministic_archive(masters)
    (common_root / common_archive_name).write_bytes(common_archive)

    existing_assets = {
        feature_entry_path(existing_landscape): png_bytes((1440, 624), (30, 80, 130)),
        feature_entry_path(existing_portrait): png_bytes((1440, 1789), (120, 60, 20)),
    }
    (medium_root / "features.zip").write_bytes(
        deterministic_archive(existing_assets)
    )

    rows = [
        entity_row(entry_path, data, "common")
        for entry_path, data in masters.items()
    ] + [
        entity_row(entry_path, data, "medium")
        for entry_path, data in existing_assets.items()
    ]
    manifest = "\n".join(rows) + "\n"
    for name in ("PathFile.csv", "10939-ios_medium.csv"):
        (entity_root / name).write_text(manifest, encoding="utf-8")
    write_path_manifest(cdn_root, common_archive_name, common_archive)

    for logical_path, color in (
        (landscape_banner, (70, 110, 180)),
        (portrait_banner, (180, 120, 50)),
    ):
        asset_hash = hash_cn_asset_path(logical_path)
        output_path = cdn_root / "production" / "bundle" / asset_hash[:2] / asset_hash[2:]
        output_path.parent.mkdir(parents=True, exist_ok=True)
        output_path.write_bytes(png_bytes((510, 180), color))
    return cdn_root, {
        "missing_landscape": missing_landscape,
        "missing_portrait": missing_portrait,
    }
# //// /生成 orderedmap, PNG 和 CDN 清单夹具 ////


# //// 验证同族尺寸, medium 归档和幂等行为 [@x380kkm 2026-08-28] ////
class RestoreFeatureImageTests(unittest.TestCase):
    def test_restores_family_sizes_through_medium_diff(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            cdn_root, paths = write_target_cdn(Path(temporary_directory))
            first = restore_feature_images(cdn_root)
            manifest_after_first = (cdn_root / "path").read_bytes()
            archive_path = cdn_root.joinpath(
                *first["archive"]["relative_path"].split("/")
            )
            archive_after_first = archive_path.read_bytes()

            self.assertEqual(2, first["restored_count"])
            self.assertEqual(0, first["exact_count"])
            self.assertEqual(2, first["generated_count"])
            self.assertEqual("1.4.53", first["original_version"])
            self.assertEqual("1.4.54", first["target_version"])
            self.assertTrue(archive_path.parent.name == FEATURE_ARCHIVE_DIRECTORY)
            self.assertTrue(archive_path.name.startswith(FEATURE_ARCHIVE_NAME_PREFIX))
            self.assertFalse((cdn_root / "production" / "medium_bundle").exists())

            by_path = {asset["logical_path"]: asset for asset in first["assets"]}
            self.assertEqual([1440, 624], by_path[paths["missing_landscape"]]["dimensions"])
            self.assertEqual([1440, 1789], by_path[paths["missing_portrait"]]["dimensions"])
            self.assertEqual(
                "dynamic/gacha_banner/thunder_element_pickup_03.png",
                by_path[paths["missing_landscape"]]["dimension_evidence"],
            )
            self.assertEqual(
                "dynamic/gacha_banner/equipment_pickup_8.png",
                by_path[paths["missing_portrait"]]["dimension_evidence"],
            )

            with zipfile.ZipFile(archive_path) as archive:
                self.assertEqual(2, len(archive.namelist()))
                for logical_path, expected_size in (
                    (paths["missing_landscape"], (1440, 624)),
                    (paths["missing_portrait"], (1440, 1789)),
                ):
                    self.assertEqual(
                        expected_size,
                        inspect_png(archive.read(feature_entry_path(logical_path))),
                    )

            for manifest_name in ("PathFile.csv", "10939-ios_medium.csv"):
                rows = {
                    row[0]: row
                    for row in csv.reader(
                        io.StringIO(
                            (cdn_root / "entities" / manifest_name).read_text(
                                encoding="utf-8"
                            )
                        )
                    )
                }
                for logical_path in paths.values():
                    entry_path = feature_entry_path(logical_path)
                    self.assertEqual("1.4.54", rows[entry_path][1])
                    self.assertEqual("medium", rows[entry_path][4])

            path_manifest = json.loads((cdn_root / "path").read_bytes())
            current_groups = [
                group
                for group in path_manifest["diff"]
                if group["version"] == "1.4.54"
                and group["original_version"] == "1.4.53"
            ]
            self.assertEqual(1, len(current_groups))
            self.assertEqual(2, len(current_groups[0]["archive"]))

            second = restore_feature_images(cdn_root)
            self.assertEqual(0, second["restored_count"])
            self.assertTrue(second["reused"])
            self.assertEqual(manifest_after_first, (cdn_root / "path").read_bytes())
            self.assertEqual(archive_after_first, archive_path.read_bytes())

    def test_prefers_exact_source_bytes_and_dimensions(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            cdn_root, paths = write_target_cdn(root)
            source_root = root / "source"
            entity_root = source_root / "entities"
            archive_root = source_root / "archive-medium-full"
            entity_root.mkdir(parents=True)
            archive_root.mkdir()

            logical_path = paths["missing_landscape"]
            exact = png_bytes((1440, 1790), (10, 150, 90))
            entry_path = feature_entry_path(logical_path)
            (archive_root / "exact.zip").write_bytes(
                deterministic_archive({entry_path: exact})
            )
            row = entity_row(entry_path, exact, "medium") + "\n"
            (entity_root / "10939-ios_medium.csv").write_text(row, encoding="utf-8")

            report = restore_feature_images(cdn_root, source_roots=[source_root])
            asset = next(
                item
                for item in report["assets"]
                if item["logical_path"] == logical_path
            )
            self.assertEqual("exact", asset["source_kind"])
            self.assertEqual([1440, 1790], asset["dimensions"])
            archive_path = cdn_root.joinpath(
                *report["archive"]["relative_path"].split("/")
            )
            with zipfile.ZipFile(archive_path) as archive:
                self.assertEqual(pseudo_png(exact), archive.read(entry_path))

    def test_migrates_legacy_feature_archive_without_bumping_target(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            cdn_root, paths = write_target_cdn(Path(temporary_directory))
            first = restore_feature_images(cdn_root)
            current_relative = first["archive"]["relative_path"]
            legacy_relative = current_relative.replace(
                FEATURE_ARCHIVE_NAME_PREFIX,
                "starpoint-gacha-feature-images-",
                1,
            )
            current_path = cdn_root.joinpath(*current_relative.split("/"))
            legacy_path = cdn_root.joinpath(*legacy_relative.split("/"))
            current_path.replace(legacy_path)

            path_manifest_path = cdn_root / "path"
            path_manifest = json.loads(path_manifest_path.read_bytes())
            for group in path_manifest["diff"]:
                for archive in group.get("archive", []):
                    if archive.get("location", "").endswith(current_relative):
                        archive["location"] = archive["location"].replace(
                            current_relative, legacy_relative
                        )
            path_manifest_path.write_text(
                json.dumps(path_manifest, separators=(",", ":")), encoding="utf-8"
            )

            migrated = restore_feature_images(cdn_root)
            migrated_path = cdn_root.joinpath(
                *migrated["archive"]["relative_path"].split("/")
            )
            self.assertEqual(2, migrated["migrated_count"])
            self.assertEqual("1.4.54", migrated["target_version"])
            self.assertTrue(migrated_path.is_file())
            self.assertFalse(legacy_path.exists())
            self.assertTrue(migrated_path.name.startswith(FEATURE_ARCHIVE_NAME_PREFIX))

            repeated = restore_feature_images(cdn_root)
            self.assertEqual(0, repeated["restored_count"])
            self.assertTrue(repeated["reused"])

    def test_package_runs_feature_restore_after_banner_archive(self) -> None:
        package_script = Path(__file__).with_name("package-ios-cn-personal-service.ps1")
        source = package_script.read_text(encoding="utf-8-sig")
        banner_call = source.index("$archiveOutput = @(& uv run --script $archiveInstaller")
        restore_call = source.index("$featureOutput = @(& uv run --script $featureRestorer")
        self.assertLess(banner_call, restore_call)
        self.assertIn("restore_cn_gacha_feature_images.py", source)
        self.assertIn("feature_reference_count", source)
# //// /验证同族尺寸, medium 归档和幂等行为 ////


if __name__ == "__main__":
    unittest.main()
