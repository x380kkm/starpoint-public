// audience: internal
// # personal-service-cn-tower-dungeon
// 幽玄域入口和客户端资源使用同一期十关. 游戏日期超出原始期别时沿用相邻的首期或末期.

use crate::PersonalServiceError;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

const PERIODS_ASSET: &str = include_str!("../assets/cn-tower-dungeon-periods.json");
static PERIODS: OnceLock<Result<BTreeMap<i64, Vec<TowerPeriod>>, String>> = OnceLock::new();

#[derive(Deserialize)]
struct TowerPeriod {
    starts_at_ms: i64,
    quest_ids: [i64; 10],
}

// //// 按游戏日期选择幽玄域的完整期别 [@x380kkm 2026-09-23] ////
pub(crate) fn quest_ids_for_time(
    event_id: i64,
    server_time_ms: i64,
) -> Result<Option<[i64; 10]>, PersonalServiceError> {
    let events = PERIODS
        .get_or_init(|| {
            serde_json::from_str(PERIODS_ASSET)
                .map_err(|error| format!("failed to decode CN tower periods: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    let Some(periods) = events.get(&event_id) else {
        return Ok(None);
    };
    Ok(periods
        .iter()
        .rev()
        .find(|period| period.starts_at_ms <= server_time_ms)
        .or_else(|| periods.first())
        .map(|period| period.quest_ids))
}
// //// /按游戏日期选择幽玄域的完整期别 ////

#[cfg(test)]
mod tests {
    // //// 核对幽玄域无评级结算的四个独立收益字段 [@x380kkm 2026-09-23] ////
    #[test]
    fn reads_tower_rewards_after_the_drop_count_column() {
        let fixture = crate::cn_battle_assets::load_battle_fixture().unwrap();
        for (quest_id, expected) in [(1001, (220, 146, 0, 146)), (1480, (782, 2450, 0, 2450))] {
            let quest = &fixture.quests[&format!("20:{quest_id}")];
            assert_eq!(
                (
                    quest.rank_point_reward,
                    quest.character_exp_reward,
                    quest.mana_reward,
                    quest.pool_exp_reward
                ),
                expected
            );
        }
    }
    // //// /核对幽玄域无评级结算的四个独立收益字段 ////
}
