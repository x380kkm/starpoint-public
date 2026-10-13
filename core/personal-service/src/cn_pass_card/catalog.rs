// audience: internal
// # personal-service-cn-pass-card-catalog
//
// 修行活动和双轨等级奖励使用客户端主表的编号与积分阈值.

use crate::cn_mission::MissionReward;
use crate::PersonalServiceError;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Deserialize)]
pub(super) struct PassCardCatalog {
    pub(super) events: BTreeMap<i64, PassCardEvent>,
    pub(super) rewards: BTreeMap<i64, PassCardReward>,
}

#[derive(Deserialize)]
pub(super) struct PassCardEvent {
    pub(super) id: i64,
    pub(super) point_cap: i64,
    pub(super) level_threshold: i64,
    pub(super) start_time: i64,
    pub(super) end_time: i64,
    pub(super) force_end_time: i64,
    pub(super) is_free: bool,
}

#[derive(Deserialize)]
pub(super) struct PassCardReward {
    pub(super) id: i64,
    pub(super) event_id: i64,
    pub(super) level: i64,
    pub(super) reward1: MissionReward,
    pub(super) reward2: MissionReward,
    pub(super) is_payment_money: bool,
}

static CATALOG: OnceLock<Result<PassCardCatalog, String>> = OnceLock::new();

// //// 读取修行活动和等级奖励 [@x380kkm 2026-09-22] ////
pub(super) fn catalog() -> Result<&'static PassCardCatalog, PersonalServiceError> {
    CATALOG
        .get_or_init(|| {
            serde_json::from_str(include_str!("../../assets/cn-pass-card.json"))
                .map_err(|error| format!("failed to decode CN pass card catalog: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}
