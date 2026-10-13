// audience: internal
// # personal-service-rescue-catalog
// 救援目录由客户端协战主表生成, 同时保留关卡和父活动的原始时间窗口.

use super::RescueQuest;
use crate::PersonalServiceError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Deserialize, Serialize)]
pub(super) struct RescueQuestDefinition {
    #[serde(flatten)]
    pub(super) quest: RescueQuest,
    name: String,
    difficulty_id: i64,
    difficulty: String,
    category_name: String,
    pub(super) activity_id: Option<String>,
    activity_name: Option<String>,
    pub(super) start_time: Option<i64>,
    pub(super) end_time: Option<i64>,
    pub(super) activity_start_time: Option<i64>,
    pub(super) activity_end_time: Option<i64>,
}

// //// 读取按分类和关卡索引的救援目录 [@x380kkm 2026-09-23] ////
pub(super) fn quests(
) -> Result<&'static BTreeMap<RescueQuest, RescueQuestDefinition>, PersonalServiceError> {
    static CATALOG: OnceLock<Result<BTreeMap<RescueQuest, RescueQuestDefinition>, String>> =
        OnceLock::new();
    CATALOG
        .get_or_init(|| {
            serde_json::from_str::<Vec<RescueQuestDefinition>>(include_str!(
                "../../assets/cn-rescue-quests.json"
            ))
            .map(|quests| {
                quests
                    .into_iter()
                    .map(|quest| (quest.quest, quest))
                    .collect()
            })
            .map_err(|error| format!("failed to decode rescue quest catalog: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}
// //// /读取按分类和关卡索引的救援目录 ////
