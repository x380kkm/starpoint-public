# audience: internal
# # test-prepared-resource-sources
# 来源验证覆盖差分复用, 漫画副本与缩略图修复.

import importlib.util
from io import StringIO
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile

from PIL import Image

from restore_cn_startup_images import image_payloads, install_images, merge_prepared_payloads


spec = importlib.util.spec_from_file_location("prepare_cn_comics", Path(__file__).with_name("prepare-cn-comics.py"))
comics = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = comics
spec.loader.exec_module(comics)


# //// 验证混合图片来源随差分复用保留 [@x380kkm 2026-09-21] ////
class StartupImageProvenanceTest(unittest.TestCase):
    def test_preserves_asset_provenance_when_reusing_a_mixed_archive(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cdn = root / "cdn"
            (cdn / "entities").mkdir(parents=True)
            (cdn / "archive-common-diff").mkdir()
            for name in ("PathFile.csv", "10939-ios_medium.csv", "10939-android_medium.csv"):
                (cdn / "entities" / name).write_text("", encoding="utf-8")
            manifest = {"info": {"target_asset_version": "1.4.0", "eventual_target_asset_version": "1.4.0"},
                        "full": {"version": "1.4.0", "archive": [{"location": "https://example.test/archive-common-full/base.zip"}]},
                        "diff": []}
            (cdn / "path").write_text(json.dumps(manifest), encoding="utf-8")
            assets = []
            for number, color in enumerate(("red", "blue")):
                preview = root / f"image-{number}.png"
                Image.new("RGB", (64, 64), color).save(preview)
                assets.append({"logical_path": f"dynamic/test/image-{number}.png", "preview": str(preview),
                               "dimensions": [64, 64], "owners": [f"image-{number}"]})
            assets[0]["provenance"] = "regional-original"
            payload_file = root / "atlas.payload"
            payload_file.write_bytes(b"client-atlas-bytes")
            raw_entry = {"entry_path": "production/upload/aa/" + "a" * 38, "payload_file": "atlas.payload",
                         "logical_path": "scene/test/atlas.atf", "provenance": "derived-substitute", "owners": ["test"]}
            prepared = root / "prepared.json"
            prepared.write_text(json.dumps({"source": {"kind": "local-preparation"},
                                            "provenance": "generated-substitute", "assets": assets,
                                            "raw_entries": [raw_entry],
                                            "remaining_missing": []}), encoding="utf-8")
            report = root / "report.json"
            installed = install_images(cdn, prepared, report)
            published_path = (cdn / "path").read_bytes()
            reused = install_images(cdn, prepared, report)
            result = json.loads(report.read_text(encoding="utf-8"))
            self.assertEqual("1.4.1", installed["target_version"])
            self.assertTrue(reused["reused"])
            self.assertEqual(published_path, (cdn / "path").read_bytes())
            self.assertEqual("mixed-resource-batch", result["source"])
            self.assertEqual(["regional-original", "generated-substitute"],
                             [asset["provenance"] for asset in result["assets"]])
            self.assertEqual([raw_entry], result["raw_entries"])
            with zipfile.ZipFile(cdn / result["archives"][0]["relative_path"]) as archive:
                self.assertEqual(b"client-atlas-bytes", archive.read(raw_entry["entry_path"]))

    def test_rejects_conflicting_payloads_and_parent_traversal(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            image = root / "image.png"
            Image.new("RGB", (64, 64), "red").save(image)
            asset = {"logical_path": "scene/test/image.png", "preview": str(image), "dimensions": [64, 64]}
            entry, data = image_payloads(asset)[0]
            raw = root / "raw.payload"
            raw.write_bytes(data)
            record = {"entry_path": entry, "payload_file": "raw.payload"}
            self.assertEqual(image_payloads(asset), merge_prepared_payloads([asset], [record], root))
            raw.write_bytes(b"different")
            with self.assertRaisesRegex(ValueError, "不同内容"):
                merge_prepared_payloads([asset], [record], root)
            with self.assertRaisesRegex(ValueError, "散列路径"):
                merge_prepared_payloads([], [{**record, "entry_path": "production/upload/../other"}], root)
# //// /验证混合图片来源随差分复用保留 ////


# //// 验证替代漫画随发布目录复用和修复 [@x380kkm 2026-09-21] ////
class ComicSubstituteTest(unittest.TestCase):
    def test_reuses_and_repairs_a_copied_library_without_original_inputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "art").mkdir()
            local_image = root / "art" / "illustration.png"
            Image.new("RGBA", (600, 900), (20, 90, 170, 255)).save(local_image)
            original = {"kind": 0, "episode": 1, "title": "原作",
                        "source_url": "https://www.taptap.cn/moment/1",
                        "image_url": "https://img.tapimg.com/original.png"}
            substitute = {"kind": 0, "episode": 2, "title": "插画",
                          "source_url": "https://www.taptap.cn/moment/2", "availability": "substitute",
                          "notice": "授权制作的插画替代页.", "image_file": "art/illustration.png"}
            source_path = root / "sources.json"
            source_path.write_text(json.dumps([original, substitute], ensure_ascii=False), encoding="utf-8")
            output = root / "library"
            with patch.object(comics, "download_image", return_value=Image.new("RGB", (600, 900), "orange")), \
                    patch("sys.stdout", new_callable=StringIO):
                comics.prepare_library(comics.read_sources(source_path), output, 1, None, None)
            published = json.loads((output / "index.json").read_text(encoding="utf-8"))
            self.assertEqual(original, published[0])
            self.assertEqual({key: value for key, value in substitute.items() if key != "image_file"}, published[1])
            copied = root / "copied"
            shutil.copytree(output, copied)
            local_image.unlink()
            original_main = copied / "0/main/1.png"
            original_bytes = original_main.read_bytes()
            originals = (output / "0/main/2.png").read_bytes()
            sources = comics.read_sources(copied / "index.json")
            with patch.object(comics, "prepare_comic", side_effect=AssertionError("同源成品发生重写")), \
                    patch("sys.stdout", new_callable=StringIO):
                comics.prepare_library(sources, copied, 1, None, None)
            (copied / "0/thumbnail_l/2.jpg").unlink()
            with patch.object(comics, "download_image", side_effect=AssertionError("本地修复访问网络")), \
                    patch("sys.stdout", new_callable=StringIO):
                comics.prepare_library(sources, copied, 1, None, None)
            self.assertEqual(original_bytes, original_main.read_bytes())
            self.assertEqual(originals, (output / "0/main/2.png").read_bytes())
            with Image.open(copied / "0/thumbnail_l/2.jpg") as thumbnail:
                self.assertEqual((984, 623), thumbnail.size)
            self.assertEqual("substitute", comics.read_sources(copied / "index.json")[1].availability)

    def test_preserves_contiguous_episode_validation_for_substitute_pages(self):
        with tempfile.TemporaryDirectory() as directory:
            source_path = Path(directory) / "sources.json"
            source_path.write_text(json.dumps([{"kind": 0, "episode": 2, "title": "插画",
                                               "availability": "substitute", "notice": "授权制作的插画替代页."}]),
                                   encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "话数缺失"):
                comics.read_sources(source_path)
# //// /验证替代漫画随发布目录复用和修复 ////


if __name__ == "__main__":
    unittest.main()
