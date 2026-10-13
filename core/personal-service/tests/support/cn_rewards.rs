// audience: internal
// # personal-service-cn-reward-test-support
// 此模块按任务主表汇总自动结算奖励, 并计算客户端掉落条目数量.

use serde_json::Value;
use std::sync::OnceLock;

// //// 汇总结算响应中任务奖励的货币数量 [@x380kkm 2026-09-22] ////
pub(crate) fn mission_currency(response: &Value, kind: i64) -> i64 {
    static MISSIONS: OnceLock<Value> = OnceLock::new();
    let catalog = MISSIONS.get_or_init(|| {
        serde_json::from_str(include_str!("../../assets/cn-mission-master.json")).unwrap()
    });
    response["mission_info"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|entry| {
            let category = entry["mission_category_id"].as_i64().unwrap().to_string();
            let mission = catalog["categories"][category]
                .as_array()
                .unwrap()
                .iter()
                .find(|mission| mission["id"] == entry["mission_id"])
                .unwrap();
            mission["stages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|stage| stage["reward_id"] == entry["mission_reward_id"])
                .unwrap()["rewards"]
                .as_array()
                .unwrap()
                .iter()
        })
        .filter(|reward| reward["kind"] == kind)
        .map(|reward| reward["amount"].as_i64().unwrap())
        .sum()
}

// //// 汇总普通和稀有掉落条目数量 [@x380kkm 2026-09-22] ////
pub(crate) fn sum_drop_amounts(response: &Value) -> i64 {
    ["drop_score_reward_ids", "drop_rare_reward_ids"]
        .into_iter()
        .flat_map(|key| response[key].as_array().unwrap())
        .map(|drop| drop["number"].as_i64().unwrap())
        .sum()
}
