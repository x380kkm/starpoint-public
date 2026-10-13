# audience: internal
# # audit-cn-master-resource-references
#
# 主检查覆盖可获得角色, 剧情, 装备, 助战及主页首次显示的资源.
# 脚本角色作为普通持有角色使用时的资源要求单列, 保留其缺失记录.
# 路径存在性按当前 EntityLists 和包内 filelist 判断, 内容证据来自指定存储报告.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

from __future__ import annotations

import argparse
import io
import json
import re
import zipfile
from collections import Counter
from pathlib import Path

from audit_cn_cdn_storage import logical_entries, read_bundle_files, read_json
from cn_gacha_banner_assets import EntityCatalog, read_logical_assets
from cn_gacha_banner_atlas import _atlas_index, _decode_atlas
from audit_cn_gacha_candidate_assets import decode_multirow_ordered_map
from cn_startup_resource_references import STARTUP_MASTER_CLASSES, add_startup_references
from cn_battle_character_resource_references import (
    BATTLE_QUEST_REFERENCE_FIELDS, EMPTY_REFERENCE_VALUES as IGNORED_VALUES,
    iter_battle_quest_master_classes, collect_battle_character_references,
)


MASTER_DIRECTORIES = (
    "character", "item", "asset", "quest", "quest/main", "quest/ex", "quest/character", "quest/practice",
    "quest/boss_battle", "story", "scene", "battle", "battle/field", "battle/floor",
    "battle/assist", "shop", "equipment_enhancement", "degree", "encyclopedia", "config",
    "feature_banner", "town", "login_bonus", "bonus", "party", "quest/event", "mana_board",
)
MASTER_CLASSES = (
    "Character", "CharacterSpeech", "Equipment", "EquipmentEnhancement", "StoryCharacter",
    "MainChapter", "ExChapter", "MainQuest", "ExQuest", "CharacterQuest", "PracticeQuestFolder", "BossBattleQuest",
    "BossBattleChapter", "BossBattleStageNode", "BossCoinShopCategory", "EquipmentEnhancementShopCategory",
    "VoiceAsset", "BgmAsset", "SoundEffectAsset", "Degree", "AssistCutin", "AssistMultiball", "AssistYakumono", "Field", "Floor", "Zone", "FixedParty", "ManaNode",
) + STARTUP_MASTER_CLASSES
RESOURCE_FIELD = re.compile(r"image|pixelart$|animation|timeline_path$|voice_path|^voice$|bgm$|sound$|story_movie|story_scenario_path|stage_node_background$|^dash_panel_directory$|^battle_(?:fixed_party|assist_character(?:_2)?)$")
SCRIPTED_CHARACTER_SOURCES = {"2", "4"}
ASSIST_MASTERS = {"assist_multiball", "assist_yakumono"}


# //// 读取输入与客户端字段索引 [@x380kkm 2026-09-20] ////
def arguments():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cdn-root", type=Path, required=True)
    parser.add_argument("--client-source-root", type=Path, required=True)
    parser.add_argument("--verified-storage-report", type=Path, required=True)
    parser.add_argument("--storage-delta-report", type=Path, action="append", default=[])
    parser.add_argument("--client-package", type=Path, action="append", default=[])
    parser.add_argument("--ios-asset-root", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args()


def source_fields(source: str):
    declared = {name for name, kind in re.findall(r"public var (\w+):(\w+);", source)
                if kind in {"String", "Option", "Array"} and name != "image_asset_rarity" and RESOURCE_FIELD.search(name)}
    fields = {}
    aliases = {}
    for number, line in enumerate(source.splitlines(), 1):
        local = re.search(r"(?:var )?(_loc\d+_)(?::[\w*]+)? = param1\[(\d+)\]", line)
        if local:
            aliases[local[1]] = int(local[2])
        assignment = re.search(r"^\s*(\w+) = (.+);", line)
        if assignment and assignment[1] in declared:
            expression = assignment[2]
            indexes = {int(index) for index in re.findall(r"param1\[(\d+)\]", expression)}
            indexes.update(aliases[alias] for alias in re.findall(r"_loc\d+_", expression) if alias in aliases)
            if len(indexes) == 1:
                fields[assignment[1]] = {"index": indexes.pop(), "line": number, "confidence": "unique"}
        nested = re.search(r'"(\w+)":\w+\.parseAt(\d+)\(param1\)', line)
        if nested and RESOURCE_FIELD.search(nested[1]):
            fields[nested[1]] = {"index": int(nested[2]), "line": number, "confidence": "unique"}
    if "class StoryCharacterValues" in source:
        for name, index in (("base_image_path", 4), ("face_image_path", 5)):
            line = next(i for i, value in enumerate(source.splitlines(), 1) if f"{name} = " in value)
            fields[name] = {"index": index, "line": line, "confidence": "unique"}
    for class_name, name, index in (("AssistCutin", "joining_voice_paths", 2), ("AssistMultiball", "skill_voice_paths", 13)):
        if f"class {class_name}Values" in source:
            line = next(i for i, value in enumerate(source.splitlines(), 1) if f"{name} = " in value)
            fields[name] = {"index": index, "line": line, "confidence": "unique"}
    if "class EquipmentEnhancementValues" in source:
        line = next(i for i, value in enumerate(source.splitlines(), 1) if "pixelart0 = " in value)
        fields["pixelart"] = {"index": 4, "line": line, "confidence": "unique"}
    if "class TownContentValues" in source:
        line = next(i for i, value in enumerate(source.splitlines(), 1) if " = param1[9];" in value)
        fields["particle_animation"] = {"index": 9, "line": line, "confidence": "unique"}
    if "class LoginBonusValues" in source:
        line = next(i for i, value in enumerate(source.splitlines(), 1) if "voice = " in value)
        fields["voice"] = {"index": 34, "line": line, "confidence": "unique"}
    if "class ActiveMissionEventValues" in source:
        line = next(i for i, value in enumerate(source.splitlines(), 1) if "home_banner_image = " in value)
        fields["home_banner_image"] = {"index": 7, "line": line, "confidence": "unique"}
    story = source.find("NormalQuestKind.Story({")
    if story >= 0:
        conditions = list(re.finditer(r'(_loc\d+_) = param1\[(\d+)\];\s*if\(\1 == "0"\)', source[:story]))
        if conditions:
            for name, field in fields.items():
                if name.startswith(("story_movie", "story_scenario")):
                    field["condition"] = {"index": int(conditions[-1][2]), "equals": "0"}
    return fields, sorted(declared - fields.keys())


def rows(value, prefix=""):
    if isinstance(value, dict):
        for key, child in value.items():
            yield from rows(child, f"{prefix}/{key}".lstrip("/"))
    elif isinstance(value, list) and all(isinstance(item, str) for item in value):
        yield prefix, value
    elif isinstance(value, list):
        for index, child in enumerate(value):
            yield from rows(child, f"{prefix}/{index}")
# //// /读取输入与客户端字段索引 ////


# //// 提取客户端声明的 master 并保留来源 [@x380kkm 2026-09-20] ////
def read_masters(args, catalog):
    specs = []
    records = {}
    for class_name in dict.fromkeys([*MASTER_CLASSES, *iter_battle_quest_master_classes(args.client_source_root)]):
        source_path = args.client_source_root / "pinball/master/generated" / f"{class_name}Values.as"
        name = re.sub(r"(?<!^)(?=[A-Z])", "_", class_name).lower()
        if not source_path.is_file():
            specs.append({"name": name, "source": str(source_path), "resolution": "unresolved_client_class", "logicalPaths": []})
            continue
        source = source_path.read_text(encoding="utf-8-sig")
        fields, unresolved = source_fields(source)
        candidates = [f"master/{directory}/{name}.orderedmap" for directory in MASTER_DIRECTORIES]
        matches = [(logical, catalog.find(logical)) for logical in candidates]
        matches = [(logical, record) for logical, record in matches if record is not None]
        spec = {"name": name, "source": str(source_path), "fields": fields, "unresolvedFields": unresolved,
                "resolution": "unique" if len(matches) == 1 else "ambiguous" if matches else "unresolved",
                "logicalPaths": [logical for logical, _ in matches]}
        specs.append(spec)
        if len(matches) == 1:
            records[matches[0][0]] = matches[0][1]
    raw, missing = read_logical_assets(args.cdn_root, records)
    if missing:
        raise ValueError(f"master entries are absent from declared CDN storage: {missing}")
    masters = {}
    for spec in specs:
        if spec["resolution"] != "unique":
            continue
        logical = spec["logicalPaths"][0]
        master = decode_multirow_ordered_map(raw[logical])
        masters[spec["name"]] = master
        spec["rowCount"] = sum(1 for _ in rows(master))
    return specs, masters
# //// /提取客户端声明的 master 并保留来源 ////


# //// 将引用按客户端加载类型展开 [@x380kkm 2026-09-20] ////
class References:
    def __init__(self):
        self.resources = {}
        self.members = {}
        self.unresolved = []
        self.dynamic = []
        self.issues = []

    def add(self, paths, owner, evidence, kind):
        paths = tuple(dict.fromkeys(paths))
        if not paths:
            return
        item = self.resources.setdefault(paths, {"logicalPath": paths[0], "alternativePaths": list(paths[1:]),
                                                 "owners": set(), "evidence": set(), "kind": kind})
        item["owners"].add(owner)
        item["evidence"].add(evidence)

    def image(self, value, owner, evidence):
        value = value.replace("\\", "/")
        namespace = next((prefix for prefix in ("quest/chapter", "quest/stage_node", "item", "item_icon")
                          if value.startswith(prefix + "/")), None)
        if "/sprite_sheet/" in value or namespace:
            sheet = value.split("/sprite_sheet/", 1)[0] + "/sprite_sheet" if "/sprite_sheet/" in value else namespace + "/sprite_sheet"
            member = value
            self.add([sheet + ".atlas.amf3.deflate"], owner, evidence, "atlas")
            self.add([sheet + ".png", sheet + ".atf.deflate"], owner, evidence, "texture")
            self.members.setdefault((sheet, member), set()).add(owner)
        elif "/" in value:
            self.add([value if value.endswith(".png") else value + ".png"], owner, evidence, "image")
        else:
            self.unresolved.append({"owner": owner, "field": evidence, "value": value, "reason": "image_identifier"})

    def animation_layout(self, value, owner, evidence):
        self.add([value + ".parts.amf3.deflate", value + ".frame.amf3.deflate"], owner, evidence, "animation")

    def field(self, name, value, owner, evidence):
        for part in value.split(","):
            part = part.strip()
            if part in IGNORED_VALUES:
                continue
            if name.endswith("_base_path"):
                self.dynamic.append({"owner": owner, "field": evidence, "prefix": part})
                continue
            if name == "story_scenario_path":
                self.add(["master/" + part.lstrip("/") + ".orderedmap"], owner, evidence, "scenario_master")
            elif name == "dash_panel_directory":
                for animation in [*(f"fixed_{angle}" for angle in range(0, 360, 45)), "smart"]:
                    logical = part + "/" + animation
                    self.animation_layout(logical, owner, evidence + ";DashPanelAnimations:constructor,resolvePathCollection")
                    self.add([logical + ".timeline.amf3.deflate"], owner, evidence + ";AssetPathCollectionBuilder.as:546", "timeline")
            elif name == "pixelart":
                self.image(part, owner, evidence)
            elif name in {"bgm", "voice", "voice_path", "sound"} or "voice_paths" in name or name.endswith("_bgm"):
                self.add([part if part.endswith(".mp3") else part + ".mp3"], owner, evidence, "audio")
            elif "story_movie" in name:
                if "/" not in part:
                    self.unresolved.append({"owner": owner, "field": evidence, "value": part, "reason": "movie_identifier"})
                    continue
                self.add([part + ".movie.amf3.deflate"], owner, evidence, "story_movie")
                if name == "story_movie_path":
                    sheet = part.rsplit("/", 1)[0] + "/sprite_sheet"
                    self.add([sheet + ".atlas.amf3.deflate"], owner, evidence, "atlas")
                    self.add([sheet + ".png", sheet + ".atf.deflate"], owner, evidence, "texture")
            elif name.endswith("timeline_path"):
                self.add([part + ".timeline.amf3.deflate"], owner, evidence, "timeline")
            elif "animation" in name or name == "stage_node_background":
                if name.endswith("_sequence_name") or "/" not in part:
                    self.dynamic.append({"owner": owner, "field": evidence, "sequence": part})
                    continue
                self.animation_layout(part, owner, evidence)
            else:
                self.image(part, owner, evidence)

    def document(self):
        return [{**item, "owners": sorted(item["owners"]), "evidence": sorted(item["evidence"])}
                for item in self.resources.values()]
# //// /将引用按客户端加载类型展开 ////


# //// 展开普通持有角色的页面和像素资源 [@x380kkm 2026-09-20] ////
def add_owned_character_references(references, key, row):
    owner = f"character:{key}"
    base = f"character/{row[0]}"
    evidence = "GeneralCharacterLogic.as:751;CharacterValues.as:string_id"
    for evolution in range(min(int(row[29]), 1) + 1):
        references.add([base + f"/ui/full_shot_1440_1920_{evolution}.png"], owner, evidence, "image")
        for suffix in ("thumb_party_main", "thumb_party_unison", "battle_member_status", "thumb_level_up", "battle_control_board"):
            references.add([base + f"/ui/{suffix}_{evolution}.png"], owner, "CharacterBaseImpl.as:102-165", "image")
        references.add([base + f"/ui/skill_cutin_{evolution}.atf.deflate"], owner, "CharacterBaseImpl.as:getSkillCutInImagePath;FileReader.as:isPlatformDependent", "texture")
        references.add([base + f"/ui/cutin_skill_chain_{evolution}.png"], owner, "CharacterBaseImpl.as:getSkillChainImagePath", "image")
    references.add([base + "/pixelart/pixelart.frame.amf3.deflate"], owner, evidence, "animation")
    references.add([base + "/pixelart/sprite_sheet.atlas.amf3.deflate"], owner, evidence, "atlas")
    references.add([base + "/pixelart/sprite_sheet.png", base + "/pixelart/sprite_sheet.atf.deflate"], owner, evidence, "texture")
    if int(row[2]) >= 4:
        references.add([base + "/pixelart/special.frame.amf3.deflate"], owner, "CharacterBaseImpl.as:getSpecialAnimationPath", "animation")
        references.add([base + "/pixelart/special_sprite_sheet.atlas.amf3.deflate"], owner, evidence, "atlas")
        references.add([base + "/pixelart/special_sprite_sheet.png", base + "/pixelart/special_sprite_sheet.atf.deflate"], owner, evidence, "texture")
# //// /展开普通持有角色的页面和像素资源 ////


# //// 按助战配置展开动画和显式图像语音 [@x380kkm 2026-09-20] ////
def add_assist_animation(references, owner, animation):
    evidence = "AssetPathCollectionBuilder.as:456;FileReader.as:482,688-706"
    folder = animation.rsplit("/", 1)[0]
    sheet = folder + ("/sprite_sheet" if animation.startswith("character/") else "/" + folder.rsplit("/", 1)[1])
    references.animation_layout(animation, owner, evidence)
    references.add([animation + ".timeline.amf3.deflate"], owner, evidence, "timeline")
    references.add([sheet + ".atlas.amf3.deflate"], owner, evidence, "atlas")
    references.add([sheet + ".png", sheet + ".atf.deflate"], owner, evidence, "texture")


def collect_assist_references(references, masters, characters):
    character_ids = {row[0]: key for key, row in characters.items()}
    definitions = []
    for name in sorted(ASSIST_MASTERS):
        for key, row in rows(masters.get(name, {})):
            owner = f"{name}:{key}"
            if name == "assist_multiball":
                animation = f"character/{row[0]}/pixelart/pixelart"
                cutin, voices = row[12], row[13]
                evidence = "AssistCharacterFactory.as:127-180;AssistMultiballValues.as:93-122"
            else:
                animation, cutin, voices = row[1], row[7], row[10]
                if row[5] in IGNORED_VALUES:
                    cutin, voices = "(None)", "(None)"
                evidence = "AssistYakumonoSource.as:125-162;AssistYakumonoValues.as:45-119"
            add_assist_animation(references, owner, animation)
            if cutin not in IGNORED_VALUES:
                references.add([cutin + ".atf.deflate", cutin + ".png"], owner, evidence, "texture")
            references.field("skill_voice_paths", voices, owner, evidence)
            character_id = character_ids.get(row[0])
            if name == "assist_multiball" and character_id is None:
                references.unresolved.append({"owner": owner, "value": row[0], "reason": "assist_character_not_in_master"})
            count = sum(owner in item["owners"] for item in references.resources.values())
            definitions.append({"owner": owner, "characterId": character_id, "stringId": row[0],
                                "scriptedCharacter": character_id is not None and characters[character_id][32] in SCRIPTED_CHARACTER_SOURCES,
                                "animationPath": animation, "cutinPath": cutin, "referenceGroupCount": count})
    return definitions
# //// /按助战配置展开动画和显式图像语音 ////


# //// 区分实际 master 引用和模板的普通持有用途 [@x380kkm 2026-09-20] ////
def collect_references(specs, masters):
    references = References()
    nonstandard = References()
    tables = {name: dict(rows(master)) for name, master in masters.items()}
    characters = tables.get("character", {})
    scripted_ids = {key for key, row in characters.items() if row[32] in SCRIPTED_CHARACTER_SOURCES}
    for key, row in characters.items():
        if row[0]:
            add_owned_character_references(nonstandard if key in scripted_ids else references, key, row)
    for spec in specs:
        if spec["name"] in ASSIST_MASTERS:
            continue
        master = masters.get(spec["name"])
        if master is None:
            continue
        for key, row in rows(master):
            owner = f"{spec['name']}:{key}"
            character_id = key.split("/", 1)[0]
            target = nonstandard if spec["name"] in {"character", "character_speech"} and character_id in scripted_ids else references
            if spec["name"] in {"voice_asset", "bgm_asset", "sound_effect_asset"}:
                target.add([key + ".mp3"], owner, spec["source"] + ":master_key", "audio")
            for field, location in spec["fields"].items():
                if field in BATTLE_QUEST_REFERENCE_FIELDS:
                    continue
                condition = location.get("condition")
                if condition and row[condition["index"]] != condition["equals"]:
                    continue
                if location["index"] >= len(row):
                    target.unresolved.append({"owner": owner, "field": field, "reason": "row_shorter_than_client_schema"})
                    continue
                value = row[location["index"]]
                evidence = f"{spec['source']}:{location['line']}:{field}:{location['index']}"
                if spec["name"] == "character_speech" and field == "voice_path" and value not in IGNORED_VALUES:
                    character = characters.get(key.split("/")[0])
                    if isinstance(character, list):
                        value = f"character/{character[0]}/voice/{value}"
                if spec["name"] == "multiball" and field == "image_prefix":
                    add_assist_animation(target, owner, f"character/{value}/pixelart/pixelart")
                    continue
                target.field(field, value, owner, evidence)
    definitions = collect_assist_references(references, masters, characters)
    collect_battle_character_references(references, specs, tables)
    return references, nonstandard, scripted_ids, definitions
# //// /区分实际 master 引用和模板的普通持有用途 ////


# //// 复用已验证实体并核对 bundle 注册与图集成员 [@x380kkm 2026-09-20] ////
def package_bundles(packages):
    registered = {}
    physical = {}
    for package in packages:
        with zipfile.ZipFile(package) as source:
            if package.suffix.lower() == ".apk":
                platform = "android"
                prefix = ""
                bundle = zipfile.ZipFile(io.BytesIO(source.read("assets/bundle.zip")))
            else:
                platform = "ios"
                roots = {name.split(".app/", 1)[0] + ".app/asset/" for name in source.namelist() if ".app/asset/" in name}
                if len(roots) != 1:
                    raise ValueError(f"IPA asset root is ambiguous: {package}")
                prefix = roots.pop()
                bundle = source
            registered[platform] = read_bundle_files(bundle, prefix)
            physical[platform] = {"/".join(name.split("/")[-2:]) for name in bundle.namelist()
                                  if name.startswith(prefix) and re.fullmatch(r"production/(?:[a-z_]+_)?bundle/[a-f0-9]{2}/[a-f0-9]{38}", name.removeprefix(prefix))}
            if bundle is not source:
                bundle.close()
    return registered, physical


def coverage(args, references, catalogs):
    bundles, physical = package_bundles(args.client_package)
    if args.ios_asset_root:
        files = set()
        registered = set()
        for family in ("bundle", "ios_bundle"):
            for kind in ("amf", "encrypt", "copy", "compress", "png", "atf"):
                entry = logical_entries(f"{family}_{kind}.filelist")[0].replace("/upload/", "/bundle/")
                path = args.ios_asset_root / entry
                if path.is_file():
                    registered.update(path.read_text(encoding="utf-8-sig").splitlines())
            root = args.ios_asset_root / "production" / family
            if root.is_dir():
                files.update(f"{path.parent.name}/{path.name}" for path in root.glob("*/*") if path.is_file())
        bundles["ios"] = files & registered
        physical["ios"] = files
    missing = []
    matched = Counter()
    bundled = Counter()
    for item in references.document():
        paths = [item["logicalPath"], *item["alternativePaths"]]
        suffixes = {"/".join(entry.split("/")[-2:]) for path in paths for entry in logical_entries(path)}
        for platform, catalog in catalogs.items():
            if any(catalog.find(path) for path in paths):
                matched[platform] += 1
            elif suffixes & bundles.get(platform, set()):
                matched[platform] += 1
                bundled[platform] += 1
            else:
                missing.append({**item, "platform": platform,
                                "availability": "bundle_file_not_registered" if suffixes & physical.get(platform, set()) else "absent_from_cdn_and_bundle",
                                "confidence": "unique" if platform in bundles else "unresolved_bundle_not_provided"})
    atlas_records = {}
    for sheet, _ in references.members:
        logical = sheet + ".atlas.amf3.deflate"
        record = catalogs["ios"].find(logical)
        if record:
            atlas_records[logical] = record
    atlas_bytes, _ = read_logical_assets(args.cdn_root, atlas_records)
    indexes = {path.removesuffix(".atlas.amf3.deflate"): _atlas_index(_decode_atlas(data)) for path, data in atlas_bytes.items()}
    member_missing = []
    for (sheet, member), owners in references.members.items():
        if sheet in indexes and member not in indexes[sheet]:
            member_missing.append({"atlas": sheet, "member": member, "owners": sorted(owners)})
    return {"matchedByPlatform": dict(matched), "bundledByPlatform": dict(bundled),
            "missing": missing, "atlasMemberCount": len(references.members), "missingAtlasMembers": member_missing}
# //// /复用已验证实体并核对 bundle 注册与图集成员 ////


# //// 关联基线内容校验和增量资源证据 [@x380kkm 2026-09-20] ////
def storage_evidence(args, version):
    verified = read_json(args.verified_storage_report)
    if any(data["missing"] for data in verified["platforms"].values()):
        raise ValueError("the storage report contains unresolved entity content")
    evidence = {"baselineReport": str(args.verified_storage_report), "baselineAssetVersion": verified["targetAssetVersion"],
                "currentCheck": "declared_reference_presence", "deltaReports": []}
    deltas = [(path, read_json(path)) for path in args.storage_delta_report]
    reached = verified["targetAssetVersion"]
    while reached != version:
        matches = [(path, delta) for path, delta in deltas if delta.get("original_version") == reached]
        if len(matches) != 1:
            raise ValueError(f"the storage evidence has no unique delta from {reached} to {version}")
        path, delta = matches[0]
        if delta.get("status") != "installed":
            raise ValueError(f"the resource delta is not installed: {path}")
        source = delta.get("source")
        if source == "current-cn-medium-resolution-derivative":
            if (delta.get("missing_medium_entries") or delta.get("decode_failures")
                    or delta.get("generated_count") != delta.get("medium_count")):
                raise ValueError(f"the Small resource delta contains unresolved images: {path}")
        elif source != "exact-regional-startup-images":
            raise ValueError(f"the resource delta has no supported content evidence: {path}")
        reached = delta["target_version"]
        evidence["deltaReports"].append(str(path))
        deltas.remove((path, delta))
    return evidence
# //// /关联基线内容校验和增量资源证据 ////


# //// 汇总每个用途的资源缺口和角色来源 [@x380kkm 2026-09-20] ////
def reference_report(args, references, catalogs, characters):
    result = coverage(args, references, catalogs)
    result["missing"].extend(references.issues)
    missing_ids = {owner.split(":", 1)[1].split("/")[0] for item in result["missing"] for owner in item["owners"]
                   if owner.split(":", 1)[0] in {"character", "character_speech"}}
    character_details = {key: {"stringId": row[0], "identityCharacterId": row[27], "maxEvolutionLevel": row[29],
                               "omitCharacterQuest": row[30], "webSitePublish": row[31], "obtainSource": row[32]}
                         for key, row in rows(characters) if key in missing_ids}
    return {"referenceCount": len(references.resources), "references": references.document(),
            "unresolvedReferences": references.unresolved, "dynamicPrefixReferences": references.dynamic,
            "unmatchedOwnerMasterRows": character_details, **result}
# //// /汇总每个用途的资源缺口和角色来源 ////


# //// 输出带源码证据的资源审计 [@x380kkm 2026-09-20] ////
def main():
    args = arguments()
    args.cdn_root = args.cdn_root.resolve(strict=True)
    version = read_json(args.cdn_root / "path")["info"]["target_asset_version"]
    evidence = storage_evidence(args, version)
    catalogs = {platform: EntityCatalog.load(args.cdn_root, args.cdn_root / "entities" / f"10939-{platform}_medium.csv")
                for platform in ("ios", "android")}
    specs, masters = read_masters(args, catalogs["ios"])
    references, nonstandard, scripted_ids, definitions = collect_references(specs, masters)
    startup = add_startup_references(references, args.client_source_root, masters)
    result = reference_report(args, references, catalogs, masters["character"])
    potential = reference_report(args, nonstandard, catalogs, masters["character"])
    affected_ids = set(potential["unmatchedOwnerMasterRows"])
    affected_assists = [item for item in definitions if item["characterId"] in affected_ids]
    report = {"scope": {"cdnRoot": str(args.cdn_root), "targetAssetVersion": version,
                         "storageEvidence": evidence,
                         "masterClasses": [Path(spec["source"]).stem.removesuffix("Values") for spec in specs],
                         "mainReferenceScope": "obtainable_character_views_explicit_master_assist_and_startup_resources",
                         "characterClassificationEvidence": "ObtainSourceKindTools.as:9-16;GlobalLogic.as:1136-1151",
                         "clientPackages": [str(package) for package in args.client_package]},
              "masters": specs, "startupResources": startup, **result,
              "scriptedAssistUsage": {"definitionCount": len(definitions),
                                      "referenceGroupCount": sum(item["referenceGroupCount"] for item in definitions),
                                      "definitions": definitions,
                                      "templatesWithPotentialGaps": {"characterIds": sorted(affected_ids),
                                                                    "definitionCount": len(affected_assists),
                                                                    "referenceGroupCount": sum(item["referenceGroupCount"] for item in affected_assists)}},
              "nonstandardTemplateUsage": {"usage": "scripted_characters_imported_or_constructed_as_owned_characters",
                                           "characterIds": sorted(scripted_ids), **potential}}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"output": str(args.output), "masters": len(masters), "references": report["referenceCount"],
                      "missingByKind": dict(Counter(item["kind"] for item in result["missing"])),
                      "unresolved": len(references.unresolved), "missingAtlasMembers": len(result["missingAtlasMembers"]),
                      "nonstandardTemplateMissingByKind": dict(Counter(item["kind"] for item in potential["missing"])),
                      "affectedTemplateAssistDefinitions": len(affected_assists),
                      "affectedTemplateAssistReferenceGroups": sum(item["referenceGroupCount"] for item in affected_assists)}, ensure_ascii=False))


if __name__ == "__main__":
    main()
# //// /输出带源码证据的资源审计 ////
