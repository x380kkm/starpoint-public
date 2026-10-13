// audience: internal
// # personal-service-cn-solo-time-attack
//
// 该模块按通关时间发放极时试炼奖励, 并保存每个奖励档位的实际领取记录.
// 通关时间以毫秒比较, 等于阈值时满足奖励条件.

use super::super::{FinishBattleInput, PreviousProgress};
use super::rewards::{settle_reward_tiers, RewardTier};
use crate::cn_mission::MissionReward;
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

const REWARD_MASTER: &str = include_str!("../../../assets/cn-solo-time-attack-rewards.json");
static REWARDS: OnceLock<Result<RewardCatalog, String>> = OnceLock::new();

#[derive(Deserialize)]
struct RewardCatalog {
    quests: BTreeMap<i64, Vec<TimeReward>>,
}

#[derive(Deserialize)]
struct TimeReward {
    elapsed_time_ms: i64,
    #[serde(flatten)]
    tier: RewardTier,
}

// //// 结算达到时间阈值的奖励档位 [@x380kkm 2026-09-21] ////
pub(super) fn apply_solo_time_attack_event(
    root: &mut Map<String, Value>,
    quest_id: i64,
    input: &FinishBattleInput<'_>,
    previous: &PreviousProgress,
    server_time: i64,
) -> Result<(Value, Vec<MissionReward>), PersonalServiceError> {
    if !input.is_accomplished {
        return Ok((Value::Null, Vec::new()));
    }
    let catalog = reward_catalog()?;
    let tiers = catalog.quests.get(&quest_id).ok_or_else(|| {
        PersonalServiceError::new(format!(
            "CN solo time attack quest rewards are missing: {quest_id}"
        ))
    })?;
    let best_time = previous
        .best_elapsed_time_ms
        .map_or(input.elapsed_time_ms, |time| {
            time.min(input.elapsed_time_ms)
        });
    let rewards = settle_reward_tiers(
        root,
        "solo_time_attack_reward_receipts",
        tiers
            .iter()
            .filter(|tier| best_time <= tier.elapsed_time_ms)
            .map(|tier| &tier.tier),
        server_time,
    )?;
    let main_character_ids = crate::cn_battle::presentation::main_character_ids(root, input.main_character_ids);
    Ok((
        json!({
            "elapsed_time_ms": input.elapsed_time_ms,
            "main_character_ids": main_character_ids,
            "new_degree_ids": rewards.new_degree_ids,
            "previous_best_elapsed_score": previous.best_elapsed_score,
            "previous_best_elapsed_time_ms": previous.best_elapsed_time_ms,
            "reward_ids": rewards.reward_ids,
        }),
        rewards.granted,
    ))
}
// //// /结算达到时间阈值的奖励档位 ////

// //// 按活动移除极时试炼领取凭据 [@x380kkm 2026-09-21] ////
pub(super) fn reset_reward_receipts(
    root: &mut Map<String, Value>,
    event_id: i64,
) -> Result<usize, PersonalServiceError> {
    let ids = reward_catalog()?
        .quests
        .iter()
        .filter(|(quest_id, _)| **quest_id / 1000 == event_id)
        .flat_map(|(_, tiers)| tiers.iter().map(|tier| tier.tier.id));
    super::rewards::reset_reward_receipts(root, "solo_time_attack_reward_receipts", ids)
}
// //// /按活动移除极时试炼领取凭据 ////

// //// 从奖励表定位极时试炼活动 [@x380kkm 2026-09-21] ////
pub(super) fn receipt_event_id(reward_id: i64) -> Result<Option<i64>, PersonalServiceError> {
    Ok(reward_catalog()?
        .quests
        .iter()
        .find_map(|(quest_id, tiers)| {
            tiers
                .iter()
                .any(|tier| tier.tier.id == reward_id)
                .then_some(quest_id / 1000)
        }))
}
// //// /从奖励表定位极时试炼活动 ////

// //// 读取极时试炼奖励表 [@x380kkm 2026-09-21] ////
fn reward_catalog() -> Result<&'static RewardCatalog, PersonalServiceError> {
    REWARDS
        .get_or_init(|| serde_json::from_str(REWARD_MASTER).map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| {
            PersonalServiceError::new(format!("invalid CN solo time attack rewards: {error}"))
        })
}
// //// /读取极时试炼奖励表 ////

#[cfg(test)]
mod tests {
    use super::*;

    // //// 构造时间奖励结算输入 [@x380kkm 2026-09-21] ////
    fn battle_input(elapsed_time_ms: i64) -> FinishBattleInput<'static> {
        FinishBattleInput {
            elapsed_time_ms,
            score: 1000,
            add_mana: 0,
            is_accomplished: true,
            character_ids: &[1],
            main_character_ids: &[Some(1), None, None],
            unison_character_ids: &[],
            equipment_ids: &[],
            ability_soul_ids: &[],
            is_multi: false,
            party_id: 1,
            is_lose: false,
            power_flip_count: None,
            dash_count: None,
            skill_count: None,
            max_skill_chain_count: None,
            max_combo_count: None,
            is_host: None,
            is_mvp: None,
        }
    }

    fn player() -> Map<String, Value> {
        json!({"user_info": {"free_vmoney": 0}})
            .as_object()
            .unwrap()
            .clone()
    }

    fn vmoney(rewards: &[MissionReward]) -> i64 {
        rewards
            .iter()
            .filter(|reward| reward.kind == 0)
            .map(|reward| reward.amount)
            .sum()
    }
    // //// /构造时间奖励结算输入 ////

    // //// 六个关卡各发放完整时间奖励且复玩保持领取状态 [@x380kkm 2026-09-21] ////
    #[test]
    fn grants_all_six_quest_rewards_once() {
        let mut root = player();
        let input = battle_input(60_000);
        for quest_id in 1001..=1006 {
            let (response, rewards) = apply_solo_time_attack_event(
                &mut root,
                quest_id,
                &input,
                &PreviousProgress::default(),
                0,
            )
            .unwrap();
            assert_eq!(vmoney(&rewards), 1000);
            assert_eq!(response["reward_ids"].as_array().unwrap().len(), 37);
            assert_eq!(response["new_degree_ids"].as_array().unwrap().len(), 2);
            assert_eq!(response["main_character_ids"], json!({"0": 1}));
            let previous = PreviousProgress {
                best_elapsed_time_ms: Some(60_000),
                best_elapsed_score: Some(1000),
                ..Default::default()
            };
            let (replayed, rewards) =
                apply_solo_time_attack_event(&mut root, quest_id, &input, &previous, 0).unwrap();
            assert_eq!(vmoney(&rewards), 0);
            assert_eq!(replayed["reward_ids"], json!([]));
            assert_eq!(replayed["new_degree_ids"], json!([]));
        }
        assert_eq!(root["user_info"]["free_vmoney"], 6000);
        assert_eq!(root["degree_ids"].as_array().unwrap().len(), 12);
        assert_eq!(
            root["solo_time_attack_reward_receipts"]
                .as_object()
                .unwrap()
                .len(),
            222
        );
    }
    // //// /六个关卡各发放完整时间奖励且复玩保持领取状态 ////

    // //// 时间阈值包含等号且失败保持领取状态 [@x380kkm 2026-09-21] ////
    #[test]
    fn awards_exact_thresholds_and_skips_unsuccessful_battles() {
        let mut root = player();
        let previous = PreviousProgress::default();
        let mut failed = battle_input(1);
        failed.is_accomplished = false;
        let (response, rewards) =
            apply_solo_time_attack_event(&mut root, 1001, &failed, &previous, 0).unwrap();
        assert!(response.is_null());
        assert_eq!(vmoney(&rewards), 0);
        assert!(!root.contains_key("solo_time_attack_reward_receipts"));
        for (boundary, reward_id, amount, degree_ids) in [
            (420_000, 1, 10, json!([])),
            (300_000, 13, 40, json!([54530])),
            (180_000, 25, 80, json!([54520])),
            (60_000, 37, 100, json!([])),
        ] {
            let (before, _) = apply_solo_time_attack_event(
                &mut root,
                1001,
                &battle_input(boundary + 1),
                &previous,
                0,
            )
            .unwrap();
            assert!(!before["reward_ids"]
                .as_array()
                .unwrap()
                .contains(&json!(reward_id)));
            let (exact, rewards) = apply_solo_time_attack_event(
                &mut root,
                1001,
                &battle_input(boundary),
                &previous,
                0,
            )
            .unwrap();
            assert_eq!(exact["reward_ids"], json!([reward_id]));
            assert_eq!(exact["new_degree_ids"], degree_ids);
            assert_eq!(vmoney(&rewards), amount);
        }
        assert_eq!(root["user_info"]["free_vmoney"], 1000);
    }
    // //// /时间阈值包含等号且失败保持领取状态 ////

    // //// 按历史最佳时间补齐领取记录并保留已有称号 [@x380kkm 2026-09-21] ////
    #[test]
    fn settles_unreceived_rewards_from_the_best_record() {
        let mut root = player();
        root.insert("degree_ids".to_owned(), json!([54530]));
        let previous = PreviousProgress {
            best_elapsed_time_ms: Some(60_000),
            best_elapsed_score: Some(5000),
            ..Default::default()
        };
        let (response, rewards) =
            apply_solo_time_attack_event(&mut root, 1001, &battle_input(430_000), &previous, 0)
                .unwrap();
        assert_eq!(vmoney(&rewards), 1000);
        assert_eq!(response["new_degree_ids"], json!([54520]));
        assert_eq!(response["previous_best_elapsed_time_ms"], 60_000);
        assert_eq!(response["previous_best_elapsed_score"], 5000);
        assert_eq!(root["degree_ids"].as_array().unwrap().len(), 2);
        let (_, repeated) =
            apply_solo_time_attack_event(&mut root, 1001, &battle_input(430_000), &previous, 0)
                .unwrap();
        assert_eq!(vmoney(&repeated), 0);
    }
    // //// /按历史最佳时间补齐领取记录并保留已有称号 ////
}
