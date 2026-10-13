# audience: internal
# # cn-battle-character-resource-references
# 固定编组的角色资源来自关卡引用的队伍槽位, 剧情助战来自关卡引用的助战配置.
# 动画格式遵循客户端 parts 与 frame 候选顺序, 结算的 special 动画按角色稀有度选择.
# 固定角色的图像状态来自实际节点, 战斗头像按关卡显示权限和主副槽位选择.

from pathlib import Path


BATTLE_QUEST_REFERENCE_FIELDS = {"battle_fixed_party", "battle_assist_character", "battle_assist_character_2"}
FIXED_CHARACTER_COLUMNS = (2, 9, 16, 23, 30, 37)
FIXED_PARTY_NORMAL_SINGLE_QUESTS = {"main_quest", "story_event_single_quest", "world_story_event_quest"}
EMPTY_REFERENCE_VALUES = {"", "(None)", "None", "null"}


# //// 从客户端声明选择含编组或助战引用的关卡主表 [@x380kkm 2026-09-28] ////
def iter_battle_quest_master_classes(source_root: Path):
    for path in sorted((source_root / "pinball/master/generated").glob("*QuestValues.as")):
        source = path.read_text(encoding="utf-8-sig")
        if any(field in source for field in BATTLE_QUEST_REFERENCE_FIELDS):
            yield path.stem.removesuffix("Values")


# //// 记录具体主表引用缺口并保留关卡来源 [@x380kkm 2026-09-28] ////
def record_missing_reference(references, master, key, owner, evidence):
    references.issues.append({"kind": "missing_battle_master_reference", "master": master,
                              "record": key, "owners": [owner], "evidence": [evidence]})


# //// 按固定角色实际启用的节点计算图像进化状态 [@x380kkm 2026-09-28] ////
def fixed_character_evolution(references, character_id, node_ids, owner, evidence, mana_nodes):
    flags = 0
    for value in node_ids.split(","):
        if value in EMPTY_REFERENCE_VALUES:
            continue
        node_id = int(value)
        if node_id // 2000 != int(character_id):
            continue
        key = f"{character_id}/{node_id // 200 % 10}/{node_id % 200}"
        node = mana_nodes.get(key)
        if node is None:
            record_missing_reference(references, "mana_node", key, owner, evidence)
            return None
        if node[1] != "0":
            continue
        if node[5] in {"1", "2"}:
            flags |= 1
        elif node[5] == "0":
            flags |= {"1": 2, "2": 4, "3": 8}.get(node[6], 0)
    return int(flags & 15 == 15)


# //// 展开固定角色在结算和普通单人战斗使用的图像 [@x380kkm 2026-09-28] ////
def add_fixed_character_images(references, character, evolution, battle_role, quest_name, owner, evidence):
    base = f"character/{character[0]}/ui/"
    source = evidence + ";FixedCharacterTools.as:50-101;AssetResolver.as:680"
    references.add([base + f"thumb_level_up_{evolution}.png"], owner, source, "image")
    if battle_role is None:
        return
    if quest_name not in FIXED_PARTY_NORMAL_SINGLE_QUESTS:
        references.unresolved.append({"owner": owner, "value": quest_name, "reason": "fixed_party_battle_ui_permissions"})
        return
    source = evidence + ";BattleFeaturePermitTools.as:56-73;BattleQuestBaseImpl.as:getFeatureValues;BattleCharacterLogic.as:206-298"
    if battle_role == "main":
        for image in ("cutin_skill_chain", "battle_member_status", "battle_control_board", "square"):
            references.add([base + f"{image}_{evolution}.png"], owner, source, "image")
    references.add([base + f"skill_cutin_{evolution}.atf.deflate"], owner, source, "texture")


# //// 展开固定编组角色在战斗和结算使用的资源 [@x380kkm 2026-09-28] ////
def add_fixed_party_resources(references, party_id, quest_name, owner, evidence, tables):
    party = tables.get("fixed_party", {}).get(party_id)
    if party is None:
        record_missing_reference(references, "fixed_party", party_id, owner, evidence)
        return
    for column in FIXED_CHARACTER_COLUMNS:
        if column + 6 >= len(party):
            record_missing_reference(references, "fixed_party_character_slot", f"{party_id}/{column}", owner, evidence)
            continue
        character_id = party[column]
        if character_id in EMPTY_REFERENCE_VALUES:
            continue
        character = tables.get("character", {}).get(character_id)
        if character is None:
            record_missing_reference(references, "character", character_id, owner, evidence)
            continue
        base = f"character/{character[0]}/pixelart/"
        source = evidence + f";FixedPartyValues.as:column:{column};AssetResolver.as:675-678;AssetPathCollectionBuilder.as:477-546"
        animations = ["pixelart"]
        if int(character[2]) >= 4:
            animations.append("special")
        for animation in animations:
            path = base + animation
            references.animation_layout(path, owner, source)
            references.add([path + ".timeline.amf3.deflate"], owner, source, "timeline")
            sheet = base + ("special_sprite_sheet" if animation == "special" else "sprite_sheet")
            references.add([sheet + ".atlas.amf3.deflate"], owner, source, "atlas")
            references.add([sheet + ".png", sheet + ".atf.deflate"], owner, source, "texture")
        evolution = fixed_character_evolution(references, character_id, party[column + 6], owner,
                                               source, tables.get("mana_node", {}))
        if evolution is not None:
            battle_role = "main" if column < 23 else "unison" if party[column - 21] not in EMPTY_REFERENCE_VALUES else None
            add_fixed_character_images(references, character, evolution, battle_role, quest_name, owner, source)


# //// 核对关卡的固定编组与剧情助战配置引用 [@x380kkm 2026-09-28] ////
def collect_battle_character_references(references, specs, tables):
    for spec in specs:
        fields = {name: value for name, value in spec.get("fields", {}).items() if name in BATTLE_QUEST_REFERENCE_FIELDS}
        for key, row in tables.get(spec["name"], {}).items():
            owner = f"{spec['name']}:{key}"
            for field, location in fields.items():
                column = location["index"]
                evidence = f"{spec['source']}:{location['line']}:{field}:{column}"
                if column >= len(row):
                    record_missing_reference(references, spec["name"], f"{key}/{column}", owner, evidence)
                    continue
                if row[column] in EMPTY_REFERENCE_VALUES:
                    continue
                if field == "battle_fixed_party":
                    add_fixed_party_resources(references, row[column], spec["name"], owner, evidence, tables)
                    continue
                reference_column = column + 1 if field == "battle_assist_character" else column
                if reference_column >= len(row):
                    record_missing_reference(references, spec["name"], f"{key}/{reference_column}", owner, evidence)
                elif row[reference_column] not in tables.get("assist_multiball", {}):
                    record_missing_reference(references, "assist_multiball", row[reference_column], owner, evidence)
