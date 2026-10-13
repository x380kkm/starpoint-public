// audience: internal
// # personal-service-cn-mission-awake
// 觉醒任务进度与领取记录共同决定资格. 载入, 动作响应和节点操作使用同一判定.

use super::{awake_progress, build_context, mission_catalog, mission_character_id, MissionCatalog};
use crate::cn_character::complete_character_response_times;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

struct AwakeSummary {
    missions: Vec<Value>,
    qualifications: BTreeMap<i64, Value>,
}

// //// 读取显式领取凭据 [@x380kkm 2026-09-29] ////
fn has_receipt(root: &Map<String, Value>, field: &str, key: &str) -> bool {
    root.get(field)
        .and_then(|receipts| receipts.get(key))
        .is_some_and(|value| value == &Value::Bool(true) || value.is_object())
}

// //// 读取存档中的主动任务阶段 [@x380kkm 2026-09-29] ////
fn recorded_mission(root: &Map<String, Value>, mission_id: i64) -> impl Iterator<Item = &Value> {
    root.get("all_active_mission_list")
        .and_then(|missions| missions.get(mission_id.to_string()))
        .into_iter()
        .chain(
            root.get("active_mission_list")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(move |mission| {
                    mission.get("mission_id").and_then(Value::as_i64) == Some(mission_id)
                }),
        )
}

// //// 合并主动任务的两种阶段存储形式 [@x380kkm 2026-09-29] ////
pub(super) fn has_active_receipt(root: &Map<String, Value>, mission_id: i64, stage: i64) -> bool {
    has_receipt(
        root,
        "active_mission_receipts",
        &format!("{mission_id}:{stage}"),
    ) || recorded_mission(root, mission_id).any(|mission| match mission.get("stages") {
        Some(Value::Object(stages)) => {
            stages.get(&stage.to_string()).and_then(Value::as_bool) == Some(true)
        }
        Some(Value::Array(stages)) => stages.iter().any(|entry| {
            entry.get("stage").and_then(Value::as_i64) == Some(stage)
                && entry.get("received").and_then(Value::as_bool) == Some(true)
        }),
        _ => false,
    })
}

// //// 按两种领奖入口读取觉醒任务领取状态 [@x380kkm 2026-09-29] ////
pub(crate) fn awake_stage_received(root: &Map<String, Value>, mission_id: i64, stage: i64) -> bool {
    has_receipt(
        root,
        "mission_stage_receipts",
        &format!("9:{mission_id}:{stage}"),
    ) || has_active_receipt(root, mission_id, stage)
}

// //// 将已记录进度和已领取阶段合并为觉醒任务进度下限 [@x380kkm 2026-09-29] ////
pub(super) fn merge_recorded_progress(
    root: &Map<String, Value>,
    catalog: &MissionCatalog,
    progress: &mut BTreeMap<String, i64>,
) {
    for mission in catalog.categories.get(&9).into_iter().flatten() {
        let recorded = recorded_mission(root, mission.id)
            .filter_map(|entry| {
                entry
                    .get("progress_value")
                    .or_else(|| entry.get("progress"))
                    .and_then(Value::as_i64)
            })
            .chain(
                mission
                    .stages
                    .iter()
                    .filter(|stage| awake_stage_received(root, mission.id, stage.stage))
                    .filter_map(|stage| stage.target),
            )
            .max()
            .unwrap_or_default();
        if recorded > 0 {
            let value = progress.entry(format!("9:{}", mission.id)).or_default();
            *value = (*value).max(recorded);
        }
    }
}

// //// 按角色的完整任务集合计算觉醒资格 [@x380kkm 2026-09-29] ////
fn summary(root: &Map<String, Value>) -> Result<AwakeSummary, PersonalServiceError> {
    let catalog = mission_catalog()?;
    let context = build_context(root, catalog, &BTreeMap::new())?;
    let mut missions = Vec::new();
    let mut completed = BTreeMap::new();
    for mission in catalog.categories.get(&9).into_iter().flatten() {
        let character_key = mission_character_id(mission.id);
        if root
            .get("user_character_list")
            .and_then(|characters| characters.get(&character_key))
            .is_none()
        {
            continue;
        }
        let character_id = character_key
            .parse::<i64>()
            .map_err(|_| PersonalServiceError::new("CN awake character id is invalid"))?;
        let progress = awake_progress(mission.id, &context, catalog);
        let all_complete = completed.entry(character_id).or_insert(true);
        let stages = mission.stages.iter().map(|stage| {
            *all_complete &= stage.target.is_some_and(|target| progress >= target);
            json!({"stage":stage.stage,"received":awake_stage_received(root, mission.id, stage.stage)})
        }).collect::<Vec<_>>();
        missions.push(json!({"mission_id":mission.id,"progress_value":progress,"stages":stages}));
    }
    let qualifications = completed
        .into_iter()
        .filter(|(_, complete)| *complete)
        .map(|(id, _)| (id, json!({"1":1})))
        .collect();
    Ok(AwakeSummary {
        missions,
        qualifications,
    })
}

// //// 将觉醒资格合并到角色响应并保留同次操作的角色字段 [@x380kkm 2026-09-29] ////
pub(super) fn apply_response(
    response: &mut Value,
    root: &Map<String, Value>,
    response_time: i64,
) -> Result<(), PersonalServiceError> {
    let qualifications = summary(root)?.qualifications;
    if qualifications.is_empty() {
        complete_character_response_times(response, root, response_time);
        return Ok(());
    }
    let characters = response
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("CN mission response is invalid"))?
        .entry("character_list".to_owned())
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("CN mission character response is invalid"))?;
    for (character_id, qualification) in qualifications {
        if let Some(character) = characters
            .iter_mut()
            .find(|entry| entry.get("character_id").and_then(Value::as_i64) == Some(character_id))
        {
            let character = character.as_object_mut().ok_or_else(|| {
                PersonalServiceError::new("CN mission character response is invalid")
            })?;
            character.insert("mana_board_awake".to_owned(), qualification);
        } else {
            characters.push(json!({
                "character_id":character_id,"mana_board_awake":qualification,
            }));
        }
    }
    complete_character_response_times(response, root, response_time);
    Ok(())
}

// //// 将完整觉醒任务和资格写入载入响应 [@x380kkm 2026-09-29] ////
pub(crate) fn inject_load_awake_summary(
    player_data: &mut Value,
    serialized_player_data: &str,
) -> Result<(), PersonalServiceError> {
    let internal = serde_json::from_str::<Value>(serialized_player_data).map_err(|error| {
        PersonalServiceError::new(format!("failed to decode CN mission player data: {error}"))
    })?;
    let internal = internal
        .as_object()
        .ok_or_else(|| PersonalServiceError::new("stored CN mission player data is invalid"))?;
    let summary = summary(internal)?;
    let root = player_data
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN player data is not an object"))?;
    root.insert(
        "active_mission_list".to_owned(),
        Value::Array(summary.missions),
    );
    if let Some(characters) = root
        .get_mut("user_character_list")
        .and_then(Value::as_object_mut)
    {
        for character in characters.values_mut().filter_map(Value::as_object_mut) {
            character.remove("mana_board_awake");
        }
        for (id, qualification) in summary.qualifications {
            if let Some(character) = characters
                .get_mut(&id.to_string())
                .and_then(Value::as_object_mut)
            {
                character.insert("mana_board_awake".to_owned(), qualification);
            }
        }
    }
    Ok(())
}

// //// 返回节点觉醒操作使用的角色资格 [@x380kkm 2026-09-29] ////
pub(crate) fn character_awake_qualification(
    root: &Map<String, Value>,
    character_id: i64,
) -> Result<Value, PersonalServiceError> {
    Ok(summary(root)?
        .qualifications
        .remove(&character_id)
        .unwrap_or_else(|| json!({})))
}
