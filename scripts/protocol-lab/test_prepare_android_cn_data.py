# audience: internal
# # test-prepare-android-cn-data
# 分发验证覆盖平台实体投影与共享 CDN 的硬链接边界.

import base64
import hashlib
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest
import zipfile


spec = importlib.util.spec_from_file_location("prepare_android_cn_data", Path(__file__).with_name("prepare-android-cn-data.py"))
distribution = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = distribution
spec.loader.exec_module(distribution)


# //// 根据实际文件内容构造实体记录 [@x380kkm 2026-09-21] ////
def entity_row(entry: str, version: str, payload: bytes) -> str:
    digest = base64.b64encode(hashlib.sha256(payload).digest()).decode().rstrip("=").replace("+", "_").replace("/", "-")
    return f"{entry},{version},{len(payload)},{digest},common"
# //// /根据实际文件内容构造实体记录 ////


# //// 验证分发清单对应 Android 文件并保留共享源 [@x380kkm 2026-09-21] ////
class AndroidPathFileProjectionTest(unittest.TestCase):
    def test_projects_platform_rows_before_publishing_the_distribution_manifest(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source"
            entities = source / "entities"
            entities.mkdir(parents=True)
            entry_paths = (
                "production/upload/2d/5cb9b28d18f984a51b345a4d7aab03d77bddfc",
                "production/upload/8b/035728b2151de044fb245988ec5f33ebb544ab",
                "production/upload/d6/88e4a5f7d84cc422357cc8acbdb72817bc8670",
            )
            android = {entry: f"android-{index}".encode() for index, entry in enumerate(entry_paths)}
            ios = {entry: f"ios-voice-overlay-{index}".encode() for index, entry in enumerate(entry_paths)}
            android_rows = [entity_row(entry, "1.4.43", data) for entry, data in android.items()]
            ios_rows = [entity_row(entry, "1.4.55", data) for entry, data in ios.items()]
            retained_entry = "production/upload/ff/" + "f" * 38
            platform_entry = "production/upload/ee/" + "e" * 38
            retained = entity_row(retained_entry, "1.4.0", b"shared")
            added_only_to_platform = entity_row(platform_entry, "1.4.0", b"platform-only")
            path_file_bytes = b"\xef\xbb\xbf" + ("\r\n".join(ios_rows) + "\r\n\r\n" + retained).encode()
            (entities / "PathFile.csv").write_bytes(path_file_bytes)
            (entities / "10939-ios_medium.csv").write_text("\n".join(ios_rows) + "\n", encoding="utf-8")
            (entities / "10939-android_medium.csv").write_text("\n".join([*android_rows, added_only_to_platform]) + "\n", encoding="utf-8")
            for archive_directory in (*distribution.ANDROID_ARCHIVE_DIRECTORIES, "archive-ios-diff"):
                archive_root = source / archive_directory
                archive_root.mkdir()
                with zipfile.ZipFile(archive_root / "payload.zip", "w") as archive:
                    for entry, payload in (ios if archive_directory == "archive-ios-diff" else android).items():
                        archive.writestr(entry, payload)
                    archive.writestr(retained_entry, b"shared")
                    if archive_directory != "archive-ios-diff":
                        archive.writestr(platform_entry, b"platform-only")
            original_files = {path.relative_to(source): path.read_bytes() for path in source.rglob("*") if path.is_file()}
            report = distribution.prepare_distribution(source, root / "distribution")
            staged = Path(report["data_directory"])
            projected = staged / distribution.ANDROID_PATH_FILE
            expected = b"\xef\xbb\xbf" + ("\r\n".join(android_rows) + "\r\n\r\n" + retained).encode()
            self.assertEqual(expected, projected.read_bytes())
            self.assertFalse(projected.samefile(entities / "PathFile.csv"))
            self.assertTrue((staged / distribution.ANDROID_ENTITY_LIST).samefile(entities / "10939-android_medium.csv"))
            self.assertFalse((staged / "archive-ios-diff").exists())
            self.assertFalse((staged / "entities/10939-ios_medium.csv").exists())
            self.assertEqual(original_files, {path.relative_to(source): path.read_bytes()
                                             for path in source.rglob("*") if path.is_file()})
            manifest = (staged / distribution.MANIFEST_NAME).read_bytes()
            record = next(line.split("\t") for line in manifest.decode().splitlines()
                          if line.endswith("\t" + distribution.ANDROID_PATH_FILE))
            self.assertEqual([hashlib.sha256(expected).hexdigest(), str(len(expected)), distribution.ANDROID_PATH_FILE], record)
            self.assertEqual(hashlib.sha256(manifest).hexdigest(), report["manifest_sha256"])
            self.assertEqual(1, report["copied_files"])
            self.assertEqual(report["files"] - 1, report["linked_files"])
# //// /验证分发清单对应 Android 文件并保留共享源 ////


if __name__ == "__main__":
    unittest.main()
