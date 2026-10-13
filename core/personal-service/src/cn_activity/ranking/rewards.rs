// audience: internal
// # personal-service-cn-ranking-rewards
// 排名奖励使用客户端 GeneralRewardKind 编码. 邮件承载物品与余额, 快照承载称号.

use super::Reward;
use crate::database::MailReward;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};

// //// 组装排名邮件和玩家称号 [@x380kkm 2026-09-21] ////
pub(super) fn prepare(
    root: &mut Map<String, Value>,
    rewards: &[Reward],
    viewer_id: i64,
) -> Result<(MailReward, Vec<Value>), PersonalServiceError> {
    let mut mail = MailReward::default();
    let mut degrees = Vec::new();
    for reward in rewards {
        match reward.kind {
            0 => *mail.item_list.entry(required_id(reward)?.to_string()).or_default() += reward.amount,
            1 => *mail.equipment_list.entry(required_id(reward)?.to_string()).or_default() += reward.amount,
            2 => mail.free_vmoney += reward.amount,
            3 => mail.free_mana += reward.amount,
            4 => mail.exp_pool += reward.amount,
            6 => mail.character_list.push(required_id(reward)?),
            7 => {
                let degree_id = required_id(reward)?;
                let values = root.entry("degree_ids".to_owned()).or_insert_with(|| json!([]))
                    .as_array_mut().ok_or_else(|| PersonalServiceError::new("stored CN degrees are invalid"))?;
                if !values.iter().any(|value| value.as_i64() == Some(degree_id)) {
                    values.push(json!(degree_id));
                }
                degrees.push(json!({"viewer_id": viewer_id, "degree_id": degree_id}));
            }
            _ => return Err(PersonalServiceError::new("unsupported CN ranking reward kind")),
        }
    }
    Ok((mail, degrees))
}

fn required_id(reward: &Reward) -> Result<i64, PersonalServiceError> {
    reward.id.filter(|id| *id > 0).ok_or_else(|| PersonalServiceError::new("CN ranking reward id is missing"))
}
