# audience: internal
# # test-cn-gacha-banner-client-archive
# 此测试验证 gacha master, banner, EntityLists 和 path 使用同一 iOS 差分归档.

from __future__ import annotations

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

from PIL import Image

from cn_gacha_banner_assets import encode_entity_digest, hash_cn_asset_path, pseudo_png
from cn_gacha_banner_client_archive import (
    ARCHIVE_NAME_PREFIX,
    GACHA_MASTER_ENTRY,
    install_banner_archive,
    standard_archive_digest,
)


POOL_IDS = (1518, 1520, 1524, 1525, 1532)


# //// 生成测试 orderedmap 和 banner 字节 [@x380kkm 2026-08-28] ////
def encode_csv_row(row: list[str]) -> bytes:
    output = io.StringIO(newline="")
    writer = csv.writer(output, lineterminator="")
    writer.writerow(row)
    return output.getvalue().encode("utf-8")


def encode_ordered_map(rows: dict[str, list[str]]) -> bytes:
    keys: list[bytes] = []
    values: list[bytes] = []
    offsets: list[tuple[int, int]] = []
    key_length = 0
    data_length = 0
    for key, row in rows.items():
        key_bytes = key.encode("utf-8")
        value = zlib.compress(encode_csv_row(row))
        keys.append(key_bytes)
        values.append(value)
        key_length += len(key_bytes)
        data_length += len(value)
        offsets.append((key_length, data_length))
    index = bytearray(struct.pack("<I", len(offsets)))
    for key_end, data_end in offsets:
        index.extend(struct.pack("<II", key_end, data_end))
    compressed_index = zlib.compress(bytes(index) + b"".join(keys))
    return struct.pack("<I", len(compressed_index)) + compressed_index + b"".join(values)


def banner_bytes(color: tuple[int, int, int]) -> bytes:
    output = io.BytesIO()
    Image.new("RGB", (510, 180), color).save(output, format="PNG")
    return pseudo_png(output.getvalue())


def deterministic_archive(entries: dict[str, bytes]) -> bytes:
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for name, data in entries.items():
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, data)
    return output.getvalue()
# //// /生成测试 orderedmap 和 banner 字节 ////


# //// 验证 iOS banner 差分完整链和幂等执行 [@x380kkm 2026-08-28] ////
class BannerClientArchiveTests(unittest.TestCase):
    def test_installs_complete_idempotent_client_chain(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            cdn_root = root / "cdn"
            report_root = root / "report"
            entity_root = cdn_root / "entities"
            archive_root = cdn_root / "archive-common-diff"
            entity_root.mkdir(parents=True)
            archive_root.mkdir()

            paths = {
                pool_id: f"dynamic/gacha_list_banner/starpoint_generated/{pool_id}"
                for pool_id in POOL_IDS
            }
            rows = {
                str(pool_id): [
                    f"pool_{pool_id}",
                    f"Pool {pool_id}",
                    "95",
                    logical_path,
                ]
                for pool_id, logical_path in paths.items()
            }
            master_data = encode_ordered_map(rows)
            source_archive_name = "source-gacha.zip"
            source_archive = deterministic_archive({GACHA_MASTER_ENTRY: master_data})
            (archive_root / source_archive_name).write_bytes(source_archive)
            master_row = (
                f"{GACHA_MASTER_ENTRY},1.4.58,{len(master_data)},"
                f"{encode_entity_digest(master_data)},common\n"
            )
            for name in ("PathFile.csv", "10939-ios_medium.csv"):
                (entity_root / name).write_text(master_row, encoding="utf-8")
            (cdn_root / "path").write_text(
                json.dumps(
                    {
                        "info": {
                            "client_asset_version": "1.4.54",
                            "target_asset_version": "1.4.58",
                            "eventual_target_asset_version": "1.4.58",
                            "is_initial": True,
                            "latest_maj_first_version": "1.4.0",
                        },
                        "full": {"version": "1.4.0", "archive": []},
                        "diff": [
                            {
                                "version": "1.4.58",
                                "original_version": "1.4.54",
                                "archive": [
                                    {
                                        "location": (
                                            "https://assets.example/patch/cn/"
                                            f"archive-common-diff/{source_archive_name}"
                                        ),
                                        "size": len(source_archive),
                                        "sha256": hashlib.sha256(source_archive).hexdigest(),
                                    }
                                ],
                            }
                        ],
                        "asset_version_hash": "fixture",
                    },
                    separators=(",", ":"),
                ),
                encoding="utf-8",
            )

            groups = []
            override_assets = []
            for index, pool_id in enumerate(POOL_IDS):
                logical_path = paths[pool_id]
                asset_hash = hash_cn_asset_path(f"{logical_path}.png")
                game_path = (
                    report_root
                    / "production"
                    / "bundle"
                    / asset_hash[:2]
                    / asset_hash[2:]
                )
                game_path.parent.mkdir(parents=True, exist_ok=True)
                game_path.write_bytes(banner_bytes((20 + index, 40 + index, 80 + index)))
                groups.append(
                    {
                        "logical_path": logical_path,
                        "game_path": str(game_path),
                    }
                )
                override_assets.append(
                    {
                        "pool_id": pool_id,
                        "logical_path": logical_path,
                        "asset_hash": asset_hash,
                    }
                )
            report_path = report_root / "gacha-banner-report.json"
            report_path.parent.mkdir(parents=True, exist_ok=True)
            report_path.write_text(
                json.dumps(
                    {
                        "groups": groups,
                        "banner_path_overrides": {
                            str(pool_id): logical_path
                            for pool_id, logical_path in paths.items()
                        },
                        "override_asset_count": len(override_assets),
                        "override_assets": override_assets,
                    },
                    separators=(",", ":"),
                ),
                encoding="utf-8",
            )

            first = install_banner_archive(cdn_root, report_path)
            path_after_first = (cdn_root / "path").read_bytes()
            archive_path = cdn_root.joinpath(*first["archive"]["relative_path"].split("/"))
            archive_after_first = archive_path.read_bytes()
            second = install_banner_archive(cdn_root, report_path)

            self.assertEqual("1.4.58", first["original_version"])
            self.assertEqual("1.4.59", first["target_version"])
            self.assertFalse(first["reused"])
            self.assertTrue(second["reused"])
            self.assertEqual(path_after_first, (cdn_root / "path").read_bytes())
            self.assertEqual(archive_after_first, archive_path.read_bytes())
            self.assertNotIn(b"PK\x06\x06", archive_after_first)
            self.assertTrue(archive_path.name.startswith(ARCHIVE_NAME_PREFIX))

            with zipfile.ZipFile(archive_path) as archive:
                names = archive.namelist()
                self.assertEqual(1 + len(POOL_IDS), len(names))
                self.assertEqual(GACHA_MASTER_ENTRY, names[0])
                for override in override_assets:
                    asset_hash = override["asset_hash"]
                    self.assertIn(
                        f"production/upload/{asset_hash[:2]}/{asset_hash[2:]}",
                        names,
                    )

            manifest = json.loads((cdn_root / "path").read_bytes())
            self.assertEqual("1.4.59", manifest["info"]["target_asset_version"])
            matching_groups = [
                group
                for group in manifest["diff"]
                if group["version"] == "1.4.59"
                and group["original_version"] == "1.4.58"
            ]
            self.assertEqual(1, len(matching_groups))
            self.assertEqual(first["archive"]["location"], matching_groups[0]["archive"][0]["location"])

            for manifest_name in ("PathFile.csv", "10939-ios_medium.csv"):
                rows_by_entry = {
                    row[0]: row
                    for row in csv.reader(
                        io.StringIO((entity_root / manifest_name).read_text(encoding="utf-8"))
                    )
                }
                self.assertEqual("1.4.59", rows_by_entry[GACHA_MASTER_ENTRY][1])
                for override in override_assets:
                    asset_hash = override["asset_hash"]
                    entry_path = f"production/upload/{asset_hash[:2]}/{asset_hash[2:]}"
                    self.assertEqual("1.4.59", rows_by_entry[entry_path][1])
                    self.assertEqual("common", rows_by_entry[entry_path][4])

            self.assertEqual(len(POOL_IDS), len(first["override_assets"]))
            self.assertTrue(
                all(
                    item["logical_path"] == item["master_banner_path"]
                    for item in first["override_assets"]
                )
            )

            manifest["info"]["target_asset_version"] = "1.4.60"
            manifest["info"]["eventual_target_asset_version"] = "1.4.60"
            manifest["diff"].append(
                {
                    "version": "1.4.60",
                    "original_version": "1.4.59",
                    "archive": [],
                }
            )
            (cdn_root / "path").write_text(
                json.dumps(manifest, separators=(",", ":")), encoding="utf-8"
            )
            path_with_later_diff = (cdn_root / "path").read_bytes()
            entities_with_later_diff = (entity_root / "PathFile.csv").read_bytes()
            after_later_diff = install_banner_archive(cdn_root, report_path)
            self.assertTrue(after_later_diff["reused"])
            self.assertEqual("1.4.59", after_later_diff["target_version"])
            self.assertEqual(path_with_later_diff, (cdn_root / "path").read_bytes())
            self.assertEqual(
                entities_with_later_diff,
                (entity_root / "PathFile.csv").read_bytes(),
            )

            manifest["diff"].append(
                {
                    "version": "1.4.61",
                    "original_version": "1.4.60",
                    "archive": [
                        {
                            "location": "https://assets.example/patch/cn/archive-ios-diff/starpoint-ios-gacha-banners-1.4.60-1.4.61.zip",
                            "size": len(archive_after_first),
                            "sha256": standard_archive_digest(archive_after_first),
                        }
                    ],
                }
            )
            manifest["info"]["target_asset_version"] = "1.4.61"
            manifest["info"]["eventual_target_asset_version"] = "1.4.61"
            (cdn_root / "archive-ios-diff" / "starpoint-ios-gacha-banners-1.4.60-1.4.61.zip").write_bytes(
                archive_after_first
            )
            (cdn_root / "path").write_text(
                json.dumps(manifest, separators=(",", ":")), encoding="utf-8"
            )
            latest_match = install_banner_archive(cdn_root, report_path)
            self.assertTrue(latest_match["reused"])
            self.assertEqual("1.4.61", latest_match["target_version"])
# //// /验证 iOS banner 差分完整链和幂等执行 ////


if __name__ == "__main__":
    unittest.main()
