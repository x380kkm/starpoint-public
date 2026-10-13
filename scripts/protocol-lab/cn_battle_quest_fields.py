# audience: internal
# # cn-battle-quest-fields
# 字段列号对应各关卡客户端主表定义. 服务时间使用毫秒, 主表时间使用秒.

from dataclasses import dataclass
from decimal import Decimal


@dataclass(frozen=True)
class QuestColumns:
    name: int
    clear_reward: int
    s_plus_reward: int | None
    base_rewards: int | None
    rank_times: int | None


QUEST_COLUMNS = {
    1: QuestColumns(1, 3, 71, 93, 84),
    2: QuestColumns(2, 4, 71, 93, 84),
    3: QuestColumns(3, 5, 73, 95, 86),
    4: QuestColumns(1, 3, 71, 93, 84),
    6: QuestColumns(1, 3, None, 67, None),
    7: QuestColumns(2, 4, 77, 99, 90),
    10: QuestColumns(2, 4, 73, 95, 86),
    11: QuestColumns(2, 4, None, None, None),
    13: QuestColumns(2, 4, 72, 94, 85),
    14: QuestColumns(2, 4, None, 68, None),
    15: QuestColumns(2, 4, 72, 94, 85),
    18: QuestColumns(2, 4, 72, 94, 85),
    19: QuestColumns(2, 4, 71, 93, 84),
    20: QuestColumns(2, 4, None, 83, None),
    21: QuestColumns(4, 6, 74, 96, 87),
    22: QuestColumns(4, 6, None, 82, None),
    23: QuestColumns(4, 6, None, 83, None),
    24: QuestColumns(4, 6, None, 82, None),
    25: QuestColumns(3, 5, None, 85, 51),
    26: QuestColumns(2, 4, 72, 94, 85),
    27: QuestColumns(4, 6, None, 86, None),
}


# //// 读取主表可选数值并保留字段缺省 [@x380kkm 2026-09-26] ////
def optional_number(row, column):
    if column is None or row[column] in ("", "(None)"):
        return None
    return Decimal(row[column])


# //// 按字段单位读取整数并检查精度 [@x380kkm 2026-09-26] ////
def integer_field(row, column, scale=1):
    number = optional_number(row, column)
    if number is None:
        return None
    number *= scale
    if not number.is_finite() or number != number.to_integral_value():
        raise ValueError(f"Non-integral quest field at {column}: {row[column]}")
    return int(number)


# //// 提取关卡基础收益和独立首通评级奖励 [@x380kkm 2026-09-26] ////
def quest_fields(category, row):
    columns = QUEST_COLUMNS[category]
    fields = {
        "name": row[columns.name],
        "clearRewardId": integer_field(row, columns.clear_reward),
        "sPlusRewardId": integer_field(row, columns.s_plus_reward),
    }
    for offset, name in enumerate(("rankPointReward", "characterExpReward", "manaReward", "poolExpReward")):
        column = columns.base_rewards + offset if columns.base_rewards is not None else None
        fields[name] = integer_field(row, column) or 0
    for offset, name in enumerate(("bRankTime", "aRankTime", "sRankTime", "sPlusRankTime")):
        column = columns.rank_times + offset if columns.rank_times is not None else None
        fields[name] = integer_field(row, column, 1000) or 0
    return fields

