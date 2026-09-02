// audience: internal
// # personal-service-cn-gacha-region
//
// 该模块加载 CN 卡池地区策略, 归一化隐藏别名状态, 并选择普通卡池时间缺口的唯一客户端入口.

use super::{
    direct_gacha_definition, gacha_document, gacha_master_times,
    gacha_master_window_status_from_definition, gacha_page_kind, gacha_type, CHARACTER_GACHA_TYPE,
    EQUIPMENT_GACHA_TYPE,
};
use crate::database::ActivityWindowStatus;
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::sync::OnceLock;

const POLICY_ASSET: &str = include_str!("../../../../assets/gacha-region-policy.json");
static POLICY: OnceLock<Result<GachaRegionPolicy, String>> = OnceLock::new();

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GachaRegionPolicy {
    excluded_regional_aliases: HashMap<String, i64>,
    normalized_coverage_aliases: HashMap<String, i64>,
    temporary_aliases: HashMap<String, i64>,
}

#[derive(Clone, Copy)]
pub(super) struct ResolvedGachaId {
    pub(super) requested: i64,
    pub(super) canonical: i64,
    pub(super) is_coverage: bool,
    pub(super) is_regional: bool,
    pub(super) is_temporary: bool,
}

// //// 解析卡池 ID 并限制通用枚举范围 [@x380kkm 2026-08-24] ////
fn policy() -> Result<&'static GachaRegionPolicy, PersonalServiceError> {
    POLICY
        .get_or_init(|| {
            serde_json::from_str::<GachaRegionPolicy>(POLICY_ASSET)
                .map_err(|error| format!("failed to decode CN gacha region policy: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

pub(super) fn resolve(gacha_id: i64) -> Result<ResolvedGachaId, PersonalServiceError> {
    let key = gacha_id.to_string();
    let policy = policy()?;
    let (canonical, is_coverage, is_regional, is_temporary) =
        if let Some(canonical) = policy.temporary_aliases.get(&key) {
            (*canonical, false, false, true)
        } else if let Some(canonical) = policy.normalized_coverage_aliases.get(&key) {
            (*canonical, true, false, false)
        } else if let Some(canonical) = policy.excluded_regional_aliases.get(&key) {
            (*canonical, false, true, false)
        } else {
            (gacha_id, false, false, false)
        };
    direct_gacha_definition(canonical)?;
    Ok(ResolvedGachaId {
        requested: gacha_id,
        canonical,
        is_coverage,
        is_regional,
        is_temporary,
    })
}

pub(super) fn is_hidden(gacha_id: &str) -> Result<bool, PersonalServiceError> {
    let policy = policy()?;
    Ok(policy.excluded_regional_aliases.contains_key(gacha_id)
        || policy.normalized_coverage_aliases.contains_key(gacha_id))
}

pub(super) fn is_enumerable(gacha_id: i64) -> bool {
    let key = gacha_id.to_string();
    let Ok(policy) = policy() else {
        return false;
    };
    !policy.excluded_regional_aliases.contains_key(&key)
        && !policy.normalized_coverage_aliases.contains_key(&key)
        && !policy.temporary_aliases.contains_key(&key)
        && direct_gacha_definition(gacha_id).is_ok()
}

pub(super) fn temporary_alias_for(canonical_id: i64) -> Result<Option<i64>, PersonalServiceError> {
    policy()?
        .temporary_aliases
        .iter()
        .find_map(|(alias_id, mapped)| {
            (*mapped == canonical_id)
                .then(|| alias_id.parse::<i64>().ok())
                .flatten()
        })
        .map_or(Ok(None), |alias_id| Ok(Some(alias_id)))
}
// //// /解析卡池 ID 并限制通用枚举范围 ////

// //// 将隐藏地区别名的持久状态归入真实 CN 卡池 [@x380kkm 2026-08-24] ////
pub(super) fn normalize_gacha_info(
    root: &mut Map<String, Value>,
) -> Result<(), PersonalServiceError> {
    let Some(list) = root.get_mut("gacha_info_list") else {
        return Ok(());
    };
    let list = list
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN gacha info is invalid"))?;
    let policy = policy()?;
    let mut normalized = Vec::with_capacity(list.len());
    for mut info in list.drain(..) {
        let Some(gacha_id) = info.get("gacha_id").and_then(Value::as_i64) else {
            normalized.push(info);
            continue;
        };
        let key = gacha_id.to_string();
        if policy.temporary_aliases.contains_key(&key) {
            continue;
        }
        let canonical_id = policy
            .excluded_regional_aliases
            .get(&key)
            .copied()
            .or_else(|| policy.normalized_coverage_aliases.get(&key).copied())
            .unwrap_or(gacha_id);
        if canonical_id != gacha_id {
            let info = info
                .as_object_mut()
                .ok_or_else(|| PersonalServiceError::new("stored CN gacha info is invalid"))?;
            info.insert("gacha_id".to_owned(), Value::from(canonical_id));
        }
        if let Some(canonical) = normalized
            .iter_mut()
            .find(|entry| entry.get("gacha_id").and_then(Value::as_i64) == Some(canonical_id))
        {
            merge_gacha_info(canonical, &info)?;
        } else {
            normalized.push(info);
        }
    }
    *list = normalized;
    Ok(())
}

fn merge_gacha_info(canonical: &mut Value, alias: &Value) -> Result<(), PersonalServiceError> {
    let canonical = canonical
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN gacha info is invalid"))?;
    let alias = alias
        .as_object()
        .ok_or_else(|| PersonalServiceError::new("stored CN gacha info is invalid"))?;
    let points = canonical
        .get("gacha_exchange_point")
        .and_then(Value::as_i64)
        .unwrap_or_default()
        .checked_add(
            alias
                .get("gacha_exchange_point")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
        )
        .ok_or_else(|| PersonalServiceError::new("CN gacha exchange points overflow"))?;
    canonical.insert("gacha_exchange_point".to_owned(), Value::from(points));
    for field in ["is_account_first", "is_daily_first"] {
        let available = canonical
            .get(field)
            .and_then(Value::as_bool)
            .unwrap_or(true)
            && alias.get(field).and_then(Value::as_bool).unwrap_or(true);
        canonical.insert(field.to_owned(), Value::Bool(available));
    }
    Ok(())
}
// //// /将隐藏地区别名的持久状态归入真实 CN 卡池 ////

// //// 选择普通角色池和装备池的唯一时间缺口入口 [@x380kkm 2026-08-24] ////
pub(super) fn coverage_aliases(now_ms: i64) -> Result<Vec<ResolvedGachaId>, PersonalServiceError> {
    let document = gacha_document()?
        .as_object()
        .ok_or_else(|| PersonalServiceError::new("CN gacha asset root is invalid"))?;
    let policy = policy()?;
    let mut bridges = Vec::new();
    for kind in [CHARACTER_GACHA_TYPE, EQUIPMENT_GACHA_TYPE] {
        let mut retained_open = false;
        for (gacha_id, gacha) in document {
            if (policy.excluded_regional_aliases.contains_key(gacha_id)
                || policy.normalized_coverage_aliases.contains_key(gacha_id))
                || gacha_page_kind(gacha)? != 0
                || gacha_type(gacha)? != kind
            {
                continue;
            }
            if gacha_master_window_status_from_definition(gacha, now_ms)?
                == ActivityWindowStatus::Open
            {
                retained_open = true;
                break;
            }
        }
        if retained_open {
            continue;
        }
        let mut candidates = Vec::new();
        for (alias_id, canonical_id) in policy
            .excluded_regional_aliases
            .iter()
            .chain(&policy.normalized_coverage_aliases)
        {
            let alias_id = alias_id
                .parse::<i64>()
                .map_err(|_| PersonalServiceError::new("CN gacha alias id is invalid"))?;
            let alias = direct_gacha_definition(alias_id)?;
            if gacha_page_kind(alias)? != 0 || gacha_type(alias)? != kind {
                continue;
            }
            let (start_at_ms, end_at_ms) = gacha_master_times(alias)?;
            if !(start_at_ms <= now_ms && now_ms < end_at_ms) {
                continue;
            }
            let list_order = alias
                .get("listOrder")
                .and_then(Value::as_i64)
                .ok_or_else(|| PersonalServiceError::new("CN gacha list order is invalid"))?;
            candidates.push((start_at_ms, list_order, alias_id, *canonical_id));
        }
        candidates.sort_unstable();
        if let Some((_, _, alias_id, canonical_id)) = candidates.pop() {
            let key = alias_id.to_string();
            bridges.push(ResolvedGachaId {
                requested: alias_id,
                canonical: canonical_id,
                is_coverage: policy.normalized_coverage_aliases.contains_key(&key),
                is_regional: policy.excluded_regional_aliases.contains_key(&key),
                is_temporary: false,
            });
        }
    }
    Ok(bridges)
}

pub(super) fn has_visible_temporary_for_type(
    list: &[Value],
    kind: i64,
) -> Result<bool, PersonalServiceError> {
    let policy = policy()?;
    for info in list {
        let Some(gacha_id) = info.get("gacha_id").and_then(Value::as_i64) else {
            continue;
        };
        let Some(canonical_id) = policy.temporary_aliases.get(&gacha_id.to_string()) else {
            continue;
        };
        let canonical = direct_gacha_definition(*canonical_id)?;
        if gacha_page_kind(canonical)? == 0 && gacha_type(canonical)? == kind {
            return Ok(true);
        }
    }
    Ok(false)
}
// //// /选择普通角色池和装备池的唯一时间缺口入口 ////

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::ServiceDatabase;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn normalizes_alias_before_canonical_without_duplicate_state() {
        let (alias_id, canonical_id) = policy()
            .unwrap()
            .excluded_regional_aliases
            .iter()
            .next()
            .map(|(alias, canonical)| (alias.parse::<i64>().unwrap(), *canonical))
            .unwrap();
        let mut root = Map::from_iter([(
            "gacha_info_list".to_owned(),
            json!([
                {"gacha_id": alias_id, "is_account_first": true, "is_daily_first": false, "gacha_exchange_point": 10},
                {"gacha_id": canonical_id, "is_account_first": false, "is_daily_first": true, "gacha_exchange_point": 20}
            ]),
        )]);

        normalize_gacha_info(&mut root).unwrap();

        let list = root["gacha_info_list"].as_array().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["gacha_id"], canonical_id);
        assert_eq!(list[0]["gacha_exchange_point"], 30);
        assert_eq!(list[0]["is_account_first"], false);
        assert_eq!(list[0]["is_daily_first"], false);
    }

    #[test]
    fn selects_only_the_required_coverage_alias_per_gap() {
        let selected = |now_ms| {
            coverage_aliases(now_ms)
                .unwrap()
                .into_iter()
                .map(|bridge| bridge.requested)
                .collect::<Vec<_>>()
        };
        assert_eq!(selected(1_575_259_260_000), Vec::<i64>::new());
        assert_eq!(selected(1_581_303_600_000), vec![5_001]);
        assert_eq!(selected(1_593_054_000_000), vec![5_006]);
        assert_eq!(selected(1_633_057_200_000), vec![94]);
        let october_gap = coverage_aliases(1_635_562_800_000).unwrap();
        assert_eq!(october_gap.len(), 1);
        assert_eq!(october_gap[0].requested, 61);
        assert_eq!(october_gap[0].canonical, 1);
        assert!(october_gap[0].is_coverage);
    }

    #[test]
    fn campaign_list_excludes_every_hidden_alias() {
        let mut root = Map::new();

        super::super::campaign::replace_available_for_load(&mut root, |_| Ok(true)).unwrap();

        let campaigns = root["gacha_campaign_list"].as_array().unwrap();
        assert!(campaigns
            .iter()
            .all(|campaign| { campaign["gacha_id"].as_i64().is_some_and(is_enumerable) }));
        assert!(!campaigns.iter().any(|campaign| campaign["gacha_id"] == 61));
    }

    #[test]
    fn injects_lease_alias_only_for_closed_master_pool() {
        let root_directory = TempDir::new().unwrap();
        let mut database = ServiceDatabase::open(root_directory.path()).unwrap();
        database
            .set_virtual_time(true, 1_575_259_260_000, 1.0)
            .unwrap();
        for activity_id in ["gacha:1", "gacha:800000"] {
            database
                .create_activity_temporary_open_lease(activity_id)
                .unwrap();
        }
        let mut root = Map::from_iter([("gacha_info_list".to_owned(), json!([]))]);

        super::super::inject_active_temporary_gacha_aliases(&mut root, &database, 7).unwrap();

        let list = root["gacha_info_list"].as_array().unwrap();
        assert!(!list
            .iter()
            .any(|info| { info["gacha_id"] == temporary_alias_for(1).unwrap().unwrap() }));
        let account_first_id = temporary_alias_for(800_000).unwrap().unwrap();
        let account_first = list
            .iter()
            .find(|info| info["gacha_id"] == account_first_id)
            .unwrap();
        assert_eq!(account_first["is_account_first"], true);
        assert_eq!(account_first["is_daily_first"], false);
    }

    #[test]
    fn temporary_alias_request_requires_the_canonical_lease() {
        let root_directory = TempDir::new().unwrap();
        let mut database = ServiceDatabase::open(root_directory.path()).unwrap();
        let temporary_id = temporary_alias_for(800_000).unwrap().unwrap();
        let resolution = resolve(temporary_id).unwrap();
        let root = Map::new();

        assert!(
            super::super::closed_gacha_response(&database, &root, resolution)
                .unwrap()
                .is_some()
        );
        database
            .create_activity_temporary_open_lease("gacha:800000")
            .unwrap();
        assert!(
            super::super::closed_gacha_response(&database, &root, resolution)
                .unwrap()
                .is_none()
        );
    }
}
