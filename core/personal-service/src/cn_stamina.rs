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

const CAMPAIGN_DATA: &str = include_str!("../assets/stamina_campaign.json");
const RECOVERY_SECONDS: f64 = 300.0;
const STAMINA_OVERFLOW_LIMIT: i64 = 999;
const UTC_EIGHT_OFFSET_MILLIS: i64 = 8 * 60 * 60 * 1_000;
static CAMPAIGNS: OnceLock<Result<Vec<StaminaCampaign>, String>> = OnceLock::new();

struct StaminaCampaign {
    rate: f64,
    quest_type: i64,
    quest_ids: Option<Vec<i64>>,
    has_event_ids: bool,
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
    let elapsed_seconds = server_time.saturating_sub(heal_time) as f64;
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
    let Some(quest_type) = campaign_quest_type(category) else {
        return Ok(base_cost);
    };
    let mut rate = 1.0_f64;
    for campaign in campaigns()? {
        if campaign.quest_type != quest_type
            || server_time < campaign.start_time
            || server_time > campaign.end_time
        {
            continue;
        }
        let matches_quest = campaign
            .quest_ids
            .as_ref()
            .map_or(true, |quest_ids| quest_ids.contains(&quest_id));
        if matches_quest || campaign.has_event_ids {
            rate = rate.min(campaign.rate);
        }
    }
    Ok(((base_cost as f64 * rate).floor() as i64).max(1))
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
        campaigns.push(StaminaCampaign {
            rate,
            quest_type,
            quest_ids: parse_id_filter(field(9)?)?,
            has_event_ids: has_filter(field(7)?),
            start_time: parse_campaign_time(field(1)?)?,
            end_time: parse_campaign_time(field(2)?)?,
        });
    }
    Ok(campaigns)
}
// //// /读取体力活动资产 ////

fn parse_id_filter(value: &str) -> Result<Option<Vec<i64>>, String> {
    if !has_filter(value) {
        return Ok(None);
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

fn has_filter(value: &str) -> bool {
    !value.is_empty() && value != "(None)"
}

fn parse_campaign_time(value: &str) -> Result<i64, String> {
    let timestamp = format!("{}.000Z", value.replace(' ', "T"));
    parse_iso_timestamp(&timestamp)
        .and_then(|millis| millis.checked_sub(UTC_EIGHT_OFFSET_MILLIS))
        .map(|millis| millis / 1_000)
        .ok_or_else(|| format!("invalid CN stamina campaign time: {value}"))
}

fn campaign_quest_type(category: i64) -> Option<i64> {
    match category {
        1 => Some(0),
        4 => Some(1),
        2 => Some(2),
        6 => Some(3),
        14 => Some(4),
        7 | 8 => Some(5),
        10 => Some(6),
        13 => Some(7),
        11 => Some(8),
        18 => Some(9),
        19 => Some(10),
        15 => Some(11),
        20 => Some(13),
        21 => Some(14),
        22 => Some(15),
        23 => Some(16),
        24 => Some(17),
        25 => Some(18),
        26 => Some(19),
        _ => None,
    }
}
