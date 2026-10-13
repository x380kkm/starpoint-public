// audience: internal
// # personal-service-activity-history
// 活动轮次随存档保存. 管理重置保留历史成绩和已到账资产, 并开启下一轮参与.

use serde_json::{Map, Value};

mod management;
mod reset;
mod snapshot;

pub(crate) use management::route;
pub(crate) use reset::reset_activity;
pub(crate) use snapshot::{collect_snapshot_events, record_battle_statistics};

pub(crate) struct ActivityHistoryEntry {
    pub(crate) activity_id: String,
    pub(crate) participation_round: i64,
    pub(crate) event_key: String,
    pub(crate) kind: &'static str,
    pub(crate) details: Value,
    pub(crate) created_at: i64,
    pub(crate) mail_id: Option<i64>,
}

// //// 读取活动当前参与轮次 [@x380kkm 2026-09-21] ////
pub(crate) fn participation_round(root: &Map<String, Value>, activity_id: &str) -> i64 {
    root.get("activity_participation_rounds")
        .and_then(|rounds| rounds.get(activity_id))
        .and_then(Value::as_i64)
        .unwrap_or_default()
        .max(0)
}
