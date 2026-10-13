// audience: internal
// # personal-service-cn-player-history-capture
// 统计值按回忆录参与轮次保存. captured_at 使用真实墙钟秒, 卡片显示选项使用当前设置.

use super::{catalog, statistics};
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

const CAPTURES_FIELD: &str = "player_history_captures";

#[derive(Deserialize, Serialize)]
struct CapturedStatistics {
    captured_at: i64,
    topic_values: Map<String, Value>,
}

// //// 固定当前轮次的真实统计并应用可编辑的主题显示设置 [@x380kkm 2026-09-26] ////
pub(super) fn topic_list(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    account_id: i64,
    history_id: i64,
    visible: &BTreeMap<i64, bool>,
) -> Result<Map<String, Value>, PersonalServiceError> {
    let activity_id = format!("player-history:{history_id}");
    let round = crate::activity_history::participation_round(root, &activity_id);
    let key = format!("{activity_id}:round:{round}");
    let captured = match root
        .get(CAPTURES_FIELD)
        .and_then(|captures| captures.get(&key))
    {
        Some(value) => {
            serde_json::from_value::<CapturedStatistics>(value.clone()).map_err(|error| {
                PersonalServiceError::new(format!("invalid CN player history capture: {error}"))
            })?
        }
        None => {
            let captured = CapturedStatistics {
                captured_at: database.current_wall_time_millis()?.div_euclid(1000),
                topic_values: statistics::topic_values(root, database, account_id, history_id)?,
            };
            let encoded = serde_json::to_value(&captured)
                .map_err(|error| PersonalServiceError::new(error.to_string()))?;
            root.entry(CAPTURES_FIELD.to_owned())
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .ok_or_else(|| PersonalServiceError::new("CN player history captures are invalid"))?
                .insert(key, encoded);
            captured
        }
    };
    let topics = catalog::definitions()?
        .topics
        .get(&history_id)
        .ok_or_else(|| PersonalServiceError::new("CN player history topics are missing"))?;
    topics.iter().map(|topic| {
        let key = topic.index.to_string();
        let values = captured.topic_values.get(&key)
            .ok_or_else(|| PersonalServiceError::new("CN player history captured topic is missing"))?;
        Ok((key, json!({
            "is_visible": visible.get(&topic.index).copied().unwrap_or(topic.toggle_default),
            "value_list": values,
        })))
    }).collect()
}
