// audience: internal
// # personal-service-single-battle-presentation-tests
// 活动结算角色列表和旧回执展示使用客户端有效角色, 重放保留持久化奖励与成绩.

use super::*;

// //// 结算空队伍并修复旧活动回执的角色展示 [@x380kkm 2026-09-28] ////
#[test]
fn settles_and_replays_event_results_with_valid_display_characters() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    set_virtual_time(&service, "2030-01-01T12:00:00.000Z");
    for activity in ["solo-time-attack:1", "score-attack:1"] {
        set_activity_always(&service, activity);
    }
    let viewer = decode_response::<SignupData>(&send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 928251 }),
    ))
    .data_headers
    .viewer_id;
    let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    database.execute(
        "UPDATE player_snapshots SET data_json=json_set(data_json, '$.user_info.stamina', 5000)",
        [],
    ).unwrap();

    for (category, field, characters, cached_characters) in [
        (
            25,
            "solo_time_attack_event",
            json!([null, null, null]),
            json!({"0":999999999}),
        ),
        (
            27,
            "score_attack_event",
            json!([null, {"id":999999999}, null]),
            json!({}),
        ),
    ] {
        let play_id = format!("event-presentation-{category}");
        let started = start_battle_for(service.port(), viewer, category, 1001, &play_id, false);
        assert!(started.starts_with("HTTP/1.1 200 OK"), "{started}");
        let body = json!({
            "viewer_id":viewer, "category":category, "quest_id":1001, "play_id":play_id,
            "elapsed_time_ms":1000, "score":1_000_000_000, "add_mana":0,
            "is_accomplished":true,
            "statistics":{"party":{
                "characters":characters, "unison_characters":[null,null,null],
                "equipments":[null,null,null], "ability_soul_ids":[null,null,null],
            },"zones":[]},
        });
        let finished = send_battle_request(service.port(), "finish", &body);
        assert!(finished.starts_with("HTTP/1.1 200 OK"), "{finished}");
        let response = decode_response::<Value>(&finished).data;
        assert_eq!(response[field]["main_character_ids"], json!({"0":1}));
        assert!(!response[field]["reward_ids"].as_array().unwrap().is_empty());
        let stored: String = database
            .query_row("SELECT data_json FROM player_snapshots", [], |row| {
                row.get(0)
            })
            .unwrap();
        let player: Value = serde_json::from_str(&stored).unwrap();
        assert!(player["user_character_list"]["999999999"].is_null());
        let mut cached = response.clone();
        cached[field]["main_character_ids"] = cached_characters;
        let receipt = cached.to_string();
        database
            .execute(
                "UPDATE single_battle_finish_receipts SET response_json=?1",
                [&receipt],
            )
            .unwrap();
        for _ in 0..2 {
            let replay = send_battle_request(service.port(), "finish", &body);
            assert!(replay.starts_with("HTTP/1.1 200 OK"), "{replay}");
            assert_eq!(decode_response::<Value>(&replay).data, response);
        }
        let replayed: String = database
            .query_row("SELECT data_json FROM player_snapshots", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(replayed, stored);
        let persisted: String = database
            .query_row(
                "SELECT response_json FROM single_battle_finish_receipts",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(persisted, receipt);
    }
    service.stop().unwrap();
}
