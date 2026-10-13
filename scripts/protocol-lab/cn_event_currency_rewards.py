# audience: internal
# # cn-event-currency-rewards
# 活动货币按本期商店成本和物品名称、图标、类别对应. 专用奖励组保留原始数量、位置、权重和概率.

from copy import deepcopy


QUEST_MASTER = "master/quest/event/advent_event_quest.orderedmap"
SCORE_MASTER = "master/reward/score_reward.orderedmap"
RARE_MASTER = "master/reward/rare_score_reward.orderedmap"
SCORE_GROUP_COLUMN = 76
CURRENCY_IDENTITY_FIELDS = ("name", "thumbnail_id", "effect_kind", "category", "group")


# //// 从活动完整掉落集合匹配本期商店的货币 [@x380kkm 2026-09-29] ////
def currency_identity(item):
    if item.get("kind") != "exchange" or not item.get("name") or not item.get("thumbnail_id"):
        return None
    return tuple(item.get(field) for field in CURRENCY_IDENTITY_FIELDS)


def score_item_ids(group, rare_groups):
    for row in group.values():
        if row[1] == "0" and row[2] == "0":
            yield row[3]
        elif row[1] == "1":
            for rare in rare_groups[row[6]].values():
                if rare[1] == "0":
                    yield rare[2]


def activity_currency_map(quests, score_groups, rare_groups, shop, items):
    costs = {str(cost["id"]) for product in shop.values() for cost in product["costs"]
             if isinstance(cost["id"], int)}
    targets = {}
    for item_id in costs:
        identity = currency_identity(items[item_id])
        if identity is not None:
            targets.setdefault(identity, set()).add(item_id)
    groups = {row[SCORE_GROUP_COLUMN] for row in quests.values()
              if row[SCORE_GROUP_COLUMN] not in {"", "(None)"}}
    mapping = {}
    for item_id in {item_id for group in groups for item_id in score_item_ids(score_groups[group], rare_groups)}:
        if item_id in costs:
            continue
        candidates = targets.get(currency_identity(items.get(item_id, {})), set())
        if len(candidates) > 1:
            raise ValueError(f"Activity shop has multiple matching currencies: {item_id}:{sorted(candidates)}")
        if candidates:
            mapping[item_id] = next(iter(candidates))
    return dict(sorted(mapping.items(), key=lambda entry: int(entry[0])))
# //// /从活动完整掉落集合匹配本期商店的货币 ////


# //// 为单个活动复制受影响的普通和稀有奖励组 [@x380kkm 2026-09-29] ////
def clone_currency_groups(quests, score_groups, rare_groups, currency_map, score_origins, rare_origins):
    score_ids, rare_ids = {}, {}
    next_score = max(map(int, score_groups)) + 1
    next_rare = max(map(int, rare_groups)) + 1
    for group_id in sorted({row[SCORE_GROUP_COLUMN] for row in quests.values()
                            if row[SCORE_GROUP_COLUMN] not in {"", "(None)"}}, key=int):
        changed = deepcopy(score_groups[group_id])
        for row in changed.values():
            if row[1] == "0" and row[2] == "0":
                row[3] = currency_map.get(row[3], row[3])
            elif row[1] == "1":
                original_rare = row[6]
                if original_rare not in rare_ids:
                    rare = deepcopy(rare_groups[original_rare])
                    for reward in rare.values():
                        if reward[1] == "0":
                            reward[2] = currency_map.get(reward[2], reward[2])
                    if rare != rare_groups[original_rare]:
                        rare_id = str(next_rare)
                        next_rare += 1
                        rare_groups[rare_id] = rare
                        rare_ids[original_rare] = rare_id
                        rare_origins[rare_id] = original_rare
                    else:
                        rare_ids[original_rare] = original_rare
                row[6] = rare_ids[original_rare]
        if changed != score_groups[group_id]:
            score_id = str(next_score)
            next_score += 1
            score_groups[score_id] = changed
            score_ids[group_id] = score_id
            score_origins[score_id] = group_id
    for row in quests.values():
        row[SCORE_GROUP_COLUMN] = score_ids.get(row[SCORE_GROUP_COLUMN], row[SCORE_GROUP_COLUMN])
# //// /为单个活动复制受影响的普通和稀有奖励组 ////


# //// 投影活动专用货币并保留未关联活动的奖励组 [@x380kkm 2026-09-29] ////
def project_event_currencies(masters, item_catalog, shops):
    items = {str(item["id"]): item for item in item_catalog["items"]}
    quests = deepcopy(masters[QUEST_MASTER])
    scores, rares = deepcopy(masters[SCORE_MASTER]), deepcopy(masters[RARE_MASTER])
    score_origins, rare_origins, events = {}, {}, {}
    for event_id, event_quests in sorted(quests.items(), key=lambda entry: int(entry[0])):
        currency_map = activity_currency_map(event_quests, scores, rares, shops.get("0", {}).get(event_id, {}), items)
        if not currency_map:
            continue
        clone_currency_groups(event_quests, scores, rares, currency_map, score_origins, rare_origins)
        events[event_id] = currency_map
    quest_groups = {row[0]: int(row[SCORE_GROUP_COLUMN]) for event in quests.values() for row in event.values()
                    if row[SCORE_GROUP_COLUMN] in score_origins}
    return {"masters": {QUEST_MASTER: quests, SCORE_MASTER: scores, RARE_MASTER: rares},
            "quest_score_groups": quest_groups, "events": events,
            "score_group_origins": score_origins, "rare_group_origins": rare_origins}
# //// /投影活动专用货币并保留未关联活动的奖励组 ////
