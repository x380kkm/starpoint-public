// audience: internal
// # personal-service-cn-carnival-rewards
//
// 该模块按各关卡组最佳分数之和发放嘉年华奖励.
// 开战时所选角色与其它关卡组重叠, 对应关卡组的成绩从活动记录移除.

use super::super::FinishBattleInput;
use super::rewards::{settle_reward_tiers, RewardTier};
use crate::cn_activity;
use crate::cn_battle_assets::BattleQuest;
use crate::cn_mission::MissionReward;
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

const REWARD_MASTER: &str = include_str!("../../../assets/cn-carnival-rewards.json");
static REWARDS: OnceLock<Result<RewardCatalog, String>> = OnceLock::new();

#[derive(Deserialize)]
struct RewardCatalog {
    events: BTreeMap<i64, Vec<ScoreReward>>,
}

#[derive(Deserialize)]
struct ScoreReward {
    score: i64,
    #[serde(flatten)]
    tier: RewardTier,
}

// //// 开战时清除与所选队伍共享角色的其它关卡成绩 [@x380kkm 2026-09-21] ////
pub(super) fn prepare_carnival_battle(
    root: &mut Map<String, Value>,
    quest: &BattleQuest,
    character_ids: &[i64],
) -> Result<(), PersonalServiceError> {
    let (Some(event_id), Some(folder_id)) = (quest.carnival_event_id, quest.carnival_folder_id)
    else {
        return Ok(());
    };
    let selected = character_ids
        .iter()
        .copied()
        .filter(|id| *id > 0)
        .collect::<BTreeSet<_>>();
    let records = carnival_records(root, event_id)?;
    records.retain(|record| {
        record.get("folder_id").and_then(Value::as_i64) == Some(folder_id)
            || !["previous_character_ids", "previous_unison_character_ids"]
                .iter()
                .flat_map(|field| {
                    record
                        .get(field)
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                })
                .filter_map(Value::as_i64)
                .any(|id| selected.contains(&id))
    });
    Ok(())
}
// //// /开战时清除与所选队伍共享角色的其它关卡成绩 ////

// //// 保存关卡成绩并发放总分档位奖励 [@x380kkm 2026-09-21] ////
pub(super) fn apply_carnival_event(
    root: &mut Map<String, Value>,
    quest: &BattleQuest,
    input: &FinishBattleInput<'_>,
    server_time: i64,
) -> Result<(Value, Vec<MissionReward>), PersonalServiceError> {
    let (Some(event_id), Some(folder_id), Some(difficulty_bonus), Some(time_limit)) = (
        quest.carnival_event_id,
        quest.carnival_folder_id,
        quest.carnival_difficulty_score,
        quest.carnival_time_limit_ms,
    ) else {
        return Ok((Value::Null, Vec::new()));
    };
    let time_bonus = time_limit.saturating_sub(input.elapsed_time_ms).max(0);
    let score = difficulty_bonus.checked_add(time_bonus).ok_or_else(|| {
        PersonalServiceError::new("CN carnival score exceeds the supported range")
    })?;
    let (previous_total, total) = update_record(root, event_id, folder_id, score, input)?;
    let catalog = reward_catalog()?;
    let tiers = catalog.events.get(&event_id).ok_or_else(|| {
        PersonalServiceError::new(format!("CN carnival rewards are missing: {event_id}"))
    })?;
    let rewards = settle_reward_tiers(
        root,
        "carnival_reward_receipts",
        tiers
            .iter()
            .filter(|tier| total >= tier.score)
            .map(|tier| &tier.tier),
        server_time,
    )?;
    let leader = input.main_character_ids.iter().copied().flatten()
        .chain(input.character_ids.iter().copied())
        .find(|id| crate::cn_character_presentation::is_valid_character(*id));
    let leader = crate::cn_character_presentation::leader_character_id(root, leader);
    Ok((
        json!({
            "is_record_valid": true,
            "leader_character_id": leader,
            "new_degree_ids": rewards.new_degree_ids,
            "previous_total_best_score": previous_total,
            "reward_ids": rewards.reward_ids,
            "score": { "difficulty_bonus": difficulty_bonus, "time_bonus": time_bonus },
        }),
        rewards.granted,
    ))
}
// //// /保存关卡成绩并发放总分档位奖励 ////

// //// 按活动移除嘉年华领取凭据 [@x380kkm 2026-09-21] ////
pub(super) fn reset_reward_receipts(
    root: &mut Map<String, Value>,
    event_id: i64,
) -> Result<usize, PersonalServiceError> {
    let ids = reward_catalog()?
        .events
        .get(&event_id)
        .into_iter()
        .flatten()
        .map(|tier| tier.tier.id);
    super::rewards::reset_reward_receipts(root, "carnival_reward_receipts", ids)
}
// //// /按活动移除嘉年华领取凭据 ////

// //// 从奖励表定位嘉年华活动 [@x380kkm 2026-09-21] ////
pub(super) fn receipt_event_id(reward_id: i64) -> Result<Option<i64>, PersonalServiceError> {
    Ok(reward_catalog()?
        .events
        .iter()
        .find_map(|(event_id, tiers)| {
            tiers
                .iter()
                .any(|tier| tier.tier.id == reward_id)
                .then_some(*event_id)
        }))
}
// //// /从奖励表定位嘉年华活动 ////

// //// 读取嘉年华奖励表 [@x380kkm 2026-09-21] ////
fn reward_catalog() -> Result<&'static RewardCatalog, PersonalServiceError> {
    REWARDS
        .get_or_init(|| serde_json::from_str(REWARD_MASTER).map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| PersonalServiceError::new(format!("invalid CN carnival rewards: {error}")))
}
// //// /读取嘉年华奖励表 ////

// //// 更新当前关卡组的最佳分数和最近队伍 [@x380kkm 2026-09-21] ////
fn update_record(
    root: &mut Map<String, Value>,
    event_id: i64,
    folder_id: i64,
    score: i64,
    input: &FinishBattleInput<'_>,
) -> Result<(i64, i64), PersonalServiceError> {
    let records = carnival_records(root, event_id)?;
    let previous_total = total_best_score(records)?;
    let existing_index = records
        .iter()
        .position(|record| record.get("folder_id").and_then(Value::as_i64) == Some(folder_id));
    let previous_best = existing_index
        .and_then(|index| records[index].get("best_score"))
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let record = json!({
        "folder_id": folder_id, "best_score": previous_best.max(score), "previous_score": score,
        "previous_character_ids": input.main_character_ids,
        "previous_unison_character_ids": input.unison_character_ids,
    });
    if let Some(index) = existing_index {
        records[index] = record;
    } else {
        records.push(record);
    }
    Ok((previous_total, total_best_score(records)?))
}
// //// /更新当前关卡组的最佳分数和最近队伍 ////

// //// 读取活动关卡组记录并合计最佳分数 [@x380kkm 2026-09-21] ////
fn carnival_records(
    root: &mut Map<String, Value>,
    event_id: i64,
) -> Result<&mut Vec<Value>, PersonalServiceError> {
    cn_activity::activate_battle_event_state(root, "carnival", event_id)?
        .entry("carnival_records".to_owned())
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN carnival records are invalid"))
}

fn total_best_score(records: &[Value]) -> Result<i64, PersonalServiceError> {
    records.iter().try_fold(0_i64, |total, record| {
        total
            .checked_add(
                record
                    .get("best_score")
                    .and_then(Value::as_i64)
                    .unwrap_or_default(),
            )
            .ok_or_else(|| {
                PersonalServiceError::new("CN carnival total score exceeds the supported range")
            })
    })
}
// //// /读取活动关卡组记录并合计最佳分数 ////

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cn_battle_assets::load_battle_fixture;

    // //// 构造嘉年华奖励玩家和战斗输入 [@x380kkm 2026-09-21] ////
    fn player() -> Map<String, Value> {
        json!({"user_info": {"free_vmoney": 0, "free_mana": 0, "exp_pool": 0},
            "user_equipment_list": {}, "item_list": {}})
        .as_object()
        .unwrap()
        .clone()
    }

    fn battle_input(elapsed_time_ms: i64) -> FinishBattleInput<'static> {
        FinishBattleInput {
            elapsed_time_ms,
            score: 0,
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
    // //// /构造嘉年华奖励玩家和战斗输入 ////

    // //// 嘉年华记录面板选择有效参战角色并保留空队伍成绩 [@x380kkm 2026-09-28] ////
    #[test]
    fn carnival_leader_uses_valid_battle_members_or_owned_display_character() {
        let fixture = load_battle_fixture().unwrap();
        let quest = &fixture.quests["22:2001"];
        for (mains, characters, expected) in [
            ([Some(0),Some(111001),None], vec![0,111001], 111001),
            ([Some(99999999),None,None], vec![111002], 111002),
            ([None,None,None], vec![], 111003),
        ] {
            let mut root = player();
            root.insert("user_character_list".to_owned(), json!({"111003":{"stack":0}}));
            let mut input = battle_input(605001);
            input.main_character_ids = &mains;
            input.character_ids = &characters;
            let (response, _) = apply_carnival_event(&mut root, quest, &input, 0).unwrap();
            assert_eq!(response["leader_character_id"], expected);
            assert_eq!(response["is_record_valid"], true);
            assert_eq!(response["score"], json!({"difficulty_bonus":1000000,"time_bonus":1194999}));
        }
    }

    // //// 总分跨越实际奖励阈值且重复和较差成绩保持资产 [@x380kkm 2026-09-21] ////
    #[test]
    fn settles_total_score_boundaries_without_regranting() {
        let fixture = load_battle_fixture().unwrap();
        let first_quest = &fixture.quests["22:2001"];
        let second_quest = &fixture.quests["22:2006"];
        let mut root = player();
        let (below, _) =
            apply_carnival_event(&mut root, first_quest, &battle_input(605_001), 0).unwrap();
        assert_eq!(below["previous_total_best_score"], 0);
        assert_eq!(below["reward_ids"], json!([]));
        let (first, _) =
            apply_carnival_event(&mut root, first_quest, &battle_input(605_000), 0).unwrap();
        assert_eq!(first["previous_total_best_score"], 2_194_999);
        assert_eq!(first["reward_ids"], json!([69]));
        assert_eq!(
            first["score"],
            json!({"difficulty_bonus": 1_000_000, "time_bonus": 1_195_000})
        );
        assert_eq!(root["item_list"]["90010"], 200);
        assert_eq!(root["user_equipment_list"]["5070030"]["stack"], 0);
        let mut second_input = battle_input(830_001);
        second_input.main_character_ids = &[Some(2), None, None];
        let (below_degree, _) =
            apply_carnival_event(&mut root, second_quest, &second_input, 0).unwrap();
        assert_eq!(below_degree["previous_total_best_score"], 2_195_000);
        assert_eq!(below_degree["new_degree_ids"], json!([]));
        second_input.elapsed_time_ms = 830_000;
        let (degree, _) = apply_carnival_event(&mut root, second_quest, &second_input, 0).unwrap();
        assert_eq!(degree["previous_total_best_score"], 4_914_999);
        assert_eq!(degree["reward_ids"], json!([68]));
        assert_eq!(degree["new_degree_ids"], json!([61000]));
        assert_eq!(root["user_equipment_list"]["5010046"]["stack"], 0);
        let assets = (
            root["user_info"].clone(),
            root["item_list"].clone(),
            root["user_equipment_list"].clone(),
        );
        for elapsed in [830_000, 850_000] {
            second_input.elapsed_time_ms = elapsed;
            let (response, granted) =
                apply_carnival_event(&mut root, second_quest, &second_input, 0).unwrap();
            assert_eq!(response["previous_total_best_score"], 4_915_000);
            assert_eq!(response["reward_ids"], json!([]));
            assert_eq!(response["new_degree_ids"], json!([]));
            assert!(granted.is_empty());
        }
        assert_eq!(
            (
                root["user_info"].clone(),
                root["item_list"].clone(),
                root["user_equipment_list"].clone()
            ),
            assets
        );
    }
    // //// /总分跨越实际奖励阈值且重复和较差成绩保持资产 ////

    // //// 开战清除其它关卡组的重复主角色和副角色记录 [@x380kkm 2026-09-21] ////
    #[test]
    fn resets_reused_character_records_before_battle_completion() {
        let fixture = load_battle_fixture().unwrap();
        let quest = &fixture.quests["22:2006"];
        let mut root = player();
        carnival_records(&mut root, 2).unwrap().extend([
            json!({"folder_id": 1, "best_score": 2000000, "previous_character_ids": [1], "previous_unison_character_ids": [2]}),
            json!({"folder_id": 2, "best_score": 3000000, "previous_character_ids": [3], "previous_unison_character_ids": []}),
            json!({"folder_id": 3, "best_score": 4000000, "previous_character_ids": [4], "previous_unison_character_ids": [5]}),
        ]);
        root.insert(
            "carnival_reward_receipts".to_owned(),
            json!({"69": {"rewards": []}}),
        );
        prepare_carnival_battle(&mut root, quest, &[2, 3]).unwrap();
        assert_eq!(
            total_best_score(carnival_records(&mut root, 2).unwrap()).unwrap(),
            7_000_000
        );
        prepare_carnival_battle(&mut root, quest, &[4]).unwrap();
        assert_eq!(
            total_best_score(carnival_records(&mut root, 2).unwrap()).unwrap(),
            3_000_000
        );
        assert_eq!(
            root["carnival_reward_receipts"],
            json!({"69": {"rewards": []}})
        );
    }
    // //// /开战清除其它关卡组的重复主角色和副角色记录 ////
}
