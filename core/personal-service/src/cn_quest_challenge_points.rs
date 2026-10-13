// audience: internal
// # personal-service-cn-quest-challenge-points
// 剧情和世界剧情关卡按客户端主表引用挑战点, 剩余次数保存在 user_daily_challenge_point_list.
// 客户端找不到挑战点条目时按 0 次显示. 次数上限来自挑战点主表; 每日恢复的挑战点在日切时回到上限,
// 不恢复的挑战点在活动重置时回到上限. 回到上限时保留条目中的活动加成次数.

use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

const CATALOG: &str = include_str!("../assets/cn-quest-challenge-points.json");
const POINTS_FIELD: &str = "user_daily_challenge_point_list";
static RULES: OnceLock<Result<Catalog, String>> = OnceLock::new();

#[derive(Deserialize)]
struct Catalog {
    points: BTreeMap<i64, PointRule>,
    quests: BTreeMap<i64, i64>,
}

#[derive(Deserialize)]
struct PointRule {
    max: i64,
    recovery: bool,
}

fn catalog() -> Result<&'static Catalog, PersonalServiceError> {
    RULES
        .get_or_init(|| {
            serde_json::from_str(CATALOG)
                .map_err(|error| format!("invalid CN quest challenge point catalog: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

// //// 返回关卡引用的挑战点 [@x380kkm 2026-10-04] ////
pub(crate) fn point_id_for_quest(quest_id: i64) -> Result<Option<i64>, PersonalServiceError> {
    Ok(catalog()?.quests.get(&quest_id).copied())
}

// //// 为存档缺少的关卡挑战点补齐满次数条目 [@x380kkm 2026-10-04] ////
pub(crate) fn add_missing_points(
    root: &mut Map<String, Value>,
) -> Result<(), PersonalServiceError> {
    let catalog = catalog()?;
    let entries = point_entries(root)?;
    for (&id, rule) in &catalog.points {
        if !entries.iter().any(|entry| entry_id(entry) == Some(id)) {
            entries.push(json!({"id": id, "point": rule.max, "campaign_list": []}));
        }
    }
    Ok(())
}

// //// 日切时把每日恢复的关卡挑战点回到上限 [@x380kkm 2026-10-04] ////
pub(crate) fn restore_recovering_points(
    root: &mut Map<String, Value>,
) -> Result<(), PersonalServiceError> {
    let catalog = catalog()?;
    restore_points(root, |id| {
        catalog.points.get(&id).filter(|rule| rule.recovery)
    })
}

// //// 活动重置时把该活动关卡的挑战点回到上限 [@x380kkm 2026-10-04] ////
pub(crate) fn restore_quest_points(
    root: &mut Map<String, Value>,
    quest_ids: impl IntoIterator<Item = i64>,
) -> Result<Vec<i64>, PersonalServiceError> {
    let catalog = catalog()?;
    let mut point_ids = quest_ids
        .into_iter()
        .filter_map(|quest_id| catalog.quests.get(&quest_id).copied())
        .collect::<Vec<_>>();
    point_ids.sort_unstable();
    point_ids.dedup();
    restore_points(root, |id| {
        point_ids
            .binary_search(&id)
            .ok()
            .and_then(|_| catalog.points.get(&id))
    })?;
    Ok(point_ids)
}

fn restore_points<'a>(
    root: &mut Map<String, Value>,
    rule_for: impl Fn(i64) -> Option<&'a PointRule>,
) -> Result<(), PersonalServiceError> {
    for entry in point_entries(root)? {
        let Some(rule) = entry_id(entry).and_then(&rule_for) else {
            continue;
        };
        let campaign_points = entry
            .get("campaign_list")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|campaign| campaign.get("additional_point").and_then(Value::as_i64))
            .sum::<i64>();
        let entry = entry
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN challenge point is invalid"))?;
        entry.insert(
            "point".to_owned(),
            json!(rule.max.saturating_add(campaign_points)),
        );
    }
    Ok(())
}

fn point_entries(root: &mut Map<String, Value>) -> Result<&mut Vec<Value>, PersonalServiceError> {
    root.entry(POINTS_FIELD.to_owned())
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN challenge points are invalid"))
}

fn entry_id(entry: &Value) -> Option<i64> {
    entry.get("id").and_then(Value::as_i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    // //// 验证四周年关卡挑战点的补齐, 日切和活动重置 [@x380kkm 2026-10-04] ////
    #[test]
    fn restores_story_points_by_recovery_rule() {
        assert_eq!(point_id_for_quest(200031001).unwrap(), Some(401));
        assert_eq!(point_id_for_quest(100400015).unwrap(), Some(2));
        let mut root = json!({POINTS_FIELD: [{"id": 401, "point": 0, "campaign_list": []}]})
            .as_object()
            .cloned()
            .unwrap();
        add_missing_points(&mut root).unwrap();
        let point = |root: &Map<String, Value>, id: i64| {
            root[POINTS_FIELD]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry_id(entry) == Some(id))
                .unwrap()["point"]
                .clone()
        };
        assert_eq!(point(&root, 401), 0);
        assert_eq!(point(&root, 402), 1);
        assert_eq!(point(&root, 2), 1);

        for entry in root[POINTS_FIELD].as_array_mut().unwrap() {
            entry["point"] = json!(0);
        }
        restore_recovering_points(&mut root).unwrap();
        assert_eq!(point(&root, 401), 0);
        assert_eq!(point(&root, 2), 1);

        assert_eq!(
            restore_quest_points(&mut root, [200031001, 200031002, 1]).unwrap(),
            vec![401, 402]
        );
        assert_eq!(point(&root, 401), 1);
        assert_eq!(point(&root, 402), 1);
        assert_eq!(point(&root, 403), 0);
    }
    // //// /验证四周年关卡挑战点的补齐, 日切和活动重置 ////
}
