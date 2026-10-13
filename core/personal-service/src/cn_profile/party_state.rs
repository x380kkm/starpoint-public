// audience: internal
// # personal-service-profile-party-state
//
// CN 客户端用 category 99 标识个人资料展示编队, 编队编号在此类别内独立编号.
// 展示编队保存在玩家快照中, 首页与个人资料读取同一份角色选择.
// 首页轮播读取首个展示编队, 空选择在响应中使用玩家持有角色.

use crate::PersonalServiceError;
use serde_json::{json, Map, Value};

pub(crate) const PROFILE_PARTY_CATEGORY: i64 = 99;
const PROFILE_PARTY_GROUPS_FIELD: &str = "profile_party_group_list";

// //// 为玩家生成独立的个人资料展示编队 [@x380kkm 2026-09-20] ////
fn default_profile_groups(
    root: &Map<String, Value>,
) -> Result<Map<String, Value>, PersonalServiceError> {
    let inventory = crate::cn_party::PartyInventory::from_player(root)?;
    let leader_id = root
        .get("user_info")
        .and_then(|info| info.get("leader_character_id"))
        .and_then(Value::as_i64)
        .filter(|id| inventory.character_ids().any(|owned| owned == *id))
        .or_else(|| inventory.character_ids().next());
    Ok(Map::from_iter([(
        "1".to_owned(),
        json!({
            "color_id": 7,
            "list": {
                "1": {
                    "ability_soul_ids": [null, null, null],
                    "before_battle_power": 0,
                    "character_ids": [leader_id, null, null],
                    "current_battle_power": 0,
                    "edited": false,
                    "equipment_ids": [null, null, null],
                    "name": "喜爱角色",
                    "options": {"allow_other_players_to_heal_me": true},
                    "unison_character_ids": [null, null, null]
                }
            }
        }),
    )]))
}
// //// /为玩家生成独立的个人资料展示编队 ////

// //// 投影完整的个人资料展示编队并保留持有角色 [@x380kkm 2026-09-30] ////
pub(super) fn read_profile_party_groups(
    root: &Map<String, Value>,
) -> Result<Map<String, Value>, PersonalServiceError> {
    let fallback = default_profile_groups(root)?;
    let Some(groups) = root
        .get(PROFILE_PARTY_GROUPS_FIELD)
        .and_then(Value::as_object)
    else {
        return Ok(fallback);
    };
    let inventory = crate::cn_party::PartyInventory::from_player(root)?;
    let default_party = fallback["1"]["list"]["1"].as_object().unwrap();
    let mut projected = Map::new();
    let mut first_party: Option<(i64, i64)> = None;
    for (group_key, group) in groups {
        let Some(group_id) = group_key.parse::<i64>().ok().filter(|id| *id > 0) else {
            continue;
        };
        let Some(parties) = group.get("list").and_then(Value::as_object) else {
            continue;
        };
        let mut projected_parties = Map::new();
        for (party_key, party) in parties {
            let Some(party_id) = party_key.parse::<i64>().ok().filter(|id| *id > 0) else {
                continue;
            };
            let mut fields = party.as_object().cloned().unwrap_or_default();
            inventory.normalize(&mut fields);
            if fields.get("name").and_then(Value::as_str).is_none() {
                fields.insert("name".to_owned(), default_party["name"].clone());
            }
            if fields.get("edited").and_then(Value::as_bool).is_none() {
                fields.insert("edited".to_owned(), Value::Bool(false));
            }
            let allow_healing = fields
                .get("options")
                .and_then(|options| options.get("allow_other_players_to_heal_me"))
                .and_then(Value::as_bool)
                .unwrap_or(true);
            fields.insert(
                "options".to_owned(),
                json!({"allow_other_players_to_heal_me": allow_healing}),
            );
            projected_parties.insert(party_id.to_string(), Value::Object(fields));
            if first_party.map_or(true, |first| (group_id, party_id) < first) {
                first_party = Some((group_id, party_id));
            }
        }
        if projected_parties.is_empty() {
            continue;
        }
        let color = group.get("color_id").and_then(Value::as_i64).unwrap_or(7);
        projected.insert(
            group_id.to_string(),
            json!({
                "color_id": crate::cn_party::normalize_group_color(color)?,
                "list": projected_parties,
            }),
        );
    }
    let Some((group_id, party_id)) = first_party else {
        return Ok(fallback);
    };
    let party = &mut projected[&group_id.to_string()]["list"][&party_id.to_string()];
    let has_character = ["character_ids", "unison_character_ids"]
        .into_iter()
        .flat_map(|field| party[field].as_array().into_iter().flatten())
        .any(Value::is_number);
    if !has_character {
        party["character_ids"] = default_party["character_ids"].clone();
    }
    Ok(projected)
}
// //// /投影完整的个人资料展示编队并保留持有角色 ////

// //// 读取或创建可编辑的个人资料展示编队 [@x380kkm 2026-09-20] ////
pub(crate) fn profile_party_groups_mut(
    root: &mut Map<String, Value>,
) -> Result<&mut Map<String, Value>, PersonalServiceError> {
    let projected = read_profile_party_groups(root)?;
    if !root
        .get(PROFILE_PARTY_GROUPS_FIELD)
        .is_some_and(Value::is_object)
    {
        root.insert(
            PROFILE_PARTY_GROUPS_FIELD.to_owned(),
            Value::Object(Map::new()),
        );
    }
    let groups = root
        .get_mut(PROFILE_PARTY_GROUPS_FIELD)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN profile party groups are invalid"))?;
    for (group_id, projected_group) in projected {
        let group = groups
            .entry(group_id)
            .or_insert_with(|| projected_group.clone());
        if !group.is_object() {
            *group = projected_group;
            continue;
        }
        if !group.get("list").is_some_and(Value::is_object) {
            group["list"] = json!({});
        }
        for (party_id, projected_party) in projected_group["list"].as_object().unwrap() {
            if !group["list"].get(party_id).is_some_and(Value::is_object) {
                group["list"][party_id] = projected_party.clone();
            }
        }
    }
    Ok(groups)
}
// //// /读取或创建可编辑的个人资料展示编队 ////

// //// 返回公开资料中的喜爱角色 [@x380kkm 2026-09-20] ////
pub(super) fn favorite_character(root: &Map<String, Value>) -> Result<Value, PersonalServiceError> {
    let groups = super::profile_party_groups(root)?;
    let party = groups
        .first()
        .and_then(|group| group.get("party_list"))
        .and_then(|parties| parties.get(0))
        .ok_or_else(|| PersonalServiceError::new("stored CN profile party is missing"))?;
    Ok(json!({
        "character_ex_boost": [],
        "character_ids": party.get("character_ids"),
        "unison_character_ex_boost": [],
        "unison_character_ids": party.get("unison_character_ids"),
    }))
}
// //// /返回公开资料中的喜爱角色 ////
