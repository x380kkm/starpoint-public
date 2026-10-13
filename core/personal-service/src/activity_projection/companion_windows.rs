// audience: internal
// # activity-companion-windows
// 收集任务活动没有自己的开放规则时, 跟随原始开始时间相同且已启用的主活动开放.
// 主活动常开时收集任务活动同样常开; 主活动按时间窗开放时, 收集任务活动从主活动的开始时间起至少保持自己的原始时长.
// 收集任务活动自己的规则, 临时开放和停用状态始终优先.
// apply 在套用区域期别之前调用: 此时窗口只来自开放规则和临时开放, disabled 列出其中被停用的活动.

use super::{
    decode_ordered_map, master_seed, matches_activity_filter, parse_master_time, projection_manifest,
    projection_window, quest_windows::QuestTimes, OrderedValue, ProjectionWindow,
};
use crate::PersonalServiceError;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

const COMPANION_MASTER: &str = "collect_item_event";
const MAIN_MASTERS: [&str; 13] = [
    "world_story_event",
    "story_event",
    "advent_event",
    "raid_event",
    "ranking_event",
    "rush_event",
    "carnival_event",
    "tower_dungeon_event",
    "challenge_dungeon_event",
    "expert_single_event",
    "hard_multi_event",
    "score_attack_event",
    "solo_time_attack_event",
];

struct Companion {
    duration_ms: i64,
    main_activity_ids: Vec<String>,
}

fn catalog() -> Result<&'static BTreeMap<String, Companion>, PersonalServiceError> {
    static VALUE: OnceLock<Result<BTreeMap<String, Companion>, String>> = OnceLock::new();
    VALUE
        .get_or_init(read_catalog)
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

// //// 按原始开始时间把收集任务活动关联到主活动 [@x380kkm 2026-10-05] ////
fn read_catalog() -> Result<BTreeMap<String, Companion>, String> {
    let mut mains_by_start = BTreeMap::<i64, Vec<String>>::new();
    for name in MAIN_MASTERS {
        for (activity_id, [start_at_ms, _]) in original_periods(name)? {
            mains_by_start.entry(start_at_ms).or_default().push(activity_id);
        }
    }
    Ok(original_periods(COMPANION_MASTER)?
        .into_iter()
        .filter_map(|(activity_id, [start_at_ms, end_at_ms])| {
            let main_activity_ids = mains_by_start.get(&start_at_ms)?.clone();
            Some((activity_id, Companion { duration_ms: end_at_ms - start_at_ms, main_activity_ids }))
        })
        .collect())
}

fn original_periods(name: &str) -> Result<Vec<(String, [i64; 2])>, String> {
    let manifest = projection_manifest().map_err(|error| error.to_string())?;
    let master = manifest
        .masters
        .iter()
        .find(|master| master.name == name)
        .ok_or_else(|| format!("missing companion projection master: {name}"))?;
    let (Some(start_index), Some(end_index)) = (master.start_index, master.end_index) else {
        return Err(format!("companion projection master has no period: {name}"));
    };
    let seed = master_seed(name).ok_or_else(|| format!("missing companion projection seed: {name}"))?;
    let OrderedValue::Map(rows) = decode_ordered_map(seed)? else {
        return Err(format!("companion projection seed is not a map: {name}"));
    };
    Ok(rows
        .into_iter()
        .filter_map(|(id, value)| {
            let OrderedValue::Row(row) = value else {
                return None;
            };
            if !matches_activity_filter(master, &row) {
                return None;
            }
            let start_at_ms = parse_master_time(row.get(start_index)?)?;
            let end_at_ms = parse_master_time(row.get(end_index)?)?;
            Some((format!("{}{id}", master.activity_id_prefix), [start_at_ms, end_at_ms]))
        })
        .collect())
}
// //// /按原始开始时间把收集任务活动关联到主活动 ////

// //// 为没有开放规则的收集任务活动派生主活动窗口 [@x380kkm 2026-10-05] ////
pub(super) fn apply(
    windows: &mut BTreeMap<String, ProjectionWindow>,
    configured: &mut BTreeSet<String>,
    disabled: &BTreeSet<String>,
) -> Result<(), PersonalServiceError> {
    for (activity_id, companion) in catalog()? {
        if configured.contains(activity_id) {
            continue;
        }
        let Some(main) = companion
            .main_activity_ids
            .iter()
            .filter(|main_id| !disabled.contains(*main_id))
            .filter_map(|main_id| windows.get(main_id))
            .min_by_key(|window| window.start_at_ms)
        else {
            continue;
        };
        let window = follow_main(main, companion.duration_ms);
        windows.insert(activity_id.clone(), window);
        configured.insert(activity_id.clone());
    }
    Ok(())
}

fn follow_main(main: &ProjectionWindow, duration_ms: i64) -> ProjectionWindow {
    let relative = matches!(main.quest_times, QuestTimes::Relative);
    let end_at_ms = if relative {
        main.end_at_ms.max(main.start_at_ms.saturating_add(duration_ms))
    } else {
        main.end_at_ms
    };
    let mut window = projection_window(main.start_at_ms, end_at_ms, main.expires_at_ms);
    if relative {
        window.quest_times = QuestTimes::Relative;
    }
    window.follows_main = true;
    window
}
// //// /为没有开放规则的收集任务活动派生主活动窗口 ////

// //// 判断活动是否为跟随主活动的收集任务活动 [@x380kkm 2026-10-05] ////
pub(super) fn is_companion(activity_id: &str) -> Result<bool, PersonalServiceError> {
    Ok(catalog()?.contains_key(activity_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    // //// 验证一周年和四周年的配套任务关联到同时开始的主活动 [@x380kkm 2026-10-05] ////
    #[test]
    fn links_anniversary_missions_to_main_events() {
        let catalog = catalog().unwrap();
        let first = &catalog["collect-item:10004"];
        assert!(first.main_activity_ids.iter().any(|id| id == "world-story:100402"));
        assert_eq!(first.duration_ms, 28 * 86_400_000 - 1_000);
        assert!(catalog["collect-item:100009"].main_activity_ids.iter().any(|id| id == "story:200031"));
        assert!(!catalog.contains_key("collect-item:1"));
    }
    // //// /验证一周年和四周年的配套任务关联到同时开始的主活动 ////

    // //// 验证配套任务窗口保持原始时长且尊重停用和自有规则 [@x380kkm 2026-10-05] ////
    #[test]
    fn derives_companion_window_from_main_rule() {
        let day = 86_400_000;
        let main_id = "world-story:100402".to_owned();
        let companion_id = "collect-item:10004";
        let mut main = projection_window(1_000 * day, 1_014 * day, i64::MAX);
        main.quest_times = QuestTimes::Relative;
        let mut windows = BTreeMap::from([(main_id.clone(), main.clone())]);
        let mut configured = BTreeSet::from([main_id.clone()]);
        apply(&mut windows, &mut configured, &BTreeSet::new()).unwrap();
        let companion = &windows[companion_id];
        assert!(companion.follows_main && matches!(companion.quest_times, QuestTimes::Relative));
        assert_eq!((companion.start_at_ms, companion.end_at_ms), (1_000 * day, 1_028 * day - 1_000));
        assert!(configured.contains(companion_id));

        let mut windows = BTreeMap::from([(main_id.clone(), main.clone())]);
        let mut configured = BTreeSet::from([main_id.clone()]);
        apply(&mut windows, &mut configured, &BTreeSet::from([main_id.clone()])).unwrap();
        assert!(!windows.contains_key(companion_id));

        let mut windows = BTreeMap::from([(main_id.clone(), main)]);
        let mut configured = BTreeSet::from([main_id, companion_id.to_owned()]);
        apply(&mut windows, &mut configured, &BTreeSet::new()).unwrap();
        assert!(!windows.contains_key(companion_id));
    }
    // //// /验证配套任务窗口保持原始时长且尊重停用和自有规则 ////
}
