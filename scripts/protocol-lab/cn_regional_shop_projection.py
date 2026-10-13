# audience: internal
# # cn-regional-shop-projection
# 区域日程使用 CN 商品和服务目录, 商品身份关联原始兑换日期, CN 追加商品沿用同组商品的末段上架期.

from collections import Counter, defaultdict


SHOP_PREFIXES = {
    "0": "advent:", "1": "ranking:", "2": "story:", "4": "challenge-dungeon:",
    "6": "world-story:", "7": "tower-dungeon:", "8": "expert-single:",
    "9": "collect-item:", "10": "carnival:", "11": "rush:", "12": "score-attack:",
}
TEXT_FIELDS = {6, 7, 11}
TIME_FIELDS = {26, 27}
QUANTITY_FIELDS = {16, 19, 21, 23, 25, 29, 30, 31, *range(34, 50, 3)}


def shop_rows(document, activity_id):
    return {key: row for key, row in document.items()
            if SHOP_PREFIXES.get(row[2], "") + row[1] == activity_id}


def item_identity(row):
    return tuple(value for index, value in enumerate(row)
                 if index not in TEXT_FIELDS | TIME_FIELDS)


def field_differences(left, right):
    return {str(index): {"cn": left[index], "regional": right[index]}
            for index in range(min(len(left), len(right)))
            if index not in TEXT_FIELDS | TIME_FIELDS and left[index] != right[index]}


# //// 从完整商品身份和交易内容识别区域商店对应的本地活动 [@x380kkm 2026-09-29] ////
def trade_identity(row):
    return tuple(row[15:17] + row[18:26] + row[32:50])


def canonical_shop(activity_id, local, regional_rows):
    direct = shop_rows(local, activity_id)
    if direct or not regional_rows:
        return activity_id, direct
    targets = {item_id: local[item_id] for item_id in regional_rows if item_id in local}
    if targets.keys() != regional_rows.keys():
        return activity_id, direct
    owners = {SHOP_PREFIXES.get(row[2], "") + row[1] for row in targets.values()}
    if len(owners) != 1:
        return activity_id, direct
    canonical_id = next(iter(owners))
    canonical = shop_rows(local, canonical_id)
    if canonical.keys() != regional_rows.keys() or any(
        canonical[item_id][2] != source[2]
        or trade_identity(canonical[item_id]) != trade_identity(source)
        for item_id, source in regional_rows.items()
    ):
        return activity_id, direct
    return canonical_id, canonical
# //// /从完整商品身份和交易内容识别区域商店对应的本地活动 ////


# //// 为同组 CN 追加商品选择已核实商品的末段上架期 [@x380kkm 2026-09-29] ////
def additional_item_window(row, local_rows, mappings, regional_rows, parse_period):
    original_window = parse_period(row, 26, 27)
    source_windows = {parse_period(source, 26, 27) for source in regional_rows.values()}
    if original_window is None or not source_windows or None in source_windows:
        return None
    if max(start for start, _ in source_windows) >= min(end for _, end in source_windows):
        return None
    source_ids = {source_id for item_id, mapping in mappings.items()
                  if parse_period(local_rows[item_id], 26, 27) == original_window
                  for source_id in mapping["regional_item_ids"]}
    if not source_ids:
        return None
    periods = {parse_period(regional_rows[source_id], 26, 27) for source_id in source_ids}
    return sorted(source_ids), (max(start for start, _ in periods), max(end for _, end in periods))
# //// /为同组 CN 追加商品选择已核实商品的末段上架期 ////


# //// 核对 CN 商品的客户端和服务端定位 [@x380kkm 2026-09-28] ////
def target_issues(item_id, row, server, item_map, references):
    issues = []
    for index in range(18, 26, 2):
        if row[index] not in {"", "(None)"} and row[index] not in references[0]:
            issues.append({"kind": "missing_cn_cost_item", "item_id": item_id,
                           "field_index": index, "target_id": row[index]})
    for index in range(32, 50, 3):
        if row[index] in {"", "(None)"}:
            continue
        kind = int(row[index])
        if kind not in {0, 1, 2, 3, 4} or kind in references and row[index + 1] not in references[kind]:
            issues.append({"kind": "missing_cn_reward_target", "item_id": item_id,
                           "field_index": index, "reward_type": kind, "target_id": row[index + 1]})
    target = server.get(row[2], {}).get(row[1], {}).get(item_id)
    if target is None:
        issues.append({"kind": "missing_server_item", "item_id": item_id})
        return issues
    expected = {"eventType": int(row[2]), "eventId": int(row[1])}
    if item_map.get(item_id) != expected:
        issues.append({"kind": "conflicting_server_item_owner", "item_id": item_id,
                       "expected": expected, "actual": item_map.get(item_id)})
    return issues
# //// /核对 CN 商品的客户端和服务端定位 ////


# //// 按商品身份和上架阶段投影完整 CN 商品集合 [@x380kkm 2026-09-29] ////
def project_shop(activity_id, local, regional, server, item_map, references, parse_period):
    right = shop_rows(regional, activity_id)
    canonical_id, left = canonical_shop(activity_id, local, right)
    candidates = defaultdict(list)
    for item_id, row in right.items():
        candidates[item_identity(row)].append(item_id)
    source_windows = {parse_period(row, 26, 27) for row in right.values()}
    changes, mappings, windows, issues, additional = {}, {}, {}, [], {}
    if right and not left:
        issues.append({"kind": "missing_cn_shop", "regional_item_ids": sorted(right)})
    for item_id, row in left.items():
        issues.extend(target_issues(item_id, row, server, item_map, references))
        if item_id in right:
            source_ids, method = [item_id], "same_item_id"
            changed = field_differences(row, right[item_id])
            if changed:
                changes[item_id] = changed
        elif candidates.get(item_identity(row)):
            source_ids, method = candidates[item_identity(row)], "matching_cn_contract"
        elif len(source_windows) == 1 and None not in source_windows:
            source_ids, method = [], "uniform_shop_window"
        else:
            additional[item_id] = row
            continue
        periods = {parse_period(right[source], 26, 27) for source in source_ids} if source_ids else source_windows
        if None in periods or len(periods) != 1:
            issues.append({"kind": "ambiguous_item_window", "item_id": item_id,
                           "regional_item_ids": source_ids})
            continue
        window = next(iter(periods))
        windows[item_id] = window
        mappings[item_id] = {"method": method, "regional_item_ids": sorted(source_ids),
                             "start_at_ms": window[0], "end_at_ms": window[1]}
    for item_id, row in additional.items():
        result = additional_item_window(row, left, mappings, right, parse_period)
        if result is None:
            issues.append({"kind": "unresolved_item_window", "item_id": item_id})
            continue
        source_ids, window = result
        windows[item_id] = window
        mappings[item_id] = {"method": "cn_addition_final_shop_phase", "regional_item_ids": source_ids,
                             "start_at_ms": window[0], "end_at_ms": window[1]}
    changed_fields = {int(index) for fields in changes.values() for index in fields}
    if issues:
        category = "reference_or_window_conflict"
    elif canonical_id != activity_id:
        category = "canonical_activity_alias"
    elif left.keys() != right.keys():
        category = "different_item_ids_with_cn_targets"
    elif not changed_fields:
        category = "time_only"
    elif changed_fields <= QUANTITY_FIELDS:
        category = "price_or_quantity_only"
    else:
        category = "cn_currency_or_reward_variant"
    report = {
        "category": category, "canonical_activity_id": canonical_id,
        "cn_item_count": len(left), "regional_item_count": len(right),
        "cn_only_item_ids": sorted(left.keys() - right.keys()),
        "regional_only_item_ids": sorted(right.keys() - left.keys()),
        "field_differences": changes, "item_mappings": mappings,
        "mapping_methods": dict(Counter(item["method"] for item in mappings.values())),
        "issues": issues,
    }
    return windows, report
# //// /按商品身份和上架阶段投影完整 CN 商品集合 ////
