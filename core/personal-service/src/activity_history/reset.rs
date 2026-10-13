// audience: internal
// # personal-service-activity-reset
// 活动重置使用主表的关卡和奖励归属. 清除参加进度时保留资产, 邮件和旧轮次历史.

use super::participation_round;
use crate::cn_battle_assets::{load_battle_fixture, BattleQuest};
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct ActivityReset {
    pub(crate) mission_ids: Vec<(i64, i64)>,
    pub(crate) details: Value,
}

// //// 清除一个活动的参赛进度并推进参与轮次 [@x380kkm 2026-09-21] ////
pub(crate) fn reset_activity(root: &mut Map<String, Value>, activity_id: &str, next_round: i64, reset_at: i64, regional_start_ms: Option<i64>) -> Result<ActivityReset, PersonalServiceError> {
    let (family, event_id) = activity_id.split_once(':')
        .and_then(|(family,id)| id.parse::<i64>().ok().filter(|id| *id > 0).map(|id| (family,id)))
        .ok_or_else(|| PersonalServiceError::new("invalid activity reset id"))?;
    let previous_round = participation_round(root, activity_id);
    let fixture = load_battle_fixture()?;
    let quests = fixture.quests.values().filter(|quest| quest.activity_id.as_deref() == Some(activity_id)).collect::<Vec<_>>();
    let previous_statistics = root.get("activity_battle_statistics").and_then(|entries| entries.get(activity_id)).cloned();
    let previous_event = if family == "pass-card" {
        root.get("pass_card_state").and_then(|state| state.get("events")).and_then(|events| events.get(event_id.to_string())).cloned()
    } else {
        crate::cn_activity::event_state(root, family, event_id).cloned().map(Value::Object)
    };
    let quest_records = remove_quest_records(root, &quests);
    let challenge_points = crate::cn_quest_challenge_points::restore_quest_points(root, quests.iter().map(|quest| quest.quest_id))?;
    let mission_ids = crate::cn_mission::reset_activity_missions(root, activity_id, regional_start_ms)?;
    let reward_receipts = crate::cn_battle_state::reset_event_reward_receipts(root, family, event_id)?;
    let shop_limits = crate::cn_shop::reset_activity_shop_purchases(root, activity_id)?;
    let exchange_reset = crate::cn_optional_exchange::reset_campaign(root, activity_id, reset_at)?;
    let previous_vote = if family == "character-election" {
        root.get_mut(crate::cn_character_election::VOTES_FIELD).and_then(Value::as_object_mut)
            .and_then(|votes| votes.remove(&event_id.to_string()))
    } else { None };
    if family == "ranking" {
        let scope = crate::cn_activity::reward_scope(root, family, event_id);
        if let Some(key) = scope.strip_prefix("ranking:") {
            remove_entry(root, "ranking_reward_receipts", key);
        }
    }
    if family == "pass-card" {
        if let Some(events) = root.get_mut("pass_card_state").and_then(|state| state.get_mut("events")).and_then(Value::as_object_mut) {
            events.remove(&event_id.to_string());
        }
        if let Some(receipts) = root.get_mut("pass_card_reward_receipts").and_then(Value::as_object_mut) {
            receipts.retain(|key, _| !key.starts_with(&format!("{event_id}:")));
        }
    }
    crate::cn_activity::reset_event_state(root, family, event_id)?;
    for field in ["rush_event_progress", "raid_event_played_party_list"] {
        if ((family == "rush" && field == "rush_event_progress") || (family == "raid" && field == "raid_event_played_party_list"))
            && crate::cn_activity::event_period_start(root, family, event_id).is_none() {
            remove_entry(root, field, &event_id.to_string());
        }
    }
    if family == "carnival" {
        if let Some(records) = root.get_mut("carnival_event_records").and_then(Value::as_object_mut) {
            records.retain(|key,_| !key.starts_with(&format!("{event_id}:")));
        }
    }
    remove_entry(root, "activity_battle_statistics", activity_id);
    root.entry("activity_participation_rounds".to_owned()).or_insert_with(|| json!({}))
        .as_object_mut().ok_or_else(|| PersonalServiceError::new("invalid activity participation rounds"))?
        .insert(activity_id.to_owned(), Value::from(next_round));
    Ok(ActivityReset { mission_ids, details: json!({
        "previous_round":previous_round,"next_round":next_round,"quest_records":quest_records,
        "restored_challenge_points":challenge_points,
        "period_start_ms":regional_start_ms,
        "previous_statistics":previous_statistics,"previous_event":previous_event,
        "cleared_reward_receipts":reward_receipts,"reset_shop_limits":shop_limits,
        "exchange_reset":exchange_reset,
        "previous_vote":previous_vote,
    }) })
}

// //// 根据关卡的活动归属移除通关和成绩记录 [@x380kkm 2026-09-21] ////
fn remove_quest_records(root: &mut Map<String, Value>, quests: &[&BattleQuest]) -> BTreeMap<String, Vec<Value>> {
    let mut categories = BTreeMap::<i64, BTreeSet<i64>>::new();
    for quest in quests { categories.entry(quest.category).or_default().insert(quest.quest_id); }
    let mut removed = BTreeMap::new();
    for (category, ids) in categories {
        let key = category.to_string();
        if let Some(records) = root.get_mut("quest_progress").and_then(|progress| progress.get_mut(&key)).and_then(Value::as_array_mut) {
            let mut retained = Vec::new();
            let mut previous = Vec::new();
            for record in std::mem::take(records) {
                if record.get("quest_id").and_then(Value::as_i64).is_some_and(|id| ids.contains(&id)) { previous.push(record); }
                else { retained.push(record); }
            }
            *records = retained;
            removed.insert(key, previous);
        }
    }
    removed
}

fn remove_entry(root: &mut Map<String, Value>, field: &str, key: &str) {
    if let Some(entries) = root.get_mut(field).and_then(Value::as_object_mut) { entries.remove(key); }
}
