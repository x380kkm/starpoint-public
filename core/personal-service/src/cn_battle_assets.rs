// audience: internal
// # personal-service-cn-battle-assets
//
// 该模块读取 Node 行为基准生成的 CN 单机战斗, 奖励和体力等级数据.

use crate::PersonalServiceError;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

const SINGLE_BATTLE_DATA: &str = include_str!("../assets/cn-single-battle.json");
const PLAYER_RANK_STAMINA_DATA: &str = include_str!("../assets/cn-player-rank-stamina.json");
static PLAYER_RANK_STAMINA: OnceLock<Result<Vec<(i64, i64, f64)>, String>> = OnceLock::new();

#[derive(Deserialize)]
pub(crate) struct BattleFixture {
    pub(crate) source: FixtureSource,
    pub(crate) characters: BTreeMap<String, CharacterAsset>,
    pub(crate) quests: BTreeMap<String, BattleQuest>,
    pub(crate) rare_reward_groups: BTreeMap<String, Vec<Reward>>,
}

#[derive(Deserialize)]
pub(crate) struct FixtureSource {
    pub(crate) source_total: usize,
    pub(crate) quest_total: usize,
    pub(crate) source_counts: BTreeMap<String, usize>,
    pub(crate) category_counts: BTreeMap<String, usize>,
    pub(crate) entry_cost_count: usize,
    pub(crate) character_count: usize,
    pub(crate) clear_reward_source_count: usize,
    pub(crate) included_clear_reward_count: usize,
    pub(crate) score_reward_source_count: usize,
    pub(crate) included_score_reward_group_count: usize,
    pub(crate) rare_reward_source_count: usize,
    pub(crate) included_rare_reward_group_count: usize,
    pub(crate) included_score_attack_border_quest_count: usize,
}

#[allow(dead_code)]
#[derive(Deserialize)]
pub(crate) struct CharacterAsset {
    pub(crate) rarity: i64,
    pub(crate) element: i64,
    #[serde(default)]
    pub(crate) races: Vec<String>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
pub(crate) struct BattleQuest {
    pub(crate) category: i64,
    pub(crate) quest_id: i64,
    pub(crate) name: String,
    pub(crate) clear_reward_id: Option<i64>,
    pub(crate) clear_reward: Option<Reward>,
    pub(crate) s_plus_reward_id: Option<i64>,
    pub(crate) s_plus_reward: Option<Reward>,
    pub(crate) score_reward_group_id: Option<i64>,
    pub(crate) score_attack_reward_group_id: Option<i64>,
    #[serde(default)]
    pub(crate) score_rewards: Vec<ScoreReward>,
    #[serde(default)]
    pub(crate) score_attack_border_rewards: Vec<ScoreAttackBorderReward>,
    pub(crate) b_rank_time: i64,
    pub(crate) a_rank_time: i64,
    pub(crate) s_rank_time: i64,
    pub(crate) s_plus_rank_time: i64,
    pub(crate) rank_point_reward: i64,
    pub(crate) character_exp_reward: i64,
    pub(crate) mana_reward: i64,
    pub(crate) pool_exp_reward: i64,
    pub(crate) element: Option<i64>,
    pub(crate) event_id: Option<i64>,
    pub(crate) folder_id: Option<i64>,
    pub(crate) fixed_party_id: Option<i64>,
    pub(crate) has_fixed_party: bool,
    pub(crate) linked_quest_id: Option<i64>,
    pub(crate) rush_event_id: Option<i64>,
    pub(crate) rush_event_folder_id: Option<i64>,
    pub(crate) rush_event_round: Option<i64>,
    pub(crate) raid_event_id: Option<i64>,
    pub(crate) carnival_event_id: Option<i64>,
    pub(crate) carnival_folder_id: Option<i64>,
    pub(crate) carnival_difficulty_score: Option<i64>,
    pub(crate) carnival_time_limit_ms: Option<i64>,
    #[serde(default)]
    pub(crate) entry_item_id: Option<i64>,
    #[serde(default)]
    pub(crate) entry_item_count: i64,
    #[serde(default)]
    pub(crate) stamina_cost: i64,
}

#[allow(dead_code)]
#[derive(Clone, Deserialize)]
pub(crate) struct Reward {
    #[serde(rename = "type")]
    pub(crate) kind: i64,
    pub(crate) id: Option<i64>,
    pub(crate) count: Option<i64>,
    pub(crate) rarity: Option<f64>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
pub(crate) struct ScoreReward {
    #[serde(rename = "type")]
    pub(crate) kind: i64,
    pub(crate) reward_type: Option<i64>,
    pub(crate) id: Option<i64>,
    pub(crate) element_rarity: Option<i64>,
    pub(crate) count: Option<i64>,
    pub(crate) rarity: Option<f64>,
    pub(crate) position: Option<i64>,
    pub(crate) field5: Option<i64>,
}

#[derive(Deserialize)]
pub(crate) struct ScoreAttackBorderReward {
    pub(crate) score: i64,
    #[serde(rename = "coinItemId")]
    pub(crate) coin_item_id: i64,
    #[serde(rename = "coinCount")]
    pub(crate) coin_count: i64,
}

// //// 校验单机战斗资产闭包 [@x380kkm 2026-08-22] ////
fn validate_battle_fixture(fixture: &BattleFixture) -> Result<(), PersonalServiceError> {
    let source = &fixture.source;
    let source_total = source.source_counts.values().sum::<usize>();
    let category_total = source.category_counts.values().sum::<usize>();
    let mut actual_category_counts = BTreeMap::<String, usize>::new();
    for (key, quest) in &fixture.quests {
        if key != &format!("{}:{}", quest.category, quest.quest_id) {
            return Err(PersonalServiceError::new(format!(
                "CN single battle quest key {key} does not match its category and id"
            )));
        }
        *actual_category_counts
            .entry(quest.category.to_string())
            .or_default() += 1;
    }

    let metadata_matches = source.source_total == source_total
        && source.quest_total == fixture.quests.len()
        && source.quest_total == category_total
        && source.category_counts == actual_category_counts
        && source.character_count == fixture.characters.len()
        && source.entry_cost_count <= source.quest_total
        && source.included_clear_reward_count <= source.clear_reward_source_count
        && source.included_score_reward_group_count <= source.score_reward_source_count
        && source.included_rare_reward_group_count <= source.rare_reward_source_count
        && source.included_rare_reward_group_count == fixture.rare_reward_groups.len()
        && source.included_score_attack_border_quest_count
            == fixture
                .quests
                .values()
                .filter(|quest| !quest.score_attack_border_rewards.is_empty())
                .count();
    if !metadata_matches {
        return Err(PersonalServiceError::new(
            "CN single battle source metadata does not match the generated fixture",
        ));
    }
    Ok(())
}
// //// /校验单机战斗资产闭包 ////

// //// 解码并校验单机战斗数据 [@x380kkm 2026-07-22] ////
pub(crate) fn load_battle_fixture() -> Result<BattleFixture, PersonalServiceError> {
    let fixture = serde_json::from_str::<BattleFixture>(SINGLE_BATTLE_DATA).map_err(|error| {
        PersonalServiceError::new(format!("failed to decode CN single battle data: {error}"))
    })?;
    if fixture.quests.is_empty() {
        return Err(PersonalServiceError::new(
            "CN single battle data contains no quests",
        ));
    }
    validate_battle_fixture(&fixture)?;
    Ok(fixture)
}
// //// /解码并校验单机战斗数据 ////

// //// 按累计段位点读取段位和体力配置 [@x380kkm 2026-08-23] ////
pub(crate) fn rank_degree_and_stamina(
    rank_point: i64,
) -> Result<(i64, i64, f64), PersonalServiceError> {
    let profiles = PLAYER_RANK_STAMINA.get_or_init(|| {
        serde_json::from_str(PLAYER_RANK_STAMINA_DATA)
            .map_err(|error| format!("failed to decode CN stamina ranks: {error}"))
    });
    let profiles = profiles
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    profiles
        .iter()
        .enumerate()
        .rev()
        .find(|(_, (threshold, _, _))| rank_point >= *threshold)
        .map(|(index, (_, stamina, heal_rate))| {
            (
                i64::try_from(index + 1).unwrap_or(i64::MAX),
                *stamina,
                *heal_rate,
            )
        })
        .ok_or_else(|| PersonalServiceError::new("CN stamina rank does not exist"))
}

pub(crate) fn stamina_profile(rank_point: i64) -> Result<(i64, f64), PersonalServiceError> {
    rank_degree_and_stamina(rank_point).map(|(_, stamina, heal_rate)| (stamina, heal_rate))
}
// //// /按累计段位点读取段位和体力配置 ////
