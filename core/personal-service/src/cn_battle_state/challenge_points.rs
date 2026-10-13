// audience: internal
// # personal-service-battle-challenge-points
// 专家活动按活动编号确定挑战点, 其他关卡按关卡目录确定挑战点.
// 挑战加成次数按入场时的活动窗口扣除, 余额和战斗领取凭据共同保存.

use crate::database::ActiveSingleQuest;
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

const POINT_MAP: &str = include_str!("../../../../assets/event_challenge_point_map.json");
const CAMPAIGN_WINDOWS: &str = include_str!("../../assets/cn-daily-challenge-windows.json");
static POINTS: OnceLock<Result<BTreeMap<String, i64>, String>> = OnceLock::new();
static CAMPAIGNS: OnceLock<Result<BTreeMap<i64, Campaign>, String>> = OnceLock::new();

#[derive(Deserialize)]
struct Campaign {
    point_id: i64,
    start_at: i64,
    end_at: i64,
}

// //// 返回本次战斗扣除次数的挑战点 [@x380kkm 2026-10-04] ////
pub(super) fn point_id(active: &ActiveSingleQuest, event_id: Option<i64>) -> Result<Option<i64>, PersonalServiceError> {
    if active.category != 21 {
        return crate::cn_quest_challenge_points::point_id_for_quest(active.quest_id);
    }
    let Some(event_id) = event_id else { return Ok(None); };
    let points = POINTS.get_or_init(|| serde_json::from_str(POINT_MAP)
        .map_err(|error| format!("invalid CN challenge point map: {error}")))
        .as_ref().map_err(|error| PersonalServiceError::new(error.clone()))?;
    Ok(points.get(&format!("expert_{event_id}")).copied())
}

// //// 扣除入场时有效的加成次数或基础次数 [@x380kkm 2026-09-28] ////
pub(super) fn consume(
    root: &mut Map<String, Value>,
    active: &ActiveSingleQuest,
    event_id: Option<i64>,
    completed_at: i64,
) -> Result<(), PersonalServiceError> {
    let Some(point_id) = point_id(active, event_id)? else { return Ok(()); };
    let started_at = root.get("battle_mission_window")
        .filter(|window| window.get("play_id").and_then(Value::as_str) == Some(active.play_id.as_str()))
        .and_then(|window| window.get("started_at"))
        .and_then(Value::as_i64)
        .unwrap_or(completed_at);
    let entries = root.get_mut("user_daily_challenge_point_list")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN daily challenge points are missing"))?;
    let Some(entry) = entries.iter_mut().find(|entry| entry.get("id").and_then(Value::as_i64) == Some(point_id)) else {
        return Ok(());
    };
    let entry = entry.as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN daily challenge point is invalid"))?;
    if consume_campaign(entry, point_id, started_at)? {
        return Ok(());
    }
    let point = entry.get("point").and_then(Value::as_i64).unwrap_or_default();
    entry.insert("point".to_owned(), Value::from(point.saturating_sub(1).max(0)));
    Ok(())
}

// //// 优先扣除最早到期的有效加成次数 [@x380kkm 2026-09-28] ////
fn consume_campaign(entry: &mut Map<String, Value>, point_id: i64, started_at: i64) -> Result<bool, PersonalServiceError> {
    let catalog = CAMPAIGNS.get_or_init(|| serde_json::from_str(CAMPAIGN_WINDOWS)
        .map_err(|error| format!("invalid CN challenge campaign windows: {error}")))
        .as_ref().map_err(|error| PersonalServiceError::new(error.clone()))?;
    let Some(campaigns) = entry.get_mut("campaign_list").and_then(Value::as_array_mut) else {
        return Ok(false);
    };
    let selected = campaigns.iter().enumerate().filter_map(|(index, value)| {
        let id = value.get("campaign_id")?.as_i64()?;
        let remaining = value.get("additional_point")?.as_i64()?;
        let campaign = catalog.get(&id)?;
        (remaining > 0 && campaign.point_id == point_id
            && (campaign.start_at..=campaign.end_at).contains(&started_at))
            .then_some((index, campaign.end_at, id, remaining))
    }).min_by_key(|(_, end, id, _)| (*end, *id));
    let Some((index, _, _, remaining)) = selected else { return Ok(false); };
    campaigns[index]["additional_point"] = Value::from(remaining - 1);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn battle() -> ActiveSingleQuest {
        ActiveSingleQuest {
            play_id: "challenge-window".to_owned(), quest_id: 1001, category: 21,
            use_boss_boost_point: false, use_boost_point: false, is_auto_start_mode: false,
        }
    }

    fn player(started_at: i64) -> Map<String, Value> {
        json!({
            "battle_mission_window":{"play_id":"challenge-window","started_at":started_at},
            "user_daily_challenge_point_list":[{"id":1,"point":2,"campaign_list":[
                {"campaign_id":2023013101_i64,"additional_point":2}
            ]}]
        }).as_object().unwrap().clone()
    }

    // //// 加成次数耗尽后扣除基础次数且余额保持非负 [@x380kkm 2026-09-28] ////
    #[test]
    fn consumes_active_campaign_then_base_points() {
        let now = 1_726_790_400;
        let mut root = player(now);
        for (base, extra) in [(2, 1), (2, 0), (1, 0), (0, 0), (0, 0)] {
            consume(&mut root, &battle(), Some(1), now).unwrap();
            let point = &root["user_daily_challenge_point_list"][0];
            assert_eq!(point["point"], base);
            assert_eq!(point["campaign_list"][0]["additional_point"], extra);
        }
    }

    // //// 活动结束后的在途战斗使用入场窗口 [@x380kkm 2026-09-28] ////
    #[test]
    fn consumes_campaign_across_its_end_using_captured_start() {
        let end = 1_727_452_799;
        let mut root = player(end);
        consume(&mut root, &battle(), Some(1), end + 600).unwrap();
        let point = &root["user_daily_challenge_point_list"][0];
        assert_eq!(point["point"], 2);
        assert_eq!(point["campaign_list"][0]["additional_point"], 1);
    }

    // //// 加成结束后的新战斗扣除基础次数 [@x380kkm 2026-09-28] ////
    #[test]
    fn leaves_inactive_campaign_balance_unchanged() {
        let now = 1_727_452_800;
        let mut root = player(now);
        consume(&mut root, &battle(), Some(1), now).unwrap();
        let point = &root["user_daily_challenge_point_list"][0];
        assert_eq!(point["point"], 1);
        assert_eq!(point["campaign_list"][0]["additional_point"], 2);
    }
}
