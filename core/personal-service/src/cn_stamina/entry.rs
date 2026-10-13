// audience: internal
// # personal-service-cn-entry-resources
// 此模块在资源足够时一起扣除战斗入场道具和体力, 并按可用余额结算奖励加成点.

use super::{battle_stamina_cost, current_stamina};
use crate::cn_battle_assets::BattleQuest;
use crate::cn_tutorial::user_info_value;
use crate::PersonalServiceError;
use serde_json::{Map, Value};

pub(crate) struct EntryResources {
    pub(crate) stamina: i64,
    pub(crate) stamina_heal_time: i64,
    pub(crate) stamina_used: i64,
    pub(crate) items: Map<String, Value>,
}

#[derive(Debug, PartialEq)]
pub(crate) enum EntryFailure {
    InsufficientItem,
    InsufficientStamina,
}

pub(crate) struct BoostSettlement {
    pub(crate) boost_point: i64,
    pub(crate) boss_boost_point: i64,
    pub(crate) applied: bool,
}

// //// 按单人活动倍率计算并扣除入场资源 [@x380kkm 2026-09-22] ////
pub(crate) fn deduct_single_entry_resources(
    root: &mut Map<String, Value>,
    quest: &BattleQuest,
    server_time: i64,
) -> Result<Result<EntryResources, EntryFailure>, PersonalServiceError> {
    let cost = battle_stamina_cost(
        quest.category,
        quest.quest_id,
        quest.stamina_cost,
        server_time,
    )?;
    deduct_entry_resources(root, quest, cost, server_time)
}

// //// 按房主关系和救援资格计算客机体力 [@x380kkm 2026-09-22] ////
pub(crate) fn multiplayer_stamina_cost(
    quest: &BattleQuest,
    host_follow_state: Option<i64>,
    is_assisted: bool,
    server_time: i64,
) -> Result<i64, PersonalServiceError> {
    let base_cost = if is_assisted || host_follow_state == Some(1) {
        0
    } else if host_follow_state.is_some() {
        quest.stamina_cost.max(0) / 2
    } else {
        quest.stamina_cost
    };
    battle_stamina_cost(quest.category, quest.quest_id, base_cost, server_time)
}

// //// 校验足额资源后保存入场费用 [@x380kkm 2026-09-22] ////
pub(crate) fn deduct_entry_resources(
    root: &mut Map<String, Value>,
    quest: &BattleQuest,
    stamina_cost: i64,
    server_time: i64,
) -> Result<Result<EntryResources, EntryFailure>, PersonalServiceError> {
    let mut items = Map::new();
    if let Some(item_id) = quest.entry_item_id.filter(|id| *id > 0) {
        let key = item_id.to_string();
        let current = root
            .get("item_list")
            .and_then(|items| items.get(&key))
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let count = quest.entry_item_count.max(0);
        if current < count {
            return Ok(Err(EntryFailure::InsufficientItem));
        }
        items.insert(key, Value::from(current - count));
    }
    let current = current_stamina(root, server_time)?;
    let stamina_used = stamina_cost.max(0);
    if current < stamina_used {
        return Ok(Err(EntryFailure::InsufficientStamina));
    }
    let stamina = current - stamina_used;
    let previous_heal_time = user_info_value(root, "stamina_heal_time")?;
    let user = root
        .get_mut("user_info")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN user info is missing"))?;
    let stamina_heal_time = if stamina_used > 0 {
        server_time
    } else {
        previous_heal_time.min(server_time)
    };
    if stamina_used > 0 {
        let total = user
            .get("total_stamina_used")
            .and_then(Value::as_i64)
            .unwrap_or_default()
            .checked_add(stamina_used)
            .ok_or_else(|| {
                PersonalServiceError::new("CN total stamina use exceeds the supported range")
            })?;
        user.insert("stamina".to_owned(), stamina.into());
        user.insert("total_stamina_used".to_owned(), total.into());
    }
    if stamina_used > 0 || previous_heal_time > server_time {
        user.insert("stamina_heal_time".to_owned(), stamina_heal_time.into());
    }
    if !items.is_empty() {
        root.get_mut("item_list")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| PersonalServiceError::new("stored CN item list is missing"))?
            .extend(items.clone());
    }
    Ok(Ok(EntryResources {
        stamina,
        stamina_heal_time,
        stamina_used,
        items,
    }))
}

// //// 按现有余额结算战斗加成点 [@x380kkm 2026-09-22] ////
pub(crate) fn settle_boost_points(
    root: &mut Map<String, Value>,
    use_boost_point: bool,
    use_boss_boost_point: bool,
) -> Result<BoostSettlement, PersonalServiceError> {
    let boost_point = user_info_value(root, "boost_point")?.max(0);
    let boss_boost_point = user_info_value(root, "boss_boost_point")?.max(0);
    let boost_used = use_boost_point && boost_point > 0;
    let boss_boost_used = use_boss_boost_point && boss_boost_point > 0;
    let boost_point = boost_point - i64::from(boost_used);
    let boss_boost_point = boss_boost_point - i64::from(boss_boost_used);
    let user = root
        .get_mut("user_info")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN user info is missing"))?;
    user.insert("boost_point".to_owned(), boost_point.into());
    user.insert("boss_boost_point".to_owned(), boss_boost_point.into());
    Ok(BoostSettlement {
        boost_point,
        boss_boost_point,
        applied: boost_used || boss_boost_used,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cn_battle_assets::load_battle_fixture;

    // //// 客机折扣逐步取整且免费资格保持零消耗 [@x380kkm 2026-09-22] ////
    #[test]
    fn calculates_multiplayer_discounts_from_the_entry_context() {
        let fixture = load_battle_fixture().unwrap();
        let mut quest = fixture.quests["2:1002001"].clone();
        for (follow, assisted, expected) in [
            (None, false, 13),
            (Some(0), false, 6),
            (Some(2), false, 6),
            (Some(3), false, 6),
            (Some(1), false, 0),
            (Some(0), true, 0),
        ] {
            assert_eq!(
                multiplayer_stamina_cost(&quest, follow, assisted, 1_893_456_000).unwrap(),
                expected
            );
        }
        quest.stamina_cost = 1;
        assert_eq!(
            multiplayer_stamina_cost(&quest, Some(0), false, 1_893_456_000).unwrap(),
            0
        );
    }
}
