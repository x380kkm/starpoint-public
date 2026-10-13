// audience: internal
// # personal-service-cn-challenge-points
// CN 客户端在日期筛选前构造 campaign_list 中的每个 master 引用.
// 同一挑战点和活动引用各自唯一, 玩家持有点数在引用整理时保持原值.

use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

const CAMPAIGN_PARENTS: &str = include_str!("../../assets/cn-daily-challenge-campaigns.json");
const POINTS_FIELD: &str = "user_daily_challenge_point_list";
static CAMPAIGNS: OnceLock<Result<BTreeMap<i64, i64>, String>> = OnceLock::new();

// //// 读取 CN 挑战活动所属挑战点 [@x380kkm 2026-09-27] ////
fn campaign_parents() -> Result<&'static BTreeMap<i64, i64>, PersonalServiceError> {
    CAMPAIGNS
        .get_or_init(|| {
            serde_json::from_str(CAMPAIGN_PARENTS)
                .map_err(|error| format!("invalid CN daily challenge campaign catalog: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

// //// 整理挑战次数和活动记录的唯一引用 [@x380kkm 2026-09-27] ////
pub(super) fn normalize(root: &mut Map<String, Value>) -> Result<(), PersonalServiceError> {
    let catalog = campaign_parents()?;
    let mut normalized = Vec::<Value>::new();
    let mut indexes = BTreeMap::<i64, usize>::new();
    for mut value in root
        .remove(POINTS_FIELD)
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
    {
        let Some(record) = value.as_object_mut() else {
            continue;
        };
        let Some(id) = record
            .get("id")
            .and_then(Value::as_i64)
            .filter(|id| *id > 0)
        else {
            continue;
        };
        let point = record
            .get("point")
            .and_then(Value::as_i64)
            .unwrap_or_default()
            .max(0);
        record.insert("point".to_owned(), json!(point));
        let campaigns = record
            .remove("campaign_list")
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default();
        record.insert(
            "campaign_list".to_owned(),
            json!(normalize_campaigns(id, campaigns, catalog)),
        );
        if let Some(&index) = indexes.get(&id) {
            let current = normalized[index]
                .as_object_mut()
                .expect("normalized challenge is an object");
            let remaining = current["point"]
                .as_i64()
                .expect("normalized point is an integer")
                .max(point);
            current.insert("point".to_owned(), json!(remaining));
            let mut campaigns = current["campaign_list"]
                .as_array()
                .expect("normalized campaigns are an array")
                .clone();
            campaigns.extend(
                record["campaign_list"]
                    .as_array()
                    .expect("normalized campaigns are an array")
                    .iter()
                    .cloned(),
            );
            current.insert(
                "campaign_list".to_owned(),
                json!(normalize_campaigns(id, campaigns, catalog)),
            );
        } else {
            indexes.insert(id, normalized.len());
            normalized.push(value);
        }
    }
    root.insert(POINTS_FIELD.to_owned(), json!(normalized));
    Ok(())
}

// //// 保留 master 所属匹配的唯一活动和剩余加成次数 [@x380kkm 2026-09-27] ////
fn normalize_campaigns(
    point_id: i64,
    entries: Vec<Value>,
    catalog: &BTreeMap<i64, i64>,
) -> Vec<Value> {
    let mut campaigns = BTreeMap::<i64, Value>::new();
    for mut value in entries {
        let Some(record) = value.as_object_mut() else {
            continue;
        };
        let Some(id) = record.get("campaign_id").and_then(Value::as_i64) else {
            continue;
        };
        if catalog.get(&id) != Some(&point_id) {
            continue;
        }
        let additional = record
            .get("additional_point")
            .and_then(Value::as_i64)
            .unwrap_or_default()
            .max(0);
        record.insert("additional_point".to_owned(), json!(additional));
        if let Some(current) = campaigns.get_mut(&id) {
            let remaining = current["additional_point"]
                .as_i64()
                .expect("normalized campaign point is an integer")
                .max(additional);
            current["additional_point"] = json!(remaining);
        } else {
            campaigns.insert(id, value);
        }
    }
    campaigns.into_values().collect()
}
