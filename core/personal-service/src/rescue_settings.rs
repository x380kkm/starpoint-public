// audience: internal
// # personal-service-rescue-settings
// 救援选择按实例保存. 活动开放使用游戏时间, 房间间隔与寿命使用现实时间.

mod access;
mod catalog;
mod routes;
#[cfg(test)]
mod tests;

pub(crate) use routes::route;
pub(crate) use access::{can_enter_rescue_quest, eligible_quest_keys};

use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RescueQuest {
    pub(crate) category_id: i64,
    pub(crate) quest_id: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct RescuePolicy {
    pub(crate) enabled: bool,
    pub(crate) spawn_interval_seconds: i64,
    pub(crate) room_lifetime_seconds: i64,
    pub(crate) max_rooms: usize,
    pub(crate) quests: Vec<RescueQuest>,
}

impl Default for RescuePolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            spawn_interval_seconds: 60,
            room_lifetime_seconds: 180,
            max_rooms: 3,
            quests: Vec::new(),
        }
    }
}

// //// 按配置和当前活动窗口筛选救援关卡 [@x380kkm 2026-09-23] ////
pub(crate) fn policy(database: &ServiceDatabase) -> Result<RescuePolicy, PersonalServiceError> {
    let mut settings = database.rescue_settings()?;
    if !settings.enabled {
        settings.quests.clear();
        return Ok(settings);
    }
    let eligible = eligible_quest_keys(database)?;
    settings
        .quests
        .retain(|quest| eligible.contains(&(quest.category_id, quest.quest_id)));
    Ok(settings)
}
// //// /按配置和当前活动窗口筛选救援关卡 ////
