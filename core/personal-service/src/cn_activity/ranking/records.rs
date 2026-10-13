// audience: internal
// # personal-service-cn-ranking-records
//
// 竞速成绩按客户端规则比较: 通关优于未通关, 通关比较用时, 未通关比较得分.
// 各期最佳成绩和关卡通关进度分别保存, 管理台重置当前期别的参与记录.

use super::Event;
use crate::cn_battle_assets::BattleQuest;
use crate::cn_battle_state::FinishBattleInput;
use crate::PersonalServiceError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Record {
    pub(super) elapsed_time_ms: i64,
    pub(super) is_accomplished: bool,
    pub(super) score: i64,
    leader_character_id: Option<i64>,
}

impl Record {
    pub(super) fn wire_value(&self) -> Value {
        json!({"elapsed_time_ms": self.elapsed_time_ms, "is_accomplished": self.is_accomplished, "score": self.score})
    }

    pub(super) fn leader(&self) -> Option<i64> { self.leader_character_id }

    fn is_better_than(&self, previous: &Self) -> bool {
        if self.is_accomplished != previous.is_accomplished { return self.is_accomplished; }
        if self.is_accomplished { self.elapsed_time_ms < previous.elapsed_time_ms } else { self.score > previous.score }
    }
}

// //// 读取活动最佳成绩并兼容已有竞速通关记录 [@x380kkm 2026-09-21] ////
pub(super) fn best_record(root: &Map<String, Value>, event_id: i64, event: &Event) -> Option<Record> {
    let state = super::super::state::event_state(root, "ranking", event_id);
    let stored = state
        .and_then(|state| state.get("best_record"))
        .and_then(|record| serde_json::from_value::<Record>(record.clone()).ok());
    if super::super::state::event_period_start(root, "ranking", event_id).is_some()
        || state.and_then(|state| state.get("quest_progress_imported")).and_then(Value::as_bool) == Some(true)
    {
        return stored;
    }
    select_best(stored, legacy_best_record(root, event))
}

fn legacy_best_record(root: &Map<String, Value>, event: &Event) -> Option<Record> {
    let legacy = root.get("quest_progress").and_then(|progress| progress.get("11"))
        .and_then(Value::as_array).into_iter().flatten()
        .filter(|entry| entry.get("quest_id").and_then(Value::as_i64).is_some_and(|id| event.quest_ids.contains(&id)))
        .filter_map(|entry| {
            let elapsed_time_ms = entry.get("best_elapsed_time_ms").and_then(Value::as_i64).filter(|time| *time > 0)?;
            Some(Record {
                elapsed_time_ms, is_accomplished: true,
                score: entry.get("best_elapsed_score").or_else(|| entry.get("high_score")).and_then(Value::as_i64).unwrap_or_default(),
                leader_character_id: entry.get("best_leader_character_id").and_then(Value::as_i64)
                    .filter(|id| crate::cn_character_presentation::is_valid_character(*id))
                    .or_else(|| entry.get("leader_character_id").and_then(Value::as_i64)),
            })
        }).min_by_key(|record| record.elapsed_time_ms);
    legacy
}

fn select_best(stored: Option<Record>, legacy: Option<Record>) -> Option<Record> {
    match (stored, legacy) {
        (Some(stored), Some(legacy)) => Some(if legacy.is_better_than(&stored) { legacy } else { stored }),
        (stored, legacy) => stored.or(legacy),
    }
}
// //// /读取活动最佳成绩并兼容已有竞速通关记录 ////

// //// 固定原始期别从全局通关记录读取的最佳成绩 [@x380kkm 2026-09-27] ////
pub(crate) fn preserve_original_record(root: &mut Map<String, Value>, event_id: i64) -> Result<(), PersonalServiceError> {
    let Some(event) = super::catalog()?.get(&event_id) else { return Ok(()); };
    let original = super::super::state::event_container(root, "ranking", event_id);
    if original.and_then(|event| event.get("quest_progress_imported")).and_then(Value::as_bool) == Some(true) {
        return Ok(());
    }
    let stored = original.and_then(|event| event.get("best_record"))
        .and_then(|record| serde_json::from_value::<Record>(record.clone()).ok());
    let best = select_best(stored, legacy_best_record(root, event));
    let original = super::super::state::event_container_mut(root, "ranking", event_id)?;
    if let Some(best) = best {
        original.insert("best_record".to_owned(), serde_json::to_value(best)
            .map_err(|error| PersonalServiceError::new(format!("failed to encode ranking record: {error}")))?);
    }
    original.insert("quest_progress_imported".to_owned(), Value::Bool(true));
    Ok(())
}

// //// 保存完整或部分竞速成绩并返回结算所需的前后记录 [@x380kkm 2026-09-21] ////
pub(crate) fn record_battle_result(
    root: &mut Map<String, Value>, quest: &BattleQuest, input: &FinishBattleInput<'_>, is_accomplished: bool,
) -> Result<Value, PersonalServiceError> {
    if quest.category != 11 { return Ok(Value::Null); }
    let Some((event_id, event)) = super::catalog()?.iter().find(|(_, event)| event.quest_ids.contains(&quest.quest_id)) else {
        return Err(PersonalServiceError::new("CN ranking quest event is missing"));
    };
    let previous = super::super::state::event_state(root, "ranking", *event_id)
        .and_then(|state| state.get("last_record"))
        .and_then(|record| serde_json::from_value::<Record>(record.clone()).ok());
    let previous_best = best_record(root, *event_id, event);
    let current = Record {
        elapsed_time_ms: input.elapsed_time_ms, is_accomplished, score: input.score,
        leader_character_id: Some(crate::cn_character_presentation::leader_character_id(root,
            input.main_character_ids.first().copied().flatten())),
    };
    let best = previous_best.as_ref().filter(|previous| !current.is_better_than(previous)).unwrap_or(&current);
    let state = crate::cn_activity::activate_battle_event_state(root, "ranking", *event_id)?;
    state.insert("best_record".to_owned(), serde_json::to_value(best)
        .map_err(|error| PersonalServiceError::new(format!("failed to encode ranking record: {error}")))?);
    state.insert("last_record".to_owned(), serde_json::to_value(&current)
        .map_err(|error| PersonalServiceError::new(format!("failed to encode ranking record: {error}")))?);
    Ok(json!({"best_record": best.wire_value(), "previous": previous.as_ref().map(Record::wire_value), "is_record_valid": true}))
}
// //// /保存完整或部分竞速成绩并返回结算所需的前后记录 ////
