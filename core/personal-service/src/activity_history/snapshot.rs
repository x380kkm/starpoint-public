// audience: internal
// # personal-service-activity-snapshot-history
// 活动领取凭据和逐场统计与玩家资产处于同一快照, 数据库在提交快照时归档新增记录.

use super::{participation_round, ActivityHistoryEntry};
use crate::cn_battle_assets::BattleQuest;
use crate::cn_battle_state::FinishBattleInput;
use crate::database::ActiveSingleQuest;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};

const RECEIPT_FIELDS: &[&str] = &[
    "ranking_reward_receipts", "solo_time_attack_reward_receipts", "carnival_reward_receipts",
    "score_attack_reward_receipts",
    "mission_stage_receipts", "active_mission_receipts", "active_mission_incentive_receipts",
    "pass_card_reward_receipts",
];

// //// 保存本次战斗成绩与结算后的最佳统计 [@x380kkm 2026-09-21] ////
pub(crate) fn record_battle_statistics(
    root: &mut Map<String, Value>,
    active: &ActiveSingleQuest,
    quest: &BattleQuest,
    input: &FinishBattleInput<'_>,
    accomplished: bool,
    created_at: i64,
) -> Result<(), PersonalServiceError> {
    let Some(activity_id) = quest.activity_id.as_deref() else { return Ok(()); };
    let (family, event_id) = activity_id.split_once(':').ok_or_else(|| PersonalServiceError::new("invalid quest activity id"))?;
    let event_id = event_id.parse::<i64>().map_err(|_| PersonalServiceError::new("invalid quest event id"))?;
    let event = crate::cn_activity::event_state(root, family, event_id).cloned();
    let best = match family {
        "ranking" => event.as_ref().and_then(|event| event.get("best_record")).cloned(),
        "raid" => event.as_ref().and_then(|event| event.get("quest_list"))
            .and_then(|quests| quests.get(quest.quest_id.to_string())).cloned(),
        _ => root.get("quest_progress").and_then(|progress| progress.get(quest.category.to_string()))
            .and_then(Value::as_array).into_iter().flatten()
            .find(|entry| entry.get("quest_id").and_then(Value::as_i64) == Some(quest.quest_id)).cloned(),
    };
    let details = json!({
        "play_id": active.play_id, "category": quest.category, "quest_id": quest.quest_id,
        "quest_name": quest.name, "elapsed_time_ms": input.elapsed_time_ms, "score": input.score,
        "is_accomplished": accomplished, "is_multi": input.is_multi,
        "main_character_ids": input.main_character_ids, "unison_character_ids": input.unison_character_ids,
        "best_record": best, "event_statistics": event, "created_at": created_at,
        "period_start_ms": crate::cn_activity::event_period_start(root, family, event_id),
    });
    root.entry("activity_battle_statistics".to_owned()).or_insert_with(|| json!({}))
        .as_object_mut().ok_or_else(|| PersonalServiceError::new("invalid activity battle statistics"))?
        .insert(activity_id.to_owned(), details);
    Ok(())
}

// //// 提取新产生的活动领奖和成绩记录 [@x380kkm 2026-09-21] ////
pub(crate) fn collect_snapshot_events(
    before: &Map<String, Value>,
    after: &Map<String, Value>,
    captured_at: i64,
) -> Result<Vec<ActivityHistoryEntry>, PersonalServiceError> {
    let mut result = Vec::new();
    for (id, vote) in after.get(crate::cn_character_election::VOTES_FIELD)
        .and_then(Value::as_object).into_iter().flatten() {
        if before.get(crate::cn_character_election::VOTES_FIELD).and_then(|votes| votes.get(id)) == Some(vote) { continue; }
        let activity_id = format!("character-election:{id}");
        result.push(ActivityHistoryEntry {
            participation_round: vote.get("participation_round").and_then(Value::as_i64)
                .unwrap_or_else(|| participation_round(after, &activity_id)),
            activity_id, event_key: format!("vote:{id}"), kind: "vote", details: vote.clone(),
            created_at: vote.get("created_at").and_then(Value::as_i64).unwrap_or(captured_at), mail_id: None,
        });
    }
    for field in RECEIPT_FIELDS {
        for (key, value) in after.get(*field).and_then(Value::as_object).into_iter().flatten() {
            if before.get(*field).and_then(|entries| entries.get(key)) == Some(value) { continue; }
            let activity_id = receipt_activity_id(field, key)?;
            let Some(activity_id) = activity_id else { continue; };
            let round = value.get("participation_round").and_then(Value::as_i64).unwrap_or_else(|| {
                if *field == "mission_stage_receipts" {
                    mission_receipt_round(key)
                } else {
                    participation_round(after, &activity_id)
                }
            });
            result.push(ActivityHistoryEntry {
                participation_round: round, activity_id,
                event_key: format!("{field}:{key}"), kind: "claim",
                details: json!({"receipt_field":field,"receipt_id":key,"receipt":value}),
                created_at: value.get("received_at").and_then(Value::as_i64).unwrap_or(captured_at), mail_id: None,
            });
        }
    }
    for (activity_id, value) in after.get("activity_battle_statistics").and_then(Value::as_object).into_iter().flatten() {
        if before.get("activity_battle_statistics").and_then(|entries| entries.get(activity_id)) == Some(value) { continue; }
        let Some(play_id) = value.get("play_id").and_then(Value::as_str) else { continue; };
        result.push(ActivityHistoryEntry {
            participation_round: participation_round(after, activity_id), activity_id: activity_id.clone(),
            event_key: format!("battle:{play_id}"), kind: "statistics", details: value.clone(),
            created_at: value.get("created_at").and_then(Value::as_i64).unwrap_or(captured_at), mail_id: None,
        });
    }
    for (key, receipt) in after.get("shop_request_receipts").and_then(Value::as_object).into_iter().flatten() {
        if before.get("shop_request_receipts").and_then(|entries| entries.get(key)) == Some(receipt) { continue; }
        for activity_id in receipt.get("activity_ids").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
            let mut details = receipt.clone();
            if let Some(details) = details.as_object_mut() { details.remove("response"); }
            result.push(ActivityHistoryEntry {
                participation_round: receipt.get("participation_rounds").and_then(|rounds| rounds.get(activity_id))
                    .and_then(Value::as_i64).unwrap_or_else(|| participation_round(after, activity_id)),
                activity_id: activity_id.to_owned(), event_key: format!("shop:{key}"), kind: "exchange", details,
                created_at: receipt.get("response_time").and_then(Value::as_i64).unwrap_or(captured_at), mail_id: None,
            });
        }
    }
    for (key, receipt) in after.get("optional_exchange_receipts").and_then(Value::as_object).into_iter().flatten() {
        if before.get("optional_exchange_receipts").and_then(|entries| entries.get(key)) == Some(receipt) { continue; }
        let Some(activity_id) = receipt.get("activity_id").and_then(Value::as_str) else { continue; };
        let mut details = receipt.clone();
        if let Some(details) = details.as_object_mut() { details.remove("response"); }
        result.push(ActivityHistoryEntry {
            participation_round: receipt.get("participation_round").and_then(Value::as_i64)
                .unwrap_or_else(|| participation_round(after, activity_id)),
            activity_id: activity_id.to_owned(), event_key: format!("optional-exchange:{key}"),
            kind: "exchange", details,
            created_at: receipt.get("received_at").and_then(Value::as_i64).unwrap_or(captured_at),
            mail_id: None,
        });
    }
    for (family, events) in after.get("cn_activity_state").and_then(|state| state.get("event_families"))
        .and_then(Value::as_object).into_iter().flatten() {
        for (event_id, event) in events.as_object().into_iter().flatten() {
            let activity_id = format!("{family}:{event_id}");
            let previous = before.get("cn_activity_state").and_then(|state| state.get("event_families"))
                .and_then(|families| families.get(family)).and_then(|events| events.get(event_id));
            let round = participation_round(after, &activity_id);
            collect_event_receipts(&mut result, &activity_id, previous, event, None, round, captured_at);
            for (period, scoped) in event.get("periods").and_then(Value::as_object).into_iter().flatten() {
                let old = previous.and_then(|event| event.get("periods")).and_then(|periods| periods.get(period));
                collect_event_receipts(&mut result, &activity_id, old, scoped, Some(period), round, captured_at);
            }
        }
    }
    Ok(result)
}

// //// 按期别保存活动领取历史的独立身份 [@x380kkm 2026-09-27] ////
fn collect_event_receipts(
    result: &mut Vec<ActivityHistoryEntry>,
    activity_id: &str,
    previous: Option<&Value>,
    event: &Value,
    period: Option<&str>,
    round: i64,
    captured_at: i64,
) {
    for (field, receipts) in event.as_object().into_iter().flatten()
        .filter(|(field, _)| field.ends_with("_receipts") || field.ends_with("_receipt"))
    {
        let values = if field.ends_with("_receipts") {
            receipts.as_object().into_iter().flatten().map(|(key, value)| (key.as_str(), value)).collect::<Vec<_>>()
        } else { vec![("reward", receipts)] };
        for (key, value) in values {
            let old = previous.and_then(|event| event.get(field));
            let old = if field.ends_with("_receipts") { old.and_then(|receipts| receipts.get(key)) } else { old };
            if old == Some(value) || value.is_null() { continue; }
            let event_key = match period {
                Some(period) => format!("period:{period}:{field}:{key}"),
                None => format!("{field}:{key}"),
            };
            result.push(ActivityHistoryEntry {
                participation_round: value.get("participation_round").and_then(Value::as_i64).unwrap_or(round),
                activity_id: activity_id.to_owned(), event_key, kind: "claim",
                details: json!({"receipt_field":field,"receipt_id":key,"receipt":value,"period_start_ms":period.and_then(|period| period.parse::<i64>().ok())}),
                created_at: value.get("received_at").and_then(Value::as_i64).unwrap_or(captured_at), mail_id: None,
            });
        }
    }
}

fn receipt_activity_id(field: &str, key: &str) -> Result<Option<String>, PersonalServiceError> {
    if field == "pass_card_reward_receipts" {
        return Ok(key.split(':').next().and_then(|id| id.parse::<i64>().ok()).filter(|id| *id > 0).map(|id| format!("pass-card:{id}")));
    }
    if field == "ranking_reward_receipts" {
        return Ok(key.split(':').next().and_then(|id| id.parse::<i64>().ok()).filter(|id| *id > 0).map(|id| format!("ranking:{id}")));
    }
    if let Some(id) = crate::cn_battle_state::reward_receipt_activity_id(field, key)? { return Ok(Some(id)); }
    crate::cn_mission::receipt_activity_id(field, key)
}

// //// 从任务领取身份读取历史参与轮次 [@x380kkm 2026-09-28] ////
fn mission_receipt_round(key: &str) -> i64 {
    key.rsplit_once(":round:")
        .and_then(|(_, suffix)| suffix.split_once(':'))
        .and_then(|(round, _)| round.parse::<i64>().ok())
        .filter(|round| *round >= 0)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    // //// 归档保留期别的任务凭据时使用领取时的轮次 [@x380kkm 2026-09-28] ////
    #[test]
    fn preserved_mission_receipts_keep_their_recorded_rounds() {
        let root = json!({
            "activity_participation_rounds": {"raid:1":9},
            "mission_stage_receipts": {
                "3:400001:event:1694059200000:1": {"received_at":1},
                "3:400001:event:regional:1637985600000:round:2:1": {"received_at":2},
                "3:400001:event:regional:1637985600000:round:4:1": {"received_at":3,"participation_round":4},
            },
        });
        let events = collect_snapshot_events(&Map::new(), root.as_object().unwrap(), 10).unwrap();
        let rounds = events.iter().map(|event| (event.created_at, event.participation_round))
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(rounds, std::collections::BTreeMap::from([(1,0),(2,2),(3,4)]));
        assert!(events.iter().all(|event| event.activity_id == "raid:1"));
    }
}
