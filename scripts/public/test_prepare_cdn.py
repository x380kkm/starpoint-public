# audience: internal
# # test-prepare-cdn
#
# 此测试验证公共 CDN 准备工具的文件合并和目录约束.

from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT_PATH = Path(__file__).with_name("prepare_cdn.py")
SPEC = importlib.util.spec_from_file_location("prepare_cdn", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


# //// 写入可用于测试的完整 CDN 根 [@x380kkm 2026-09-02] ////
def write_source_layout(root: Path) -> None:
    (root / "entities").mkdir(parents=True)
    (root / "path").write_bytes(b"path-layout")
    (root / "entities" / "PathFile.csv").write_bytes(b"asset,row\n")
# //// /写入可用于测试的完整 CDN 根 ////


# //// 验证 CDN 合并行为 [@x380kkm 2026-09-02] ////
class PrepareCdnTests(unittest.TestCase):
    # //// 合并相对路径并让后置覆盖层替换文件 [@x380kkm 2026-09-02] ////
    def test_merges_paths_and_applies_overlays(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            source = root / "source"
            first_overlay = root / "first-overlay"
            last_overlay = root / "last-overlay"
            destination = root / "destination"
            write_source_layout(source)
            (source / "assets").mkdir()
            (source / "assets" / "shared.bin").write_bytes(b"source")
            (source / "assets" / "base.bin").write_bytes(b"base")
            (first_overlay / "assets").mkdir(parents=True)
            (first_overlay / "assets" / "shared.bin").write_bytes(b"first")
            (last_overlay / "assets").mkdir(parents=True)
            (last_overlay / "assets" / "shared.bin").write_bytes(b"last")
            (last_overlay / "assets" / "added.bin").write_bytes(b"added")

            layout = MODULE.prepare_cdn(
                source, destination, [first_overlay, last_overlay]
            )

            self.assertEqual(b"last", (destination / "assets" / "shared.bin").read_bytes())
            self.assertEqual(b"base", (destination / "assets" / "base.bin").read_bytes())
            self.assertEqual(b"added", (destination / "assets" / "added.bin").read_bytes())
            written_files = [
                path
                for path in destination.rglob("*")
                if path.is_file() and path.name != MODULE.LAYOUT_NAME
            ]
            self.assertEqual(len(written_files), layout["file_count"])
            self.assertEqual(
                sum(path.stat().st_size for path in written_files),
                layout["total_bytes"],
            )
            self.assertEqual("source", layout["source"])
            self.assertEqual(["first-overlay", "last-overlay"], layout["overlays"])
            self.assertEqual(
                layout,
                json.loads((destination / MODULE.LAYOUT_NAME).read_text(encoding="utf-8")),
            )
            (destination / "assets" / "base.bin").write_bytes(b"changed")
            self.assertEqual(b"base", (source / "assets" / "base.bin").read_bytes())
    # //// /合并相对路径并让后置覆盖层替换文件 ////

    # //// 仅在请求清理时删除目标目录的多余文件 [@x380kkm 2026-09-02] ////
    def test_prune_removes_only_unmerged_files(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            source = root / "source"
            destination = root / "destination"
            write_source_layout(source)
            destination.mkdir()
            extra = destination / "local-note.txt"
            extra.write_text("keep", encoding="utf-8")

            MODULE.prepare_cdn(source, destination)
            self.assertTrue(extra.is_file())

            MODULE.prepare_cdn(source, destination, prune=True)
            self.assertFalse(extra.exists())
            self.assertTrue((destination / "path").is_file())
            self.assertTrue((destination / MODULE.LAYOUT_NAME).is_file())
    # //// /仅在请求清理时删除目标目录的多余文件 ////

    # //// 拒绝跨层目录名称的大小写冲突 [@x380kkm 2026-09-02] ////
    def test_rejects_case_conflicts(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            source = root / "source"
            overlay = root / "overlay"
            write_source_layout(source)
            (source / "Textures").mkdir()
            (source / "Textures" / "base.bin").write_bytes(b"base")
            (overlay / "textures").mkdir(parents=True)
            (overlay / "textures" / "replacement.bin").write_bytes(b"replacement")

            with self.assertRaisesRegex(MODULE.CdnPreparationError, "大小写冲突"):
                MODULE.prepare_cdn(source, root / "destination", [overlay])
    # //// /拒绝跨层目录名称的大小写冲突 ////

    # //// 校验 path 和 EntityLists 两种入口结构 [@x380kkm 2026-09-02] ////
    def test_validates_source_layout(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            source = root / "source"
            source.mkdir()
            with self.assertRaisesRegex(MODULE.CdnPreparationError, "path"):
                MODULE.prepare_cdn(source, root / "missing-path")

            (source / "path").write_bytes(b"layout")
            with self.assertRaisesRegex(MODULE.CdnPreparationError, "EntityLists"):
                MODULE.prepare_cdn(source, root / "missing-lists")

            (source / "EntityLists").mkdir()
            layout = MODULE.prepare_cdn(source, root / "entity-lists")
            self.assertEqual(1, layout["file_count"])
    # //// /校验 path 和 EntityLists 两种入口结构 ////
# //// /验证 CDN 合并行为 ////


# //// 运行 CDN 准备测试 [@x380kkm 2026-09-02] ////
if __name__ == "__main__":
    unittest.main()
# //// /运行 CDN 准备测试 ////
