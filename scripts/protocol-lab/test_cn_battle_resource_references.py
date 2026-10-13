# audience: internal
# # test-cn-battle-resource-references
# 此测试检查战斗资源的动态路径, 动画格式回退和图集依赖缺口.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

import io
import unittest
import zlib

from PIL import Image

from audit_cn_game_package_resources import audit_references
from audit_cn_master_resource_references import References, collect_references, source_fields
from cn_battle_character_resource_references import collect_battle_character_references
from prepare_cn_atlas_aliases import encode_frame, encode_string, encode_u29


# //// 为动画引用提供内存中的资源和内容位置 [@x380kkm 2026-09-28] ////
class MemoryResourceReader:
    def __init__(self, files):
        self.files = files

    def locate(self, logical, resolution="medium"):
        data = self.files.get(logical)
        if data is None:
            return None
        return {"entry": logical, "bytes": len(data), "crc": zlib.crc32(data)}

    def read_location(self, location):
        return self.files[location["entry"]]


# //// 构造具有九种方向和实际图集成员的冲刺板资源 [@x380kkm 2026-09-28] ////
def create_panel_resources():
    directory = "battle/field_object/example/yakumono/dash_panel"
    references = References()
    references.field("dash_panel_directory", directory, "zone:example/0", "ZoneValues.as:36")
    files, frames = {}, []
    for reference in references.document():
        if reference["kind"] == "timeline":
            files[reference["logicalPath"]] = zlib.compress(b"\x0a\x0b\x01\x01", wbits=-15)
            continue
        logical = reference["alternativePaths"][0]
        animation = logical.rsplit("/", 1)[1].split(".", 1)[0]
        prefix = directory + "/.gen/" + animation + "/f"
        value = b"\x0a\x0b\x01" + encode_string("name") + b"\x06" + encode_string(prefix) + b"\x01"
        files[logical] = zlib.compress(value, wbits=-15)
        frames.append({"n": prefix + "0", "x": 0, "y": 0, "w": 1, "h": 1})
    atlas = b"\x09" + encode_u29((len(frames) << 1) | 1) + b"\x01" + b"".join(map(encode_frame, frames))
    files[directory + "/dash_panel.atlas.amf3.deflate"] = zlib.compress(atlas, wbits=-15)
    image = io.BytesIO()
    Image.new("RGBA", (1, 1), (30, 60, 90, 255)).save(image, format="PNG")
    files[directory + "/dash_panel.png"] = image.getvalue()
    return directory, references, files


# //// 验证客户端目录字段进入共享引用集合 [@x380kkm 2026-09-28] ////
class BattleReferenceCollectionTest(unittest.TestCase):
    def test_zone_directory_expands_fixed_directions_and_smart_panel(self):
        source = """
public var dash_panel_directory:Option;
var _loc37_:String = param1[36];
dash_panel_directory = _loc37_ == "(None)" ? Option.None : Option.Some(_loc37_);
"""
        fields, unresolved = source_fields(source)
        row = ["(None)"] * 37
        row[36] = "battle/field_object/zone/panels"
        specs = [{"name": "zone", "fields": fields, "source": "ZoneValues.as"}]
        references, _, _, _ = collect_references(specs, {"zone": {"example": {"0": row}}})
        self.assertEqual([], unresolved)
        expected = {"fixed_0", "fixed_45", "fixed_90", "fixed_135", "fixed_180", "fixed_225", "fixed_270", "fixed_315", "smart"}
        self.assertEqual(expected, {item["logicalPath"].rsplit("/", 1)[1].split(".")[0] for item in references.document()})
        for reference in references.document():
            if reference["kind"] == "animation":
                self.assertEqual([reference["logicalPath"].replace(".parts.", ".frame.")], reference["alternativePaths"])
            self.assertEqual(["zone:example/0"], reference["owners"])

    def test_quest_party_uses_scripted_character_result_resources(self):
        fields, _ = source_fields('"battle_fixed_party":MainQuestValues.parseAt118(param1)')
        quest = ["(None)"] * 119
        quest[118] = "26"
        party = ["(None)"] * 44
        party[2] = "700016"
        party[8] = ",".join(str(700016 * 2000 + node) for node in range(1, 5))
        party[8] += ",2002001"
        mana_nodes = {"700016/0/1": ["1400032001", "0", "", "", "", "2", ""]}
        mana_nodes.update({f"700016/0/{slot + 1}": [str(700016 * 2000 + slot + 1), "0", "", "", "", "0", str(slot)]
                           for slot in range(1, 4)})
        references = References()
        collect_battle_character_references(references,
            [{"name": "main_quest", "fields": fields, "source": "MainQuestValues.as"}],
            {"main_quest": {"12/4/1": quest}, "fixed_party": {"26": party},
             "character": {"700016": ["light_chapter12", "", "5"]}, "mana_node": mana_nodes})
        self.assertEqual([], references.issues)
        self.assertEqual({"character/light_chapter12/pixelart/pixelart.parts.amf3.deflate",
                          "character/light_chapter12/pixelart/special.parts.amf3.deflate"},
                         {item["logicalPath"] for item in references.document() if item["kind"] == "animation"})
        self.assertEqual({"character/light_chapter12/ui/" + image for image in (
            "thumb_level_up_1.png", "cutin_skill_chain_1.png", "battle_member_status_1.png",
            "battle_control_board_1.png", "square_1.png", "skill_cutin_1.atf.deflate")},
            {item["logicalPath"] for item in references.document() if "/ui/" in item["logicalPath"]})
        self.assertEqual(14, len(references.resources))
        self.assertTrue(all(item["owners"] == ["main_quest:12/4/1"] for item in references.document()))

    def test_fixed_party_images_follow_main_and_unison_consumers(self):
        party = ["(None)"] * 44
        party[2], party[23], party[30] = "1", "2", "3"
        tables = {"main_quest": {"1/1/1": ["5"]}, "fixed_party": {"5": party},
                  "character": {str(index): [name, "", "3"] for index, name in
                                ((1, "main"), (2, "unison"), (3, "orphan"))}}
        specs = [{"name": "main_quest", "source": "MainQuestValues.as",
                  "fields": {"battle_fixed_party": {"index": 0, "line": 1}}}]
        references = References()
        collect_battle_character_references(references, specs, tables)
        images = {item["logicalPath"] for item in references.document() if "/ui/" in item["logicalPath"]}
        self.assertEqual({"character/unison/ui/thumb_level_up_0.png",
                          "character/unison/ui/skill_cutin_0.atf.deflate"},
                         {path for path in images if path.startswith("character/unison/")})
        self.assertEqual({"character/orphan/ui/thumb_level_up_0.png"},
                         {path for path in images if path.startswith("character/orphan/")})
        party[8] = "2001"
        references = References()
        collect_battle_character_references(references, specs, tables)
        self.assertEqual([("mana_node", "1/0/1")],
                         [(issue["master"], issue["record"]) for issue in references.issues])
        self.assertFalse(any("/main/ui/" in item["logicalPath"] for item in references.document()))

    def test_quest_assist_formats_use_their_actual_configuration_keys(self):
        fields, _ = source_fields('''
"battle_assist_character":MainQuestValues.parseAt112(param1)
"battle_assist_character_2":MainQuestValues.parseAt115(param1)
''')
        quest = ["(None)"] * 118
        quest[112:118] = ["77", "ally_a", "15", "ally_b", "20", "(None)"]
        specs = [{"name": "main_quest", "fields": fields, "source": "MainQuestValues.as"}]
        tables = {"main_quest": {"12/4/1": quest}, "assist_multiball": {"ally_a": [], "ally_b": []}}
        references = References()
        collect_battle_character_references(references, specs, tables)
        self.assertEqual([], references.issues)
        del tables["assist_multiball"]["ally_a"]
        del tables["assist_multiball"]["ally_b"]
        collect_battle_character_references(references, specs, tables)
        report = audit_references(MemoryResourceReader({}), references, {}, set())
        self.assertEqual({"ally_a", "ally_b"}, {gap["record"] for gap in report["gaps"]})


# //// 验证可选格式回退和真实动画依赖的缺口报告 [@x380kkm 2026-09-28] ////
class BattleAnimationResolutionTest(unittest.TestCase):
    def test_frame_fallback_keeps_missing_animation_visible(self):
        directory, references, files = create_panel_resources()
        report = audit_references(MemoryResourceReader(files), references, {}, set())
        self.assertEqual([], report["gaps"])
        self.assertEqual(19, len(report["files"]))
        missing = directory + "/fixed_45.frame.amf3.deflate"
        del files[missing]
        report = audit_references(MemoryResourceReader(files), references, {}, set())
        self.assertEqual([("missing_reference", missing.replace(".frame.", ".parts."))],
                         [(gap["kind"], gap["logicalPath"]) for gap in report["gaps"]])
        self.assertEqual(18, len(report["files"]))

    def test_frame_prefix_requires_its_atlas(self):
        directory, references, files = create_panel_resources()
        del files[directory + "/dash_panel.atlas.amf3.deflate"]
        report = audit_references(MemoryResourceReader(files), references, {}, set())
        self.assertEqual(9, len(report["gaps"]))
        self.assertEqual({"missing_frame_atlas"}, {gap["kind"] for gap in report["gaps"]})

    def test_frame_atlas_requires_image_and_matching_members(self):
        directory, references, files = create_panel_resources()
        del files[directory + "/dash_panel.png"]
        frames = [{"n": "other/texture", "x": 0, "y": 0, "w": 1, "h": 1}]
        atlas = b"\x09\x03\x01" + encode_frame(frames[0])
        files[directory + "/dash_panel.atlas.amf3.deflate"] = zlib.compress(atlas, wbits=-15)
        report = audit_references(MemoryResourceReader(files), references, {}, set())
        self.assertEqual(9, sum(gap["kind"] == "missing_frame_prefix" for gap in report["gaps"]))
        self.assertEqual([directory + "/dash_panel.png"],
                         [gap["logicalPath"] for gap in report["gaps"] if gap["kind"] == "missing_reference"])


if __name__ == "__main__":
    unittest.main()
