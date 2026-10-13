// audience: internal
// # personal-service-character-presentation
// 成绩和队伍展示引用客户端角色主表中的角色, 空存档使用主表中的初始角色.
// 历史队伍的可选成员使用 null 表示空槽位, 保留有效成员的原位置.

use serde_json::{Map, Value};

const STARTER_CHARACTER_ID: i64 = 1;

// //// 验证展示角色存在于共享角色主表 [@x380kkm 2026-09-28] ////
pub(crate) fn is_valid_character(character_id: i64) -> bool {
    character_id > 0 && crate::cn_character::character_asset_data(character_id).is_ok()
}

// //// 将失效的可选角色引用转换为客户端空成员 [@x380kkm 2026-10-02] ////
pub(crate) fn normalize_optional_character_reference(value: &mut Value) {
    if !value.as_i64().is_some_and(is_valid_character) {
        *value = Value::Null;
    }
}

// //// 从成绩和玩家队伍选择真实队长 [@x380kkm 2026-09-28] ////
pub(crate) fn leader_character_id(root: &Map<String, Value>, preferred: Option<i64>) -> i64 {
    let party_id = root.get("user_info").and_then(|user| user.get("party_slot"))
        .and_then(Value::as_i64).unwrap_or(1);
    let party = crate::cn_party::party_character_ids(root, party_id).unwrap_or_default();
    let profile = root.get("user_info").and_then(|user| user.get("leader_character_id"))
        .and_then(Value::as_i64);
    let owned = root.get("user_character_list").and_then(Value::as_object)
        .into_iter().flat_map(|characters| characters.keys()).filter_map(|id| id.parse::<i64>().ok());
    preferred.into_iter().chain(party).chain(profile).chain(owned)
        .find(|id| is_valid_character(*id))
        .unwrap_or(STARTER_CHARACTER_ID)
}
