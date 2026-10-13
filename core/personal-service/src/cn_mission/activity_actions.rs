// audience: internal
// # personal-service-cn-mission-activity-actions
// 活动界面与投票动作使用服务端已保存的活动归属. 动作凭据按活动参与轮次去重.

use super::{action, mission_catalog, mission_scope, window::mission_activity_id};
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

// //// 将首次有效投票计入对应活动任务 [@x380kkm 2026-09-26] ////
pub(crate) fn record_election_vote_action(
    root: &mut Map<String, Value>, database: &mut ServiceDatabase, account_id: i64, event_id: i64,
) -> Result<Vec<Value>, PersonalServiceError> {
    record_activity_action(root, database, account_id, "character_election_vote_count", &format!("character-election:{event_id}"))
}

// //// 将团队战首页访问计入对应活动任务 [@x380kkm 2026-09-26] ////
pub(crate) fn record_raid_top_action(
    root: &mut Map<String, Value>, database: &mut ServiceDatabase, account_id: i64, event_id: i64,
) -> Result<Vec<Value>, PersonalServiceError> {
    record_activity_action(root, database, account_id, "raid_event_top_check", &format!("raid:{event_id}"))
}

// //// 将已保存的团队战主队与副队编辑分别计入任务 [@x380kkm 2026-09-26] ////
pub(crate) fn record_raid_party_actions(
    root: &mut Map<String, Value>, database: &mut ServiceDatabase, account_id: i64, party_ids: &BTreeSet<i64>,
) -> Result<Vec<Value>, PersonalServiceError> {
    let Some(event_id) = crate::cn_activity::current_event_id(root, "raid") else { return Ok(Vec::new()); };
    let activity_id = format!("raid:{event_id}");
    let mut completed = Vec::new();
    for (party_id, pattern) in [(1, "raid_event_set_edit_main"), (2, "raid_event_set_edit_sub1"), (3, "raid_event_set_edit_sub2")] {
        if party_ids.contains(&party_id) {
            completed.extend(record_activity_action(root, database, account_id, pattern, &activity_id)?);
        }
    }
    Ok(completed)
}

// //// 将合法回忆录访问计入当前活动的回忆录任务 [@x380kkm 2026-09-26] ////
pub(crate) fn record_player_history_action(
    root: &mut Map<String, Value>, database: &mut ServiceDatabase, account_id: i64,
) -> Result<Vec<Value>, PersonalServiceError> {
    let catalog = mission_catalog()?;
    let activities: BTreeSet<String> = catalog.categories.iter().flat_map(|(category, missions)| {
        missions.iter().filter(|mission| mission.pattern.as_deref() == Some("player_history_check"))
            .filter_map(|mission| mission_activity_id(*category, mission))
    }).collect();
    let mut completed = Vec::new();
    for activity_id in activities {
        completed.extend(record_activity_action(root, database, account_id, "player_history_check", &activity_id)?);
    }
    Ok(completed)
}

// //// 按活动归属和当前时窗保存一次动作及其任务奖励 [@x380kkm 2026-09-26] ////
fn record_activity_action(
    root: &mut Map<String, Value>, database: &mut ServiceDatabase, account_id: i64, pattern: &str, activity_id: &str,
) -> Result<Vec<Value>, PersonalServiceError> {
    let round = crate::activity_history::participation_round(root, activity_id);
    let receipt = format!("{activity_id}:round:{round}:{pattern}");
    if root.get("mission_action_receipts").and_then(|receipts| receipts.get(&receipt)).is_some() {
        return Ok(Vec::new());
    }
    let catalog = mission_catalog()?;
    let mut eligible = false;
    for key in catalog.pattern_index.get(pattern).into_iter().flatten() {
        let Some(mission) = catalog.categories.get(&key.category)
            .and_then(|missions| missions.iter().find(|mission| mission.id == key.mission_id)) else { continue; };
        if mission_activity_id(key.category, mission).as_deref() == Some(activity_id)
            && mission_scope(*key, mission, database)?.is_some() {
            eligible = true;
            break;
        }
    }
    if !eligible { return Ok(Vec::new()); }
    let progress = database.mission_counters(account_id)?.get(pattern).copied().unwrap_or_default()
        .checked_add(1).ok_or_else(|| PersonalServiceError::new("CN mission action progress exceeds range"))?;
    let collections = super::plan_collect_item_missions(database)?;
    let completed = action::record_pattern_progress_with_scope(root, database, account_id, pattern, progress, &collections, |key, mission, database| {
        if mission_activity_id(key.category, mission).as_deref() == Some(activity_id) {
            mission_scope(key, mission, database)
        } else {
            Ok(None)
        }
    })?;
    root.entry("mission_action_receipts".to_owned()).or_insert_with(|| json!({})).as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN mission action receipts are invalid"))?
        .insert(receipt, Value::from(true));
    Ok(completed)
}
