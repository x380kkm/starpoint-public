// audience: internal
// # personal-service-cn-character-overflow
// 满堆叠角色按来源和星级转换为 EXP 与星粒, 资产入库与邮件余量使用共同投递事务.

use crate::cn_character::character_asset_data;
use crate::cn_expod::{calculate_conversion_rewards, REWARD_ITEM_ID};
use crate::cn_reward_overflow::{grant, Conversion};
use crate::database::MailReward;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};

// //// 按主表价格结算满堆叠角色的转换奖励 [@x380kkm 2026-09-26] ////
pub(super) fn convert(
    root: &mut Map<String, Value>,
    character_id: i64,
    now: i64,
) -> Result<(), PersonalServiceError> {
    let rarity = character_asset_data(character_id)?.rarity;
    let (experience, star_grain) = calculate_conversion_rewards(character_id, rarity)?;
    let mut reward = MailReward {
        exp_pool: experience,
        ..MailReward::default()
    };
    if star_grain > 0 {
        reward
            .item_list
            .insert(REWARD_ITEM_ID.to_owned(), star_grain);
    }
    grant(
        root,
        Conversion {
            created_at: now,
            subject: "角色转换奖励".to_owned(),
            description: format!("角色 {character_id} 满堆叠转换"),
            notice: Some(json!({"process_type":5,"character":{"character_id":character_id,"number":1},"converted_pooled_exp":experience,"converted_star_grain":star_grain})),
            reward,
        },
    )
}
// //// /按主表价格结算满堆叠角色的转换奖励 ////

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cn_battle_assets::Reward;
    use crate::cn_battle_rewards::apply_reward_at;
    use crate::cn_character_reward::grant_character;
    use crate::cn_reward_overflow::{apply_response, Delivery, DELIVERIES_FIELD};

    // //// 混合奖励在角色之后发放时仍保留全部转换资产 [@x380kkm 2026-09-26] ////
    #[test]
    fn mails_conversion_share_when_later_rewards_fill_the_remaining_capacity() {
        let mut data = json!({"user_info":{"exp_pool":1_999_990_000_i64,"exp_pooled_time":100},"item_list":{"990008":999969},"user_character_list":{}});
        let root = data.as_object_mut().unwrap();
        grant_character(root, 1, 111001, 100).unwrap();
        root["user_character_list"]["111001"]["stack"] = json!(99);
        grant_character(root, 1, 111001, 100).unwrap();
        root["user_info"]["exp_pool"] = json!(2_000_005_000_i64);
        root["item_list"][REWARD_ITEM_ID] = json!(1000019);
        let mut response = json!({});
        apply_response(&mut response, root).unwrap();
        assert_eq!(root["user_info"]["exp_pool"], 2_000_000_000_i64);
        assert_eq!(root["item_list"][REWARD_ITEM_ID], 999999);
        let pending: Vec<Delivery> =
            serde_json::from_value(root[DELIVERIES_FIELD].clone()).unwrap();
        assert_eq!(pending[0].mail.exp_pool, 5000);
        assert_eq!(pending[0].mail.item_list[REWARD_ITEM_ID], 20);
        let before = root.clone();
        apply_response(&mut response, root).unwrap();
        assert_eq!(*root, before);
    }
    // //// /混合奖励在角色之后发放时仍保留全部转换资产 ////

    // //// 批量角色跨越堆叠边界时保留数量和原有角色状态 [@x380kkm 2026-09-26] ////
    #[test]
    fn converts_only_excess_copies_with_shared_bulk_rewards() {
        let mut data = json!({"user_info":{"exp_pool":0,"exp_pooled_time":100},"item_list":{},"user_character_list":{}});
        let root = data.as_object_mut().unwrap();
        grant_character(root, 1, 111001, 100).unwrap();
        root["user_character_list"]["111001"]["stack"] = json!(98);
        root["user_character_list"]["111001"]["protection"] = json!(true);
        let rewards = apply_reward_at(
            root,
            &Reward {
                kind: 2,
                id: Some(111001),
                count: Some(3),
                rarity: None,
            },
            100,
        )
        .unwrap();
        assert_eq!(rewards.character_list.len(), 1);
        assert_eq!(rewards.character_list[0]["stack"], 99);
        assert_eq!(rewards.character_list[0]["protection"], true);
        assert_eq!(root["item_list"]["14003"], 9);
        assert_eq!(root["item_list"][REWARD_ITEM_ID], 60);
        assert_eq!(root["user_info"]["exp_pool"], 20000);
        let mut response = json!({});
        apply_response(&mut response, root).unwrap();
        assert_eq!(response["over_max"].as_array().unwrap().len(), 2);
        assert_eq!(response["user_info"]["exp_pool"], 20000);
        assert_eq!(response["item_list"][REWARD_ITEM_ID], 60);
        assert_ne!(response["mail_arrived"], true);
    }
    // //// /批量角色跨越堆叠边界时保留数量和原有角色状态 ////

    // //// 非扭蛋来源只转换经验且保留旧存档中的额外副本 [@x380kkm 2026-09-26] ////
    #[test]
    fn respects_obtain_source_and_preserves_legacy_stack() {
        let mut data = json!({"user_info":{"exp_pool":0,"exp_pooled_time":100},"item_list":{},"user_character_list":{}});
        let root = data.as_object_mut().unwrap();
        grant_character(root, 1, 10, 100).unwrap();
        root["user_character_list"]["10"]["stack"] = json!(105);
        grant_character(root, 1, 10, 100).unwrap();
        assert_eq!(root["user_character_list"]["10"]["stack"], 105);
        assert!(root["user_info"]["exp_pool"].as_i64().unwrap() > 0);
        assert!(root["item_list"].get(REWARD_ITEM_ID).is_none());
        let mut response = json!({});
        apply_response(&mut response, root).unwrap();
        assert_eq!(response["over_max"][0]["converted_star_grain"], 0);
    }
    // //// /非扭蛋来源只转换经验且保留旧存档中的额外副本 ////
}
