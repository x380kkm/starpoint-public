// audience: internal
// # personal-service-cn-shop-capacity
// 商店检查物品和余额的持有上限, 角色与装备副本由发奖流程转换.

use crate::cn_reward_capacity::{capacities, has_room, held_count};
use crate::PersonalServiceError;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

// //// 检查整份商品奖励是否容纳于客户端库存 [@x380kkm 2026-09-24] ////
pub(super) fn can_receive(
    root: &Map<String, Value>,
    item: &Map<String, Value>,
    purchase_count: i64,
) -> Result<bool, PersonalServiceError> {
    let limits = capacities()?;
    let mut totals = BTreeMap::<(i64, i64), i64>::new();
    for reward in item
        .get("rewards")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let kind = reward
            .get("type")
            .and_then(Value::as_i64)
            .ok_or_else(|| PersonalServiceError::new("CN shop reward type is invalid"))?;
        let id = reward.get("id").and_then(Value::as_i64).unwrap_or_default();
        let Some(amount) = reward
            .get("count")
            .and_then(Value::as_i64)
            .unwrap_or(1)
            .checked_mul(purchase_count)
        else {
            return Ok(false);
        };
        let total = totals.entry((kind, id)).or_default();
        let Some(next) = total.checked_add(amount) else {
            return Ok(false);
        };
        *total = next;
    }
    for ((kind, id), amount) in totals {
        let (held, limit) = match kind {
            6 => continue,
            0 => {
                let limit = limits
                    .items
                    .get(&id)
                    .ok_or_else(|| PersonalServiceError::new("CN shop item capacity is missing"))?;
                (held_count(root, "item_list", &id.to_string()), *limit)
            }
            1 => (held_count(root, "user_info", "exp_pool"), limits.exp_pool),
            2 => {
                let free = held_count(root, "user_info", "free_mana");
                let paid = held_count(root, "user_info", "paid_mana");
                let Some(mana) = free.checked_add(paid) else {
                    return Ok(false);
                };
                (mana, limits.mana)
            }
            3 | 4 if amount >= 0 => continue,
            3 | 4 => return Ok(false),
            _ => return Err(PersonalServiceError::new("CN shop reward type is invalid")),
        };
        if !has_room(held, amount, limit) {
            return Ok(false);
        }
    }
    Ok(true)
}
// //// /检查整份商品奖励是否容纳于客户端库存 ////
