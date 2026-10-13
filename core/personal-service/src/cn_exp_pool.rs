// audience: internal
// # personal-service-cn-exp-pool
// 经验池按客户端主表的整档间隔增长, 结算保留余下计时.
// 余额与计时锚点随调用方的玩家事务保存, 时间回拨时保留余额并重新计时.

use crate::cn_reward_capacity::capacities;
use crate::cn_tutorial::require_object;
use crate::PersonalServiceError;
use serde_json::{Map, Value};

// //// 结算经验池自然增长并推进已兑现的计时区间 [@x380kkm 2026-09-26] ////
pub(crate) fn settle(
    root: &mut Map<String, Value>,
    server_time: i64,
) -> Result<(), PersonalServiceError> {
    let limits = capacities()?;
    if limits.exp_pool_gain_seconds <= 0 || limits.exp_pool_gain_value < 0 {
        return Err(PersonalServiceError::new(
            "CN EXP regeneration settings are invalid",
        ));
    }
    let user = require_object(root, "user_info")?;
    let stored = user
        .get("exp_pool")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let pooled_time = user
        .get("exp_pooled_time")
        .and_then(Value::as_i64)
        .unwrap_or(server_time);
    let intervals = server_time.saturating_sub(pooled_time).max(0) / limits.exp_pool_gain_seconds;
    let accrued = intervals.saturating_mul(limits.exp_pool_gain_value);
    let total = stored
        .saturating_add(accrued)
        .min(limits.exp_pool.max(stored));
    let settled_time = if server_time < pooled_time || total >= limits.exp_pool {
        server_time
    } else {
        pooled_time.saturating_add(intervals * limits.exp_pool_gain_seconds)
    };
    user.insert("exp_pool".to_owned(), Value::from(total));
    user.insert("exp_pooled_time".to_owned(), Value::from(settled_time));
    Ok(())
}
// //// /结算经验池自然增长并推进已兑现的计时区间 ////

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // //// 保留整档边界并限制自然增长的最大余额 [@x380kkm 2026-09-26] ////
    #[test]
    fn settles_intervals_once_and_discards_time_accrued_at_capacity() {
        let mut data = json!({"user_info": {"exp_pool": 0, "exp_pooled_time": 1_000}});
        let root = data.as_object_mut().unwrap();
        settle(root, 1_599).unwrap();
        assert_eq!(root["user_info"]["exp_pool"], 0);
        settle(root, 1_600).unwrap();
        settle(root, 2_199).unwrap();
        assert_eq!(root["user_info"]["exp_pool"], 10);
        assert_eq!(root["user_info"]["exp_pooled_time"], 1_600);
        root["user_info"]["exp_pool"] = json!(1_999_999_995);
        settle(root, 20_000).unwrap();
        assert_eq!(root["user_info"]["exp_pool"], 2_000_000_000_i64);
        assert_eq!(root["user_info"]["exp_pooled_time"], 20_000);
        root["user_info"]["exp_pool"] = json!(1_999_999_980);
        settle(root, 20_599).unwrap();
        assert_eq!(root["user_info"]["exp_pool"], 1_999_999_980);
        settle(root, 20_600).unwrap();
        assert_eq!(root["user_info"]["exp_pool"], 1_999_999_990);
    }

    // //// 回拨与旧高余额存档保留已有经验 [@x380kkm 2026-09-26] ////
    #[test]
    fn rebases_future_time_without_erasing_existing_exp() {
        let mut data = json!({"user_info": {"exp_pool": 25, "exp_pooled_time": 20_000}});
        let root = data.as_object_mut().unwrap();
        settle(root, 1_000).unwrap();
        assert_eq!(root["user_info"]["exp_pool"], 25);
        settle(root, 1_600).unwrap();
        assert_eq!(root["user_info"]["exp_pool"], 35);
        root["user_info"]["exp_pool"] = json!(2_000_000_010_i64);
        settle(root, 2_200).unwrap();
        assert_eq!(root["user_info"]["exp_pool"], 2_000_000_010_i64);
    }
}
