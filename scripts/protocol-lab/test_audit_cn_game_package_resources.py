# audience: internal
# # test-audit-cn-game-package-resources
# 此测试覆盖服务图片目录缺失和损坏时的缺口报告, 以及其余有效图片的继续检查.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import io
import json
from contextlib import ExitStack, redirect_stdout
from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from PIL import Image

from audit_cn_game_package_resources import audit_service_images
import audit_cn_game_package_resources as audit


# //// 提供内存中的分发目录和资源内容 [@x380kkm 2026-09-21] ////
class MemoryCdn:
    def __init__(self, files):
        self.entries = files

    def read(self, path):
        return self.entries[path]

    def read_json(self, path):
        return json.loads(self.read(path))


# //// 构造具有漫画和活动图片引用的目录 [@x380kkm 2026-09-21] ////
def catalog_files():
    return {
        "comic/index.json": json.dumps([{"kind": 0, "episode": 1, "title": "漫画"}]).encode(),
        "activity-catalog.json": json.dumps({"activities": [{"activity_id": "event:1",
            "image_candidates": [{"source_type": "activity_banner", "key": "banner.png"}]}]}).encode(),
    }


# //// 验证目录缺口与图片缺口共同进入报告 [@x380kkm 2026-09-21] ////
class ServiceCatalogAuditTest(unittest.TestCase):
    def audit(self, files):
        return audit_service_images(SimpleNamespace(cdn=MemoryCdn(files)))

    def test_reports_both_missing_catalogs(self):
        report = self.audit({})
        self.assertEqual({("missing_service_catalog", "comic/index.json"),
                          ("missing_service_catalog", "activity-catalog.json")},
                         {(gap["kind"], gap["path"]) for gap in report["gaps"]})

    def test_catalog_failure_preserves_other_catalog_image_checks(self):
        failures = [
            ("comic/index.json", b"[]", "empty_service_catalog", "activity-banners/banner.png"),
            ("comic/index.json", b"{}", "invalid_service_catalog", "activity-banners/banner.png"),
            ("comic/index.json", b"{", "invalid_service_catalog", "activity-banners/banner.png"),
            ("activity-catalog.json", b'{"activities":[]}', "empty_service_catalog", "comic/0/main/1.png"),
            ("activity-catalog.json", b'{"activities":{}}', "invalid_service_catalog", "comic/0/main/1.png"),
            ("activity-catalog.json", b"{}", "invalid_service_catalog", "comic/0/main/1.png"),
        ]
        for path, content, kind, remaining_image in failures:
            with self.subTest(path=path, content=content):
                files = catalog_files()
                files[path] = content
                report = self.audit(files)
                issues = {(gap["kind"], gap["path"]) for gap in report["gaps"]}
                self.assertIn((kind, path), issues)
                self.assertIn(("missing_service_image", remaining_image), issues)

    def test_broken_records_preserve_valid_image_and_substitution_checks(self):
        files = catalog_files()
        comics = json.loads(files["comic/index.json"])
        comics[0].update(availability="substitute", notice="本地原作图片.")
        files["comic/index.json"] = json.dumps([None, {"kind": 0}, *comics]).encode()
        activity = json.loads(files["activity-catalog.json"])["activities"][0]
        activity["image_candidates"].insert(0, {"source_type": "activity_banner"})
        files["activity-catalog.json"] = json.dumps({"activities": [None,
            {"activity_id": "event:2", "image_candidates": {}}, activity]}).encode()
        for name, color in (("main/1.png", (20, 90, 170, 255)), ("thumbnail_s/1.jpg", (0, 0, 0, 0))):
            image = io.BytesIO()
            Image.new("RGBA", (8, 8), color).save(image, format="PNG")
            files["comic/0/" + name] = image.getvalue()
        files["comic/0/thumbnail_l/1.jpg"] = b"broken image"
        report = self.audit(files)
        self.assertEqual(2, report["decodedImages"])
        self.assertEqual(4, report["imageCount"])
        self.assertEqual("comic:0:1", report["substitutions"][0]["owner"])
        self.assertEqual(5, sum(gap["kind"] == "invalid_service_catalog_entry" for gap in report["gaps"]))
        self.assertEqual({("missing_service_image", "activity-banners/banner.png"),
                          ("invalid_service_image", "comic/0/thumbnail_l/1.jpg"),
                          ("empty_service_image", "comic/0/thumbnail_s/1.jpg")},
                         {(gap["kind"], gap["path"]) for gap in report["gaps"]
                          if gap["kind"] != "invalid_service_catalog_entry"})
# //// /验证目录缺口与图片缺口共同进入报告 ////


# //// 将实际服务资源缺口传递到发布命令退出状态 [@x380kkm 2026-09-26] ////
class AuditExitStatusTest(unittest.TestCase):
    def test_writes_report_and_rejects_missing_service_resources(self):
        image = io.BytesIO()
        Image.new("RGB", (2, 2), (20, 90, 170)).save(image, format="PNG")
        complete = catalog_files()
        for path in ("activity-banners/banner.png", "comic/0/main/1.png",
                     "comic/0/thumbnail_s/1.jpg", "comic/0/thumbnail_l/1.jpg"):
            complete[path] = image.getvalue()
        for files, exit_code in ((complete, 0), (catalog_files(), 1)):
            with self.subTest(exit_code=exit_code), TemporaryDirectory() as directory, ExitStack() as stack:
                output = Path(directory) / "report.json"
                reader = SimpleNamespace(cdn=MemoryCdn(files),
                    bundle=SimpleNamespace(platform="android"), versions=[])
                factory = stack.enter_context(patch.object(audit, "PackagedResourceReader"))
                factory.return_value.__enter__.return_value = reader
                stack.enter_context(patch.object(audit, "collect_game_references", return_value=(None, {})))
                inventory = stack.enter_context(patch.object(audit, "GachaResourceInventory"))
                inventory.return_value.references = {}
                inventory.return_value.master_report = {"scope": {}}
                inventory.return_value.gaps = []
                stack.enter_context(patch.object(audit, "audit_distribution", return_value={"gaps": []}))
                stack.enter_context(patch.object(audit, "audit_references",
                    return_value={"references": 0, "images": {}, "gaps": []}))
                stack.enter_context(patch("sys.argv", ["audit", "--client-package", directory,
                    "--client-source-root", directory, "--activity-projection", directory,
                    "--output", str(output)]))
                stack.enter_context(redirect_stdout(io.StringIO()))
                if exit_code:
                    with self.assertRaises(SystemExit) as failure:
                        audit.main()
                    self.assertEqual(exit_code, failure.exception.code)
                else:
                    audit.main()
                report = json.loads(output.read_text(encoding="utf-8"))["packages"][0]
                self.assertEqual(bool(exit_code), bool(report["gaps"]))
# //// /将实际服务资源缺口传递到发布命令退出状态 ////


if __name__ == "__main__":
    unittest.main()
