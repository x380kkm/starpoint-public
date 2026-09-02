// audience: internal
// # personal-service-cn-raid-rush-contract-tests
//
// 该文件验证 raid 和 rush 活动路由的客户端响应结构与持久化状态转换.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

fn signup(service: &PersonalService, device_id: i64) -> i64 {
    decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id }),
    ))
    .data_headers
    .viewer_id
}

fn send(service: &PersonalService, path: &str, body: Value) -> String {
    cn_support::send_request(service.port(), path, &encode_request(&body))
}

fn seed_rush_ranking_state(root: &Path, event_id: i64) {
    let database = Connection::open(root.join("personal-service.sqlite3"))
        .expect("service database is opened");
    let (account_id, serialized) = database
        .query_row(
            "SELECT account_id, data_json FROM player_snapshots LIMIT 1",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .expect("player snapshot is read");
    let mut player: Value = serde_json::from_str(&serialized).expect("player snapshot is decoded");
    let event = &mut player["cn_activity_state"]["event_families"]["rush"][event_id.to_string()];
    event["endless_battle_max_round"] = Value::from(2);
    event["endless_battle_max_round_time"] = Value::from(12_345);
    event["endless_battle_next_round"] = Value::from(3);
    event["endless_battle_max_round_character_ids"] = json!([111_001, null, null]);
    event["endless_battle_max_round_character_evolution_img_lvls"] = json!([2, null, null]);
    event["rank_number"] = Value::from(2);
    event["endless_played_party_list"] = json!({
        "1": {"party_id": 1, "character_ids": [111_001, null, null]},
        "2": {"party_id": 2, "character_ids": [111_001, null, null]},
    });
    database
        .execute(
            "UPDATE player_snapshots SET data_json = ?1 WHERE account_id = ?2",
            params![
                serde_json::to_string(&player).expect("player snapshot is encoded"),
                account_id
            ],
        )
        .expect("player snapshot is updated");
}

// //// 验证 raid 路由响应结构和 folder 状态转换 [@x380kkm 2026-08-22] ////
#[test]
fn returns_raid_contract_and_persists_folder_battle_state() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(&service, 68);
    let event_id = 9_004;

    let party = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/raid/party",
        json!({"viewer_id": viewer_id}),
    ));
    let groups = party.data["user_party_group_list"]
        .as_array()
        .expect("raid party groups are an array");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0]["party_group_color_id"], 15);
    assert_eq!(groups[0]["party_group_id"], 1);
    assert_eq!(groups[0]["party_list"].as_array().map(Vec::len), Some(3));

    let ranking = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/raid/ranking",
        json!({"viewer_id": viewer_id}),
    ));
    assert_eq!(ranking.data["aggregated_time"], "");
    assert!(ranking.data["quest_list"].is_object());
    let ranking_party = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/raid/ranking/party",
        json!({"viewer_id": viewer_id, "rank_number": 1}),
    ));
    assert!(ranking_party.data["raid_ranking_party"].is_array());

    let summary = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/raid/summary",
        json!({"event_id": event_id, "viewer_id": viewer_id}),
    ));
    assert!(summary.data["aggregated_time"].is_string());
    assert_eq!(summary.data["auto_start_point"], 0);
    assert!(summary.data["kill_count_reward_data"].is_object());
    assert!(summary.data["quest_list"].is_object());
    assert!(summary.data["raid_boss"].is_object());
    assert_eq!(summary.data["endless_battle_next_round"], 1);
    assert!(summary.data["active_rush_battle_folder_id"].is_null());
    assert_eq!(summary.data["endless_battle_played_max_round"], 1);
    assert!(summary.data["cleared_folder_id_list"].is_array());
    assert!(summary.data["endless_battle_played_party_list"].is_object());
    assert!(summary.data["rush_battle_played_party_list"].is_object());
    assert!(summary.data["endless_battle_my_ranking"].is_null());

    let boss = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/raid/get_boss",
        json!({"event_id": event_id, "viewer_id": viewer_id}),
    ));
    assert!(boss.data["raid_boss"]["hp_percentage"].is_number());
    assert!(boss.data["raid_boss"]["total_kill_count"].is_number());
    let reward = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/raid/ranking_reward",
        json!({"event_id": event_id, "viewer_id": viewer_id}),
    ));
    assert!(reward.data["reward_list"].is_array());
    assert_eq!(reward.data["status"], 0);

    let selected = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/raid/select_folder",
        json!({"event_id": event_id, "folder_id": 3, "viewer_id": viewer_id}),
    ));
    assert!(selected
        .data
        .as_object()
        .is_some_and(serde_json::Map::is_empty));
    let battle = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/raid/battle/start",
        json!({
            "event_id": event_id,
            "is_auto_start_mode": false,
            "party_group_id": 1,
            "play_id": "raid-contract",
            "quest_id": 9_004_001,
            "viewer_id": viewer_id,
        }),
    ));
    assert!(battle
        .data
        .as_object()
        .is_some_and(serde_json::Map::is_empty));
    service.stop().expect("service stops cleanly");
    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let persisted_summary = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/raid/summary",
        json!({"event_id": event_id, "viewer_id": viewer_id}),
    ));
    assert_eq!(persisted_summary.data["active_rush_battle_folder_id"], 3);
    let reset = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/raid/reset",
        json!({"event_id": event_id, "quest_type": 1, "viewer_id": viewer_id}),
    ));
    assert!(reset
        .data
        .as_object()
        .is_some_and(serde_json::Map::is_empty));
    let reset_summary = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/raid/summary",
        json!({"event_id": event_id, "viewer_id": viewer_id}),
    ));
    assert!(reset_summary.data["active_rush_battle_folder_id"].is_null());
    service.stop().expect("service stops cleanly");
}
// //// /验证 raid 路由响应结构和 folder 状态转换 ////

// //// 验证 rush 路由响应结构和 endless 状态转换 [@x380kkm 2026-08-22] ////
#[test]
fn returns_rush_contract_and_persists_endless_state() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(&service, 69);
    let event_id = 700_001;

    let unopened = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/rush/endless_battle",
        json!({"event_id": event_id, "viewer_id": viewer_id}),
    ));
    assert!(unopened.data["endless_battle_max_round"].is_null());
    assert_eq!(unopened.data["endless_battle_next_round"], 1);
    assert!(unopened.data["endless_battle_played_party_list"].is_null());
    let early_select = send(
        &service,
        "/api/index.php/event/rush/select_folder",
        json!({"event_id": event_id, "folder_id": 1, "viewer_id": viewer_id}),
    );
    assert!(early_select.starts_with("HTTP/1.1 400 Bad Request"));

    let party = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/rush/party",
        json!({"viewer_id": viewer_id}),
    ));
    assert!(party.data["user_party_group_list"].is_array());
    let summary = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/rush/summary",
        json!({"event_id": event_id, "viewer_id": viewer_id}),
    ));
    assert_eq!(summary.data["endless_battle_next_round"], 1);
    assert!(summary.data["endless_battle_max_round"].is_null());
    assert!(summary.data["active_rush_battle_folder_id"].is_null());
    assert!(summary.data["endless_battle_played_max_round"].is_null());
    assert!(summary.data["cleared_folder_id_list"].is_array());
    assert!(summary.data["endless_battle_played_party_list"].is_object());
    assert!(summary.data["rush_battle_played_party_list"].is_object());
    assert!(summary.data["endless_battle_my_ranking"].is_null());
    assert!(summary.data["aggregated_time"].is_string());
    let unknown_folder = send(
        &service,
        "/api/index.php/event/rush/select_folder",
        json!({"event_id": event_id, "folder_id": 99, "viewer_id": viewer_id}),
    );
    assert!(unknown_folder.ends_with("{\"error\":\"folder_not_found\"}"));

    let selected = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/rush/select_folder",
        json!({"event_id": event_id, "folder_id": 2, "viewer_id": viewer_id}),
    ));
    assert_eq!(selected.data["folder_id"], 2);
    assert_eq!(selected.data["event_id"], event_id);
    let repeated_select = send(
        &service,
        "/api/index.php/event/rush/select_folder",
        json!({"event_id": event_id, "folder_id": 3, "viewer_id": viewer_id}),
    );
    assert!(repeated_select.starts_with("HTTP/1.1 400 Bad Request"));
    let aggregated = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/rush/aggregated_time",
        json!({"event_id": event_id, "viewer_id": viewer_id}),
    ));
    assert!(aggregated.data["aggregated_time"].is_string());
    let ranking = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/rush/ranking",
        json!({"event_id": event_id, "page": 0, "viewer_id": viewer_id}),
    ));
    assert_eq!(ranking.data["current_page"], 1);
    assert_eq!(ranking.data["page_max"], 0);
    assert!(ranking.data["my_data"].is_null());
    assert!(ranking.data["ranking_list"].is_array());
    let played_party = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/rush/ranking/played_party",
        json!({"event_id": event_id, "rank_number": 1, "viewer_id": viewer_id}),
    ));
    assert!(played_party.data["rush_ranking_party"].is_array());
    let reward = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/rush/reward",
        json!({"event_id": event_id, "viewer_id": viewer_id}),
    ));
    assert!(reward.data["rank_number"].is_null());
    assert!(reward.data["ranking_reward"]["reward_list"].is_array());
    assert_eq!(reward.data["ranking_reward"]["status"], 0);

    let invalid_battle = send(
        &service,
        "/api/index.php/event/rush/battle/start",
        json!({
            "is_auto_start_mode": false,
            "party_id": 1,
            "play_id": "invalid-rush",
            "quest_id": 799_999_999,
            "viewer_id": viewer_id,
        }),
    );
    assert!(invalid_battle.starts_with("HTTP/1.1 400 Bad Request"));
    let battle = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/rush/battle/start",
        json!({
            "is_auto_start_mode": false,
            "party_id": 1,
            "play_id": "rush-contract",
            "quest_id": 700_001_001,
            "viewer_id": viewer_id,
        }),
    ));
    assert_eq!(battle.data["user_info"]["last_main_quest_id"], 700_001_001);
    assert_eq!(battle.data["is_multi"], "single");
    assert!(battle.data["start_time"].is_number());
    assert_eq!(battle.data["quest_name"], "");
    let folder_reset = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/rush/reset",
        json!({"event_id": event_id, "quest_type": 1, "viewer_id": viewer_id}),
    ));
    assert!(folder_reset.data.is_array());
    decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/rush/select_folder",
        json!({"event_id": event_id, "folder_id": 2, "viewer_id": viewer_id}),
    ));
    service.stop().expect("service stops cleanly");

    seed_rush_ranking_state(root.path(), event_id);
    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let ranked_summary = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/rush/summary",
        json!({"event_id": event_id, "viewer_id": viewer_id}),
    ));
    assert_eq!(ranked_summary.data["active_rush_battle_folder_id"], 2);
    assert_eq!(ranked_summary.data["endless_battle_played_max_round"], 2);
    assert_eq!(
        ranked_summary.data["endless_battle_my_ranking"]["best_round"],
        2
    );
    assert_eq!(
        ranked_summary.data["endless_battle_my_ranking"]["party_member_list"][0]["character_id"],
        111_001
    );
    let ranked = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/rush/ranking",
        json!({"event_id": event_id, "page": 0, "viewer_id": viewer_id}),
    ));
    assert_eq!(ranked.data["page_max"], 1);
    assert_eq!(ranked.data["my_data"]["rank_number"], 0);
    assert_eq!(ranked.data["ranking_list"][0]["rank_number"], 1);
    let ranked_party = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/rush/ranking/played_party",
        json!({"event_id": event_id, "rank_number": 1, "viewer_id": viewer_id}),
    ));
    assert!(ranked_party.data["rush_ranking_party"].is_object());
    assert!(ranked_party.data["rush_ranking_party"]["1"].is_object());
    let ranked_reward = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/rush/reward",
        json!({"event_id": event_id, "viewer_id": viewer_id}),
    ));
    assert_eq!(ranked_reward.data["rank_number"], 2);
    assert_eq!(
        ranked_reward.data["ranking_reward"]["reward_list"][0]["kind"],
        7
    );
    assert_eq!(
        ranked_reward.data["ranking_reward"]["reward_list"][0]["kind_id"],
        64_000
    );
    let mail = decode_response::<Value>(&send(
        &service,
        "/api/index.php/mail/index",
        json!({"viewer_id": viewer_id, "current_page": 1}),
    ));
    assert_eq!(mail.data["total_count"], 1);
    assert_eq!(mail.data["mail"][0]["type_id"], 64_000);
    decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/rush/reward",
        json!({"event_id": event_id, "viewer_id": viewer_id}),
    ));
    let repeated_mail = decode_response::<Value>(&send(
        &service,
        "/api/index.php/mail/index",
        json!({"viewer_id": viewer_id, "current_page": 1}),
    ));
    assert_eq!(repeated_mail.data["total_count"], 1);
    let endless = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/rush/endless_battle",
        json!({"event_id": event_id, "viewer_id": viewer_id}),
    ));
    assert_eq!(endless.data["endless_battle_max_round"], 2);
    assert_eq!(endless.data["endless_battle_next_round"], 3);
    assert!(endless.data["endless_battle_played_party_list"].is_object());

    let endless_reset = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/rush/reset",
        json!({
            "event_id": event_id,
            "is_reset_after_target_round": true,
            "quest_type": 99,
            "reset_target_id": 2,
            "viewer_id": viewer_id,
        }),
    ));
    assert!(endless_reset.data.is_array());
    let reset_summary = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/rush/summary",
        json!({"event_id": event_id, "viewer_id": viewer_id}),
    ));
    assert_eq!(reset_summary.data["endless_battle_next_round"], 2);
    assert!(reset_summary.data["endless_battle_played_party_list"]["1"].is_object());
    assert!(reset_summary.data["endless_battle_played_party_list"]
        .get("2")
        .is_none());
    service.stop().expect("service stops cleanly");
}
// //// /验证 rush 路由响应结构和 endless 状态转换 ////
