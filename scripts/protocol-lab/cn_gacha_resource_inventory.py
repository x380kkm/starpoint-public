# audience: internal
# # cn-gacha-resource-inventory
# 此模块从实际客户端 master 和资源加载代码展开卡池页面, 特辑和抽取结果的资源引用.

from __future__ import annotations

import re
from collections import defaultdict
from pathlib import Path

from audit_cn_gacha_resource_links import audit_masters, MASTER_PATHS
from cn_gacha_banner_assets import decode_ordered_map


# //// 按卡池和候选追踪每个资源的引用来源 [@x380kkm 2026-09-21] ////
class GachaResourceInventory:
    def __init__(self, reader):
        self.reader = reader
        self.references = defaultdict(set)
        self.characters = set()
        self.preview_characters = set()
        self.equipment = set()
        self.equipment_keys = {}
        self.gaps = []

    def add(self, logical: str, owner: str):
        self.references[logical].add(owner)

    def add_sheet(self, path: str, owner: str):
        self.add(path + ".atlas.amf3.deflate", owner)
        self.add(path + ".png", owner)

    def master(self, logical: str) -> dict:
        payload = self.reader.read(logical)
        if payload is None:
            self.gaps.append({"kind": "missing_master", "logicalPath": logical})
            return {}
        return decode_ordered_map(payload)

    def collect_masters(self):
        masters = {name: self.master(logical) for name, logical in MASTER_PATHS.items()}
        self.master_report = audit_masters(masters)
        for record in self.master_report["references"]:
            for owner in record["owners"]:
                self.add(record["logicalPath"], owner)
        odds = {}
        for pool, row in masters["gacha"].items():
            destination = self.characters if row[13] == "0" else self.equipment
            for field in ((14, 15, 16) if row[13] == "0" else (22, 23, 24)):
                name = row[field]
                if name in ("", "(None)"):
                    continue
                if name not in odds:
                    odds[name] = self.master(f"master/gacha_odds/{name}.orderedmap")
                group = odds[name].get(name, {})
                if not group:
                    self.gaps.append({"kind": "missing_odds_group", "pool": pool, "odds": name})
                destination.update(int(value[0]) for value in group.values())
        for group in masters["feature"].values():
            for row in group.values():
                if row[0] == "2":
                    self.characters.add(int(row[5]))
                    self.preview_characters.add(int(row[5]))
                if row[6] == "0":
                    self.characters.add(int(row[7]))
        self.collect_candidates()

    def collect_candidates(self):
        characters = self.master("master/character/character.orderedmap")
        equipment = self.master("master/item/equipment.orderedmap")
        for character in sorted(self.characters):
            row = characters.get(str(character))
            if row is None:
                self.gaps.append({"kind": "missing_character", "id": character})
                continue
            base = f"character/{row[0]}"
            owner = f"character:{character}"
            self.add_sheet(base + "/pixelart/sprite_sheet", owner)
            self.add(base + "/pixelart/pixelart.frame.amf3.deflate", owner)
            self.add(base + "/ui/full_shot_1440_1920_0.png", owner)
            self.add(base + "/ui/square_0.png", owner)
            self.add(base + "/ui/square_132_132_0.png", owner)
            if character in self.preview_characters:
                self.add(base + "/ui/skill_cutin_0.png", owner)
            if int(row[2]) >= 4:
                self.add_sheet(base + "/pixelart/special_sprite_sheet", owner)
                self.add(base + "/pixelart/special.frame.amf3.deflate", owner)
        self.add_sheet("item/sprite_sheet", "equipment:all")
        self.add_sheet("item_sec/sprite_sheet", "equipment:all")
        for item in sorted(self.equipment):
            row = equipment.get(str(item))
            if row is None:
                self.gaps.append({"kind": "missing_equipment", "id": item})
                continue
            self.equipment_keys[item] = row[6]
# //// /按卡池和候选追踪每个资源的引用来源 ////


# //// 从真实加载器提取卡池共用布局和动画 [@x380kkm 2026-09-21] ////
def collect_scene_resources(inventory: GachaResourceInventory, sources: list[Path]):
    for source in sources:
        text = source.read_text(encoding="utf-8-sig")
        calls = re.finditer(r"(?:add(Uis?|SpriteSheets?|AnimationLayouts?)|movie\.add(?:All)?)\(([^;]+)\);", text)
        for call in calls:
            kind = call[1] or "Movie"
            for path in re.findall(r'"([^"\n]+)"', call[2]):
                if not path.startswith(("scene/gacha", "scene/crazy_gacha", "scene/character_get")) and path != "scene/general/animation/footer_icon_gacha":
                    continue
                owner = "client:" + source.name
                if kind.startswith("Ui"):
                    inventory.add(path + ".ui.amf3.deflate", owner)
                elif kind.startswith("SpriteSheet"):
                    inventory.add_sheet(path, owner)
                elif kind.startswith("AnimationLayout"):
                    extension = ".parts.amf3.deflate" if inventory.reader.locate(path + ".parts.amf3.deflate") else ".frame.amf3.deflate"
                    inventory.add(path + extension, owner)
                else:
                    inventory.add(path + ".movie.amf3.deflate", owner)
    inventory.add_sheet("scene/general/sprite_sheet_minimal", "client:shared")
    inventory.add_sheet("scene/general/sprite_sheet", "client:shared")
    inventory.add_sheet("scene/character/sprite_sheet", "client:shared")
# //// /从真实加载器提取卡池共用布局和动画 ////
