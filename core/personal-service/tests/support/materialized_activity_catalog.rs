// audience: internal
// # materialized-activity-catalog-test-data
// 管理目录由 CDN 活动目录, 内置关卡窗口和回忆录主表共同组成. 空存档保留每组日常的最后一个入口.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

// //// 从完整资源和内置主表整理管理目录及候选图 [@x380kkm 2026-09-26] ////
pub(super) fn expected_activity_images(cdn_root: &Path) -> BTreeMap<String, BTreeSet<String>> {
    let manifest: Value = serde_json::from_slice(
        &fs::read(cdn_root.join("activity-catalog.json"))
            .expect("CN resource directory contains activity-catalog.json"),
    )
    .expect("activity source catalog is JSON");
    let windows: Value = serde_json::from_str(include_str!("../../assets/cn-quest-windows.json"))
        .expect("quest windows are JSON");
    let histories: Value =
        serde_json::from_str(include_str!("../../assets/cn-player-history.json"))
            .expect("player history master is JSON");
    let hidden_daily_ids = windows["daily_groups"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|group| {
            let group = group.as_array().unwrap();
            let (_, aliases) = group.split_last().expect("daily group has an entry");
            aliases.iter().map(|id| id.as_str().unwrap())
        })
        .collect::<BTreeSet<_>>();
    let source_activities = manifest["activities"].as_array().unwrap();
    assert!(
        !source_activities.is_empty(),
        "CN source catalog has activities"
    );
    let mut expected = BTreeMap::new();
    for activity in source_activities {
        let id = activity["activity_id"].as_str().unwrap();
        let mut images = BTreeSet::new();
        if let Some(candidates) = activity["image_candidates"].as_array() {
            for candidate in candidates {
                let key = candidate["key"].as_str().unwrap();
                assert!(
                    images.insert(key.to_owned()),
                    "duplicate source image {id}: {key}"
                );
            }
        }
        if let Some(key) = activity["banner_key"].as_str() {
            images.insert(key.to_owned());
        }
        assert!(
            expected.insert(id.to_owned(), images).is_none(),
            "duplicate source activity {id}"
        );
    }
    expected.retain(|id, _| !hidden_daily_ids.contains(id.as_str()));
    for (key, quest) in windows["quests"].as_object().unwrap() {
        if quest["manage"] == true
            && !quest["activity_id"]
                .as_str()
                .is_some_and(|id| hidden_daily_ids.contains(id))
        {
            expected.entry(format!("quest:{key}")).or_default();
        }
    }
    for history in histories["histories"].as_array().unwrap() {
        let id = history["id"].as_i64().unwrap();
        expected.entry(format!("player-history:{id}")).or_default();
    }
    expected
}
// //// /从完整资源和内置主表整理管理目录及候选图 ////
