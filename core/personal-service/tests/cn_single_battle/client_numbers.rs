// audience: internal
// # cn-battle-client-numbers
// 客户端 FinalScore 按 Number 计算整数分数, MessagePack 的浮点编码参与战斗结算和重放.

use super::*;

// //// 组装客户端 Number 分数和整数统计字段 [@x380kkm 2026-09-27] ////
fn finish_body(viewer_id: i64, category: i64, quest_id: i64, score: f64) -> Value {
    json!({
        "viewer_id": viewer_id,
        "play_id": "client-number-battle",
        "quest_id": quest_id,
        "category": category,
        "elapsed_time_ms": 100_000,
        "score": score,
        "add_mana": 7,
        "is_accomplished": true,
        "statistics": {
            "clear_phase": 1,
            "score": score,
            "battle_score": score,
            "party": {
                "characters": [{"id": 1}, null, null],
                "unison_characters": [null, null, null],
            },
            "zones": [{
                "use_power_flip_count": 2,
                "use_dash_count": 3,
                "use_skill_count": 4,
                "damage": score,
            }],
        },
    })
}
// //// /组装客户端 Number 分数和整数统计字段 ////

// //// 将客户端高分编码结算到奥格和机兵关卡并重放原有奖励 [@x380kkm 2026-09-27] ////
#[test]
fn settles_client_float_scores_without_losing_progress_or_regranting_rewards() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    set_virtual_time(&service, "2030-01-01T12:00:00.000Z");
    let viewer = decode_response::<SignupData>(&send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 8014001 }),
    ))
    .data_headers
    .viewer_id;
    let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    database.execute(
        "UPDATE player_snapshots SET data_json=json_set(data_json, '$.user_info.stamina', 1000)",
        [],
    ).unwrap();
    for (category, quest_id, score) in [
        (1, 8_014_001, 2_147_483_648.0),
        (2, 1_061_001, 1000.0),
        (2, 1_061_004, 4_294_967_296.0),
    ] {
        let started = start_battle_for(
            service.port(),
            viewer,
            category,
            quest_id,
            "client-number-battle",
            false,
        );
        assert!(
            started.starts_with("HTTP/1.1 200 OK"),
            "{quest_id}: {started}"
        );
        let body = finish_body(viewer, category, quest_id, score);
        for invalid_score in [1.5, 9_007_199_254_740_992.0] {
            let invalid = finish_body(viewer, category, quest_id, invalid_score);
            let rejected = send_battle_request(service.port(), "finish", &invalid);
            assert!(rejected.starts_with("HTTP/1.1 400 Bad Request"));
        }
        let finished = send_battle_request(service.port(), "finish", &body);
        assert!(
            finished.starts_with("HTTP/1.1 200 OK"),
            "{quest_id}: {finished}"
        );
        let result = decode_response::<Value>(&finished);
        assert_eq!(result.data_headers.result_code, 1);
        assert_eq!(result.data["clear_rank"], 5);
        let stored: String = database
            .query_row("SELECT data_json FROM player_snapshots", [], |row| {
                row.get(0)
            })
            .unwrap();
        let snapshot: Value = serde_json::from_str(&stored).unwrap();
        let progress = snapshot["quest_progress"][category.to_string()]
            .as_array()
            .unwrap();
        let progress = progress
            .iter()
            .find(|row| row["quest_id"] == quest_id)
            .unwrap();
        assert_eq!(progress["high_score"], score as i64);
        let replay =
            decode_response::<Value>(&send_battle_request(service.port(), "finish", &body));
        assert_eq!(replay.data, result.data);
        let replayed: String = database
            .query_row("SELECT data_json FROM player_snapshots", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(replayed, stored);
    }
    service.stop().unwrap();
}
// //// /将客户端高分编码结算到奥格和机兵关卡并重放原有奖励 ////
