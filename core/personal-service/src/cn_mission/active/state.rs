// audience: internal
// # personal-service-cn-active-mission-state
//
// 角色与编队条件读取玩家当前状态. 登录任务按活动范围记录北京时间 05:00 对应的日期.

use super::{advance, catalog, complete_dependencies, is_available, Mission};
use crate::cn_mission::conditions::{
    character_level, fully_awakened_equipment_count, has_complete_second_board,
};
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};

fn has_party_slot(root: &Map<String, Value>, field: &str) -> bool {
    root.get("user_party_group_list")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|groups| groups.values())
        .filter_map(|group| group.get("list").and_then(Value::as_object))
        .flat_map(|parties| parties.values())
        .filter_map(|party| party.get(field))
        .any(|slots| {
            let values: Vec<&Value> = match slots {
                Value::Array(values) => values.iter().collect(),
                Value::Object(values) => values.values().collect(),
                _ => Vec::new(),
            };
            values
                .into_iter()
                .any(|id| id.as_i64().is_some_and(|id| id > 0))
        })
}

fn state_progress(
    root: &Map<String, Value>,
    mission: &Mission,
) -> Result<Option<i64>, PersonalServiceError> {
    let characters = root.get("user_character_list").and_then(Value::as_object);
    let character_ids = characters
        .into_iter()
        .flat_map(|characters| characters.keys())
        .filter_map(|id| id.parse::<i64>().ok())
        .collect::<Vec<_>>();
    Ok(match mission.pattern.as_str() {
        "set_party_character" => Some(i64::from(has_party_slot(root, "character_ids"))),
        "equipped_first_time" => Some(i64::from(has_party_slot(root, "equipment_ids"))),
        "set_unison_first_time" => Some(i64::from(has_party_slot(root, "unison_character_ids"))),
        "set_soul_sphere_count" => Some(i64::from(has_party_slot(root, "ability_soul_ids"))),
        "characters_count" => Some(
            character_ids
                .into_iter()
                .filter(|id| {
                    mission
                        .character_id
                        .map_or(true, |required| required == *id)
                })
                .count() as i64,
        ),
        "character_level_achievement" => Some(
            character_ids
                .into_iter()
                .map(|id| character_level(root, id))
                .max()
                .unwrap_or_default(),
        ),
        "evolved_character_count" => Some(
            characters
                .into_iter()
                .flat_map(|characters| characters.values())
                .filter(|character| {
                    character
                        .get("evolution_level")
                        .and_then(Value::as_i64)
                        .unwrap_or_default()
                        > 0
                })
                .count() as i64,
        ),
        "level_max_equipment_count" => Some(fully_awakened_equipment_count(root)?),
        "mana_board_2nd_complete_count" => {
            let mut count = 0;
            for id in character_ids {
                count += i64::from(has_complete_second_board(root, id)?);
            }
            Some(count)
        }
        _ => None,
    })
}

// //// 根据玩家当前状态和活动登录日期刷新主动任务 [@x380kkm 2026-09-21] ////
pub(crate) fn refresh_active_missions(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
) -> Result<Vec<Value>, PersonalServiceError> {
    let day = crate::cn_mission::window::game_day(
        database.current_server_time_millis()?.div_euclid(1000),
    );
    let mut updates = Vec::new();
    for (id, mission) in &catalog()?.missions {
        if !is_available(root, database, mission)? {
            continue;
        }
        let value = if mission.pattern == "total_login_days" {
            let all_days = root
                .entry("active_mission_login_days".to_owned())
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .ok_or_else(|| {
                    PersonalServiceError::new("stored active mission login dates are invalid")
                })?;
            let days = all_days
                .entry(mission.event_id.to_string())
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .ok_or_else(|| {
                    PersonalServiceError::new("stored active mission login dates are invalid")
                })?;
            if !days.iter().any(|value| value.as_i64() == Some(day)) {
                days.push(json!(day));
            }
            Some(days.len() as i64)
        } else {
            state_progress(root, mission)?
        };
        if let Some(value) = value {
            if let Some(update) = advance(root, *id, value)? {
                updates.push(update);
            }
        }
    }
    complete_dependencies(root, database, &mut updates)?;
    Ok(updates)
}
// //// /根据玩家当前状态和活动登录日期刷新主动任务 ////
