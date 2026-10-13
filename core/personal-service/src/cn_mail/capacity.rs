// audience: internal
// # personal-service-cn-mail-capacity
// 邮件按各资产剩余容量分配到账数量, 余量保留在原邮件中.

use crate::cn_reward_capacity::{capacities, held_count, owned_count};
use crate::database::{MailClaim, MailClaimReward, MailReward};
use crate::PersonalServiceError;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

// //// 按仓库空间分配邮件奖励与保留数量 [@x380kkm 2026-09-28] ////
pub(super) fn allocate(
    root: &Map<String, Value>,
    rewards: &MailReward,
) -> Result<MailClaim, PersonalServiceError> {
    let limits = capacities()?;
    let mut granted = MailReward::default();
    let mut remaining = rewards.clone();
    let mana_room = limits.mana
        .saturating_sub(held_count(root, "user_info", "free_mana"))
        .saturating_sub(held_count(root, "user_info", "paid_mana")).max(0);
    granted.free_mana = take(&mut remaining.free_mana, mana_room);
    granted.paid_mana = take(&mut remaining.paid_mana, mana_room - granted.free_mana);
    let money_room = limits.virtual_money
        .saturating_sub(held_count(root, "user_info", "free_vmoney"))
        .saturating_sub(held_count(root, "user_info", "vmoney")).max(0);
    granted.free_vmoney = take(&mut remaining.free_vmoney, money_room);
    granted.vmoney = take(&mut remaining.vmoney, money_room - granted.free_vmoney);
    for (key, amount, received, limit) in [
        ("exp_pool", &mut remaining.exp_pool, &mut granted.exp_pool, limits.exp_pool),
        ("star_crumb", &mut remaining.star_crumb, &mut granted.star_crumb, limits.star_crumb),
        ("bond_token", &mut remaining.bond_token, &mut granted.bond_token, limits.bond_token),
        ("boss_boost_point", &mut remaining.boss_boost_point, &mut granted.boss_boost_point, limits.boss_boost_point),
        ("boost_point", &mut remaining.boost_point, &mut granted.boost_point, limits.boost_point),
        ("rank_point", &mut remaining.rank_point, &mut granted.rank_point, i64::MAX),
    ] {
        *received = take(amount, limit.saturating_sub(held_count(root, "user_info", key)));
    }
    for (id, amount) in &mut remaining.item_list {
        let item_id = id.parse::<i64>()
            .map_err(|_| PersonalServiceError::new("mail item id is invalid"))?;
        let limit = if limits.overflow_receivable_items.contains(&item_id) {
            i64::from(i32::MAX)
        } else {
            *limits.items.get(&item_id)
                .ok_or_else(|| PersonalServiceError::new("CN mail item capacity is missing"))?
        };
        let received = take(amount, limit.saturating_sub(held_count(root, "item_list", id)));
        if received > 0 {
            granted.item_list.insert(id.clone(), received);
        }
    }
    remaining.item_list.retain(|_, amount| *amount > 0);
    for (id, amount) in &mut remaining.equipment_list {
        let room = (limits.equipment_stack + 1)
            .saturating_sub(owned_count(root, "user_equipment_list", id));
        let received = take(amount, room);
        if received > 0 {
            granted.equipment_list.insert(id.clone(), received);
        }
    }
    remaining.equipment_list.retain(|_, amount| *amount > 0);
    let mut character_rooms = BTreeMap::new();
    remaining.character_list.retain(|id| {
        let room = character_rooms.entry(*id).or_insert_with(|| {
            let entries = root.get("user_character_list")
                .and_then(|characters| characters.get(id.to_string()))
                .map(|character| character.get("entry_count").and_then(Value::as_i64).unwrap_or(1))
                .unwrap_or_default();
            i64::from(i32::MAX).saturating_sub(entries).max(0)
        });
        if *room == 0 {
            return true;
        }
        *room -= 1;
        granted.character_list.push(*id);
        false
    });
    Ok(MailClaim { granted, remaining, updates: MailClaimReward::default() })
}

// //// 从待领取数量中取出可用空间容纳的部分 [@x380kkm 2026-09-28] ////
fn take(amount: &mut i64, available: i64) -> i64 {
    let received = (*amount).max(0).min(available.max(0));
    *amount -= received;
    received
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // //// 核对共享货币容量与角色入手数量及装备的邮件余量 [@x380kkm 2026-09-28] ////
    #[test]
    fn allocates_combined_balances_and_duplicate_stacks_independently() {
        let player = json!({
            "user_info": {"free_mana": 999_999_997, "paid_mana": 1,
                "free_vmoney": 999_997, "vmoney": 1},
            "user_character_list": {"111001": {"stack": 98, "entry_count": 2_147_483_646}},
            "user_equipment_list": {"101001": {"stack": 998}},
            "item_list": {"14018": 99_998, "40000": 999_999}
        });
        let reward: MailReward = serde_json::from_value(json!({
            "free_mana": 2, "paid_mana": 3, "free_vmoney": 2, "vmoney": 3,
            "item_list": {"14018": 3, "40000": 1},
            "character_list": [111001, 111001], "equipment_list": {"101001": 3}
        })).unwrap();
        let claim = allocate(player.as_object().unwrap(), &reward).unwrap();
        assert_eq!((claim.granted.free_mana, claim.remaining.free_mana, claim.remaining.paid_mana), (1, 1, 3));
        assert_eq!((claim.granted.free_vmoney, claim.remaining.free_vmoney, claim.remaining.vmoney), (1, 1, 3));
        assert_eq!(claim.granted.item_list["14018"], 1);
        assert_eq!(claim.remaining.item_list["14018"], 2);
        assert_eq!(claim.granted.item_list["40000"], 1);
        assert_eq!(claim.granted.character_list, vec![111001]);
        assert_eq!(claim.remaining.character_list, vec![111001]);
        assert_eq!(claim.granted.equipment_list["101001"], 1);
        assert_eq!(claim.remaining.equipment_list["101001"], 2);
    }
}
