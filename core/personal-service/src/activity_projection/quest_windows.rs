// audience: internal
// # personal-service-quest-windows
// 目录和入场使用同一父活动与关卡窗口. 日常覆盖保留等价关卡的最早开放日和独立难度日期.
// 崩坏域的初始关卡使用常驻窗口, 后续关卡保留各自的时间窗口.

use super::{
    active_windows, projected_time, projection_window, OrderedValue, ProjectionMaster,
    ProjectionWindow, TimeKind, OPEN_RULE_END_MS, OPEN_RULE_START_MS,
};
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

pub(super) const CATALOG: &str = include_str!("../../assets/cn-quest-windows.json");
const PERMANENT_CHALLENGE_ACTIVITY: &str = "challenge-dungeon:1";

#[derive(Clone, Copy, Debug)]
pub(super) enum QuestTimes {
    Open,
    FirstRelease,
    Relative,
    Coverage,
}

#[derive(Deserialize)]
struct Catalog {
    events: BTreeMap<String, EventWindow>,
    quests: BTreeMap<String, QuestWindow>,
    daily_groups: Vec<Vec<String>>,
    boss_master: Value,
}

#[derive(Deserialize)]
struct EventWindow {
    start_at_ms: Option<i64>,
    end_at_ms: Option<i64>,
    weekdays: Vec<bool>,
    coverage_start_at_ms: Option<i64>,
}

#[derive(Deserialize)]
struct QuestWindow {
    activity_id: Option<String>,
    name: String,
    start_at_ms: Option<i64>,
    end_at_ms: Option<i64>,
    coverage_start_at_ms: Option<i64>,
    manage: bool,
}

#[derive(Clone, Copy)]
struct Window {
    start: i64,
    end: i64,
}

impl Window {
    fn source(start: Option<i64>, end: Option<i64>) -> Self {
        Self {
            start: start.unwrap_or_default(),
            end: end.unwrap_or(OPEN_RULE_END_MS),
        }
    }
    fn projected(window: &ProjectionWindow) -> Self {
        Self {
            start: window.start_at_ms,
            end: window.end_at_ms,
        }
    }
    fn contains(self, now: i64) -> bool {
        now >= self.start && now < self.end
    }
    fn intersect(self, other: Self) -> Self {
        Self {
            start: self.start.max(other.start),
            end: self.end.min(other.end),
        }
    }
}

fn catalog() -> Result<&'static Catalog, PersonalServiceError> {
    static VALUE: OnceLock<Result<Catalog, String>> = OnceLock::new();
    VALUE
        .get_or_init(|| {
            serde_json::from_str(CATALOG).map_err(|error| format!("invalid quest windows: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

pub(super) fn boss_master() -> Result<ProjectionMaster, PersonalServiceError> {
    serde_json::from_value(catalog()?.boss_master.clone())
        .map_err(|error| PersonalServiceError::new(format!("invalid boss projection: {error}")))
}

pub(super) fn closed_window() -> ProjectionWindow {
    projection_window(0, 1_000, i64::MAX)
}

// //// 为崩坏域的初始关卡应用默认常驻窗口 [@x380kkm 2026-09-29] ////
pub(super) fn apply_permanent_challenge(windows: &mut BTreeMap<String, ProjectionWindow>) {
    windows
        .entry(PERMANENT_CHALLENGE_ACTIVITY.to_owned())
        .or_insert_with(|| {
            let mut window = projection_window(OPEN_RULE_START_MS, OPEN_RULE_END_MS, i64::MAX);
            window.quest_times = QuestTimes::FirstRelease;
            window
        });
}

pub(super) fn matches_master(master: &ProjectionMaster, activity_id: &str) -> bool {
    activity_id.starts_with(&master.activity_id_prefix)
        || master
            .quest_category
            .is_some_and(|category| activity_id.starts_with(&format!("quest:{category}:")))
}

// //// 为等价日常选择一组入口并保留各难度开放日 [@x380kkm 2026-09-24] ////
pub(super) fn apply_daily_coverage(
    windows: &mut BTreeMap<String, ProjectionWindow>,
    configured: &BTreeSet<String>,
    now: i64,
) -> Result<BTreeSet<String>, PersonalServiceError> {
    let catalog = catalog()?;
    let mut hidden = BTreeSet::new();
    for group in &catalog.daily_groups {
        let canonical = group.last().expect("daily groups contain candidates");
        if !group.iter().any(|id| configured.contains(id)) {
            let start = catalog.events[canonical]
                .coverage_start_at_ms
                .expect("daily coverage has a start");
            let mut window = projection_window(start, OPEN_RULE_END_MS, i64::MAX);
            window.quest_times = QuestTimes::Coverage;
            windows.insert(canonical.clone(), window);
        }
        let selected = group
            .iter()
            .rev()
            .find(|id| {
                windows
                    .get(*id)
                    .is_some_and(|window| Window::projected(window).contains(now))
            })
            .or_else(|| group.iter().rev().find(|id| windows.contains_key(*id)))
            .cloned();
        for id in group.iter().filter(|id| Some(*id) != selected.as_ref()) {
            windows.insert(id.clone(), closed_window());
            hidden.insert(id.clone());
        }
    }
    Ok(hidden)
}

fn resolve_quest(
    catalog: &Catalog,
    key: &str,
    quest: &QuestWindow,
    windows: &BTreeMap<String, ProjectionWindow>,
) -> Window {
    let mut window = Window::source(quest.start_at_ms, quest.end_at_ms);
    let parent = quest
        .activity_id
        .as_ref()
        .and_then(|id| catalog.events.get(id).map(|event| (id, event)));
    let parent_window = parent.map(|(id, event)| {
        if let Some(managed) = windows.get(id) {
            match managed.quest_times {
                QuestTimes::Open => window = Window::projected(managed),
                QuestTimes::FirstRelease => {
                    if quest.start_at_ms == event.start_at_ms && quest.end_at_ms == event.end_at_ms
                    {
                        window = Window::projected(managed);
                    }
                }
                QuestTimes::Coverage => {
                    window = Window::source(quest.coverage_start_at_ms.or(quest.start_at_ms), None)
                }
                QuestTimes::Relative => {
                    if let Some(period) = managed.regional_period {
                        if let Some([start, end]) = period.quests.get(key) {
                            window = Window {
                                start: *start,
                                end: *end,
                            };
                        }
                        return Window::projected(managed);
                    }
                    let source_start = event.start_at_ms.unwrap_or_default();
                    window.start = managed
                        .start_at_ms
                        .saturating_add(window.start.saturating_sub(source_start).max(0));
                    window.end = quest
                        .end_at_ms
                        .filter(|end| event.end_at_ms.map_or(true, |parent_end| *end < parent_end))
                        .map_or(managed.end_at_ms, |end| {
                            managed
                                .start_at_ms
                                .saturating_add(end.saturating_sub(source_start).max(0))
                        });
                }
            }
            Window::projected(managed)
        } else {
            Window::source(event.start_at_ms, event.end_at_ms)
        }
    });
    if let Some(managed) = windows.get(&format!("quest:{key}")) {
        window = Window::projected(managed);
    }
    parent_window.map_or(window, |parent| window.intersect(parent))
}

// //// 以投影相同的窗口判断新战斗资格 [@x380kkm 2026-09-24] ////
pub(crate) fn is_quest_open(
    database: &ServiceDatabase,
    category: i64,
    quest_id: i64,
) -> Result<bool, PersonalServiceError> {
    let catalog = catalog()?;
    let key = format!("{category}:{quest_id}");
    let Some(quest) = catalog.quests.get(&key) else {
        return Ok(true);
    };
    let windows = active_windows(database)?;
    let now = database.current_server_time_millis()?;
    Ok(is_open(catalog, &key, quest, &windows, now))
}

// //// 一次读取活动规则并筛出开放关卡 [@x380kkm 2026-09-25] ////
pub(crate) fn open_quest_keys(
    database: &ServiceDatabase,
) -> Result<BTreeSet<(i64, i64)>, PersonalServiceError> {
    let catalog = catalog()?;
    let windows = active_windows(database)?;
    let now = database.current_server_time_millis()?;
    Ok(catalog
        .quests
        .iter()
        .filter_map(|(key, quest)| {
            if !is_open(catalog, key, quest, &windows, now) {
                return None;
            }
            let (category, quest_id) = key.split_once(':')?;
            Some((category.parse().ok()?, quest_id.parse().ok()?))
        })
        .collect())
}

fn is_open(
    catalog: &Catalog,
    key: &str,
    quest: &QuestWindow,
    windows: &BTreeMap<String, ProjectionWindow>,
    now: i64,
) -> bool {
    if let Some(window) = quest.activity_id.as_ref().and_then(|id| windows.get(id)) {
        if let Some(quest_ids) = window.tower_quest_ids {
            let selected = key
                .split_once(':')
                .and_then(|(_, id)| id.parse::<i64>().ok())
                .is_some_and(|id| quest_ids.contains(&id));
            return selected && Window::projected(window).contains(now);
        }
    }
    if !resolve_quest(catalog, key, quest, windows).contains(now) {
        return false;
    }
    if let Some(id) = &quest.activity_id {
        if !windows.contains_key(id) {
            if let Some(event) = catalog
                .events
                .get(id)
                .filter(|event| event.weekdays.len() == 7)
            {
                let weekday =
                    ((now + 3 * 3_600_000).div_euclid(86_400_000) + 3).rem_euclid(7) as usize;
                return event.weekdays[weekday];
            }
        }
    }
    true
}

// //// 将关卡时间和管理解锁规则写入原关卡行 [@x380kkm 2026-09-24] ////
pub(super) fn project_quests(
    value: &mut OrderedValue,
    master: &ProjectionMaster,
    windows: &BTreeMap<String, ProjectionWindow>,
) -> Result<(), PersonalServiceError> {
    let Some(category) = master.quest_category.filter(|category| *category != 20) else {
        return Ok(());
    };
    let (Some(start_index), Some(end_index)) = (master.start_index, master.end_index) else {
        return Ok(());
    };
    let catalog = catalog()?;
    fn visit(
        value: &mut OrderedValue,
        category: i64,
        start_index: usize,
        end_index: usize,
        catalog: &Catalog,
        windows: &BTreeMap<String, ProjectionWindow>,
    ) {
        match value {
            OrderedValue::Map(entries) => {
                for (_, value) in entries {
                    visit(value, category, start_index, end_index, catalog, windows);
                }
            }
            OrderedValue::Row(row) => {
                let Some(id) = row.first() else {
                    return;
                };
                let key = format!("{category}:{id}");
                let Some(quest) = catalog.quests.get(&key) else {
                    return;
                };
                let window = resolve_quest(catalog, &key, quest, windows);
                let projection = if window.start < window.end {
                    projection_window(window.start, window.end, i64::MAX)
                } else {
                    closed_window()
                };
                row[start_index] = projected_time(TimeKind::Start, &projection);
                row[end_index] = projected_time(TimeKind::End, &projection);
            }
        }
    }
    visit(value, category, start_index, end_index, catalog, windows);
    Ok(())
}

pub(crate) fn management_definitions(
    hidden_daily_ids: &BTreeSet<String>,
) -> Result<Vec<Value>, PersonalServiceError> {
    Ok(catalog()?.quests.iter().filter(|(_, quest)| quest.manage && !quest.activity_id.as_ref().is_some_and(|id| hidden_daily_ids.contains(id))).map(|(key, quest)| json!({
        "activity_id":format!("quest:{key}"), "name":quest.name, "kind":"quest",
        "tags":[if key.starts_with("2:") { "领主战" } else { "日常关卡" }],
        "description":"关卡开放时间", "default_start_at_ms":quest.coverage_start_at_ms.or(quest.start_at_ms),
        "default_end_at_ms":if quest.coverage_start_at_ms.is_some() { OPEN_RULE_END_MS } else { quest.end_at_ms.unwrap_or(OPEN_RULE_END_MS) }
    })).collect())
}

pub(crate) fn default_quest_window(
    activity_id: &str,
) -> Result<Option<(i64, i64)>, PersonalServiceError> {
    if activity_id == PERMANENT_CHALLENGE_ACTIVITY {
        return Ok(Some((OPEN_RULE_START_MS, OPEN_RULE_END_MS)));
    }
    let catalog = catalog()?;
    if let Some(start) = catalog
        .events
        .get(activity_id)
        .and_then(|event| event.coverage_start_at_ms)
    {
        return Ok(Some((start, OPEN_RULE_END_MS)));
    }
    let Some(key) = activity_id.strip_prefix("quest:") else {
        return Ok(None);
    };
    Ok(catalog.quests.get(key).map(|quest| {
        (
            quest
                .coverage_start_at_ms
                .or(quest.start_at_ms)
                .unwrap_or_default(),
            if quest.coverage_start_at_ms.is_some() {
                OPEN_RULE_END_MS
            } else {
                quest.end_at_ms.unwrap_or(OPEN_RULE_END_MS)
            },
        )
    }))
}

#[cfg(test)]
mod tests;
