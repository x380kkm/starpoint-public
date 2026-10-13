// audience: internal
// # personal-service-cn-activity-battle
// 活动入场费用, 任务资格和收据随当前局原子保存, 同一局重试使用原始响应.

use super::{error_response, state::ActivityPlayer};
use crate::cn::{msgpack_response_at, server_time};
use crate::cn_battle_assets::BattleQuest;
use crate::cn_stamina::entry::{deduct_single_entry_resources, EntryFailure};
use crate::cn_tutorial::encode_player_data;
use crate::database::{ActiveSingleQuest, ServiceDatabase};
use crate::http::HttpResponse;
use crate::PersonalServiceError;
use serde_json::{json, Value};

// //// 重放当前活动战斗的入场收据 [@x380kkm 2026-09-26] ////
pub(super) fn resume(
    player: &ActivityPlayer,
    database: &ServiceDatabase,
    viewer_id: i64,
    requested: &ActiveSingleQuest,
) -> Result<Option<HttpResponse>, PersonalServiceError> {
    let Some(active) = database.get_active_single_quest(player.account_id)? else {
        return Ok(None);
    };
    if active.play_id != requested.play_id {
        return Ok(None);
    }
    if active.quest_id != requested.quest_id || active.category != requested.category {
        return Ok(Some(error_response(
            "409 Conflict",
            "active_quest_mismatch",
        )));
    }
    let mut data = if let Some(receipt) =
        database.single_battle_start_receipt(player.account_id, &active.play_id)?
    {
        serde_json::from_str::<Value>(&receipt).map_err(|error| {
            PersonalServiceError::new(format!("invalid activity battle start receipt: {error}"))
        })?
    } else {
        response(&active, server_time(database)?)
    };
    let started_at = data
        .get("start_time")
        .and_then(Value::as_i64)
        .unwrap_or(server_time(database)?);
    if active.category != 24 {
        data.as_object_mut().map(|data| data.remove("start_time"));
    }
    msgpack_response_at(viewer_id, false, started_at, data).map(Some)
}

// //// 原子保存活动入场费用与本局任务资格 [@x380kkm 2026-09-26] ////
pub(super) fn start(
    player: &mut ActivityPlayer,
    database: &mut ServiceDatabase,
    active: &ActiveSingleQuest,
    quest: &BattleQuest,
    started_at: i64,
) -> Result<Result<Value, HttpResponse>, PersonalServiceError> {
    let root = player.root_mut()?;
    let resources = match deduct_single_entry_resources(root, quest, started_at)? {
        Ok(resources) => resources,
        Err(EntryFailure::InsufficientItem) => {
            return Ok(Err(error_response(
                "400 Bad Request",
                "insufficient_entry_items",
            )));
        }
        Err(EntryFailure::InsufficientStamina) => {
            return Ok(Err(error_response(
                "400 Bad Request",
                "insufficient_stamina",
            )));
        }
    };
    let mut active_missions = crate::cn_mission::record_active_quest_challenge(
        root,
        database,
        active.category,
        active.quest_id,
    )?;
    active_missions.extend(crate::cn_mission::record_active_counters(
        root,
        database,
        &[("used_stamina_count", resources.stamina_used)],
    )?);
    crate::cn_mission::capture_battle_mission_window(root, database, &active.play_id, started_at)?;
    let mut data = response(active, started_at);
    data["user_info"]["stamina"] = resources.stamina.into();
    data["user_info"]["stamina_heal_time"] = resources.stamina_heal_time.into();
    data["item_list"] = Value::Object(resources.items);
    data["active_mission_list"] = json!(active_missions);
    let mut receipt = data.clone();
    receipt["start_time"] = Value::from(started_at);
    database.start_active_single_quest_with_receipt(
        player.account_id,
        &encode_player_data(&player.data)?,
        active,
        &serde_json::to_string(&receipt).map_err(|error| {
            PersonalServiceError::new(format!(
                "failed to encode activity battle start receipt: {error}"
            ))
        })?,
    )?;
    Ok(Ok(data))
}

// //// 构造活动开战的客户端响应 [@x380kkm 2026-09-26] ////
fn response(active: &ActiveSingleQuest, started_at: i64) -> Value {
    if active.category == 24 {
        json!({
            "user_info": {"last_main_quest_id": active.quest_id},
            "is_multi": "single",
            "start_time": started_at,
            "quest_name": "",
        })
    } else {
        json!({})
    }
}
