# audience: internal
# # test-cn-character-full-shot-previews
# 预览适配保持图集外部像素和裁剪坐标, 普通和觉醒图使用对应日服原图.
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import io
import tempfile
import unittest
import zipfile
import zlib
from pathlib import Path
from types import SimpleNamespace

from PIL import Image

from cn_character_full_shot_assets import PSEUDO_PNG_SIGNATURE, _standard_png, hash_cn_asset_path
from cn_character_full_shot_previews import add_illustration_previews, render_preview_sheet, select_preview_sheet
from cn_gacha_banner_assets import encode_entity_digest
from prepare_cn_atlas_aliases import encode_frame, encode_u29


def png(image):
    output = io.BytesIO()
    image.save(output, format="PNG")
    return PSEUDO_PNG_SIGNATURE + output.getvalue()[8:]


# //// 验证普通和觉醒预览各自替换并保留图集其余区域 [@x380kkm 2026-09-27] ////
class PreviewTests(unittest.TestCase):
    def test_stages_both_medium_and_small_preview_entities(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            cdn, source, spool = root / "cdn", root / "source", root / "spool"
            logical = "character/hero/ui/illustration_setting_sprite_sheet.png"
            atlas_logical = logical.replace(".png", ".atlas.amf3.deflate")
            image_hash, atlas_hash = hash_cn_asset_path(logical), hash_cn_asset_path(atlas_logical)
            medium_path = f"production/medium_upload/{image_hash[:2]}/{image_hash[2:]}"
            small_path = f"production/small_upload/{image_hash[:2]}/{image_hash[2:]}"
            atlas_path = f"production/upload/{atlas_hash[:2]}/{atlas_hash[2:]}"
            frame = {"n": "character/hero/ui/full_shot_illustration_setting_0",
                     "x": 0, "y": 0, "w": 8, "h": 8, "fx": 0, "fy": 0, "fw": 8, "fh": 8}
            atlas = zlib.compress(b"\x09\x03\x01" + encode_frame(frame), wbits=-15)
            original = {
                medium_path: (png(Image.new("RGBA", (8, 8), (20, 30, 40, 255))), "medium"),
                small_path: (png(Image.new("RGBA", (4, 4), (20, 30, 40, 255))), "common"),
                atlas_path: (atlas, "common"),
            }
            for category in ("medium", "common"):
                directory = cdn / f"archive-{category}-full"
                directory.mkdir(parents=True)
                with zipfile.ZipFile(directory / "images.zip", "w") as archive:
                    for path, (data, asset_category) in original.items():
                        if asset_category == category:
                            archive.writestr(path, data)
            (cdn / "entities").mkdir()
            (cdn / "entities/PathFile.csv").write_text("".join(
                f"{path},1.4.0,{len(data)},{encode_entity_digest(data)},{category}\n"
                for path, (data, category) in original.items()), encoding="utf-8")
            (source / logical).parent.mkdir(parents=True)
            jp_sheet = png(Image.new("RGBA", (4, 4), (200, 30, 40, 255)))
            (source / logical).write_bytes(jp_sheet)
            (source / atlas_logical).write_bytes(atlas)
            (spool / "replacements").mkdir(parents=True)
            archive_replacements, replacements = {}, {}
            entries = add_illustration_previews(
                cdn, source, [SimpleNamespace(target="character/hero/ui/full_shot_1440_1920_0.png")],
                spool, archive_replacements, replacements,
            )
            self.assertEqual(set(replacements), {medium_path, small_path})
            self.assertEqual(len(entries), 1)
            self.assertEqual(len(entries[0]["variants"]), 2)
            self.assertEqual(replacements[small_path].payload_path.read_bytes(), jp_sheet)
            with Image.open(io.BytesIO(_standard_png(replacements[medium_path].payload_path.read_bytes())[0])) as image:
                self.assertEqual(image.size, (8, 8))
                self.assertEqual(image.getpixel((4, 4)), (200, 30, 40, 255))

    def test_scales_small_quality_frames_and_preserves_padding(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            sheet = png(Image.new("RGBA", (14, 7), (10, 20, 30, 255)))
            frames = []
            replacements = {}
            for evolution, color in enumerate(((240, 20, 40, 255), (20, 240, 40, 255))):
                logical = f"character/hero/ui/full_shot_1440_1920_{evolution}.png"
                digest = hash_cn_asset_path(logical)
                path = root / str(evolution)
                path.write_bytes(png(Image.new("RGBA", (16, 20), color)))
                replacements[f"production/medium_upload/{digest[:2]}/{digest[2:]}"] = SimpleNamespace(payload_path=path)
                frames.append({"n": f"character/hero/ui/full_shot_illustration_setting_{evolution}",
                               "x": 1 + evolution * 10, "y": 1, "w": 8, "h": 8,
                               "fx": -2, "fy": -1, "fw": 10, "fh": 10})
            data, count = render_preview_sheet(sheet, frames, replacements, (20, 10))
            self.assertEqual(count, 2)
            with Image.open(io.BytesIO(_standard_png(data)[0])) as result:
                self.assertEqual(result.size, (14, 7))
                self.assertEqual(result.getpixel((3, 3)), (240, 20, 40, 255))
                self.assertEqual(result.getpixel((10, 3)), (20, 240, 40, 255))
                self.assertEqual(result.getpixel((13, 3)), (10, 20, 30, 255))
                self.assertEqual(result.getpixel((3, 0)), (10, 20, 30, 255))

    def test_scales_the_jp_sheet_as_a_whole_when_atlas_coordinates_match(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            logical = "character/hero/ui/illustration_setting_sprite_sheet.png"
            source = Image.new("RGBA", (12, 8))
            source.paste((240, 20, 40, 255), (0, 0, 6, 8))
            source.paste((20, 240, 40, 255), (6, 0, 12, 8))
            frames = [{"n": f"character/hero/ui/full_shot_illustration_setting_{evolution}",
                       "x": evolution * 6, "y": 0, "w": 6, "h": 8,
                       "fx": 0, "fy": 0, "fw": 6, "fh": 8}
                      for evolution in range(2)]
            encoded = b"\x09" + encode_u29((len(frames) << 1) | 1) + b"\x01"
            atlas = zlib.compress(encoded + b"".join(map(encode_frame, frames)), wbits=-15)
            (root / logical).parent.mkdir(parents=True)
            (root / logical).write_bytes(png(source))
            (root / logical.replace(".png", ".atlas.amf3.deflate")).write_bytes(atlas)
            target = png(Image.new("RGBA", (24, 16)))
            data, source_kind, count = select_preview_sheet(root, logical, target, atlas, {})
            self.assertEqual((source_kind, count), ("jp_sheet_adapted", 2))
            self.assertTrue(data.startswith(PSEUDO_PNG_SIGNATURE))
            with Image.open(io.BytesIO(_standard_png(data)[0])) as result:
                self.assertEqual(result.size, (24, 16))
                self.assertEqual(result.getpixel((2, 8)), (240, 20, 40, 255))
                self.assertEqual(result.getpixel((21, 8)), (20, 240, 40, 255))

    def test_adapts_portraits_inside_the_existing_atlas_rectangles(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            sheet = png(Image.new("RGBA", (12, 8), (10, 20, 30, 255)))
            replacements = {}
            frames = []
            for evolution, color in enumerate(((240, 20, 40, 255), (20, 240, 40, 255))):
                logical = f"character/hero/ui/full_shot_1440_1920_{evolution}.png"
                digest = hash_cn_asset_path(logical)
                path = root / str(evolution)
                path.write_bytes(png(Image.new("RGBA", (8, 12), color)))
                replacements[f"production/medium_upload/{digest[:2]}/{digest[2:]}"] = SimpleNamespace(payload_path=path)
                frames.append({"n": f"character/hero/ui/full_shot_illustration_setting_{evolution}",
                               "x": evolution * 5, "y": 1, "w": 4, "h": 6,
                               "fx": -2, "fy": -1, "fw": 8, "fh": 8})
            original_frames = [dict(frame) for frame in frames]
            data, count = render_preview_sheet(sheet, frames, replacements)
            self.assertEqual(count, 2)
            self.assertEqual(frames, original_frames)
            with Image.open(io.BytesIO(_standard_png(data)[0])) as result:
                self.assertEqual(result.getpixel((1, 3)), (240, 20, 40, 255))
                self.assertEqual(result.getpixel((6, 3)), (20, 240, 40, 255))
                self.assertEqual(result.getpixel((11, 3)), (10, 20, 30, 255))
                self.assertEqual(result.getpixel((1, 0)), (10, 20, 30, 255))
            self.assertTrue(data.startswith(PSEUDO_PNG_SIGNATURE))
# //// /验证普通和觉醒预览各自替换并保留图集其余区域 ////


if __name__ == "__main__":
    unittest.main()
