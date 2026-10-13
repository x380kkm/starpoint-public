// audience: internal
// # personal-service-cn-profile-degrees
//
// 称号列表和资料计数使用同一组默认, 已获得和当前展示称号.

use serde_json::Value;
use std::collections::BTreeSet;

// //// 汇总玩家持有的有效称号编号 [@x380kkm 2026-09-26] ////
pub(crate) fn owned_degree_ids(player: &Value) -> BTreeSet<i64> {
    let mut ids = BTreeSet::from([1]);
    ids.extend(
        player
            .get("degree_ids")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_i64)
            .filter(|id| *id > 0),
    );
    if let Some(id) = player
        .get("user_info")
        .and_then(|user| user.get("degree_id"))
        .and_then(Value::as_i64)
        .filter(|id| *id > 0)
    {
        ids.insert(id);
    }
    ids
}
// //// /汇总玩家持有的有效称号编号 ////
