// audience: internal
// # personal-service-quest-window-tests
// 日常覆盖保留后续难度日期, 管理改期同步影响目录和入场.

use super::*;
use tempfile::TempDir;

// //// 按关卡编号读取嵌套主表行 [@x380kkm 2026-09-25] ////
fn quest_row(value: &OrderedValue, wanted: &str) -> Option<Vec<String>> {
    match value {
        OrderedValue::Row(row) => {
            (row.first().map(String::as_str) == Some(wanted)).then(|| row.clone())
        }
        OrderedValue::Map(entries) => entries
            .iter()
            .find_map(|(_, value)| quest_row(value, wanted)),
    }
}

// //// 崩坏域常驻投影保留解锁条件和后续关卡日程 [@x380kkm 2026-09-29] ////
#[test]
fn permanent_challenge_projects_only_initial_release_times() {
    let root = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(root.path()).unwrap();
    database
        .set_virtual_time(true, 1_575_259_260_000, 0.0001)
        .unwrap();
    let windows = active_windows(&database).unwrap();
    let permanent = &windows[PERMANENT_CHALLENGE_ACTIVITY];
    assert_eq!(
        default_quest_window(PERMANENT_CHALLENGE_ACTIVITY).unwrap(),
        Some((permanent.start_at_ms, permanent.end_at_ms))
    );
    assert_eq!(
        crate::activity_catalog::default_activity_window_status(
            root.path(),
            PERMANENT_CHALLENGE_ACTIVITY,
            1_575_259_260_000,
        ),
        Some(crate::database::ActivityWindowStatus::Open)
    );
    let open = open_quest_keys(&database).unwrap();
    for quest_id in 1001..=1019 {
        assert!(open.contains(&(13, quest_id)));
    }
    for name in ["challenge_dungeon_event", "challenge_dungeon_event_quest"] {
        let master = super::super::projection_manifest()
            .unwrap()
            .masters
            .iter()
            .find(|master| master.name == name)
            .unwrap();
        let mut value =
            super::super::decode_ordered_map(super::super::master_seed(&master.name).unwrap())
                .unwrap();
        let source = value.clone();
        super::super::project_master_value(&mut value, master, "1", permanent);
        project_quests(&mut value, master, &windows).unwrap();
        if name == "challenge_dungeon_event" {
            let OrderedValue::Map(source_rows) = &source else {
                panic!("challenge events have root keys")
            };
            let OrderedValue::Map(projected_rows) = &value else {
                panic!("projected challenge events have root keys")
            };
            for (key, row) in source_rows {
                let OrderedValue::Row(mut expected) = row.clone() else {
                    panic!("challenge events have scalar rows")
                };
                if key == "1" {
                    expected[13] = projected_time(TimeKind::Start, permanent);
                    expected[14] = projected_time(TimeKind::End, permanent);
                }
                let projected = &projected_rows.iter().find(|(id, _)| id == key).unwrap().1;
                assert_eq!(*projected, OrderedValue::Row(expected));
            }
            continue;
        }
        for quest_id in (1001..=1040).chain(2001..=2006) {
            let key = quest_id.to_string();
            let mut expected = quest_row(&source, &key).unwrap();
            if quest_id <= 1019 {
                expected[5] = projected_time(TimeKind::Start, permanent);
                expected[6] = projected_time(TimeKind::End, permanent);
            } else if super::super::parse_master_time(&expected[6]).is_none() {
                expected[6] = projected_time(TimeKind::End, permanent);
            }
            assert_eq!(
                quest_row(&value, &key).unwrap(),
                expected,
                "quest {quest_id}"
            );
        }
    }
    database
        .upsert_manual_activity(PERMANENT_CHALLENGE_ACTIVITY, false)
        .unwrap();
    assert!(!is_quest_open(&database, 13, 1001).unwrap());
    database
        .delete_activity_schedule(PERMANENT_CHALLENGE_ACTIVITY)
        .unwrap();
    assert!(is_quest_open(&database, 13, 1001).unwrap());
}

// //// 保留一组元素日常并约束独立开放难度 [@x380kkm 2026-09-24] ////
#[test]
fn daily_coverage_preserves_late_difficulties_and_distinct_rewards() {
    let temp = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(temp.path()).unwrap();
    database
        .set_virtual_time(true, 1_575_259_260_000, 0.0001)
        .unwrap();
    assert!(is_quest_open(&database, 6, 13001).unwrap());
    assert!(!is_quest_open(&database, 6, 2001).unwrap());
    assert!(!is_quest_open(&database, 6, 7001).unwrap());
    assert!(!is_quest_open(&database, 6, 13006).unwrap());
    assert!(!is_quest_open(&database, 14, 1006).unwrap());
    assert!(!is_quest_open(&database, 6, 19001).unwrap());
    database
        .set_virtual_time(true, 1_597_291_200_000, 0.0001)
        .unwrap();
    assert!(is_quest_open(&database, 6, 13006).unwrap());
    assert!(is_quest_open(&database, 14, 1006).unwrap());
    database
        .set_virtual_time(true, 1_653_969_600_000, 0.0001)
        .unwrap();
    assert!(is_quest_open(&database, 6, 19001).unwrap());
    assert!(is_quest_open(&database, 6, 13001).unwrap());
    let windows = active_windows(&database).unwrap();
    assert_eq!(catalog().unwrap().daily_groups.len(), 6);
    for group in &catalog().unwrap().daily_groups {
        assert_eq!(
            group
                .iter()
                .filter(|id| {
                    windows
                        .get(*id)
                        .is_some_and(|window| Window::projected(window).contains(1_653_969_600_000))
                })
                .count(),
            1
        );
    }
    database
        .upsert_manual_activity("daily-week:2", true)
        .unwrap();
    assert!(is_quest_open(&database, 6, 2001).unwrap());
    assert!(!is_quest_open(&database, 6, 13001).unwrap());
    let hidden = super::super::hidden_daily_ids(&database).unwrap();
    assert!(!hidden.contains("daily-week:2"));
    assert!(hidden.contains("daily-week:13"));
}

// //// 普通改期保留晚开间隔并延长同日结束的子关卡 [@x380kkm 2026-09-25] ////
#[test]
fn rescheduled_quests_keep_release_delay_and_parent_end() {
    let root = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(root.path()).unwrap();
    let start = 1_800_000_000_000;
    let day = 86_400_000;
    database
        .set_virtual_time(true, start + 20 * day, 0.0001)
        .unwrap();
    database
        .upsert_activity_schedule("advent:1", true, start, start + 30 * day)
        .unwrap();
    assert!(is_quest_open(&database, 7, 1004).unwrap());
    let windows = active_windows(&database).unwrap();
    let catalog = catalog().unwrap();
    assert_eq!(
        resolve_quest(catalog, "7:1004", &catalog.quests["7:1004"], &windows).end,
        start + 30 * day
    );

    database
        .upsert_activity_schedule("world-story:100400", true, start, start + 30 * day)
        .unwrap();
    database
        .set_virtual_time(true, start + 7 * day - 1000, 0.0001)
        .unwrap();
    assert!(!is_quest_open(&database, 19, 100400003).unwrap());
    assert!(!open_quest_keys(&database)
        .unwrap()
        .contains(&(19, 100400003)));
    database
        .set_virtual_time(true, start + 7 * day, 0.0001)
        .unwrap();
    assert!(is_quest_open(&database, 19, 100400003).unwrap());
    assert!(open_quest_keys(&database)
        .unwrap()
        .contains(&(19, 100400003)));
}

// //// 临时开放保留晚开关卡且手动开启遵循管理状态 [@x380kkm 2026-09-25] ////
#[test]
fn temporary_open_preserves_delays_and_manual_open_matches_database() {
    let root = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(root.path()).unwrap();
    database
        .set_virtual_time(true, 1_800_000_000_000, 1.0)
        .unwrap();
    database
        .create_activity_temporary_open_lease("world-story:100400")
        .unwrap();
    assert!(is_quest_open(&database, 19, 100400001).unwrap());
    assert!(!is_quest_open(&database, 19, 100400003).unwrap());
    database
        .upsert_manual_activity("hard-multi:1", true)
        .unwrap();
    assert!(is_quest_open(&database, 26, 1001).unwrap());
    database
        .upsert_manual_activity("hard-multi:1", false)
        .unwrap();
    assert!(!is_quest_open(&database, 26, 1001).unwrap());
}

// //// 连续请求按同一时钟锚点投影临时活动 [@x380kkm 2026-09-25] ////
#[test]
fn temporary_windows_keep_the_same_clock_anchor_between_requests() {
    let root = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(root.path()).unwrap();
    database
        .set_virtual_time(true, 1_800_000_000_000, 1000.0)
        .unwrap();
    let connection =
        rusqlite::Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let (virtual_anchor, real_anchor): (i64, i64) = connection
        .query_row(
            "SELECT virtual_anchor_ms, real_anchor_ms FROM virtual_time WHERE id=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let opened = real_anchor - 10_000;
    let expires = real_anchor + 86_400_000;
    connection.execute(
        "INSERT INTO activity_temporary_open_leases (activity_id,opened_at_ms,expires_at_ms) VALUES ('ranking:1',?1,?2)",
        rusqlite::params![opened, expires],
    ).unwrap();
    let expected_start = virtual_anchor - 10_000_000;
    let expected_end = virtual_anchor + 86_400_000_000;
    for _ in 0..128 {
        let windows = active_windows(&database).unwrap();
        let window = &windows["ranking:1"];
        assert_eq!(
            (window.start_at_ms, window.end_at_ms),
            (expected_start, expected_end)
        );
    }
    database
        .set_virtual_time(true, 1_700_000_000_000, 2.0)
        .unwrap();
    let new_anchor: i64 = connection
        .query_row(
            "SELECT real_anchor_ms FROM virtual_time WHERE id=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let changed = active_windows(&database).unwrap();
    let window = &changed["ranking:1"];
    assert_eq!(
        window.start_at_ms,
        (1_700_000_000_000 + (opened - new_anchor) * 2) / 1000 * 1000
    );
    assert_eq!(
        window.end_at_ms,
        (1_700_000_000_000 + (expires - new_anchor) * 2) / 1000 * 1000
    );
    assert_ne!(window.start_at_ms, expected_start);
    database.set_virtual_time(false, 0, 1.0).unwrap();
    let real_time = active_windows(&database).unwrap();
    assert_eq!(real_time["ranking:1"].start_at_ms, opened / 1000 * 1000);
    assert_eq!(real_time["ranking:1"].end_at_ms, expires / 1000 * 1000);
}

// //// 临时比赛到期后保持统计点并投影通行证真实领奖期 [@x380kkm 2026-09-25] ////
#[test]
fn expired_temporary_projection_preserves_aggregation_and_reward_tail() {
    let root = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(root.path()).unwrap();
    let now = 1_800_000_000_000;
    let day = 86_400_000;
    database.set_virtual_time(true, now, 1.0).unwrap();
    let wall = database.current_wall_time_millis().unwrap();
    let connection =
        rusqlite::Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    for (id, elapsed_days) in [("ranking:1", 4), ("pass-card:1", 11)] {
        connection.execute(
            "INSERT INTO activity_temporary_open_leases (activity_id, opened_at_ms, expires_at_ms) VALUES (?1, ?2, ?3)",
            rusqlite::params![id, wall - (elapsed_days + 1) * day, wall - elapsed_days * day],
        ).unwrap();
    }
    let windows = active_windows(&database).unwrap();
    let ranking = &windows["ranking:1"];
    assert!((ranking.end_at_ms - (now - 4 * day)).abs() < 2000);
    assert!(ranking.end_at_ms + super::super::AGGREGATION_DELAY_MS < now);
    assert!(ranking.end_at_ms + ranking.reward_delay_ms > now);
    let pass = &windows["pass-card:1"];
    let tail = crate::cn_pass_card::reward_period_millis("pass-card:1")
        .unwrap()
        .unwrap();
    assert!(tail >= 30 * day);
    assert_eq!(pass.reward_delay_ms, tail);
    assert!(pass.end_at_ms < now && pass.end_at_ms + tail > now);
    let master = super::super::projection_manifest()
        .unwrap()
        .masters
        .iter()
        .find(|master| master.name == "pass_card_event")
        .unwrap();
    let mut value =
        super::super::decode_ordered_map(super::super::master_seed(&master.name).unwrap()).unwrap();
    super::super::project_master_value(&mut value, master, "1", pass);
    if let OrderedValue::Map(entries) = value {
        let (_, OrderedValue::Row(row)) = entries.iter().find(|(key, _)| key == "1").unwrap()
        else {
            panic!("pass card row is missing")
        };
        assert_eq!(row[10], projected_time(TimeKind::Reward, pass));
    } else {
        panic!("pass card map is missing")
    }
}

// //// 管理改期保留关卡开放间隔并允许单关解锁 [@x380kkm 2026-09-24] ////
#[test]
fn managed_quest_windows_match_projected_times() {
    let temp = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(temp.path()).unwrap();
    let now = 1_750_000_000_000;
    database.set_virtual_time(true, now, 0.0001).unwrap();
    database
        .upsert_activity_schedule("daily-week:13", true, now - 1_000, now + 86_400_000)
        .unwrap();
    assert!(is_quest_open(&database, 6, 13001).unwrap());
    assert!(!is_quest_open(&database, 6, 13006).unwrap());
    database
        .upsert_activity_schedule("quest:6:13006", true, now - 1_000, now + 3_600_000)
        .unwrap();
    assert!(is_quest_open(&database, 6, 13006).unwrap());
    let master = super::super::projection_manifest()
        .unwrap()
        .masters
        .iter()
        .find(|master| master.name == "daily_week_event_quest")
        .unwrap();
    let mut value =
        super::super::decode_ordered_map(super::super::master_seed(&master.name).unwrap()).unwrap();
    project_quests(&mut value, master, &active_windows(&database).unwrap()).unwrap();
    let projected = quest_row(&value, "13006").unwrap();
    let expected = projection_window(now - 1_000, now + 3_600_000, i64::MAX);
    assert_eq!(projected[4], projected_time(TimeKind::Start, &expected));
    assert_eq!(projected[5], projected_time(TimeKind::End, &expected));
    database
        .upsert_activity_schedule("quest:6:13006", false, now - 1_000, now + 3_600_000)
        .unwrap();
    assert!(!is_quest_open(&database, 6, 13006).unwrap());
    let release = catalog().unwrap().quests["2:1002003"].start_at_ms.unwrap();
    database
        .set_virtual_time(true, release - 1_000, 0.0001)
        .unwrap();
    assert!(!is_quest_open(&database, 2, 1002003).unwrap());
    database.set_virtual_time(true, release, 0.0001).unwrap();
    assert!(is_quest_open(&database, 2, 1002003).unwrap());
}

// //// 修改高难关卡时间时保留战斗配置列 [@x380kkm 2026-09-25] ////
#[test]
fn projects_hard_multi_period_without_changing_battle_configuration() {
    let root = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(root.path()).unwrap();
    database
        .upsert_manual_activity("hard-multi:1", true)
        .unwrap();
    let master = super::super::projection_manifest()
        .unwrap()
        .masters
        .iter()
        .find(|master| master.name == "hard_multi_event_quest")
        .unwrap();
    let mut value =
        super::super::decode_ordered_map(super::super::master_seed(&master.name).unwrap()).unwrap();
    let source = quest_row(&value, "1001").unwrap();
    project_quests(&mut value, master, &active_windows(&database).unwrap()).unwrap();
    let projected = quest_row(&value, "1001").unwrap();
    let expected = projection_window(
        super::super::OPEN_RULE_START_MS,
        super::super::OPEN_RULE_END_MS,
        i64::MAX,
    );
    assert_eq!(projected[5], projected_time(TimeKind::Start, &expected));
    assert_eq!(projected[6], projected_time(TimeKind::End, &expected));
    assert_eq!(projected[..5], source[..5]);
    assert_eq!(projected[7..], source[7..]);
}
