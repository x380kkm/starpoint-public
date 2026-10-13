# audience: internal
# # test-client-package-archive
# 此测试用实际独立资源打包入口验证审计读取, 截断与清单不匹配的处理.

import hashlib
import importlib.util
import io
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
import zipfile

from client_package_archive import ClientPackageCdn


# //// 构建与读取同一套 APK 和独立资源文档 [@x380kkm 2026-09-26] ////
class ExternalResourceArchiveTest(unittest.TestCase):
    def test_reads_packaged_resources_and_rejects_mismatched_documents(self):
        source = Path(__file__).with_name("package-android-resource-document.py")
        spec = importlib.util.spec_from_file_location("resource_document", source)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        with TemporaryDirectory() as directory:
            root = Path(directory)
            data = root / "cdn"
            data.mkdir()
            archive_bytes = io.BytesIO()
            with zipfile.ZipFile(archive_bytes, "w") as archive:
                archive.writestr("production/item", b"resource content")
            files = {"path": b'{"version":"1.4.63"}', "assets.zip": archive_bytes.getvalue()}
            manifest = bytearray()
            for name, content in files.items():
                (data / name).write_bytes(content)
                manifest.extend(f"{hashlib.sha256(content).hexdigest()}\t{len(content)}\t{name}\n".encode())
            (data / "manifest.sha256").write_bytes(manifest)
            apk = root / "sp.apk"
            document = apk.with_suffix(".starpoint-resources")
            with zipfile.ZipFile(apk, "w") as package:
                package.writestr("assets/starpoint-personal-service-cdn/manifest.sha256", manifest)
                package.writestr("assets/starpoint-personal-service-cdn/resource-delivery.txt", "external")
            module.package_resources(apk, data, document)
            with ClientPackageCdn(apk) as cdn:
                self.assertEqual(cdn.read_json("path"), {"version": "1.4.63"})
                with cdn.open_archive("assets.zip") as archive:
                    self.assertEqual(archive.read("production/item"), b"resource content")
            original = document.read_bytes()
            wrong_digest = bytearray(original)
            wrong_digest[63] ^= 1
            for content, message in ((original[:-1], "边界无效"), (wrong_digest, "清单不匹配")):
                with self.subTest(message=message):
                    document.write_bytes(content)
                    with self.assertRaisesRegex(ValueError, message):
                        with ClientPackageCdn(apk):
                            self.fail("损坏资源文档被接受")
# //// /构建与读取同一套 APK 和独立资源文档 ////


if __name__ == "__main__":
    unittest.main()
