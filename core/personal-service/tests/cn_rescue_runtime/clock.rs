// audience: internal
// # personal-service-cn-rescue-clock-tests
// 隔离实例按游戏日期执行铃铛接入, BOT 开战, 续战和关卡关闭后的结算.

use super::*;

fn set_time(service: &PersonalService, iso: &str) {
    management(
        service,
        "/v1/time",
        json!({"enabled":true, "iso":iso, "rate":1.0}),
    );
}

fn set_timestamp(service: &PersonalService, seconds: i64) {
    management(
        service,
        "/v1/time",
        json!({"enabled":true, "unix_time_ms":seconds * 1000, "rate":1.0}),
    );
}

fn close_quest(service: &PersonalService, room: &Value) {
    let token = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        "POST",
        &format!(
            "/v1/activities/quest%3A{}%3A{}/close",
            room["category_id"], room["quest_id"]
        ),
        "application/json",
        &[("Authorization", &token)],
        b"{}",
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

// //// 同一铃铛关卡跨日期完成入房和续战并结算已关闭场次 [@x380kkm 2026-09-27] ////
#[test]
fn rescue_completes_before_and_after_2024_with_the_same_quest() {
    for iso in [
        "2023-12-31T12:00:00.000Z",
        "2024-07-12T12:00:00.000Z",
        "2026-09-27T12:00:00.000Z",
    ] {
        let root = TempDir::new().unwrap();
        let (service, viewer) = start_player(root.path());
        set_time(&service, iso);
        let db = database(root.path());
        unlock_attention(&db);
        save_equipped_ai_teams(&service, &db, viewer);
        management(
            &service,
            "/v1/rescue-settings",
            json!({
                "enabled":true, "spawn_interval_seconds":60, "room_lifetime_seconds":180,
                "max_rooms":1, "quests":[{"category_id":2, "quest_id":1028004}]
            }),
        );
        let offered = data(
            &service,
            "attention/check",
            json!({
                "viewer_id":viewer, "request_number":3, "holding_number":0
            }),
        );
        let offer = &offered["multi"][0];
        assert!(offer["attention_key"].is_string(), "{iso}: {offered}");
        let room = data(
            &service,
            "multi_battle_quest/select_room",
            json!({
                "viewer_id":viewer, "category":2, "quest_id":1028004, "party_id":1,
                "accepted_type":2, "room_number":offer["quest_info"]["room_number"]
            }),
        );
        assert_eq!(room["raising_state"], 1, "{iso}: {room}");
        set_time(&service, "2025-07-12T04:00:00.000Z");
        let (lobby, connection) = enter_lobby(&service, viewer, &room);
        let started = start(&service, viewer, &room, &offer["attention_key"], "date-one");
        assert!(started["start_time"].is_number(), "{iso}: {started}");
        let mut battle = Socket::connect(room["port"].as_u64().unwrap() as u16);
        battle.send(
            json!({"socklet":"cooperation_battle", "roomNumber":room["room_number"],
            "connectionId":connection, "reconnected":0}),
        );
        assert_eq!(battle.receive()[0], 0, "{iso}");
        battle.send(json!([0, [0]]));
        assert_eq!(battle.receive(), json!([1, [1]]), "{iso}");
        drop(lobby);
        let result = finish_wind_dragon(&service, viewer, "date-one");
        assert_eq!(result["host_finished"], false, "{iso}: {result}");
        assert_eq!(
            finish_wind_dragon(&service, viewer, "date-one"),
            result,
            "{iso}"
        );
        assert_eq!(rooms(&db), 1, "{iso}");
        let resumed = data(
            &service,
            "multi_battle_quest/select_room",
            json!({
                "viewer_id":viewer, "category":2, "quest_id":1028004, "party_id":1,
                "accepted_type":6, "room_number":room["room_number"]
            }),
        );
        assert_eq!(resumed["raising_state"], 1, "{iso}: {resumed}");
        set_time(&service, iso);
        let (second_lobby, _) = enter_lobby(&service, viewer, &resumed);
        start(
            &service,
            viewer,
            &resumed,
            &offer["attention_key"],
            "date-two",
        );
        close_quest(&service, &room);
        let unavailable = data(&service, "attention/check", json!({"viewer_id":viewer}));
        assert!(unavailable["multi"].as_array().unwrap().is_empty(), "{iso}");
        assert_eq!(rooms(&db), 1, "{iso}: active room survives closure");
        let result = finish_wind_dragon(&service, viewer, "date-two");
        assert_eq!(
            finish_wind_dragon(&service, viewer, "date-two"),
            result,
            "{iso}"
        );
        wait_until(|| rooms(&db) == 0);
        drop((second_lobby, battle));
        service.stop().unwrap();
        eprintln!("rescue date {iso}: join, ready, start, finish, replay, continue, closure");
    }
}
// //// /同一铃铛关卡跨日期完成入房和续战并结算已关闭场次 ////

fn finish_wind_dragon(service: &PersonalService, viewer: i64, play: &str) -> Value {
    data(
        service,
        "multi_battle_quest/finish",
        json!({"viewer_id":viewer,"category":2,"quest_id":1028004,"play_id":play,
        "is_accomplished":true,"elapsed_time_ms":10000,"score":4_294_967_296_f64,"add_mana":0,
        "statistics":rescue_statistics(1)}),
    )
}

// //// 各铃铛类别和难度经过大厅开战并跨活动结束结算 [@x380kkm 2026-09-27] ////
#[test]
fn rescue_activity_difficulties_keep_started_battles_when_time_expires() {
    let rows: Vec<Value> =
        serde_json::from_str(include_str!("../../assets/cn-rescue-quests.json")).unwrap();
    let mut cases = std::collections::BTreeMap::new();
    for row in rows {
        cases.insert(
            (
                row["category_id"].as_i64().unwrap(),
                row["difficulty_id"].as_i64().unwrap(),
            ),
            row,
        );
    }
    assert_eq!(cases.len(), 15);
    for ((category, difficulty), row) in cases {
        let time = row["start_time"]
            .as_i64()
            .unwrap()
            .max(row["activity_start_time"].as_i64().unwrap_or_default())
            + 60;
        exercise_rescue(&row, time, true);
        eprintln!("rescue category {category} difficulty {difficulty} quest {}: socket start and closed-period settlement", row["quest_id"]);
    }
}
// //// /各铃铛类别和难度经过大厅开战并跨活动结束结算 ////

// //// 风废龙全部难度跨日期通过铃铛入场并完成结算 [@x380kkm 2026-09-27] ////
#[test]
fn wind_dragon_rescue_completes_all_difficulties_in_july_2025() {
    let rows: Vec<Value> =
        serde_json::from_str(include_str!("../../assets/cn-rescue-quests.json")).unwrap();
    for row in rows.into_iter().filter(|row| {
        row["category_id"] == 2 && (1028001..=1028004).contains(&row["quest_id"].as_i64().unwrap())
    }) {
        for time in [1_704_024_000, 1_752_292_800] {
            exercise_rescue(&row, time, false);
            eprintln!("wind dragon {} at {time}: attention, select, prepare, socket ready, start, finish and replay", row["quest_id"]);
        }
    }
}
// //// /风废龙全部难度跨日期通过铃铛入场并完成结算 ////

fn exercise_rescue(row: &Value, time: i64, expire: bool) {
    let category = row["category_id"].as_i64().unwrap();
    let quest = row["quest_id"].as_i64().unwrap();
    let root = TempDir::new().unwrap();
    let (service, viewer) = start_player(root.path());
    let db = database(root.path());
    unlock_attention(&db);
    set_timestamp(&service, time);
    save_equipped_ai_teams(&service, &db, viewer);
    management(
        &service,
        "/v1/rescue-settings",
        json!({
            "enabled":true, "spawn_interval_seconds":60, "room_lifetime_seconds":180,
            "max_rooms":1, "quests":[{"category_id":category, "quest_id":quest}]
        }),
    );
    let offered = data(&service, "attention/check", json!({"viewer_id":viewer}));
    let offer = &offered["multi"][0];
    assert!(
        offer["attention_key"].is_string(),
        "{category}:{quest}: {offered}"
    );
    let room = data(
        &service,
        "multi_battle_quest/select_room",
        json!({
            "viewer_id":viewer, "category":category, "quest_id":quest, "party_id":1,
            "accepted_type":2, "room_number":offer["quest_info"]["room_number"]
        }),
    );
    assert_eq!(room["raising_state"], 1, "{category}:{quest}: {room}");
    let (lobby, connection) = enter_lobby(&service, viewer, &room);
    let started = start(
        &service,
        viewer,
        &room,
        &offer["attention_key"],
        "event-rescue",
    );
    assert_eq!(started["category_id"], category);
    let mut battle = Socket::connect(room["port"].as_u64().unwrap() as u16);
    battle.send(
        json!({"socklet":"cooperation_battle", "roomNumber":room["room_number"],
            "connectionId":connection, "reconnected":0}),
    );
    assert_eq!(battle.receive()[0], 0);
    battle.send(json!([0, [0]]));
    assert_eq!(battle.receive(), json!([1, [1]]));
    drop(lobby);
    if let Some(end) = row["end_time"]
        .as_i64()
        .or(row["activity_end_time"].as_i64())
        .filter(|_| expire)
    {
        set_timestamp(&service, end + 60);
    }
    let rules: Value =
        serde_json::from_str(include_str!("../../assets/cn-battle-reward-rules.json")).unwrap();
    let phase = rules["phase_quests"][format!("{category}:{quest}")]["phases"]
        .as_array()
        .and_then(|phases| phases.last())
        .and_then(|phase| phase["phase"].as_i64())
        .unwrap_or(1);
    let body = json!({
        "viewer_id":viewer, "category":category, "quest_id":quest, "play_id":"event-rescue",
        "is_accomplished":true, "elapsed_time_ms":30_000, "score":4_294_967_296_f64,
        "contribution_score":4_294_967_296_f64, "add_mana":0,
        "mate_player_result":[{"viewer_id":900000001, "com_id":1, "score":4_294_967_296_f64, "contribution_score":4_294_967_296_f64}],
        "statistics":rescue_statistics(phase)
    });
    let result = data(&service, "multi_battle_quest/finish", body.clone());
    assert_eq!(data(&service, "multi_battle_quest/finish", body), result);
    assert!(
        result["item_list"].is_object(),
        "{category}:{quest}: {result}"
    );
    drop(battle);
    service.stop().unwrap();
}

fn rescue_statistics(phase: i64) -> Value {
    json!({"clear_phase":phase, "is_mvp":true, "max_combo_count":180, "max_skill_chain_count":9,
        "damage":4_294_967_296_f64,
        "zones":[{"use_power_flip_count":18, "use_dash_count":36, "use_skill_count":12,
            "max_combo_count":180, "max_skill_chain_count":9, "damage":4_294_967_296_f64}],
        "party":{"characters":[{"id":1},null,null], "unison_characters":[null,null,null],
            "equipments":[{"id":5010034},null,null], "ability_soul_ids":[null,null,null]}})
}

fn save_equipped_ai_teams(service: &PersonalService, db: &Connection, viewer: i64) {
    let stored: String = db
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    let mut player: Value = serde_json::from_str(&stored).unwrap();
    player["user_character_list"]["1"]["over_limit_step"] = json!(6);
    player["user_character_list"]["1"]["mana_board_index"] = json!(2);
    player["user_character_list"]["1"]["ex_boost"] = Value::Null;
    player["item_list"]["10003"] = json!(2);
    player["user_equipment_list"]["5010034"] =
        json!({"equipment_id":5010034, "level":3, "protection":false});
    player["user_character_mana_node_list"]["1"] = json!([
        {"mana_node_multiplied_id":2201, "awake_level":1},
        {"mana_node_multiplied_id":2202, "awake_level":1},
        {"mana_node_multiplied_id":2401, "awake_level":0}
    ]);
    for party in ["1", "2"] {
        player["user_party_group_list"]["1"]["list"][party]["equipment_ids"] =
            json!([5010034, null, null]);
    }
    db.execute(
        "UPDATE player_snapshots SET data_json=?1",
        [player.to_string()],
    )
    .unwrap();
    let draw = data(
        service,
        "ex_boost/first_draw",
        json!({"viewer_id":viewer, "character_id":1, "cost_item_id":10003, "api_count":1}),
    );
    assert!(draw["character_list"][0]["ex_boost"]["ability_id_list"].is_array());
    let slot: i64 = db
        .query_row("SELECT slot_id FROM local_save_access LIMIT 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    management(
        service,
        &format!("/v1/local-saves/{slot}/ai-teams"),
        json!({"party_ids":[1,2]}),
    );
}
