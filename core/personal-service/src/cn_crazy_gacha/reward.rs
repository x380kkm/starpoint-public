// audience: internal
// # personal-service-cn-crazy-gacha-reward
// 选定组按普通角色扭蛋结算重复素材和抽取任务.

use super::{Mutation, ResultRequest};
use crate::cn_character_reward::{grant_character, record_character_encyclopedia_state};
use crate::database::{ReceiveHistoryEntry, ServiceDatabase};
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};

// //// 发放所选角色并返回每个角色的完整最终状态 [@x380kkm 2026-09-23] ////
pub(super) fn grant(
    root: &mut Map<String, Value>,
    viewer_id: i64,
    character_ids: &[i64],
    response_time: i64,
) -> Result<Mutation, PersonalServiceError> {
    let mut items = Map::new();
    let mut encyclopedia = Map::new();
    let mut characters = Map::new();
    let mut history = Vec::new();
    for &character_id in character_ids {
        let reward = grant_character(root, viewer_id, character_id, response_time)?;
        characters.insert(character_id.to_string(), reward.character);
        if let Some(item) = reward.duplicate_item {
            items.insert(item.id.to_string(), json!(item.total));
        }
        encyclopedia.extend(record_character_encyclopedia_state(root, character_id)?);
        let mut entry = ReceiveHistoryEntry::reward(5, Some(character_id), 1);
        entry.subject = Some("乐享扭蛋".to_owned());
        history.push(entry);
    }
    Ok(Mutation {
        response: json!({
            "character_list": characters.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
            "item_list": items,
            "encyclopedia_info": encyclopedia,
        }),
        history,
    })
}
// //// /发放所选角色并返回每个角色的完整最终状态 ////

// //// 以最终获得的十名角色更新抽取任务 [@x380kkm 2026-09-23] ////
pub(super) fn record_missions(
    root: &mut Map<String, Value>,
    request: &ResultRequest,
    database: &ServiceDatabase,
    response_time: i64,
    response: &mut Value,
) -> Result<(), PersonalServiceError> {
    let account_id = database
        .player_account_id_for_viewer(request.viewer_id)?
        .ok_or_else(|| PersonalServiceError::new("CN crazy gacha account is missing"))?;
    let mut active = crate::cn_mission::record_active_counters(
        root,
        database,
        &[("total_gacha_character_count", 10)],
    )?;
    let missions = crate::cn_mission::record_economic_actions(
        root,
        &crate::cn_mission::plan_economic_missions(database, account_id)?,
        crate::cn_mission::EconomicActions {
            character_draws: 10,
            ..crate::cn_mission::EconomicActions::default()
        },
        response_time,
    )?;
    crate::cn_mission::apply_mission_reward_response(
        response,
        root,
        &missions,
        request.viewer_id,
        response_time,
    )?;
    active.extend(crate::cn_mission::refresh_active_missions(root, database)?);
    response["mission_info"] = json!(missions);
    response["active_mission_list"] = json!(active);
    Ok(())
}
// //// /以最终获得的十名角色更新抽取任务 ////
