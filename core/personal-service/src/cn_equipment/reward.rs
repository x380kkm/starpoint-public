// audience: internal
// # personal-service-cn-equipment-reward
// 已有装备的强化与保护状态随新增副本保留. 超出主表堆叠容量的副本按主表转换为分解物品.

use super::dissolve::catalog::equipment_definitions;
use super::{serialize_equipment, WRIGHTPIECE_ITEM_ID};
use crate::cn_reward_capacity::capacities;
use crate::cn_reward_overflow::{self, Conversion};
use crate::cn_tutorial::require_object;
use crate::database::MailReward;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};

// //// 发放完整装备数量并转换超过堆叠容量的副本 [@x380kkm 2026-09-26] ////
pub(crate) fn grant(
    root: &mut Map<String, Value>,
    equipment_id: i64,
    count: i64,
    now: i64,
) -> Result<Value, PersonalServiceError> {
    if count <= 0 {
        return Err(PersonalServiceError::new(
            "CN equipment reward count is invalid",
        ));
    }
    let equipments = require_object(root, "user_equipment_list")?;
    let key = equipment_id.to_string();
    let was_owned = equipments.contains_key(&key);
    let mut stored = equipments
        .get(&key)
        .cloned()
        .unwrap_or_else(|| json!({"enhancement_level":0,"level":1,"protection":false,"stack":0}));
    let equipment = stored
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN equipment data is invalid"))?;
    let current = equipment
        .get("stack")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    if current < 0 {
        return Err(PersonalServiceError::new(
            "stored CN equipment stack is invalid",
        ));
    }
    let capacity = if was_owned {
        capacities()?.equipment_stack.saturating_sub(current).max(0)
    } else {
        capacities()?.equipment_stack + 1
    };
    let acquired = count.min(capacity);
    equipment.insert(
        "stack".to_owned(),
        json!(if was_owned {
            current + acquired
        } else {
            acquired - 1
        }),
    );
    let response = serialize_equipment(equipment_id, equipment);
    equipments.insert(key, stored);
    if count > acquired {
        convert(root, equipment_id, count - acquired, now)?;
    }
    Ok(response)
}
// //// /发放完整装备数量并转换超过堆叠容量的副本 ////

// //// 按来源和星级主表发放装备转换物品 [@x380kkm 2026-09-26] ////
fn convert(
    root: &mut Map<String, Value>,
    equipment_id: i64,
    count: i64,
    now: i64,
) -> Result<(), PersonalServiceError> {
    let definition = equipment_definitions()?
        .get(&equipment_id)
        .ok_or_else(|| PersonalServiceError::new("CN equipment definition is missing"))?;
    let craft_points = definition
        .craft_points
        .checked_mul(count)
        .ok_or_else(|| PersonalServiceError::new("CN equipment craft reward exceeds range"))?;
    let star_grains = definition
        .star_grains
        .checked_mul(count)
        .ok_or_else(|| PersonalServiceError::new("CN equipment star grain reward exceeds range"))?;
    let mut reward = MailReward::default();
    let mut notice =
        json!({"process_type":6,"equipment":{"equipment_id":equipment_id,"number":count}});
    if craft_points > 0 {
        reward
            .item_list
            .insert(WRIGHTPIECE_ITEM_ID.to_string(), craft_points);
        notice["converted_craft_point"] = json!(craft_points);
    }
    if star_grains > 0 {
        reward
            .item_list
            .insert(crate::cn_expod::REWARD_ITEM_ID.to_owned(), star_grains);
        notice["converted_star_grain"] = json!(star_grains);
    }
    if definition.generate_ability_soul {
        reward
            .item_list
            .insert(definition.ability_soul_id.to_string(), count);
        notice["converted_ability_soul_id"] = json!(definition.ability_soul_id);
    }
    cn_reward_overflow::grant(
        root,
        Conversion {
            created_at: now,
            subject: "装备转换奖励".to_owned(),
            description: format!("装备 {equipment_id} 满堆叠转换 {count} 件"),
            notice: Some(notice),
            reward,
        },
    )
}
// //// /按来源和星级主表发放装备转换物品 ////

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cn_battle_assets::Reward;

    // //// 批量装备跨越容量时保留强化状态并准确转换余量 [@x380kkm 2026-09-26] ////
    #[test]
    fn equipment_overflow_preserves_inventory_and_bulk_quantity() {
        for (stack, expected_stack, converted) in [(998, 999, 2), (1005, 1005, 3)] {
            let equipment =
                json!({"stack":stack,"level":5,"enhancement_level":2,"protection":true,"custom":7});
            let mut player =
                json!({"user_info":{},"item_list":{},"user_equipment_list":{"5030037":equipment}});
            let root = player.as_object_mut().unwrap();
            let result = crate::cn_battle_rewards::apply_reward_at(
                root,
                &Reward {
                    kind: 1,
                    id: Some(5030037),
                    count: Some(3),
                    rarity: None,
                },
                100,
            )
            .unwrap();
            assert_eq!(result.equipment_list[0]["stack"], expected_stack);
            let mut expected = equipment;
            expected["stack"] = json!(expected_stack);
            assert_eq!(root["user_equipment_list"]["5030037"], expected);
            assert_eq!(root["item_list"]["100000"], converted * 5);
            assert_eq!(root["item_list"]["5030037"], converted);
            assert!(root["item_list"].get("990008").is_none());
            let mut response = json!({});
            cn_reward_overflow::apply_response(&mut response, root).unwrap();
            assert_eq!(
                response["over_max"][0],
                json!({
                    "process_type":6,"equipment":{"equipment_id":5030037,"number":converted},
                    "converted_craft_point":converted*5,"converted_ability_soul_id":5030037
                })
            );
            assert_ne!(response["mail_arrived"], true);
        }
    }
    // //// /批量装备跨越容量时保留强化状态并准确转换余量 ////
}
