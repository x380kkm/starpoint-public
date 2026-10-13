// audience: internal
// # personal-service-cn-player-history-catalog
// 回忆录与背景编号来自四份 CN 资源主表. 服务端窗口使用 Unix 秒, 管理目录使用毫秒.

use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Deserialize)]
pub(super) struct Definitions {
    pub(super) histories: Vec<History>,
    pub(super) topics: BTreeMap<i64, Vec<Topic>>,
    pub(super) backgrounds: Vec<Background>,
    pub(super) challenge_single_bosses: Vec<ChallengeBoss>,
    pub(super) milestones: Vec<Milestone>,
    pub(super) equipment_max_enhancement: BTreeMap<i64, i64>,
}
#[derive(Deserialize)]
pub(super) struct History {
    pub(super) id: i64,
    pub(super) start_time: i64,
    pub(super) end_time: i64,
}
#[derive(Deserialize)]
pub(super) struct Topic {
    pub(super) index: i64,
    pub(super) aggregation_target: i64,
    pub(super) toggle_default: bool,
}
#[derive(Deserialize)]
pub(super) struct Background {
    pub(super) id: i64,
    pub(super) is_default: bool,
}
#[derive(Deserialize)]
pub(super) struct ChallengeBoss {
    pub(super) id: i64,
    pub(super) degree_mission_id: i64,
}
#[derive(Deserialize)]
pub(super) struct Milestone {
    pub(super) category: i64,
    pub(super) mission_id: i64,
    pub(super) stage: i64,
    pub(super) pattern: String,
    pub(super) chapter_id: Option<i64>,
    pub(super) character_id: Option<i64>,
}

pub(super) fn definitions() -> Result<&'static Definitions, PersonalServiceError> {
    static DATA: OnceLock<Result<Definitions, String>> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!("../../assets/cn-player-history.json"))
            .map_err(|error| error.to_string())
    })
    .as_ref()
    .map_err(|error| PersonalServiceError::new(error.clone()))
}

// //// 向管理目录提供主表回忆录和默认展示窗口 [@x380kkm 2026-09-26] ////
pub(crate) fn activity_definitions() -> Result<Vec<Value>, PersonalServiceError> {
    Ok(definitions()?.histories.iter().map(|history| json!({
        "activity_id": format!("player-history:{}", history.id), "kind":"player-history", "name":"回忆录",
        "tags":["CN", "Player history"], "description":"", "image_candidates":[],
        "default_start_at_ms":history.start_time * 1000, "default_end_at_ms":history.end_time.saturating_add(1) * 1000,
    })).collect())
}

pub(crate) fn default_activity_window(
    activity_id: &str,
) -> Result<Option<(i64, i64)>, PersonalServiceError> {
    let Some(id) = activity_id
        .strip_prefix("player-history:")
        .and_then(|id| id.parse::<i64>().ok())
    else {
        return Ok(None);
    };
    Ok(definitions()?
        .histories
        .iter()
        .find(|history| history.id == id)
        .map(|history| {
            (
                history.start_time * 1000,
                history.end_time.saturating_add(1) * 1000,
            )
        }))
}
