// audience: internal
// # personal-service-cn-score-attack
// 分数档位奖励按关卡和主表奖励编号保存领取凭据, 与战斗成绩共同提交.

use super::super::FinishBattleInput;
use crate::cn_battle_assets::BattleQuest;
use crate::cn_mission::{settle_rewards, MissionReward};
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};

const RECEIPT_FIELD: &str = "score_attack_reward_receipts";

// //// 发放最高成绩达到的未领取档位奖励 [@x380kkm 2026-09-26] ////
pub(super) fn apply_score_attack_event(
    root: &mut Map<String, Value>,
    quest: &BattleQuest,
    input: &FinishBattleInput<'_>,
    best_score: i64,
    server_time: i64,
) -> Result<(Value, Vec<MissionReward>), PersonalServiceError> {
    let mut granted = Vec::new();
    let mut reward_ids = Vec::new();
    for border in &quest.score_attack_border_rewards {
        if best_score < border.score {
            continue;
        }
        let key = format!("{}:{}", quest.quest_id, border.id);
        if settle_rewards(root, RECEIPT_FIELD, &key, &border.rewards, &[], server_time)?.is_some() {
            reward_ids.push(border.id);
            granted.extend(border.rewards.iter().cloned());
        }
    }
    let main_character_ids = crate::cn_battle::presentation::main_character_ids(root, input.main_character_ids);
    Ok((json!({
        "reward_ids": reward_ids,
        "main_character_ids": main_character_ids,
    }), granted))
}
// //// /发放最高成绩达到的未领取档位奖励 ////

// //// 按关卡编号解析领取凭据所属活动 [@x380kkm 2026-09-26] ////
pub(super) fn receipt_event_id(key: &str) -> Option<i64> {
    let (quest, reward) = key.split_once(':')?;
    let quest = quest.parse::<i64>().ok()?;
    let reward = reward.parse::<i64>().ok()?;
    (quest >= 1000 && reward > 0).then_some(quest / 1000)
}

// //// 按活动重置分数档位领取凭据 [@x380kkm 2026-09-26] ////
pub(super) fn reset_reward_receipts(
    root: &mut Map<String, Value>,
    event_id: i64,
) -> Result<usize, PersonalServiceError> {
    let Some(receipts) = root.get_mut(RECEIPT_FIELD) else {
        return Ok(0);
    };
    let receipts = receipts.as_object_mut().ok_or_else(|| {
        PersonalServiceError::new("stored CN score attack reward receipts are invalid")
    })?;
    let before = receipts.len();
    receipts.retain(|key, _| receipt_event_id(key) != Some(event_id));
    Ok(before - receipts.len())
}
// //// /按活动重置分数档位领取凭据 ////
