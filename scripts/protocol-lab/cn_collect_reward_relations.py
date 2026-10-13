# audience: internal
# # cn-collect-reward-relations
# 同系列收集活动使用相同关卡范围, 奖励关系按完整门槛和奖励结构匹配复刻币.

from copy import deepcopy

COLLECT_RELATION_TEMPLATES = {
    "20025": {"source_event": "20000", "source_item": 70014, "target_item": 70048,
              "source_relation_start": 901, "target_relation_start": 923, "relation_count": 22},
}


# //// 展开门槛奖励并映射同系列活动币 [@x380kkm 2026-09-29] ////
def expanded_relation(relation_id: int, relations: dict, groups: dict, item_ids: dict) -> list[dict]:
    expanded = deepcopy(relations[str(relation_id)])
    for threshold in expanded:
        threshold["rewards"] = deepcopy(groups[str(threshold.pop("group_id"))])
        for entry in threshold["rewards"]:
            reward = entry["reward"]
            if reward["type"] == 0:
                reward["id"] = item_ids.get(reward["id"], reward["id"])
    return expanded


# //// 生成同系列活动的客户端关卡范围 [@x380kkm 2026-09-29] ////
def complete_collect_quest_relations(events: dict, quest_relations: dict, relations: dict, groups: dict) -> dict:
    result = deepcopy(quest_relations)
    for event_id, template in COLLECT_RELATION_TEMPLATES.items():
        if event_id not in events or result.get(event_id):
            continue
        source_id = template["source_event"]
        source, target = events[source_id], events[event_id]
        if source[1] != target[1] or source[7] != target[7]:
            raise ValueError(f"收集活动的名称或图标不同: {source_id}:{event_id}")
        if int(source[6]) != template["source_item"] or int(target[6]) != template["target_item"]:
            raise ValueError(f"收集活动币与目录不同: {source_id}:{event_id}")
        item_ids = {template["source_item"]: template["target_item"]}
        relation_ids = {}
        for offset in range(template["relation_count"]):
            source_relation = template["source_relation_start"] + offset
            target_relation = template["target_relation_start"] + offset
            if expanded_relation(source_relation, relations, groups, item_ids) != expanded_relation(target_relation, relations, groups, {}):
                raise ValueError(f"收集活动奖励结构不同: {source_relation}:{target_relation}")
            relation_ids[source_relation] = target_relation
        copied = deepcopy(result[source_id])
        for position, row in copied.items():
            row[0] = str(int(event_id) * 1_000 + int(position))
            row[5] = str(relation_ids[int(row[5])])
        result[event_id] = copied
    return result
