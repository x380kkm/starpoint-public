// audience: internal
// # personal-service-rush-reward-period-tests
// 临时比赛通过真实接口按比赛, 统计和领奖时窗处理排名凭据.

use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

const EVENT: i64 = 700_001;
const DAY_MS: i64 = 86_400_000;

// //// 按管理接口设置临时活动与时钟 [@x380kkm 2026-09-26] ////
fn manage(service: &PersonalService, method: &str, path: &str, body: Value) {
    let authorization = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        method,
        path,
        "application/json",
        &[("Authorization", &authorization)],
        &serde_json::to_vec(&body).unwrap(),
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

// //// 从隔离存档读取玩家资产和活动凭据 [@x380kkm 2026-09-26] ////
fn snapshot(root: &Path) -> Value {
    let database = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    let data: String = database
        .query_row(
            "SELECT data_json FROM player_snapshots LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str(&data).unwrap()
}

// //// 按虚拟已过时长设置隔离租约的截止点 [@x380kkm 2026-09-26] ////
fn set_elapsed_reward_time(root: &Path, elapsed_ms: i64, rate: i64) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let end = now - elapsed_ms / rate;
    let database = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    assert_eq!(database.execute(
        "UPDATE activity_temporary_open_leases SET opened_at_ms=?1, expires_at_ms=?2 WHERE activity_id=?3",
        params![end - 3_600_000, end, format!("rush:{EVENT}")],
    ).unwrap(), 1);
}

// //// 临时比赛统计完成后发奖并保留重启领取凭据 [@x380kkm 2026-09-26] ////
#[test]
fn temporary_rush_claims_follow_competition_and_aggregation_windows() {
    for rate in [1, 1000] {
        let root = TempDir::new().unwrap();
        let service = PersonalService::start(root.path(), 0).unwrap();
        let viewer = signup(&service, 696);
        manage(
            &service,
            "PUT",
            "/v1/time",
            json!({
                "enabled":true, "iso":"2025-01-02T12:00:00.000Z", "rate":rate,
            }),
        );
        manage(
            &service,
            "PUT",
            &format!("/v1/activities/calendar/rush:{EVENT}"),
            json!({
                "enabled":true, "start_at":"2024-01-01T00:00:00.000Z",
                "end_at":"2024-01-02T00:00:00.000Z",
            }),
        );
        manage(
            &service,
            "POST",
            &format!("/v1/activities/rush:{EVENT}/temporary-open"),
            json!({}),
        );
        let body = json!({"viewer_id":viewer, "event_id":EVENT});
        decode_response::<Value>(&send(
            &service,
            "/api/index.php/event/rush/summary",
            body.clone(),
        ));
        seed_rush_ranking_state(root.path(), EVENT);
        let before = snapshot(root.path());
        let path = "/api/index.php/event/rush/reward";
        let during_competition = send(&service, path, body.clone());
        assert!(
            during_competition.contains("activity_reward_not_ready"),
            "{during_competition}"
        );
        assert_eq!(snapshot(root.path()), before);
        set_elapsed_reward_time(root.path(), DAY_MS, rate);
        let during_aggregation = send(&service, path, body.clone());
        assert!(
            during_aggregation.contains("activity_reward_not_ready"),
            "{during_aggregation}"
        );
        assert_eq!(snapshot(root.path()), before);
        set_elapsed_reward_time(root.path(), 4 * DAY_MS, rate);
        let claimed = decode_response::<Value>(&send(&service, path, body.clone())).data;
        assert_eq!(claimed["ranking_reward"]["status"], 1);
        assert_eq!(claimed["rank_number"], 2);
        assert_eq!(claimed["degree_list"][0]["degree_id"], 64_000);
        let granted = snapshot(root.path());
        assert_ne!(granted, before);
        service.stop().unwrap();
        let service = PersonalService::start(root.path(), 0).unwrap();
        let repeated = decode_response::<Value>(&send(&service, path, body.clone())).data;
        assert_eq!(repeated["ranking_reward"]["status"], 2);
        assert_eq!(
            repeated["ranking_reward"]["reward_list"],
            claimed["ranking_reward"]["reward_list"]
        );
        assert_eq!(snapshot(root.path()), granted);
        set_elapsed_reward_time(root.path(), 11 * DAY_MS, rate);
        let expired = send(&service, path, body.clone());
        assert!(expired.contains("activity_reward_expired"), "{expired}");
        assert_eq!(snapshot(root.path()), granted);
        service.stop().unwrap();
    }
}

// //// 正常无限挑战成绩在领奖期可查询并领取榜首奖励 [@x380kkm 2026-09-28] ////
#[test]
fn completed_rush_battle_claims_first_rank_after_competition() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 698);
    let event = 700_006;
    open_activity(&service, "rush", event);
    let request = json!({"viewer_id": viewer, "event_id": event});
    decode_response::<Value>(&send(&service, "/api/index.php/event/rush/summary", request.clone()));
    let started = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/rush/battle/start",
        json!({"viewer_id":viewer,"event_id":event,"quest_id":700006008,
            "party_id":1,"play_id":"rush-first-rank","is_auto_start_mode":false}),
    ));
    assert_eq!(started.data_headers.result_code, 1);
    let finish_body = json!({"viewer_id":viewer,"category":24,"quest_id":700006008,
        "play_id":"rush-first-rank","elapsed_time_ms":20000,"score":9000000,
        "add_mana":0,"is_accomplished":true,"statistics":{"zones":[],
        "party":{"characters":[{"id":111001}],"unison_characters":[],"equipments":[],"ability_soul_ids":[]}}});
    let finish = decode_response::<Value>(&send(&service, "/api/index.php/single_battle_quest/finish", finish_body.clone()));
    assert_eq!(finish.data["rush_event"]["endless_battle_max_round"], 1);
    assert_eq!(decode_response::<Value>(&send(&service, "/api/index.php/single_battle_quest/finish", finish_body)).data, finish.data);

    manage(&service, "PUT", &format!("/v1/activities/calendar/rush:{event}"), json!({
        "enabled":true,"start_at":"2026-09-01T00:00:00Z","end_at":"2026-09-02T00:00:00Z",
    }));
    manage(&service, "PUT", "/v1/time", json!({"enabled":true,"iso":"2026-09-06T00:00:00Z","rate":1}));
    for path in ["summary", "ranking", "aggregated_time", "ranking/played_party"] {
        let mut body = request.clone();
        body["page"] = json!(0);
        body["rank_number"] = json!(1);
        let result = decode_response::<Value>(&send(&service, &format!("/api/index.php/event/rush/{path}"), body));
        assert_eq!(result.data_headers.result_code, 1, "{path}");
        if path == "ranking" {
            assert_eq!(result.data["my_data"]["rank_number"], 1);
        }
    }
    let claimed = decode_response::<Value>(&send(&service, "/api/index.php/event/rush/reward", request.clone()));
    assert_eq!(claimed.data["rank_number"], 1);
    assert_eq!(claimed.data["ranking_reward"]["status"], 1);
    assert_eq!(claimed.data["degree_list"][0]["degree_id"], 64015);
    let saved = snapshot(root.path());
    let replay = decode_response::<Value>(&send(&service, "/api/index.php/event/rush/reward", request));
    assert_eq!(replay.data["ranking_reward"]["status"], 2);
    assert_eq!(snapshot(root.path()), saved);
    let closed = decode_response::<Value>(&send(&service, "/api/index.php/event/rush/battle/start", json!({
        "viewer_id":viewer,"event_id":event,"quest_id":700006008,
        "party_id":1,"play_id":"rush-after-end","is_auto_start_mode":false,
    })));
    assert_eq!(closed.data_headers.result_code, 4050);
    service.stop().unwrap();
}
