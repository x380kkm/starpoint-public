# audience: internal
# # cn-mission-equipment-metadata
#
# 装备属性来自生效的能力条件与内容. 客户端生成类提供列选择, 运行时能力解析类提供元素转换.

from __future__ import annotations

import csv
import io
import re
import struct
import zlib
from functools import lru_cache
from pathlib import Path

from cn_gacha_banner_assets import decode_ordered_map


ELEMENTS = {name: index for index, name in enumerate(("Red", "Blue", "Yellow", "Green", "White", "Black"), 1)}
OPPOSITE_ELEMENTS = {1: 2, 2: 3, 3: 4, 4: 1, 5: 6, 6: 5}
RUNTIME_SOURCES = {
    "AbilityPreconditionMasterValue": ("pre/AbilityPreconditionMasterValueTools.as", "param1"),
    "InstantAbilityTriggerMasterValue": ("instant/InstantAbilityTriggerMasterValueTools.as", "param1"),
    "InstantAbilityContentMasterValue": ("instant/InstantAbilitySource.as", "_loc4_"),
    "DuringAbilityTriggerMasterValue": ("during/DuringAbilitySource.as", "_loc17_"),
    "CommonAbilityContentMasterValue": ("during/DuringAbilitySource.as", "_loc21_"),
}


def block_at(source: str, start: int) -> tuple[str, int]:
    opening = source.index("{", start)
    depth = 1
    quote = None
    index = opening + 1
    while depth:
        character = source[index]
        if quote:
            if character == "\\":
                index += 1
            elif character == quote:
                quote = None
        elif character in ('"', "'"):
            quote = character
        elif character == "{":
            depth += 1
        elif character == "}":
            depth -= 1
        index += 1
    return source[opening + 1:index - 1], index


def decode_row_lists(data: bytes) -> dict[str, list[list[str]]]:
    size = struct.unpack_from("<I", data)[0]
    index = zlib.decompress(data[4:4 + size])
    count = struct.unpack_from("<I", index)[0]
    names = index[4 + count * 8:]
    key_start = data_start = 0
    result = {}
    for position in range(count):
        key_end, data_end = struct.unpack_from("<II", index, 4 + position * 8)
        key = names[key_start:key_end].decode("utf-8")
        text = zlib.decompress(data[4 + size + data_start:4 + size + data_end]).decode("utf-8")
        result[key] = list(csv.reader(io.StringIO(text, newline=""), strict=True))
        key_start, data_start = key_end, data_end
    return result


# //// 从生成类选择实际列分支并读取对应能力构造 [@x380kkm 2026-09-21] ////
class AbilityColumns:
    def __init__(self, source: str):
        self.methods = {
            int(match[1]): block_at(source, match.end())[0]
            for match in re.finditer(r"public static function parseAt(\d+)\(param1:Array\)", source)
        }

    @lru_cache(maxsize=None)
    def selected(self, column: int, value: str) -> str:
        source = self.methods[column]
        if "CharacterGroupId_Impl_._new" in source:
            return source
        for match in re.finditer(r'if\(_loc\d+_ == "([^"\n]*)"\)', source):
            if match[1] == value:
                return block_at(source, match.end())[0]
        for match in re.finditer(r'if\(_loc\d+_ != "([^"\n]*)"\)', source):
            if match[1] == value:
                return source[block_at(source, match.end())[1]:]
        if "MasterValue." in source:
            raise ValueError(f"Unknown ability column {column}: {value}")
        return source

    def parse(self, row: list[str], column: int):
        value = row[column]
        if value in ("", "(None)"):
            return None
        source = self.selected(column, value)
        if "CharacterGroupId_Impl_._new" in source:
            return {"groups": value.split(",")}
        constructor = re.search(r"return (\w+MasterValue)\.(\w+)", source)
        if constructor is None:
            return int(value) if value.lstrip("-").isdigit() else value
        fields = {
            match[1]: self.parse(row, int(match[2]))
            for match in re.finditer(r'"([a-z_0-9]+)":\w+Values\.parseAt(\d+)\(param1\)', source)
        }
        return {"type": constructor[1], "kind": constructor[2], "fields": fields}
# //// /从生成类选择实际列分支并读取对应能力构造 ////


# //// 对照客户端能力元素提取器生成元素集合 [@x380kkm 2026-09-21] ////
class AbilityElements:
    def __init__(self, scripts: Path):
        root = scripts / "pinball/common/data/ability"
        self.runtime = {}
        for enum_name, (relative, variable) in RUNTIME_SOURCES.items():
            enum_source = (root / Path(relative).parent / f"{enum_name}.as").read_text(encoding="utf-8")
            names = {int(match[2]): match[1] for match in re.finditer(rf'new {enum_name}\("(\w+)",(\d+),', enum_source)}
            source = (root / relative).read_text(encoding="utf-8")
            switch = source.find(f"switch({variable}.index)")
            selector_match = re.search(rf"var (\w+):int = {variable}\.index;", source)
            if switch >= 0 and (selector_match is None or switch < selector_match.start()):
                body = block_at(source, switch)[0]
                cases = list(re.finditer(r"^            case (\d+):", body, re.M))
                branches = {int(match[1]): body[match.end():cases[index + 1].start() if index + 1 < len(cases) else len(body)] for index, match in enumerate(cases)}
            else:
                selector = selector_match[1]
                branches = {int(match[1]): block_at(source, match.end())[0]
                            for match in re.finditer(rf"if\({selector} == (\d+)\)", source)}
            self.runtime[enum_name] = {name: branches[index] for index, name in names.items() if index in branches}

    def extract(self, node) -> set[int]:
        if not isinstance(node, dict):
            return set()
        if "groups" in node:
            return {ELEMENTS[name] for name in node["groups"] if name in ELEMENTS}
        found = set()
        for field in node["fields"].values():
            found.update(self.extract(field))
        enum_name, kind = node["type"], node["kind"]
        if enum_name not in self.runtime:
            return found
        if kind not in self.runtime[enum_name]:
            raise ValueError(f"Missing ability resolution: {enum_name}.{kind}")
        source = self.runtime[enum_name][kind]
        if "CommonAbilityCharacterContent.ElementResistanceToConditionEnemy(" in source:
            element = node["fields"]["element"]
            if element in OPPOSITE_ELEMENTS:
                found.add(OPPOSITE_ELEMENTS[element])
            return found
        if "CommonAbilityCharacterContent.OverrideCharacterElement(" in source:
            found.add(node["fields"]["element"])
            return found
        for match in re.finditer(r"(?:ConditionTargetKind\.ElementResistance|CharacterFloatParameterKind\.Resistance|CommonAbilityCharacterContent\.Element(?:Resistance|DamageCut))\((\d+)[,)]", source):
            if int(match[1]) in OPPOSITE_ELEMENTS:
                found.add(OPPOSITE_ELEMENTS[int(match[1])])
        for match in re.finditer(r"ConditionChangeContent\.Element(?:Resistance|DamageCut|DamageCutPercent)\((\d+),", source):
            element = int(match[1])
            if element in OPPOSITE_ELEMENTS:
                enemy = re.search(r"InstantAbilityContent\.Condition(?:Enemy|NearestEnemy|TriggerEnemy)\(", source)
                found.add(element if enemy else OPPOSITE_ELEMENTS[element])
        for match in re.finditer(r"CommonAbilityCharacterContent\.ConditionShare\((\d+),", source):
            if 4 <= int(match[1]) <= 9:
                found.add(OPPOSITE_ELEMENTS[int(match[1]) - 3])
        return found

    def row_elements(self, columns: AbilityColumns, row: list[str], offset: int = 0) -> list[int]:
        kind = row[2 + offset]
        if kind == "2":
            return []
        indices = [3, 10, 17] + ([24, 44] if kind == "0" else [94, 106])
        found = set()
        for column in indices:
            found.update(self.extract(columns.parse(row, column + offset)))
        return sorted(found)
# //// /对照客户端能力元素提取器生成元素集合 ////


def generate_equipment(reader, scripts: Path) -> dict:
    equipment = decode_ordered_map(reader.read("master/item/equipment.orderedmap"))
    souls = decode_row_lists(reader.read("master/ability/ability_soul.orderedmap"))
    enhancements = decode_row_lists(reader.read("master/equipment_enhancement/equipment_enhancement_ability.orderedmap"))
    extractor = AbilityElements(scripts)
    generated = scripts / "pinball/master/generated"
    soul_columns = AbilityColumns((generated / "AbilitySoulValues.as").read_text(encoding="utf-8"))
    enhancement_columns = AbilityColumns((generated / "EquipmentEnhancementAbilityValues.as").read_text(encoding="utf-8"))
    result = {}
    for equipment_id, row in equipment.items():
        result[equipment_id] = {
            "max_level": int(row[8]),
            "abilities": [{"slot": int(ability[0]), "learn_level": int(ability[1]), "elements": extractor.row_elements(soul_columns, ability)} for ability in souls[row[10]]],
            "enhancement_abilities": [{"slot": int(ability[0]), "learn_level": int(ability[1]), "elements": extractor.row_elements(enhancement_columns, ability, 3)} for ability in enhancements.get(equipment_id, [])],
        }
    return result
