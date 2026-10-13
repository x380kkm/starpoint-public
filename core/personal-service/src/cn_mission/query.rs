// audience: internal
// # personal-service-cn-mission-query
// 任务页和小助手共用任务进度与阶段计算, 奖励结算由任务页处理.

use super::window::MissionScope;
use super::{
    build_context, compute_progress, current_stage, mission_catalog, mission_matches_category,
    mission_scope, scoped_progress, ComputeContext, MissionCatalog, MissionCategory,
    MissionDefinition, MissionKey,
};
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct EvaluatedMission {
    pub(super) key: MissionKey,
    pub(super) mission: &'static MissionDefinition,
    pub(super) scope: Option<MissionScope>,
    pub(super) progress: i64,
}

impl EvaluatedMission {
    pub(super) fn response(&self) -> Value {
        json!({
            "mission_category": self.key.category,
            "mission_id": self.key.mission_id,
            "progress_value": self.progress,
            "stage": current_stage(self.mission, self.progress),
        })
    }
}

// //// 查询当前时窗的真实任务进度 [@x380kkm 2026-09-23] ////
pub(crate) fn project_progress(
    root: &Map<String, Value>,
    database: &ServiceDatabase,
    account_id: i64,
    categories: &[MissionCategory],
) -> Result<Vec<Value>, PersonalServiceError> {
    if categories.is_empty() {
        return Ok(Vec::new());
    }
    let mut projected = root.clone();
    let now = database.current_server_time_millis()?.div_euclid(1000);
    super::window::record_login_day(&mut projected, now)?;
    super::record_activity_login_days(&mut projected, database, now)?;
    let root = &projected;
    let evaluator = ProgressEvaluator::new(root, database, account_id)?;
    evaluator
        .entries(categories)
        .into_iter()
        .map(|(key, mission)| Ok(evaluator.evaluate(root, database, key, mission)?.response()))
        .collect()
}

pub(super) struct ProgressEvaluator {
    catalog: &'static MissionCatalog,
    stored: BTreeMap<(i64, i64), i64>,
    counters: BTreeMap<String, i64>,
    context: ComputeContext,
}

impl ProgressEvaluator {
    pub(super) fn new(
        root: &Map<String, Value>,
        database: &ServiceDatabase,
        account_id: i64,
    ) -> Result<Self, PersonalServiceError> {
        let catalog = mission_catalog()?;
        let counters = database.mission_counters(account_id)?;
        Ok(Self {
            catalog,
            stored: database.mission_progress(account_id)?,
            context: build_context(root, catalog, &counters)?,
            counters,
        })
    }

    pub(super) fn entries(
        &self,
        categories: &[MissionCategory],
    ) -> Vec<(MissionKey, &'static MissionDefinition)> {
        let mut result = Vec::new();
        let mut seen = BTreeSet::new();
        for requested in categories {
            let Some(missions) = self.catalog.categories.get(&requested.category) else {
                continue;
            };
            for mission in missions {
                if !mission_matches_category(mission, requested) {
                    continue;
                }
                let key = MissionKey {
                    category: requested.category,
                    mission_id: mission.id,
                };
                if seen.insert(key) {
                    result.push((key, mission));
                }
            }
        }
        result
    }

    pub(super) fn evaluate(
        &self,
        root: &Map<String, Value>,
        database: &ServiceDatabase,
        key: MissionKey,
        mission: &'static MissionDefinition,
    ) -> Result<EvaluatedMission, PersonalServiceError> {
        let scope = mission_scope(key, mission, database)?;
        let database_progress = self
            .stored
            .get(&(key.category, key.mission_id))
            .copied()
            .unwrap_or_default();
        let progress = match scope.as_ref() {
            _ if key.category == 9 => super::awake_progress(mission.id, &self.context, self.catalog),
            Some(scope)
                if !scope.is_lifetime
                    || mission.pattern.as_deref() == Some("get_item_count")
                    || root
                        .get("mission_window_progress")
                        .and_then(|values| values.get(&scope.key))
                        .is_some() =>
            {
                scoped_progress(root, key, mission, scope, database)?
            }
            _ if matches!(key.category, 2 | 3 | 4 | 10) => 0,
            _ => compute_progress(
                key.category,
                mission,
                &self.context,
                &self.counters,
                database_progress,
                self.catalog,
            ),
        };
        Ok(EvaluatedMission {
            key,
            mission,
            scope,
            progress,
        })
    }
}
// //// /查询当前时窗的真实任务进度 ////
