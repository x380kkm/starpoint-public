// audience: internal
// # personal-service-cn-mission-conditions
//
// 普通任务和主动任务共用客户端的关卡范围编码与队伍条件. 关卡档位取自 CN 关卡主表.

mod battle;
mod character;
mod party;

pub(crate) use battle::{battle_power, has_complete_skill_gauges};
pub(crate) use character::{
    character_level, has_complete_second_board, has_second_board_abilities,
};
pub(crate) use party::{fully_awakened_equipment_count, PartyConditions};

use crate::PersonalServiceError;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Default)]
pub(crate) struct QuestConditions<'a> {
    pub(crate) kind: Option<i64>,
    pub(crate) parts: [&'a [i64]; 3],
    pub(crate) categories: &'a [i64],
    pub(crate) ids: &'a [i64],
    pub(crate) rank: Option<i64>,
}

#[derive(Deserialize)]
struct QuestRanks {
    quest_ranks: BTreeMap<String, i64>,
}

static QUEST_RANKS: OnceLock<Result<QuestRanks, String>> = OnceLock::new();
static BATTLE_FIXTURE: OnceLock<Result<crate::cn_battle_assets::BattleFixture, String>> =
    OnceLock::new();

pub(super) fn battle_fixture(
) -> Result<&'static crate::cn_battle_assets::BattleFixture, PersonalServiceError> {
    BATTLE_FIXTURE
        .get_or_init(|| {
            crate::cn_battle_assets::load_battle_fixture().map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

fn quest_rank(category: i64, quest_id: i64) -> Result<Option<i64>, PersonalServiceError> {
    let ranks = QUEST_RANKS
        .get_or_init(|| {
            serde_json::from_str(include_str!(
                "../../assets/cn-active-mission-incentives.json"
            ))
            .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    Ok(ranks
        .quest_ranks
        .get(&format!("{category}:{quest_id}"))
        .copied())
}

fn categories_for_kind(kind: i64) -> &'static [i64] {
    match kind {
        0 => &[1],
        1 => &[4],
        2 => &[2],
        3 => &[6],
        4 => &[14],
        5 => &[7],
        6 => &[10],
        7 => &[13],
        8 => &[11],
        9 => &[18],
        10 => &[19],
        11 => &[15],
        12 => &[6, 14, 13, 20],
        13 => &[20],
        14 => &[21],
        15 => &[22],
        16 => &[23],
        17 => &[24],
        18 => &[25],
        19 => &[26],
        20 => &[27],
        _ => &[],
    }
}

// //// 按主表复合编号与关卡档位匹配任务范围 [@x380kkm 2026-09-21] ////
impl QuestConditions<'_> {
    pub(crate) fn matches(
        &self,
        category: i64,
        quest_id: i64,
    ) -> Result<bool, PersonalServiceError> {
        if !self.categories.is_empty() && !self.categories.contains(&category) {
            return Ok(false);
        }
        if !self.ids.is_empty() && !self.ids.contains(&quest_id) {
            return Ok(false);
        }
        if let Some(rank) = self.rank {
            if quest_rank(category, quest_id)? != Some(rank) {
                return Ok(false);
            }
        }
        let Some(kind) = self.kind else {
            return Ok(true);
        };
        if !categories_for_kind(kind).contains(&category) {
            return Ok(false);
        }
        let parts = if matches!(kind, 0 | 1 | 2) {
            [
                quest_id / 1_000_000,
                quest_id / 1_000 % 1_000,
                quest_id % 1_000,
            ]
        } else if matches!(kind, 11 | 12) {
            [0, 0, quest_id]
        } else {
            [quest_id / 1_000, 0, quest_id % 1_000]
        };
        Ok(self
            .parts
            .iter()
            .zip(parts)
            .all(|(expected, actual)| expected.is_empty() || expected.contains(&actual)))
    }
}
// //// /按主表复合编号与关卡档位匹配任务范围 ////

pub(crate) fn battle_kind_matches(kind: Option<i64>, is_multi: bool) -> bool {
    match kind {
        Some(1) => !is_multi,
        Some(2) => is_multi,
        _ => true,
    }
}

pub(crate) fn party_slots_match(expected: &[Option<i64>], actual: &[Option<i64>]) -> bool {
    expected.iter().enumerate().all(|(index, expected)| {
        expected.map_or(true, |id| actual.get(index).copied().flatten() == Some(id))
    })
}
