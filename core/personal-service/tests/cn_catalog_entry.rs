// audience: internal
// # personal-service-cn-catalog-entry-tests
// 实际关卡主表中的战斗通过 HTTP 开战, 加载和结算, 关闭期间保持入场资源, 结算重试保持奖励和进度.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{
    decode_response, encode_request, send_request, LoadRequest, SignupData, SignupRequest,
};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::collections::BTreeMap;
use std::path::Path;
use tempfile::TempDir;

const SOURCES: &[(i64, &str)] = &[
    (1, "main_quest"),
    (2, "boss_battle_quest"),
    (3, "character_quest"),
    (4, "ex_quest"),
    (6, "daily_week_event_quest"),
    (7, "advent_event_quest"),
    (10, "story_event_single_quest"),
    (11, "ranking_event_single_quest"),
    (13, "challenge_dungeon_event_quest"),
    (14, "daily_exp_mana_event_quest"),
    (15, "practice_quest"),
    (18, "world_story_event_quest"),
    (19, "world_story_event_boss_battle_quest"),
    (20, "tower_dungeon_event_quest"),
    (21, "expert_single_event_quest"),
    (22, "carnival_event_quest"),
    (23, "raid_event_quest"),
    (24, "rush_event_quest"),
    (25, "solo_time_attack_event_quest"),
    (26, "hard_multi_event_quest"),
    (27, "score_attack_event_quest"),
];

// //// 发送隔离服务的时间与战斗请求 [@x380kkm 2026-09-27] ////
fn manage(service: &PersonalService, path: &str, body: Value) {
    let token = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        "PUT",
        path,
        "application/json",
        &[("Authorization", &token)],
        body.to_string().as_bytes(),
    );
    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "{path}: {response}"
    );
}

fn set_time(service: &PersonalService, now: i64) {
    manage(
        service,
        "/v1/time",
        json!({"enabled":true, "unix_time_ms":now, "rate":0.0001}),
    );
}

fn battle(service: &PersonalService, action: &str, body: &Value) -> String {
    send_request(
        service.port(),
        &format!("/api/index.php/single_battle_quest/{action}"),
        &encode_request(body),
    )
}

fn read_snapshot(db: &Connection) -> String {
    db.query_row("SELECT data_json FROM player_snapshots", [], |row| {
        row.get(0)
    })
    .unwrap()
}
// //// /发送隔离服务的时间与战斗请求 ////

// //// 从客户端战斗字段和服务目录取得同一关卡集合 [@x380kkm 2026-09-27] ////
fn battle_catalog(fixture: &Value) -> Vec<(i64, i64)> {
    let asset_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let mut entries = Vec::new();
    for &(category, source) in SOURCES {
        let master: Value = serde_json::from_str(
            &std::fs::read_to_string(asset_root.join(format!("{source}.json"))).unwrap(),
        )
        .unwrap();
        for (id, row) in master.as_object().unwrap() {
            let key = format!("{category}:{id}");
            if row.get("bRankTime").is_some() && fixture["quests"].get(&key).is_some() {
                entries.push((category, id.parse().unwrap()));
            }
        }
    }
    entries.sort_unstable();
    entries
}
// //// /从客户端战斗字段和服务目录取得同一关卡集合 ////

// //// 逐关执行开战与结算并验证关闭边界和奖励重放 [@x380kkm 2026-09-27] ////
#[test]
fn settles_battle_catalog_through_open_and_closed_windows() {
    let workers = (0..4)
        .map(|worker| std::thread::spawn(move || exercise_catalog(worker, 4)))
        .collect::<Vec<_>>();
    for worker in workers {
        worker.join().unwrap();
    }
}

fn exercise_catalog(worker: usize, worker_count: usize) {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let signup = decode_response::<SignupData>(&send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 9273140 }),
    ));
    let viewer = signup.data_headers.viewer_id;
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    db.busy_timeout(std::time::Duration::from_secs(5)).unwrap();
    let fixture: Value =
        serde_json::from_str(include_str!("../assets/cn-single-battle.json")).unwrap();
    let windows: Value =
        serde_json::from_str(include_str!("../assets/cn-quest-windows.json")).unwrap();
    let rules: Value =
        serde_json::from_str(include_str!("../assets/cn-battle-reward-rules.json")).unwrap();
    let entries = battle_catalog(&fixture);
    let mut base: Value = serde_json::from_str(&read_snapshot(&db)).unwrap();
    base["user_info"]["stamina"] = json!(1_000_000);
    base["quest_progress"] = json!({});
    for quest in fixture["quests"].as_object().unwrap().values() {
        if let Some(id) = quest["entry_item_id"].as_i64().filter(|id| *id > 0) {
            base["item_list"][id.to_string()] = json!(1_000_000);
        }
    }
    let mut failures = Vec::new();
    let mut totals = BTreeMap::<i64, usize>::new();
    let mut boundaries = 0;
    for (category, quest_id) in entries
        .into_iter()
        .enumerate()
        .filter(|(index, _)| index % worker_count == worker)
        .map(|(_, entry)| entry)
    {
        let key = format!("{category}:{quest_id}");
        let window = &windows["quests"][&key];
        let parent_id = window["activity_id"].as_str();
        let parent = parent_id
            .map(|id| &windows["events"][id])
            .unwrap_or(&Value::Null);
        let start = window["start_at_ms"]
            .as_i64()
            .unwrap_or(0)
            .max(parent["start_at_ms"].as_i64().unwrap_or(0));
        let end = window["end_at_ms"]
            .as_i64()
            .unwrap_or(i64::MAX)
            .min(parent["end_at_ms"].as_i64().unwrap_or(i64::MAX));
        let mut now = if start == 0 {
            1_893_456_000_000
        } else {
            start + 1_000
        };
        if let Some(weekdays) = parent["weekdays"].as_array().filter(|days| days.len() == 7) {
            for _ in 0..7 {
                let weekday =
                    ((now + 3 * 3_600_000).div_euclid(86_400_000) + 3).rem_euclid(7) as usize;
                if weekdays[weekday] == true {
                    break;
                }
                now += 86_400_000;
            }
        }
        let body = json!({"viewer_id":viewer, "api_count":1, "category":category, "quest_id":quest_id,
            "play_id":format!("catalog-{key}"), "party_id":1, "use_boss_boost_point":false,
            "use_boost_point":false, "is_auto_start_mode":false});
        base["user_info"]["stamina_heal_time"] = json!(now / 1000);
        db.execute(
            "UPDATE player_snapshots SET data_json=?1",
            [base.to_string()],
        )
        .unwrap();
        set_time(&service, now);
        if category == 6 {
            manage(
                &service,
                &format!("/v1/activities/{}/mode", parent_id.unwrap()),
                json!({"mode":"always"}),
            );
        } else if now >= end {
            failures.push(format!(
                "{key}: source window {start}..{end} has no chosen open time"
            ));
            continue;
        }
        let mut finished = None;
        let response = battle(&service, "start", &body);
        if !response.starts_with("HTTP/1.1 200 OK") {
            failures.push(format!("{key} start: {response}"));
        } else {
            let started = decode_response::<Value>(&response);
            if started.data_headers.result_code != 1
                || started.data["category_id"] != category
                || !started.data["start_time"].is_number()
            {
                failures.push(format!(
                    "{key} start result {}: {}",
                    started.data_headers.result_code, started.data
                ));
            } else {
                let identity: (String, i64, i64) = db
                    .query_row(
                        "SELECT play_id,category,quest_id FROM active_single_quests",
                        [],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .unwrap();
                assert_eq!(
                    identity,
                    (format!("catalog-{key}"), category, quest_id),
                    "{key} active battle identity"
                );
                let loaded = decode_response::<Value>(&send_request(
                    service.port(),
                    "/api/index.php/load",
                    &encode_request(&LoadRequest {
                        viewer_id: viewer,
                        keychain: viewer,
                    }),
                ))
                .data;
                assert!(
                    loaded["unfinished_quest_list"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|quest| quest["play_id"] == body["play_id"]),
                    "{key} load loses active battle"
                );
                let scripted_loss = rules["lose_quests"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|quest| quest == &key);
                let finish_body = json!({
                    "viewer_id":viewer, "play_id":body["play_id"], "category":category, "quest_id":quest_id,
                    "elapsed_time_ms":60_000, "score":4_294_967_296_f64, "add_mana":7,
                    "is_accomplished":!scripted_loss, "is_lose":scripted_loss,
                    "statistics":{"clear_phase":1, "zones":[], "party":{
                        "characters":[{"id":1},null,null], "unison_characters":[null,null,null],
                        "equipments":[null,null,null], "ability_soul_ids":[null,null,null]}}
                });
                let response = battle(&service, "finish", &finish_body);
                if response.starts_with("HTTP/1.1 200 OK") {
                    let settled = decode_response::<Value>(&response);
                    if settled.data_headers.result_code == 1 {
                        *totals.entry(category).or_default() += 1;
                        finished = Some((finish_body, settled.data));
                        let saved: Value = serde_json::from_str(&read_snapshot(&db)).unwrap();
                        assert!(
                            saved["quest_progress"][category.to_string()]
                                .as_array()
                                .unwrap()
                                .iter()
                                .any(|quest| quest["quest_id"] == quest_id
                                    && quest["finished"] == true),
                            "{key} finish loses progress"
                        );
                    } else {
                        failures.push(format!(
                            "{key} finish result {}",
                            settled.data_headers.result_code
                        ));
                    }
                } else {
                    failures.push(format!("{key} finish: {response}"));
                }
            }
        }
        if category == 6 {
            db.execute(
                "DELETE FROM activity_schedules WHERE activity_id=?1",
                [parent_id.unwrap()],
            )
            .unwrap();
        }
        // 崩坏域首发关卡使用常驻窗口, 关闭边界取常驻窗口两端.
        let (closed_start, closed_end) = if parent_id == Some("challenge-dungeon:1")
            && window["start_at_ms"] == parent["start_at_ms"]
        {
            (946_684_800_000, 4_102_444_799_000)
        } else {
            (start, end)
        };
        for closed_at in [
            closed_start.checked_sub(1_000).filter(|time| *time > 0),
            closed_end.checked_add(1_000),
        ]
        .into_iter()
        .flatten()
        .filter(|_| category != 6)
        {
            let before = read_snapshot(&db);
            set_time(&service, closed_at);
            let response = battle(&service, "start", &body);
            if response.starts_with("HTTP/1.1 200 OK")
                && decode_response::<Value>(&response).data_headers.result_code == 4050
            {
                boundaries += 1;
                assert_eq!(
                    read_snapshot(&db),
                    before,
                    "{key} closed start changes snapshot"
                );
            } else {
                failures.push(format!("{key} closed at {closed_at}: {response}"));
                let _ = battle(&service, "abort", &body);
            }
        }
        if let Some((finish_body, data)) = finished {
            let before = read_snapshot(&db);
            assert_eq!(
                decode_response::<Value>(&battle(&service, "finish", &finish_body)).data,
                data,
                "{key} finish replay"
            );
            assert_eq!(read_snapshot(&db), before, "{key} repeated reward");
        }
        if totals.values().sum::<usize>() % 50 == 0 {
            println!("worker {worker}: settled {totals:?}, checked {boundaries} closed boundaries, failures {}", failures.len());
        }
    }
    println!("catalog settlement totals={totals:?}, closed boundaries={boundaries}");
    service.stop().unwrap();
    assert!(
        failures.is_empty(),
        "{} catalog entry failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
// //// /逐关执行开战与结算并验证关闭边界和奖励重放 ////
