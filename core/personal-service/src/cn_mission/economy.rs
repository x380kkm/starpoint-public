// audience: internal
// # personal-service-cn-mission-economy
//
// 经济任务使用实际抽数, 购买份数和玛纳金额. 免费玛纳与付费玛纳共同计入收入和支出.
// 计划读取数据库, 应用计划只修改玩家快照. 任务奖励产生的玛纳继续计入收入.

use super::{mission_catalog, mission_scope, settle_mission_stage, MissionDefinition, MissionKey};
use super::window::{advance_progress, MissionScope};
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct EconomicActions {
    pub(crate) character_draws: i64,
    pub(crate) mana_acquired: i64,
    pub(crate) mana_spent: i64,
    pub(crate) treasure_purchases: i64,
    pub(crate) treasure_mana_spent: i64,
}

impl EconomicActions {
    fn counters(&self) -> [(&'static str, i64); 5] {
        [
            ("total_gacha_character_count", self.character_draws),
            ("mana_addition_count", self.mana_acquired),
            ("total_used_mana_count", self.mana_spent),
            ("treasure_shop_bought_item_count", self.treasure_purchases),
            ("treasure_shop_used_mana_count", self.treasure_mana_spent),
        ]
    }
}

pub(crate) struct EconomicMissionPlan {
    entries: Vec<(MissionKey, &'static MissionDefinition, MissionScope)>,
    previous: BTreeMap<String, i64>,
    collections: super::collect::CollectItemMissionPlan,
}

pub(crate) fn total_mana(root: &Map<String, Value>) -> Result<i64, PersonalServiceError> {
    ["free_mana", "paid_mana"].into_iter().try_fold(0_i64, |total, field| {
        total.checked_add(root.get("user_info").and_then(|user| user.get(field)).and_then(Value::as_i64).unwrap_or_default())
            .ok_or_else(|| PersonalServiceError::new("CN mana balance exceeds supported range"))
    })
}

// //// 读取有效经济任务和持久化累计值 [@x380kkm 2026-09-22] ////
pub(crate) fn plan_economic_missions(database: &ServiceDatabase, account_id: i64) -> Result<EconomicMissionPlan, PersonalServiceError> {
    let catalog = mission_catalog()?;
    let patterns = EconomicActions::default().counters();
    let mut previous = database.mission_counters(account_id)?;
    let stored = database.mission_progress(account_id)?;
    let mut entries = Vec::new();
    for (category, missions) in &catalog.categories {
        for mission in missions.iter().filter(|mission| patterns.iter().any(|(pattern, _)| mission.pattern.as_deref() == Some(pattern))) {
            let key = MissionKey { category: *category, mission_id: mission.id };
            let Some(scope) = mission_scope(key, mission, database)? else { continue; };
            if scope.is_lifetime {
                let counter = previous.entry(mission.pattern.clone().unwrap()).or_default();
                *counter = (*counter).max(stored.get(&(*category, mission.id)).copied().unwrap_or_default());
            }
            entries.push((key, mission, scope));
        }
    }
    Ok(EconomicMissionPlan { entries, previous, collections: super::plan_collect_item_missions(database)? })
}
// //// /读取有效经济任务和持久化累计值 ////

// //// 在动作快照中累计经济任务和实际奖励收入 [@x380kkm 2026-09-22] ////
pub(crate) fn record_economic_actions(
    root: &mut Map<String, Value>, plan: &EconomicMissionPlan, actions: EconomicActions, response_time: i64,
) -> Result<Vec<Value>, PersonalServiceError> {
    let mut pending = actions.counters().into_iter().filter(|(_, amount)| *amount > 0).collect::<BTreeMap<_, _>>();
    let mut completed = Vec::new();
    while !pending.is_empty() {
        let before = total_mana(root)?;
        let items_before = super::capture_collect_item_counts(root)?;
        let counters = root.entry("mission_economic_counters".to_owned()).or_insert_with(|| json!({})).as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN economic counters are invalid"))?;
        let mut totals = BTreeMap::new();
        for (pattern, amount) in &pending {
            let previous = counters.get(*pattern).and_then(Value::as_i64).unwrap_or_else(|| plan.previous.get(*pattern).copied().unwrap_or_default());
            let total = previous.checked_add(*amount).ok_or_else(|| PersonalServiceError::new("CN economic counter exceeds range"))?;
            counters.insert((*pattern).to_owned(), json!(total));
            totals.insert(*pattern, total);
        }
        for (key, mission, scope) in &plan.entries {
            let pattern = mission.pattern.as_deref().unwrap_or_default();
            let Some(amount) = pending.get(pattern) else { continue; };
            let progress = if scope.is_lifetime {
                let value = totals[pattern];
                root.entry("mission_window_progress".to_owned()).or_insert_with(|| json!({})).as_object_mut()
                    .ok_or_else(|| PersonalServiceError::new("stored CN mission progress is invalid"))?.insert(scope.key.clone(), json!(value));
                value
            } else { advance_progress(root, scope, pattern, *amount, *amount)? };
            for stage in mission.stages.iter().filter(|stage| stage.target.is_some_and(|target| progress >= target)) {
                if settle_mission_stage(root, *key, stage, &scope.receipt(root, stage.stage), response_time)?.is_some() {
                    completed.push(json!({"mission_category_id":key.category,"mission_id":key.mission_id,"mission_reward_id":stage.reward_id}));
                }
            }
        }
        completed.extend(super::collect::record_collected_item_changes(root, &plan.collections, &items_before, response_time)?);
        let acquired = total_mana(root)?.saturating_sub(before);
        pending.clear();
        if acquired > 0 { pending.insert("mana_addition_count", acquired); }
    }
    Ok(completed)
}
// //// /在动作快照中累计经济任务和实际奖励收入 ////

pub(crate) fn record_acquired_mana(
    root: &mut Map<String, Value>, database: &ServiceDatabase, account_id: i64, amount: i64, response_time: i64,
) -> Result<Vec<Value>, PersonalServiceError> {
    if amount <= 0 { return Ok(Vec::new()); }
    record_economic_actions(root, &plan_economic_missions(database, account_id)?, EconomicActions { mana_acquired: amount, ..EconomicActions::default() }, response_time)
}
