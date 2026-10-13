// audience: internal
// # personal-service-cn-box-gacha-rules
//
// 箱体重置类型来自 CN box 主表, 由 export_cn_box_reset_kinds.py 导出.

use crate::PersonalServiceError;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

type BoxResetKinds = BTreeMap<String, BTreeMap<String, i64>>;

const RESET_KINDS_ASSET: &str = include_str!("../../assets/cn-box-reset-kinds.json");
static RESET_KINDS: OnceLock<Result<BoxResetKinds, String>> = OnceLock::new();

// //// 按箱体重置类型启用特奖停止条件 [@x380kkm 2026-09-27] ////
pub(super) fn can_stop_on_featured_rewards(
    pool_id: i64,
    box_id: i64,
) -> Result<bool, PersonalServiceError> {
    let kinds = RESET_KINDS
        .get_or_init(|| {
            serde_json::from_str(RESET_KINDS_ASSET)
                .map_err(|error| format!("failed to decode CN box reset kinds: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    let reset_kind = kinds
        .get(&pool_id.to_string())
        .and_then(|boxes| boxes.get(&box_id.to_string()))
        .ok_or_else(|| PersonalServiceError::new("CN box reset kind is missing"))?;
    Ok(*reset_kind == 0)
}
// //// /按箱体重置类型启用特奖停止条件 ////

// //// 统计箱内尚余的特奖胶囊 [@x380kkm 2026-09-27] ////
pub(super) fn remaining_featured_reward_count(
    rewards: &Map<String, Value>,
    drawn: &BTreeMap<i64, i64>,
) -> Result<i64, PersonalServiceError> {
    rewards
        .iter()
        .filter(|(_, reward)| is_featured_reward(reward))
        .try_fold(0_i64, |total, (id, reward)| {
            let reward_id = id
                .parse::<i64>()
                .map_err(|_| PersonalServiceError::new("CN box reward id is invalid"))?;
            let available = reward
                .get("available")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let remaining = available
                .saturating_sub(drawn.get(&reward_id).copied().unwrap_or_default())
                .max(0);
            total
                .checked_add(remaining)
                .ok_or_else(|| PersonalServiceError::new("CN box featured count exceeds range"))
        })
}
// //// /统计箱内尚余的特奖胶囊 ////

// //// 识别主表定义的特奖奖励 [@x380kkm 2026-09-27] ////
pub(super) fn is_featured_reward(reward: &Value) -> bool {
    reward.get("tier").and_then(Value::as_i64) == Some(2)
}
// //// /识别主表定义的特奖奖励 ////

