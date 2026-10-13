// audience: internal
// # personal-service-cn-battle-phase-rewards
//
// clear_phase 表示已完成阶段数, 竞速奖励取已完成阶段中最高一档.
// 剧情败北战和计分活动使用各自结算奖励, 普通失败结算的基础奖励和掉落为零.

use super::FinishBattleInput;
use crate::cn_battle_assets::{BattleFixture, BattleQuest, RareScoreReward};
use serde_json::Value;
use std::borrow::Cow;
use std::collections::BTreeMap;

const SCORE_ATTACK_CATEGORY: i64 = 27;

pub(super) struct BattleRewardSelection<'a> {
    pub(super) receives_rewards: bool,
    pub(super) quest: Cow<'a, BattleQuest>,
    pub(super) field_mana: i64,
    pub(super) rare_reward_groups: &'a BTreeMap<String, Vec<RareScoreReward>>,
}

// //// 识别主数据中的剧情败北战 [@x380kkm 2026-09-21] ////
pub(super) fn is_scripted_loss_quest(fixture: &BattleFixture, quest: &BattleQuest) -> bool {
    fixture
        .reward_rules
        .lose_quests
        .contains(&format!("{}:{}", quest.category, quest.quest_id))
}

// //// 按战斗结果和已完成阶段选择结算奖励 [@x380kkm 2026-09-21] ////
pub(super) fn select_battle_rewards<'a>(
    fixture: &'a BattleFixture,
    quest: &'a BattleQuest,
    input: &FinishBattleInput<'_>,
    statistics: &Value,
) -> BattleRewardSelection<'a> {
    let key = format!("{}:{}", quest.category, quest.quest_id);
    if let Some(phase_quest) = fixture.reward_rules.phase_quests.get(&key) {
        let completed_phases = statistics
            .get("clear_phase")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let phase = phase_quest
            .phases
            .iter()
            .filter(|phase| input.is_accomplished || phase.phase <= completed_phases)
            .max_by_key(|phase| phase.phase);
        let mut selected = empty_reward_quest(quest);
        if let Some(phase) = phase {
            selected.rank_point_reward = phase.rank_point_reward;
            selected.character_exp_reward = phase.character_exp_reward;
            selected.mana_reward = phase.mana_reward;
            selected.pool_exp_reward = phase.pool_exp_reward;
            selected.score_reward_group_id = phase.score_reward_group_id;
            selected.score_rewards = phase.score_rewards.clone();
            selected.score_reward_counts = [phase.score_reward_count; 5];
        }
        return BattleRewardSelection {
            receives_rewards: phase.is_some(),
            quest: Cow::Owned(selected),
            field_mana: input.add_mana,
            rare_reward_groups: &phase_quest.rare_reward_groups,
        };
    }
    let receives_rewards = input.is_accomplished
        || quest.category == SCORE_ATTACK_CATEGORY
        || (input.is_lose && is_scripted_loss_quest(fixture, quest));
    BattleRewardSelection {
        receives_rewards,
        quest: if receives_rewards {
            Cow::Borrowed(quest)
        } else {
            Cow::Owned(empty_reward_quest(quest))
        },
        field_mana: if receives_rewards { input.add_mana } else { 0 },
        rare_reward_groups: &fixture.rare_reward_groups,
    }
}

// //// 清除未完成战斗的基础奖励和掉落 [@x380kkm 2026-09-21] ////
fn empty_reward_quest(quest: &BattleQuest) -> BattleQuest {
    let mut selected = quest.clone();
    selected.rank_point_reward = 0;
    selected.character_exp_reward = 0;
    selected.mana_reward = 0;
    selected.pool_exp_reward = 0;
    selected.score_reward_group_id = None;
    selected.score_attack_reward_group_id = None;
    selected.score_rewards.clear();
    selected.score_reward_counts = [0; 5];
    selected
}
