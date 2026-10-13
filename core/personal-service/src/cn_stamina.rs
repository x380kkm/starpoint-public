// audience: internal
// # personal-service-cn-stamina
//
// 该模块按虚拟服务器时间计算 CN 体力恢复和活动折扣. 活动表时间使用 UTC+8.

use crate::cn_battle_assets::stamina_profile;
use crate::cn_tutorial::user_info_value;
use crate::database::parse_iso_timestamp;
use crate::PersonalServiceError;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

pub(crate) mod entry;

const CAMPAIGN_DATA: &str = include_str!("../assets/stamina_campaign.json");
const RECOVERY_SECONDS: f64 = 300.0;
const STAMINA_OVERFLOW_LIMIT: i64 = 999;
const UTC_EIGHT_OFFSET_MILLIS: i64 = 8 * 60 * 60 * 1_000;
static CAMPAIGNS: OnceLock<Result<Vec<StaminaCampaign>, String>> = OnceLock::new();

struct StaminaCampaign {
    rate: f64,
    categories: &'static [i64],
    quest_keys: Vec<Option<Vec<i64>>>,
    start_time: i64,
    end_time: i64,
}

// //// 计算玩家当前体力 [@x380kkm 2026-08-23] ////
pub(crate) fn current_stamina(
    root: &Map<String, Value>,
    server_time: i64,
) -> Result<i64, PersonalServiceError> {
    let stored_stamina = user_info_value(root, "stamina")?;
    let heal_time = user_info_value(root, "stamina_heal_time")?;
    let rank_point = user_info_value(root, "rank_point")?;
    let (rank_stamina, heal_rate) = stamina_profile(rank_point)?;
    let recovery_seconds = RECOVERY_SECONDS * (1.0 - heal_rate).max(f64::EPSILON);
    let elapsed_seconds = server_time.saturating_sub(heal_time).max(0) as f64;
    let recovered =
        stored_stamina.saturating_add((elapsed_seconds / recovery_seconds).floor() as i64);
    Ok(recovered
        .clamp(0, rank_stamina.max(stored_stamina))
        .min(STAMINA_OVERFLOW_LIMIT))
}
// //// /计算玩家当前体力 ////

// //// 计算活动折扣后的战斗体力 [@x380kkm 2026-08-23] ////
pub(crate) fn battle_stamina_cost(
    category: i64,
    quest_id: i64,
    base_cost: i64,
    server_time: i64,
) -> Result<i64, PersonalServiceError> {
    let base_cost = base_cost.max(0);
    if base_cost == 0 {
        return Ok(0);
    }
    let mut rate = 1.0_f64;
    for campaign in campaigns()? {
        if !campaign.categories.contains(&category)
            || server_time < campaign.start_time
            || server_time > campaign.end_time
        {
            continue;
        }
        if matches_quest_keys(&campaign.quest_keys, quest_id) {
            rate = rate.min(campaign.rate);
        }
    }
    Ok(if rate == 0.0 {
        0
    } else {
        ((base_cost as f64 * rate).floor() as i64).max(1)
    })
}
// //// /计算活动折扣后的战斗体力 ////

// //// 读取体力活动资产 [@x380kkm 2026-08-23] ////
fn campaigns() -> Result<&'static [StaminaCampaign], PersonalServiceError> {
    match CAMPAIGNS.get_or_init(load_campaigns) {
        Ok(campaigns) => Ok(campaigns),
        Err(message) => Err(PersonalServiceError::new(message.clone())),
    }
}

fn load_campaigns() -> Result<Vec<StaminaCampaign>, String> {
    let source = serde_json::from_str::<BTreeMap<String, Vec<Vec<String>>>>(CAMPAIGN_DATA)
        .map_err(|error| format!("failed to decode CN stamina campaigns: {error}"))?;
    let mut campaigns = Vec::new();
    for rows in source.into_values() {
        let Some(row) = rows.first() else {
            continue;
        };
        if row.get(5).map_or(true, String::is_empty) {
            continue;
        }
        let field = |index: usize| {
            row.get(index)
                .map(String::as_str)
                .ok_or_else(|| format!("CN stamina campaign row misses field {index}"))
        };
        let rate = field(5)?
            .parse::<f64>()
            .map_err(|error| format!("invalid CN stamina campaign rate: {error}"))?;
        let quest_type = field(6)?
            .parse::<i64>()
            .map_err(|error| format!("invalid CN stamina campaign quest type: {error}"))?;
        let (categories, columns) = campaign_quest_scope(quest_type)?;
        campaigns.push(StaminaCampaign {
            rate,
            categories,
            quest_keys: columns
                .iter()
                .map(|index| parse_id_filter(field(*index)?))
                .collect::<Result<_, _>>()?,
            start_time: parse_campaign_time(field(1)?)?,
            end_time: parse_campaign_time(field(2)?)?,
        });
    }
    Ok(campaigns)
}
// //// /读取体力活动资产 ////

fn parse_id_filter(value: &str) -> Result<Option<Vec<i64>>, String> {
    if value == "(None)" {
        return Ok(None);
    }
    if value.is_empty() {
        return Ok(Some(Vec::new()));
    }
    value
        .split(',')
        .map(|id| {
            id.parse::<i64>()
                .map_err(|error| format!("invalid CN stamina campaign quest id: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn parse_campaign_time(value: &str) -> Result<i64, String> {
    let timestamp = format!("{}.000Z", value.replace(' ', "T"));
    parse_iso_timestamp(&timestamp)
        .and_then(|millis| millis.checked_sub(UTC_EIGHT_OFFSET_MILLIS))
        .map(|millis| millis / 1_000)
        .ok_or_else(|| format!("invalid CN stamina campaign time: {value}"))
}

// //// 按主表范围逐级匹配关卡编号 [@x380kkm 2026-09-22] ////
fn matches_quest_keys(keys: &[Option<Vec<i64>>], mut quest_id: i64) -> bool {
    for (index, filter) in keys.iter().enumerate().rev() {
        let key = if index == 0 {
            quest_id
        } else {
            quest_id % 1000
        };
        if filter.as_ref().is_some_and(|values| !values.contains(&key)) {
            return false;
        }
        quest_id /= 1000;
    }
    true
}

fn campaign_quest_scope(quest_type: i64) -> Result<(&'static [i64], &'static [usize]), String> {
    Ok(match quest_type {
        0 => (&[1], &[7, 8, 9]),
        1 => (&[4], &[7, 8, 9]),
        2 => (&[2], &[7, 8, 9]),
        3 => (&[6], &[7, 9]),
        4 => (&[14], &[7, 9]),
        5 => (&[7], &[7, 9]),
        6 => (&[10], &[7, 9]),
        7 => (&[13], &[7, 9]),
        8 => (&[11], &[7, 9]),
        9 => (&[18], &[7, 9]),
        10 => (&[19], &[7, 9]),
        11 => (&[15], &[9]),
        12 => (&[6, 14, 13, 20], &[]),
        13 => (&[20], &[7, 9]),
        14 => (&[21], &[7, 9]),
        15 => (&[22], &[7, 9]),
        16 => (&[23], &[7, 9]),
        17 => (&[24], &[7, 9]),
        18 => (&[25], &[7, 9]),
        19 => (&[26], &[7, 9]),
        20 => (&[27], &[7, 9]),
        _ => {
            return Err(format!(
                "unsupported CN stamina campaign quest type: {quest_type}"
            ))
        }
    })
}
// //// /按主表范围逐级匹配关卡编号 ////

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cn_battle_assets::load_battle_fixture;

    // //// 定向折扣保留范围外关卡的实际消耗 [@x380kkm 2026-09-22] ////
    #[test]
    fn applies_campaigns_only_to_their_encoded_quest_ranges() {
        let fixture = load_battle_fixture().unwrap();
        for (time, key, expected) in [
            ("2025-06-20 12:00:00", "13:1001", 5),
            ("2025-06-20 12:00:00", "13:1030", 26),
            ("2025-06-20 12:00:00", "13:2001", 10),
            ("2025-03-28 12:00:00", "2:1002001", 6),
            ("2025-03-28 12:00:00", "2:1028001", 13),
        ] {
            let quest = &fixture.quests[key];
            assert_eq!(
                battle_stamina_cost(
                    quest.category,
                    quest.quest_id,
                    quest.stamina_cost,
                    parse_campaign_time(time).unwrap()
                )
                .unwrap(),
                expected,
                "{key}"
            );
        }
        assert_eq!(
            entry::multiplayer_stamina_cost(
                &fixture.quests["2:1002001"],
                Some(0),
                false,
                parse_campaign_time("2025-03-28 12:00:00").unwrap()
            )
            .unwrap(),
            3
        );
    }
}
