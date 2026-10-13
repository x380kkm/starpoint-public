// audience: internal
// # personal-service-cn-story-catalog-tests
// 纯剧情按原始主表缺省 bRankTime 字段分类, 与当前服务关卡集合取交集.
// 两种真实结算入口分别首通全部关卡, 保存奖励和进度, 重试保持同一份状态.

use super::*;
use serde_json::json;
use std::collections::BTreeMap;

const STORY_SOURCES: &[(i64, &str)] = &[
    (1, include_str!("../../../../assets/main_quest.json")),
    (3, include_str!("../../../../assets/character_quest.json")),
    (
        7,
        include_str!("../../../../assets/advent_event_quest.json"),
    ),
    (
        10,
        include_str!("../../../../assets/story_event_single_quest.json"),
    ),
    (
        18,
        include_str!("../../../../assets/world_story_event_quest.json"),
    ),
];

struct StoryCase<'a> {
    key: String,
    quest: &'a Value,
    at_ms: i64,
}

// //// 从实际主表取得纯剧情和有效开放时刻 [@x380kkm 2026-09-27] ////
fn story_catalog(fixture: &Value) -> Vec<StoryCase<'_>> {
    let windows: Value =
        serde_json::from_str(include_str!("../../assets/cn-quest-windows.json")).unwrap();
    let mut quests = Vec::new();
    let mut counts = BTreeMap::new();
    for (category, raw) in STORY_SOURCES {
        let master: Value = serde_json::from_str(raw).unwrap();
        for (id, definition) in master.as_object().unwrap() {
            let key = format!("{category}:{id}");
            if definition.get("bRankTime").is_some() {
                continue;
            }
            let Some(quest) = fixture["quests"].get(&key) else {
                continue;
            };
            let own_window = &windows["quests"][&key];
            let activity = quest["activity_id"].as_str().unwrap_or_default();
            let event_window = &windows["events"][activity];
            let start = [own_window, event_window]
                .into_iter()
                .filter_map(|window| window["start_at_ms"].as_i64())
                .max();
            let end = [own_window, event_window]
                .into_iter()
                .filter_map(|window| window["end_at_ms"].as_i64())
                .min();
            let mut at_ms = start.map_or(1_704_067_200_000, |start| start + 1000);
            if let Some(end) = end {
                at_ms = at_ms.min(end - 1000);
            }
            assert!(
                start.map_or(true, |start| at_ms >= start),
                "{key}: invalid activity window"
            );
            *counts.entry(*category).or_insert(0) += 1;
            quests.push(StoryCase { key, quest, at_ms });
        }
    }
    assert_eq!(
        counts,
        BTreeMap::from([(1, 197), (3, 1318), (7, 98), (10, 25), (18, 472)])
    );
    quests.sort_by_key(|case| {
        (
            case.quest["category"].as_i64(),
            case.quest["quest_id"].as_i64(),
        )
    });
    quests
}

// //// 在隔离服务调整关卡合法时钟并读取已提交状态 [@x380kkm 2026-09-27] ////
fn set_clock(service: &PersonalService, db: &Connection, at_ms: i64) {
    let iso: String = db
        .query_row(
            "SELECT strftime('%Y-%m-%dT%H:%M:%fZ', ?1 / 1000.0, 'unixepoch')",
            [at_ms],
            |row| row.get(0),
        )
        .unwrap();
    let authorization = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        "PUT",
        "/v1/time",
        "application/json",
        &[("Authorization", &authorization)],
        json!({"enabled":true, "iso":iso, "rate":1})
            .to_string()
            .as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

fn snapshot_text(db: &Connection) -> String {
    db.query_row("SELECT data_json FROM player_snapshots", [], |row| {
        row.get(0)
    })
    .unwrap()
}

fn episode_count(db: &Connection) -> i64 {
    db.query_row("SELECT COALESCE(SUM(value),0) FROM player_mission_counters WHERE pattern='episode_clear_count'", [], |row| row.get(0)).unwrap()
}
// //// /在隔离服务调整关卡合法时钟并读取已提交状态 ////

// //// 依据首通主表和实际任务通知核对奖励入账 [@x380kkm 2026-09-27] ////
fn rewards_for_response(
    quest: &Value,
    response: &Value,
    missions: &Value,
) -> BTreeMap<(i64, i64), i64> {
    let mut rewards = BTreeMap::new();
    if let Some(kind) = quest["clear_reward"]["type"].as_i64() {
        let reward = &quest["clear_reward"];
        rewards.insert(
            (kind, reward["id"].as_i64().unwrap_or_default()),
            reward["count"].as_i64().unwrap_or(1),
        );
    }
    for info in response["mission_info"].as_array().unwrap() {
        let category = info["mission_category_id"].as_i64().unwrap().to_string();
        let mission = missions["categories"][category]
            .as_array()
            .unwrap()
            .iter()
            .find(|mission| mission["id"] == info["mission_id"])
            .unwrap();
        let stage = mission["stages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|stage| stage["reward_id"] == info["mission_reward_id"])
            .unwrap();
        for reward in stage["rewards"].as_array().unwrap() {
            let (kind, id) = match reward["kind"].as_i64().unwrap() {
                0 => (3, 0),
                1 => (0, reward["item_id"].as_i64().unwrap()),
                2 => (1, reward["equipment_id"].as_i64().unwrap()),
                3 => (4, 0),
                4 => (2, reward["character_id"].as_i64().unwrap()),
                5 => (5, 0),
                6 | 7 => continue,
                kind => panic!("unsupported mission reward {kind}"),
            };
            *rewards.entry((kind, id)).or_default() += reward["amount"].as_i64().unwrap();
        }
    }
    rewards
}

fn assert_rewards(
    case: &StoryCase<'_>,
    before: &Value,
    after: &Value,
    response: &Value,
    missions: &Value,
) {
    let rewards = rewards_for_response(case.quest, response, missions);
    for (kind, field) in [(3, "free_vmoney"), (4, "free_mana"), (5, "exp_pool")] {
        let expected = before["user_info"][field].as_i64().unwrap()
            + rewards.get(&(kind, 0)).copied().unwrap_or_default();
        assert_eq!(after["user_info"][field], expected, "{}: {field}", case.key);
    }
    for ((kind, id), count) in rewards {
        let id_key = id.to_string();
        match kind {
            0 | 6 | 7 => {
                let expected = before["item_list"][&id_key].as_i64().unwrap_or_default() + count;
                assert_eq!(
                    after["item_list"][&id_key], expected,
                    "{}: item {id}",
                    case.key
                );
                assert_eq!(
                    response["item_list"][&id_key], expected,
                    "{}: item response {id}",
                    case.key
                );
            }
            1 | 2 => {
                let (field, response_field, id_field) = if kind == 1 {
                    ("user_equipment_list", "equipment_list", "equipment_id")
                } else {
                    ("user_character_list", "character_list", "character_id")
                };
                let previous = before[field].get(&id_key);
                let expected =
                    previous.map_or(-1, |owned| owned["stack"].as_i64().unwrap()) + count;
                assert_eq!(
                    after[field][&id_key]["stack"], expected,
                    "{}: {field} {id}",
                    case.key
                );
                let delivered = response[response_field]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|entry| entry[id_field] == id)
                    .unwrap_or_else(|| {
                        panic!(
                            "{}: missing reward response {response_field} {id}",
                            case.key
                        )
                    });
                let delivered_stack = if kind == 2 && previous.is_none() {
                    delivered["stack"].as_i64().unwrap_or_default()
                } else {
                    delivered["stack"].as_i64().unwrap()
                };
                assert_eq!(
                    delivered_stack, expected,
                    "{}: reward response {id}",
                    case.key
                );
                if kind == 2 && previous.is_none() {
                    assert!(
                        response["joined_character_id_list"]
                            .as_array()
                            .unwrap()
                            .contains(&json!(id)),
                        "{}: missing joined character {id}",
                        case.key
                    );
                }
            }
            3..=5 => {}
            kind => panic!("{}: unsupported story reward {kind}", case.key),
        }
    }
    for field in ["free_vmoney", "free_mana"] {
        assert_eq!(
            response["user_info"][field], after["user_info"][field],
            "{}: user info {field}",
            case.key
        );
    }
}
// //// /依据首通主表和实际任务通知核对奖励入账 ////

// //// 执行全集首通和交叉入口重试并在重启后读取进度 [@x380kkm 2026-09-27] ////
fn settle_catalog(first: &str, repeated: &str, device: i64) {
    let fixture: Value =
        serde_json::from_str(include_str!("../../assets/cn-single-battle.json")).unwrap();
    let missions: Value =
        serde_json::from_str(include_str!("../../assets/cn-mission-master.json")).unwrap();
    let quests = story_catalog(&fixture);
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    set_clock(&service, &db, 1_704_067_200_000);
    let viewer_id = signup_viewer(&service, device);
    let mut previous: Value = serde_json::from_str(&snapshot_text(&db)).unwrap();
    for (category, _) in STORY_SOURCES {
        previous["quest_progress"][category.to_string()] = json!([]);
    }
    for case in &quests {
        let reward = &case.quest["clear_reward"];
        let field = match reward["type"].as_i64() {
            Some(1) => "user_equipment_list",
            Some(2) => "user_character_list",
            _ => continue,
        };
        previous[field]
            .as_object_mut()
            .unwrap()
            .remove(&reward["id"].as_i64().unwrap().to_string());
    }
    db.execute(
        "UPDATE player_snapshots SET data_json=?1",
        [previous.to_string()],
    )
    .unwrap();
    let mut expected_episodes = episode_count(&db);
    let mut clock = 1_704_067_200_000;
    let mut active_updates = 0;
    for (index, case) in quests.iter().enumerate() {
        if clock != case.at_ms {
            set_clock(&service, &db, case.at_ms);
            clock = case.at_ms;
        }
        let body = json!({"viewer_id":viewer_id, "category":case.quest["category"], "quest_id":case.quest["quest_id"], "party_id":1});
        let response = cn_support::send_request(service.port(), first, &encode_request(&body));
        assert!(
            response.starts_with("HTTP/1.1 200"),
            "{}: {first}: {response}",
            case.key
        );
        let result = decode_response::<Value>(&response).data;
        let serialized = snapshot_text(&db);
        let settled: Value = serde_json::from_str(&serialized).unwrap();
        assert_rewards(case, &previous, &settled, &result, &missions);
        let category = case.quest["category"].as_i64().unwrap().to_string();
        let progress = settled["quest_progress"][category]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["quest_id"] == case.quest["quest_id"])
            .unwrap();
        assert_eq!(
            progress["finished"], true,
            "{}: first clear progress",
            case.key
        );
        if case.quest["category"] == 3 {
            expected_episodes += 1;
        }
        assert_eq!(
            episode_count(&db),
            expected_episodes,
            "{}: story mission count",
            case.key
        );
        active_updates += result["active_mission_list"].as_array().unwrap().len();
        let response = cn_support::send_request(service.port(), repeated, &encode_request(&body));
        assert!(
            response.starts_with("HTTP/1.1 200"),
            "{}: {repeated}: {response}",
            case.key
        );
        assert_eq!(
            decode_response::<Value>(&response).data,
            json!([]),
            "{}: repeat response",
            case.key
        );
        assert_eq!(
            snapshot_text(&db),
            serialized,
            "{}: retry changes saved rewards or progress",
            case.key
        );
        assert_eq!(
            episode_count(&db),
            expected_episodes,
            "{}: retry changes story mission count",
            case.key
        );
        previous = settled;
        if (index + 1) % 250 == 0 {
            println!(
                "{first}: settled {} / {} pure story quests",
                index + 1,
                quests.len()
            );
        }
    }
    service.stop().unwrap();
    let restarted = PersonalService::start(root.path(), 0).unwrap();
    let loaded = decode_response::<Value>(&cn_support::send_request(
        restarted.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ))
    .data;
    for case in &quests {
        let category = case.quest["category"].as_i64().unwrap().to_string();
        let records = loaded["quest_progress"][category]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["quest_id"] == case.quest["quest_id"])
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 1, "{}: saved progress identity", case.key);
        assert_eq!(
            records[0]["finished"], true,
            "{}: saved story progress",
            case.key
        );
        let reward = &case.quest["clear_reward"];
        let field = match reward["type"].as_i64() {
            Some(1) => "user_equipment_list",
            Some(2) => "user_character_list",
            _ => continue,
        };
        let id = reward["id"].as_i64().unwrap().to_string();
        assert_eq!(
            loaded[field][&id]["stack"], previous[field][&id]["stack"],
            "{}: reward after restart",
            case.key
        );
    }
    println!("{first}: settled {} pure story quests, checked clear rewards, retry state, mission count and restart progress; active mission updates={active_updates}", quests.len());
    restarted.stop().unwrap();
}

#[test]
fn settles_all_story_categories_once() {
    settle_catalog(
        "/api/index.php/story_quest/finish",
        "/api/index.php/story_quest/finish_with_skip",
        270901,
    );
}

#[test]
fn settles_all_skipped_story_categories_once() {
    settle_catalog(
        "/api/index.php/story_quest/finish_with_skip",
        "/api/index.php/story_quest/finish",
        270902,
    );
}
// //// /执行全集首通和交叉入口重试并在重启后读取进度 ////
