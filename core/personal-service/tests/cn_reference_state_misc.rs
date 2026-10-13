// audience: internal
// # personal-service-cn-reference-state-misc-tests
//
// 该文件验证任务解锁, 主动任务领奖, 物品出售和漫画图片的参考契约.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

#[path = "cn_reference_state_misc/quest.rs"]
mod quest;

use cn_support::{decode_response, encode_request, LoadRequest, SignupData, SignupRequest};
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

#[derive(Serialize)]
struct QuestUnlockRequest {
    api_count: i64,
    category: i64,
    quest_id: i64,
    viewer_id: i64,
}

#[derive(Serialize)]
struct ItemSellRequest {
    api_count: i64,
    item_id: i64,
    sell_number: i64,
    viewer_id: i64,
}

#[derive(Serialize)]
struct ActiveMissionReceiveRequest {
    api_count: i64,
    active_mission_list: Vec<ActiveMissionReceiveEntry>,
    viewer_id: i64,
}

#[derive(Serialize)]
struct ActiveMissionReceiveEntry {
    mission_id: i64,
    stages: Vec<i64>,
}

fn update_player(root: &Path, update: impl FnOnce(&mut Value)) {
    let database = Connection::open(root.join("personal-service.sqlite3"))
        .expect("service database is opened");
    let (account_id, serialized) = database
        .query_row(
            "SELECT account_id, data_json FROM player_snapshots",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .expect("one player snapshot exists");
    let mut player_data =
        serde_json::from_str::<Value>(&serialized).expect("player snapshot is JSON");
    update(&mut player_data);
    database
        .execute(
            "UPDATE player_snapshots SET data_json = ?1 WHERE account_id = ?2",
            params![
                serde_json::to_string(&player_data).expect("player snapshot is encoded"),
                account_id,
            ],
        )
        .expect("player snapshot is updated");
}

// //// 验证参考状态接口共享并持久化玩家快照 [@x380kkm 2026-08-22] ////
#[test]
fn persists_reference_quest_mission_and_item_mutations() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 91 }),
    ))
    .data_headers
    .viewer_id;
    service.stop().expect("service stops cleanly");
    seed_items(root.path());

    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let unlocked = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/quest/unlock",
        &encode_request(&QuestUnlockRequest {
            api_count: 1,
            category: 14,
            quest_id: 1001,
            viewer_id,
        }),
    ));
    assert_eq!(unlocked.data["item_list"], json!({}));

    let sold = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/item/sell",
        &encode_request(&ItemSellRequest {
            api_count: 2,
            item_id: 1,
            sell_number: 2,
            viewer_id,
        }),
    ));
    assert_eq!(sold.data["item_list"]["1"], 0);
    assert_eq!(sold.data["user_info"]["free_mana"], 1_010);

    let mission_request = ActiveMissionReceiveRequest {
        api_count: 3,
        active_mission_list: vec![ActiveMissionReceiveEntry {
            mission_id: 20_002,
            stages: vec![1],
        }],
        viewer_id,
    };
    let received = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/active_mission/receive",
        &encode_request(&mission_request),
    ));
    assert_eq!(received.data["item_list"]["500000"], 6);
    assert_eq!(received.data["user_info"]["free_mana"], 227_490);
    assert_eq!(received.data["user_info"]["exp_pool"], 759_976);
    assert_eq!(received.data["active_mission_list"][0]["progress_value"], 4);

    let repeated = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/active_mission/receive",
        &encode_request(&mission_request),
    ));
    assert!(repeated.data["item_list"]
        .as_object()
        .is_some_and(serde_json::Map::is_empty));
    assert_eq!(repeated.data["user_info"]["free_mana"], 227_490);

    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(loaded.data["item_list"]["1"], 0);
    assert_eq!(loaded.data["item_list"]["60000"], 1);
    assert_eq!(loaded.data["item_list"]["500000"], 6);
    assert_eq!(
        loaded.data["all_active_mission_list"]["20002"]["stages"]["1"],
        true
    );
    assert_eq!(loaded.data["quest_progress"]["14"][0]["quest_id"], 1001);
    assert_eq!(loaded.data["quest_progress"]["14"][0]["unlocked"], true);

    service.stop().expect("service stops cleanly");
}

fn seed_items(root: &Path) {
    update_player(root, |player_data| {
        player_data["item_list"] = json!({"1": 2, "60000": 1});
        player_data["all_active_mission_list"] =
            json!({"20002": {"progress": 4, "stages": {"1": false}}});
    });
}
// //// /验证参考状态接口共享并持久化玩家快照 ////

// //// 验证角色觉醒主动任务按任务 ID 发放阶段奖励 [@x380kkm 2026-08-24] ////
#[test]
fn grants_awake_active_mission_rewards() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 52 }),
    ));
    let request = ActiveMissionReceiveRequest {
        api_count: 1,
        active_mission_list: vec![ActiveMissionReceiveEntry {
            mission_id: 1_110_011,
            stages: vec![1],
        }],
        viewer_id: signup.data_headers.viewer_id,
    };
    let rejected = cn_support::send_request(
        service.port(),
        "/api/index.php/active_mission/receive",
        &encode_request(&request),
    );
    assert!(rejected.starts_with("HTTP/1.1 400 Bad Request"));
    service.stop().unwrap();
    update_player(root.path(), |player| {
        player["quest_progress"]["3"] = json!([
            {"quest_id":11100101,"finished":true}, {"quest_id":11100102,"finished":true}, {"quest_id":11100103,"finished":true},
        ]);
    });
    let service = PersonalService::start(root.path(), 0).unwrap();
    let received = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/active_mission/receive",
        &encode_request(&request),
    ));
    assert_eq!(received.data["item_list"]["1"], 10);

    let repeated = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/active_mission/receive",
        &encode_request(&request),
    ));
    assert!(repeated.data["item_list"]
        .as_object()
        .is_some_and(serde_json::Map::is_empty));
    service.stop().expect("service stops cleanly");
}
// //// /验证角色觉醒主动任务按任务 ID 发放阶段奖励 ////
