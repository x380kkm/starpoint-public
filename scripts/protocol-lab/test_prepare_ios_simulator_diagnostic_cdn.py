# audience: internal
# # test-prepare-ios-simulator-diagnostic-cdn
# 该测试验证微型 CDN 的 get_path 清单与 ZIP 内容一致.

from __future__ import annotations

import base64
import csv
import hashlib
import importlib.util
import io
import json
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path


SCRIPT_PATH = Path(__file__).with_name("prepare-ios-simulator-diagnostic-cdn.py")
SPEC = importlib.util.spec_from_file_location("ios_simulator_cdn", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class DiagnosticCdnTests(unittest.TestCase):
    # //// 验证清单, 摘要和 ZIP 使用同一内容 [@x380kkm 2026-08-22] ////
    def test_archive_contract_is_self_consistent(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            output_path = Path(temporary_directory) / "cdn.tar.gz"
            source_root = Path(temporary_directory) / "source"
            startup_static_bodies = {
                relative_path: ("fixture:%s\n" % relative_path).encode("utf-8")
                for relative_path in MODULE.STARTUP_STATIC_PATHS
            }
            for relative_path, body in startup_static_bodies.items():
                source_path = source_root / relative_path
                source_path.parent.mkdir(parents=True, exist_ok=True)
                source_path.write_bytes(body)
            MODULE.write_diagnostic_cdn(output_path, source_root)

            with tarfile.open(output_path, "r:gz") as archive:
                names = set(archive.getnames())
                self.assertIn("activity-catalog.json", names)
                self.assertIn(MODULE.TITLE_ENTITY_LIST_PATH, names)
                self.assertIn(
                    "EntityLists/%s" % MODULE.IOS_TITLE_ENTITY_LIST_NAME,
                    names,
                )
                self.assertTrue(set(MODULE.STARTUP_STATIC_PATHS).issubset(names))
                self.assertIn("path", names)
                for relative_path, body in startup_static_bodies.items():
                    self.assertEqual(archive.extractfile(relative_path).read(), body)
                manifest = json.loads(archive.extractfile("path").read())

                for entity_list_path in (
                    MODULE.TITLE_ENTITY_LIST_PATH,
                    "EntityLists/%s" % MODULE.IOS_TITLE_ENTITY_LIST_NAME,
                    "EntityLists/%s" % MODULE.ANDROID_TITLE_ENTITY_LIST_NAME,
                ):
                    entity_list = archive.extractfile(entity_list_path).read().decode("utf-8")
                    self.assertEqual(list(csv.reader(io.StringIO(entity_list))), [])

                full_archives = manifest["full"]["archive"]
                diff_archives = [
                    metadata
                    for group in manifest["diff"]
                    for metadata in group["archive"]
                ]
                self.assertGreaterEqual(len(full_archives), 2)
                self.assertGreaterEqual(len(diff_archives), 1)

                for metadata in full_archives + diff_archives:
                    archive_path = "/".join(metadata["location"].rsplit("/", 2)[-2:])
                    archive_body = archive.extractfile(archive_path).read()
                    self.assertEqual(metadata["size"], len(archive_body))
                    self.assertEqual(
                        metadata["sha256"],
                        base64.b64encode(hashlib.sha256(archive_body).digest()).decode(
                            "ascii"
                        ),
                    )
                    self.assertTrue(archive_body.startswith(b"PK\x03\x04"))
                    with zipfile.ZipFile(io.BytesIO(archive_body)) as zip_archive:
                        fixture_names = zip_archive.namelist()
                        self.assertEqual(len(fixture_names), 1)
                        self.assertTrue(zip_archive.read(fixture_names[0]))

    # //// /验证清单, 摘要和 ZIP 使用同一内容 ////

    # //// 缺少来源时生成合法的最小启动配置 [@x380kkm 2026-08-22] ////
    def test_missing_source_includes_minimum_wf_config(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            output_path = Path(temporary_directory) / "cdn.tar.gz"

            MODULE.write_diagnostic_cdn(output_path, None)

            with tarfile.open(output_path, "r:gz") as archive:
                names = set(archive.getnames())
                self.assertIn(MODULE.WF_CONFIG_PATH, names)
                optional_paths = set(MODULE.STARTUP_STATIC_PATHS) - {
                    MODULE.WF_CONFIG_PATH
                }
                self.assertTrue(optional_paths.isdisjoint(names))
                config = json.loads(archive.extractfile(MODULE.WF_CONFIG_PATH).read())
                self.assertRegex(config["token"], r"^[0-9a-f]{32}$")
                self.assertRegex(config["config"], r"^[0-9a-f]+$")

    # //// /缺少来源时生成合法的最小启动配置 ////

    # //// 保留诊断 CDN 的来源目录语义 [@x380kkm 2026-08-25] ////
    def test_source_directory_selects_matching_entity_lists(self) -> None:
        for source_directory, expected_paths, unexpected_paths in (
            (
                "EntityLists",
                {
                    MODULE.TITLE_ENTITY_LIST_PATH,
                    "EntityLists/%s" % MODULE.IOS_TITLE_ENTITY_LIST_NAME,
                    "EntityLists/%s" % MODULE.ANDROID_TITLE_ENTITY_LIST_NAME,
                },
                {
                    MODULE.IOS_TITLE_ENTITY_LIST_PATH,
                    MODULE.ANDROID_TITLE_ENTITY_LIST_PATH,
                    MODULE.GAME_ENTITY_LIST_PATH,
                },
            ),
            (
                "entities",
                {
                    MODULE.GAME_ENTITY_LIST_PATH,
                    MODULE.IOS_TITLE_ENTITY_LIST_PATH,
                    MODULE.ANDROID_TITLE_ENTITY_LIST_PATH,
                },
                {MODULE.TITLE_ENTITY_LIST_PATH},
            ),
        ):
            with self.subTest(source_directory=source_directory):
                with tempfile.TemporaryDirectory() as temporary_directory:
                    output_path = Path(temporary_directory) / "cdn.tar.gz"
                    source_root = Path(temporary_directory) / "source"
                    (source_root / source_directory).mkdir(parents=True)
                    MODULE.write_diagnostic_cdn(output_path, source_root)

                    with tarfile.open(output_path, "r:gz") as archive:
                        names = set(archive.getnames())
                        self.assertTrue(expected_paths.issubset(names))
                        self.assertTrue(unexpected_paths.isdisjoint(names))

    # //// /保留诊断 CDN 的来源目录语义 ////

    # //// 继承来源 CDN 的 iOS 卡池 banner 差分 [@x380kkm 2026-08-28] ////
    def test_source_banner_diff_is_reachable_from_diagnostic_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            output_path = root / "cdn.tar.gz"
            source_root = root / "source"
            entity_root = source_root / "entities"
            archive_root = source_root / "archive-ios-diff"
            entity_root.mkdir(parents=True)
            archive_root.mkdir(parents=True)

            archive_name = "starpoint-ios-gacha-banners-1.4.58-1.4.59.zip"
            entry_bodies = {
                "production/upload/15/master": b"gacha-master",
                "production/upload/aa/banner": b"banner",
            }
            archive_output = io.BytesIO()
            with zipfile.ZipFile(archive_output, "w") as source_archive:
                for name, body in entry_bodies.items():
                    source_archive.writestr(name, body)
            archive_body = archive_output.getvalue()
            (archive_root / archive_name).write_bytes(archive_body)

            entity_body = "".join(
                f"{name},1.4.59,{len(body)},fixture,common\n"
                for name, body in entry_bodies.items()
            )
            (entity_root / "PathFile.csv").write_text(entity_body, encoding="utf-8")
            (entity_root / MODULE.IOS_TITLE_ENTITY_LIST_NAME).write_text(
                entity_body, encoding="utf-8"
            )
            (source_root / "path").write_text(
                json.dumps(
                    {
                        "info": {
                            "client_asset_version": "1.4.58",
                            "target_asset_version": "1.4.59",
                            "eventual_target_asset_version": "1.4.59",
                            "is_initial": True,
                            "latest_maj_first_version": "1.4.0",
                        },
                        "full": {"version": "1.4.0", "archive": []},
                        "diff": [
                            {
                                "version": "1.4.59",
                                "original_version": "1.4.58",
                                "archive": [
                                    {
                                        "location": (
                                            "https://assets.example/archive-ios-diff/"
                                            + archive_name
                                        ),
                                        "size": len(archive_body),
                                        "sha256": base64.b64encode(
                                            hashlib.sha256(archive_body).digest()
                                        ).decode("ascii"),
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

            MODULE.write_diagnostic_cdn(output_path, source_root)

            with tarfile.open(output_path, "r:gz") as archive:
                names = set(archive.getnames())
                archive_path = f"archive-ios-diff/{archive_name}"
                self.assertIn(archive_path, names)
                self.assertEqual(archive_body, archive.extractfile(archive_path).read())
                manifest = json.loads(archive.extractfile("path").read())
                self.assertEqual("1.4.59", manifest["info"]["target_asset_version"])
                banner_groups = [
                    group for group in manifest["diff"] if group["version"] == "1.4.59"
                ]
                self.assertEqual(1, len(banner_groups))
                base_groups = [
                    group
                    for group in manifest["diff"]
                    if group["version"] == "1.4.54"
                ]
                self.assertEqual(1, len(base_groups))
                self.assertEqual("1.4.0", base_groups[0]["original_version"])
                self.assertTrue(
                    banner_groups[0]["archive"][0]["location"].endswith(archive_path)
                )
                for manifest_name in ("PathFile.csv", MODULE.IOS_TITLE_ENTITY_LIST_NAME):
                    body = archive.extractfile(f"entities/{manifest_name}").read().decode(
                        "utf-8"
                    )
                    self.assertEqual(
                        set(entry_bodies),
                        {row[0] for row in csv.reader(io.StringIO(body))},
                    )

    # //// /继承来源 CDN 的 iOS 卡池 banner 差分 ////


if __name__ == "__main__":
    unittest.main()
