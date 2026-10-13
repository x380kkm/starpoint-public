// audience: internal
// # personal-service-cn-mission-collect
//
// 收集任务按物品编号和活动时窗记录实际入库数量. 库存快照同时保留收集进度, 嵌套结算按同一任务范围扣除已计入数量.
// 任务奖励继续按实际入库数量结算关联任务, 阶段凭据约束每项奖励的发放次数.

use super::{mission_catalog, settle_mission_stage, MissionDefinition, MissionKey};
use super::battle_time::{battle_mission_window, BattleMissionWindow};
use super::window::{advance_progress, mission_scope_for_battle, MissionScope};
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub(crate) struct CollectItemMissionPlan {
    entries: Vec<(MissionKey, &'static MissionDefinition, MissionScope)>,
}

pub(crate) struct CollectItemCounts {
    items: BTreeMap<i64, i64>,
    progress: BTreeMap<String, i64>,
}

// //// 读取收集任务的物品持有量与已计入进度 [@x380kkm 2026-09-28] ////
pub(crate) fn capture_collect_item_counts(root: &Map<String, Value>) -> Result<CollectItemCounts, PersonalServiceError> {
    Ok(CollectItemCounts {
        items: item_counts(root)?,
        progress: root.get("mission_window_progress").and_then(Value::as_object)
            .into_iter().flatten().filter_map(|(key, value)| value.as_i64().map(|value| (key.clone(), value))).collect(),
    })
}

fn item_counts(root: &Map<String, Value>) -> Result<BTreeMap<i64, i64>, PersonalServiceError> {
    let catalog = mission_catalog()?;
    Ok(catalog.categories.values().flatten().filter(|mission| mission.pattern.as_deref() == Some("get_item_count"))
        .filter_map(|mission| mission.item_id).map(|item_id| {
            let count = root.get("item_list").and_then(|items| items.get(item_id.to_string())).and_then(Value::as_i64).unwrap_or_default();
            (item_id, count)
        }).collect())
}
// //// /读取收集任务的物品持有量与已计入进度 ////

// //// 读取当前有效的物品任务结算计划 [@x380kkm 2026-09-21] ////
pub(crate) fn plan_collect_item_missions(database: &ServiceDatabase) -> Result<CollectItemMissionPlan, PersonalServiceError> {
    let now = database.current_server_time_millis()?.div_euclid(1000);
    plan_collect_item_missions_at(database, &BattleMissionWindow { started_at: now, activities: None }, now)
}

pub(super) fn plan_battle_collect_item_missions(
    root: &Map<String, Value>, database: &ServiceDatabase, account_id: i64,
) -> Result<CollectItemMissionPlan, PersonalServiceError> {
    let now = database.current_server_time_millis()?.div_euclid(1000);
    let window = battle_mission_window(root, database, account_id, now)?;
    plan_collect_item_missions_at(database, &window, now)
}

fn plan_collect_item_missions_at(database: &ServiceDatabase, window: &BattleMissionWindow, completed_at: i64) -> Result<CollectItemMissionPlan, PersonalServiceError> {
    let catalog = mission_catalog()?;
    let mut entries = Vec::new();
    for (category, missions) in &catalog.categories {
        for mission in missions.iter().filter(|mission| mission.pattern.as_deref() == Some("get_item_count") && mission.item_id.is_some()) {
            let key = MissionKey { category: *category, mission_id: mission.id };
            if let Some(scope) = mission_scope_for_battle(key, mission, database, window, completed_at)? { entries.push((key, mission, scope)); }
        }
    }
    Ok(CollectItemMissionPlan { entries })
}
// //// /读取当前有效的物品任务结算计划 ////

// //// 在玩家事务内按实际入库数量结算任务 [@x380kkm 2026-09-21] ////
pub(crate) fn record_collected_item_rewards(
    root: &mut Map<String, Value>, plan: &CollectItemMissionPlan, acquired: &BTreeMap<i64, i64>, response_time: i64,
) -> Result<Vec<Value>, PersonalServiceError> {
    let pending = plan.entries.iter().map(|(_, mission, scope)| {
        (scope.key.clone(), mission.item_id.and_then(|id| acquired.get(&id)).copied().unwrap_or_default())
    }).collect();
    record_scoped_collections(root, plan, pending, response_time)
}
// //// /在玩家事务内按实际入库数量结算任务 ////

// //// 将任务奖励产生的入库增量逐次计入关联收集任务 [@x380kkm 2026-09-28] ////
fn record_scoped_collections(
    root: &mut Map<String, Value>, plan: &CollectItemMissionPlan, mut pending: BTreeMap<String, i64>, response_time: i64,
) -> Result<Vec<Value>, PersonalServiceError> {
    let mut completed = Vec::new();
    while pending.values().any(|count| *count > 0) {
        crate::cn_reward_overflow::reconcile_capacity(root)?;
        let before = item_counts(root)?;
        for (key, mission, scope) in &plan.entries {
            let count = pending.get(&scope.key).copied().unwrap_or_default();
            if count <= 0 { continue; }
            let progress = advance_progress(root, scope, "get_item_count", count, count)?;
            for stage in mission.stages.iter().filter(|stage| stage.target.is_some_and(|target| progress >= target)) {
                if settle_mission_stage(root, *key, stage, &scope.receipt(root, stage.stage), response_time)?.is_some() {
                    completed.push(json!({"mission_category_id":key.category,"mission_id":key.mission_id,"mission_reward_id":stage.reward_id}));
                }
            }
        }
        crate::cn_reward_overflow::reconcile_capacity(root)?;
        let acquired = inventory_increase(root, &before);
        pending = plan.entries.iter().filter_map(|(_, mission, scope)| {
            let count = mission.item_id.and_then(|id| acquired.get(&id)).copied().unwrap_or_default();
            (count > 0).then(|| (scope.key.clone(), count))
        }).collect();
    }
    Ok(completed)
}
// //// /将任务奖励产生的入库增量逐次计入关联收集任务 ////

fn inventory_increase(root: &Map<String, Value>, before: &BTreeMap<i64, i64>) -> BTreeMap<i64, i64> {
    before.iter().filter_map(|(item_id, previous)| {
        let current = root.get("item_list").and_then(|items| items.get(item_id.to_string())).and_then(Value::as_i64).unwrap_or_default();
        let amount = current.saturating_sub(*previous);
        (amount > 0).then_some((*item_id, amount))
    }).collect()
}

// //// 将奖励入库前后的正增量计入收集任务 [@x380kkm 2026-09-21] ////
pub(crate) fn record_collected_items(
    root: &mut Map<String, Value>, database: &ServiceDatabase, _account_id: i64, before: &CollectItemCounts,
) -> Result<Vec<Value>, PersonalServiceError> {
    let now = database.current_server_time_millis()?.div_euclid(1000);
    record_collected_item_changes(root, &plan_collect_item_missions(database)?, before, now)
}

pub(crate) fn record_battle_collected_items(
    root: &mut Map<String, Value>, database: &ServiceDatabase, account_id: i64, before: &CollectItemCounts,
) -> Result<Vec<Value>, PersonalServiceError> {
    let now = database.current_server_time_millis()?.div_euclid(1000);
    let plan = plan_battle_collect_item_missions(root, database, account_id)?;
    record_collected_item_changes(root, &plan, before, now)
}
// //// /将奖励入库前后的正增量计入收集任务 ////

// //// 从库存增量中扣除内层结算已计入同一任务的数量 [@x380kkm 2026-09-28] ////
pub(super) fn record_collected_item_changes(
    root: &mut Map<String, Value>, plan: &CollectItemMissionPlan, before: &CollectItemCounts, now: i64,
) -> Result<Vec<Value>, PersonalServiceError> {
    crate::cn_reward_overflow::reconcile_capacity(root)?;
    let acquired = inventory_increase(root, &before.items);
    let pending = plan.entries.iter().filter_map(|(_, mission, scope)| {
        let amount = mission.item_id.and_then(|id| acquired.get(&id)).copied().unwrap_or_default();
        let previous = before.progress.get(&scope.key).copied().unwrap_or_default();
        let current = root.get("mission_window_progress").and_then(|entries| entries.get(&scope.key)).and_then(Value::as_i64).unwrap_or_default();
        let counted = current.saturating_sub(previous).max(0);
        let remaining = amount.saturating_sub(counted).max(0);
        (remaining > 0).then(|| (scope.key.clone(), remaining))
    }).collect();
    record_scoped_collections(root, plan, pending, now)
}
// //// /从库存增量中扣除内层结算已计入同一任务的数量 ////
