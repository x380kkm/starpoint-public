// audience: internal
// # personal-service-cn-raid-battle
// 团队战结算记录个人关卡讨伐次数和最佳队伍, 全局领主状态由服务数据库提供.

use super::{get_or_create_object, played_party, FinishBattleInput};
use crate::cn_activity;
use crate::cn_battle_assets::BattleQuest;
use crate::database::RaidBossState;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};

// //// 记录团队战通关次数并返回领主状态 [@x380kkm 2026-09-21] ////
pub(super) fn apply_raid_event(
    root: &mut Map<String, Value>,
    quest: &BattleQuest,
    input: &FinishBattleInput<'_>,
    boss: Option<&RaidBossState>,
) -> Result<Value, PersonalServiceError> {
    let Some(event_id) = quest.raid_event_id.or(quest.event_id) else {
        return Ok(Value::Null);
    };
    let party = played_party(root, input);
    let event = cn_activity::activate_battle_event_state(root, "raid", event_id)?;
    let quest_key = quest.quest_id.to_string();
    let record = get_or_create_object(event, "quest_list")?
        .entry(quest_key.clone())
        .or_insert_with(|| json!({"kill_count": 0}));
    let record = record
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN raid quest record is invalid"))?;
    let mut kill_count = record
        .get("kill_count")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let mut new_record = false;
    let accomplished = input.is_accomplished && !input.is_lose;
    if accomplished {
        kill_count = kill_count.checked_add(1).ok_or_else(|| {
            PersonalServiceError::new("CN raid kill count exceeds the supported range")
        })?;
        record.insert("kill_count".to_owned(), json!(kill_count));
        new_record = record
            .get("high_score")
            .and_then(Value::as_i64)
            .map_or(true, |best| input.score > best);
        if new_record {
            record.insert("high_score".to_owned(), json!(input.score));
            record.insert("elapsed_time_ms".to_owned(), json!(input.elapsed_time_ms));
        }
    }
    if accomplished {
        get_or_create_object(event, "folder_played_party_list")?
            .insert(quest_key.clone(), party.clone());
    }
    if new_record {
        get_or_create_object(event, "quest_best_party_list")?.insert(quest_key, party);
    }
    Ok(json!({
        "auto_start_point": event.get("auto_start_point").and_then(Value::as_i64).unwrap_or(0),
        "is_out_of_period": false,
        "quest_boss": {"kill_count": kill_count},
        "raid_boss": {"hp_percentage": boss.map_or(100, |boss| boss.hp_percentage), "total_kill_count": boss.map_or(0, |boss| boss.total_kill_count)},
    }))
}
