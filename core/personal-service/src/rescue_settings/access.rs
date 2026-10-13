// audience: internal
// # personal-service-rescue-access
// 救援系统解锁条件来自 game_system_unlock 主表, 通关状态使用玩家存档的 finished 字段.

use super::{catalog, RescueQuest};
use crate::cn_tutorial::decode_player_data;
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::OnceLock;

// //// 核对铃铛系统的真实玩家通关条件 [@x380kkm 2026-09-25] ////
fn player_can_receive_rescue(player: &Value) -> Result<bool, PersonalServiceError> {
    static UNLOCK_QUESTS: OnceLock<Result<Vec<RescueQuest>, String>> = OnceLock::new();
    let requirements = UNLOCK_QUESTS
        .get_or_init(|| {
            serde_json::from_str(include_str!("../../assets/cn-rescue-unlock.json"))
                .map_err(|error| format!("failed to decode rescue unlock quests: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    Ok(requirements.iter().all(|quest| {
        player
            .get("quest_progress")
            .and_then(|progress| progress.get(quest.category_id.to_string()))
            .and_then(Value::as_array)
            .is_some_and(|progress| {
                progress.iter().any(|entry| {
                    entry.get("quest_id").and_then(Value::as_i64) == Some(quest.quest_id)
                        && entry.get("finished").and_then(Value::as_bool) == Some(true)
                })
            })
    }))
}

// //// 读取当前存档已解锁且处于开放期的救援关卡 [@x380kkm 2026-09-25] ////
pub(crate) fn eligible_quest_keys(
    database: &ServiceDatabase,
) -> Result<BTreeSet<(i64, i64)>, PersonalServiceError> {
    let Some(account_id) = database.attention_owner_account()? else {
        return Ok(BTreeSet::new());
    };
    let player = decode_player_data(&database.activity_player_snapshot(account_id)?)?;
    if !player_can_receive_rescue(&player)? {
        return Ok(BTreeSet::new());
    }
    let open = crate::cn_quest::open_quest_keys(database)?;
    Ok(catalog::quests()?
        .keys()
        .map(|quest| (quest.category_id, quest.quest_id))
        .filter(|quest| open.contains(quest))
        .collect())
}

// //// 对照玩家解锁和当前窗口校验新的救援入场 [@x380kkm 2026-09-25] ////
pub(crate) fn can_enter_rescue_quest(
    database: &ServiceDatabase,
    player: &Value,
    quest: &RescueQuest,
) -> Result<bool, PersonalServiceError> {
    Ok(player_can_receive_rescue(player)?
        && catalog::quests()?.contains_key(quest)
        && crate::cn_quest::is_quest_open(database, quest.category_id, quest.quest_id)?)
}
