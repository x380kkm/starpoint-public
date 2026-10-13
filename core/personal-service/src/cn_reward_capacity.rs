// audience: internal
// # personal-service-cn-reward-capacity
// 客户端持有上限与经验增长参数来自主表, 已有角色和装备各占一份数量.

use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

pub(crate) const MAX_CHARACTER_STACK: i64 = 99;

#[derive(Deserialize)]
pub(crate) struct Capacities {
    pub(crate) virtual_money: i64,
    pub(crate) mana: i64,
    pub(crate) star_crumb: i64,
    pub(crate) exp_pool: i64,
    pub(crate) exp_pool_gain_value: i64,
    pub(crate) exp_pool_gain_seconds: i64,
    pub(crate) equipment_stack: i64,
    pub(crate) bond_token: i64,
    pub(crate) boss_boost_point: i64,
    pub(crate) boost_point: i64,
    pub(crate) items: BTreeMap<i64, i64>,
    pub(crate) overflow_receivable_items: BTreeSet<i64>,
}

static CAPACITIES: OnceLock<Result<Capacities, String>> = OnceLock::new();

// //// 读取客户端奖励持有上限 [@x380kkm 2026-09-25] ////
pub(crate) fn capacities() -> Result<&'static Capacities, PersonalServiceError> {
    CAPACITIES
        .get_or_init(|| {
            serde_json::from_str(include_str!("../assets/cn-shop-capacities.json"))
                .map_err(|error| format!("CN reward capacities are invalid: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

// //// 检查奖励新增数量与持有上限 [@x380kkm 2026-09-25] ////
pub(crate) fn has_room(held: i64, amount: i64, limit: i64) -> bool {
    amount == 0 || (amount > 0 && held.checked_add(amount).is_some_and(|total| total <= limit))
}

// //// 读取库存中的货币或物品数量 [@x380kkm 2026-09-25] ////
pub(crate) fn held_count(root: &Map<String, Value>, field: &str, key: &str) -> i64 {
    root.get(field)
        .and_then(|values| values.get(key))
        .and_then(Value::as_i64)
        .unwrap_or_default()
}

// //// 将角色或装备实体和堆叠合并为持有数量 [@x380kkm 2026-09-25] ////
pub(crate) fn owned_count(root: &Map<String, Value>, field: &str, key: &str) -> i64 {
    root.get(field)
        .and_then(|values| values.get(key))
        .map(|entry| {
            entry
                .get("stack")
                .and_then(Value::as_i64)
                .unwrap_or_default()
                .saturating_add(1)
        })
        .unwrap_or_default()
}
