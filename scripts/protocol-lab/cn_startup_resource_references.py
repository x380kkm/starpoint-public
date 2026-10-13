# audience: internal
# # cn-startup-resource-references
# 主页资源包含本地未读公告, 城镇布景以及常驻 UI, 字体和动画.
# 字面资源来自指定客户端源码, master 字段由调用方按同一客户端声明解析.

from __future__ import annotations

import re


STARTUP_MASTER_CLASSES = (
    "FeatureAnnouncement", "FeatureGuideDialog", "TownContent", "LoginBonus", "PremiumLoginBonus",
)
CALL_PATTERN = re.compile(
    r"(?P<method>addAnimationLayouts?|addSpriteSheets?|addUis?|addImages?|addBgm|addSoundEffect|"
    r"bitmapFont\.addAll|timeline\.addAll|timeline\.add|html\.addAll|css\.addAll)"
    r"\((?P<argument>\[[^\]]*\]|\"[^\"]*\")\)", re.DOTALL,
)


# //// 展开主页加载器的字面资源 [@x380kkm 2026-09-20] ////
def add_sprite_sheet(references, path, owner, evidence):
    references.add([path + ".atlas.amf3.deflate"], owner, evidence, "atlas")
    references.add([path + ".png"], owner, evidence, "texture")


def add_animation(references, path, owner, evidence):
    references.add([path + ".parts.amf3.deflate", path + ".frame.amf3.deflate"], owner, evidence, "animation")
    references.add([path + ".timeline.amf3.deflate"], owner, evidence, "timeline")
    if path.startswith("character/"):
        suffix = "special_sprite_sheet" if path.endswith("/special") else "sprite_sheet"
        add_sprite_sheet(references, path.rsplit("/", 1)[0] + "/" + suffix, owner, evidence)
    elif path.startswith("battle/") and not path.startswith("battle/common"):
        folder = path.rsplit("/", 1)[0]
        add_sprite_sheet(references, folder + "/" + folder.rsplit("/", 1)[-1], owner, evidence)


def add_literal_reference(references, method, path, owner, evidence):
    if method.startswith("addAnimationLayout"):
        add_animation(references, path, owner, evidence)
    elif method.startswith("addSpriteSheet"):
        add_sprite_sheet(references, path, owner, evidence)
    elif method.startswith("addUi"):
        references.add([path + ".ui.amf3.deflate"], owner, evidence, "ui")
    elif method.startswith("addImage"):
        references.image(path, owner, evidence)
    elif method == "bitmapFont.addAll":
        sheet = "scene/general/sprite_sheet"
        add_sprite_sheet(references, sheet, owner, evidence)
        references.members.setdefault((sheet, path), set()).add(owner)
        references.add([path + ".xml.deflate"], owner, evidence, "font")
    elif method.startswith("timeline."):
        references.add([path + ".timeline.amf3.deflate"], owner, evidence, "timeline")
    elif method in {"addBgm", "addSoundEffect"}:
        references.add([path + ".mp3"], owner, evidence, "audio")
    else:
        extension = "html" if method == "html.addAll" else "css"
        references.add([path + "." + extension + ".deflate"], owner, evidence, extension)


def collect_source_references(references, source_path, case_index=None):
    if not source_path.is_file():
        return {"source": str(source_path), "confidence": "unresolved", "reason": "client_source_missing"}
    source = source_path.read_text(encoding="utf-8-sig")
    offset = 0
    if case_index is not None:
        match = re.search(rf"case {case_index}:(.*?)case {case_index + 1}:", source, re.DOTALL)
        if match is None:
            return {"source": str(source_path), "confidence": "unresolved", "reason": "asset_group_case_missing"}
        offset = source[:match.start(1)].count("\n")
        source = match[1]
    owner = f"startup:{source_path.stem}"
    count = 0
    for call in CALL_PATTERN.finditer(source):
        line = offset + source[:call.start()].count("\n") + 1
        for path in re.findall(r'"([^"\n]+)"', call["argument"]):
            add_literal_reference(references, call["method"], path, owner, f"{source_path}:{line}")
            count += 1
    return {"source": str(source_path), "confidence": "unique", "literalReferenceCount": count}
# //// /展开主页加载器的字面资源 ////


# //// 关联首次主页资源与客户端加载条件 [@x380kkm 2026-09-20] ////
def add_startup_references(references, source_root, masters):
    sources = [collect_source_references(references, source_root / path, case_index) for path, case_index in (
        ("pinball/asset/AssetResolver.as", 4),
        ("pinball/asset/builder/GlobalAssetPathCollectionBuilder.as", None),
        ("pinball/asset/builder/HomeAssetPathCollectionBuilder.as", None),
    )]
    for key, row in masters.get("town_content", {}).items():
        animation = row[9]
        if animation in {"", "(None)"}:
            continue
        owner = f"town_content:{key}"
        evidence = "TownAssetPathCollectionBuilder.as:leadingLogicLoadedHandler"
        add_animation(references, animation, owner, evidence)
        if animation.startswith("town/particle/"):
            add_sprite_sheet(references, animation.rsplit("/", 1)[0] + "/sprite_sheet", owner, evidence)
    references.add(["dynamic/error/not_found.png"], "startup:dynamic_image_fallback",
                   "FileReader.as:failedReadAssetFileHandler", "image")
    return {
        "sources": sources,
        "masterClasses": list(STARTUP_MASTER_CLASSES),
        "announcementCondition": "current_master_time_and_uncompleted_local_feature_announcement",
        "loginBonusCondition": "load.login_bonus_received_at_has_value",
        "dynamicImageFallback": "dynamic/error/not_found.png",
    }
# //// /关联首次主页资源与客户端加载条件 ////
