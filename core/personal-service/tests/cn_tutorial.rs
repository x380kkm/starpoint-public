// audience: internal
// # personal-service-cn-tutorial-tests
//
// 该文件验证 CN 教程步骤, 教程扭蛋和首个免费角色的持久化协议.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, LoadRequest, SignupData, SignupRequest};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

const TUTORIAL_GACHA_ID: i64 = 1704;
const TUTORIAL_GACHA_COST: i64 = 150;
const TUTORIAL_STARTING_FREE_VMONEY: i64 = 100;
const TUTORIAL_STARTING_VMONEY: i64 = 100;

#[derive(Serialize)]
struct UpdateStepRequest {
    viewer_id: i64,
    step: i64,
    skip: bool,
    gacha_id: Option<i64>,
}

#[derive(Serialize)]
struct FinishTriggerRequest {
    viewer_id: i64,
    tutorial_ids: Vec<i64>,
}

#[derive(Deserialize)]
struct TutorialResponse {
    step: i64,
    #[serde(default)]
    user_info: Value,
    #[serde(default)]
    gacha: Value,
    #[serde(default)]
    character_list: Vec<Value>,
    #[serde(default)]
    mail_arrived: bool,
}

// //// 设置未完成教程的测试快照 [@x380kkm 2026-08-22] ////
fn set_incomplete_tutorial_snapshot(root: &Path) {
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
    player_data["user_tutorial"] = json!({"skip_flag": null, "tutorial_step": 0, "viewer_id": 0});
    player_data["user_triggered_tutorial"] = json!([]);
    player_data["tutorial_gacha"] = Value::Null;
    player_data["user_info"]["free_vmoney"] = Value::from(TUTORIAL_STARTING_FREE_VMONEY);
    player_data["user_info"]["vmoney"] = Value::from(TUTORIAL_STARTING_VMONEY);
    player_data["user_character_list"]
        .as_object_mut()
        .expect("character list is an object")
        .retain(|character_id, _| character_id == "1");
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
// //// /设置未完成教程的测试快照 ////

// //// 验证 CN 教程扭蛋和免费角色完整流程 [@x380kkm 2026-07-24] ////
#[test]
fn persists_tutorial_gacha_and_first_character() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 7 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    set_incomplete_tutorial_snapshot(root.path());

    let first_step = update_step(&service, viewer_id, 0, false, None);
    assert_eq!(first_step.step, 1);
    assert!(first_step.mail_arrived);

    let missing_gacha = update_step(&service, viewer_id, 14, false, None);
    assert_eq!(missing_gacha.step, 15);
    assert!(missing_gacha.gacha.is_null());

    let gacha_step = update_step(&service, viewer_id, 14, false, Some(TUTORIAL_GACHA_ID));
    assert_eq!(gacha_step.step, 15);
    assert_eq!(gacha_step.user_info["free_vmoney"].as_i64(), Some(0));
    assert_eq!(
        gacha_step.gacha["gacha_info_list"][0]["gacha_id"],
        TUTORIAL_GACHA_ID
    );
    let character_id = gacha_step.gacha["draw"][0]["character_id"]
        .as_i64()
        .expect("tutorial draw contains a character");
    let gacha_master = serde_json::from_str::<Value>(include_str!("../../../assets/gacha.json"))
        .expect("CN gacha master is decoded");
    assert_eq!(
        gacha_master[TUTORIAL_GACHA_ID.to_string()]["singleCost"],
        TUTORIAL_GACHA_COST
    );
    assert!(gacha_master[TUTORIAL_GACHA_ID.to_string()]["pool"]
        .as_object()
        .is_some_and(|ranks| ranks.values().any(|pool| pool
            .as_array()
            .is_some_and(|entries| entries.iter().any(|entry| entry["id"] == character_id)))));
    assert_eq!(gacha_step.character_list[0]["viewer_id"], 0);
    assert_eq!(gacha_step.character_list[0]["character_id"], character_id);

    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(loaded.data["tutorial_gacha"]["character_id"], character_id);
    assert_eq!(
        loaded.data["user_info"]["vmoney"],
        TUTORIAL_STARTING_FREE_VMONEY + TUTORIAL_STARTING_VMONEY - TUTORIAL_GACHA_COST
    );
    assert_eq!(loaded.data["user_tutorial"]["powerflip_failure"], 0);
    assert!(loaded.data["user_character_list"]
        .get(character_id.to_string())
        .is_some());

    let reward_step = update_step(&service, viewer_id, 15, false, None);
    assert_eq!(reward_step.step, 16);
    assert_eq!(reward_step.user_info["free_vmoney"].as_i64(), Some(1500));
    assert!(reward_step.mail_arrived);
    assert_eq!(reward_step.character_list[0]["character_id"], 243001);
    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(loaded.data["user_tutorial"]["tutorial_step"], 16);
    let triggered = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tutorial/finish_trigger",
        &encode_request(&FinishTriggerRequest {
            viewer_id,
            tutorial_ids: vec![12],
        }),
    ));
    assert_eq!(triggered.data, json!([]));
    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert!(loaded.data["user_tutorial"].is_null());
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 教程扭蛋和免费角色完整流程 ////

// //// 拒绝无效教程扭蛋并完成短教程 [@x380kkm 2026-08-23] ////
#[test]
fn rejects_invalid_tutorial_gacha_and_completes_short_tutorial() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 8 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    set_incomplete_tutorial_snapshot(root.path());

    for (step, skip) in [(14, false), (3, true)] {
        for gacha_id in [Some(0), Some(-1), Some(9999)] {
            let invalid = cn_support::send_request(
                service.port(),
                "/api/index.php/tutorial/update_step",
                &encode_request(&UpdateStepRequest {
                    viewer_id,
                    step,
                    skip,
                    gacha_id,
                }),
            );
            assert!(invalid.starts_with("HTTP/1.1 400 Bad Request"));
        }
    }

    let unchanged = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(unchanged.data["user_tutorial"]["tutorial_step"], 0);
    assert_eq!(
        unchanged.data["user_info"]["free_vmoney"],
        TUTORIAL_STARTING_FREE_VMONEY
    );
    assert_eq!(
        unchanged.data["user_info"]["vmoney"],
        TUTORIAL_STARTING_VMONEY
    );
    assert!(unchanged.data["tutorial_gacha"].is_null());

    let missing_gacha = update_step(&service, viewer_id, 3, true, None);
    assert_eq!(missing_gacha.step, 15);
    assert!(missing_gacha.gacha.is_null());
    let gacha = update_step(&service, viewer_id, 3, true, Some(TUTORIAL_GACHA_ID));
    assert_eq!(gacha.step, 15);
    assert_eq!(gacha.user_info["free_vmoney"], 0);
    let character_id = gacha.gacha["draw"][0]["character_id"]
        .as_i64()
        .expect("short tutorial draw contains a character");
    let gacha_replay = update_step(&service, viewer_id, 3, true, Some(TUTORIAL_GACHA_ID));
    assert_eq!(gacha_replay.step, 15);
    assert_eq!(gacha_replay.gacha["draw"][0]["character_id"], character_id);
    assert_eq!(gacha_replay.user_info["free_vmoney"], 0);

    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(loaded.data["user_tutorial"]["tutorial_step"], 4);
    assert_eq!(loaded.data["user_tutorial"]["skip_flag"], true);

    let request = encode_request(&UpdateStepRequest {
        viewer_id,
        step: 4,
        skip: true,
        gacha_id: None,
    });
    let completed = cn_support::send_request(
        service.port(),
        "/api/index.php/tutorial/update_step",
        &request,
    );
    let completed = decode_response::<TutorialResponse>(&completed).data;
    assert_eq!(completed.step, 16);
    assert_eq!(completed.user_info["free_vmoney"], 1500);
    assert_eq!(completed.character_list[0]["character_id"], 243001);
    assert!(completed.mail_arrived);

    let replay = cn_support::send_request(
        service.port(),
        "/api/index.php/tutorial/update_step",
        &request,
    );
    let replay = decode_response::<TutorialResponse>(&replay).data;
    assert_eq!(replay.step, 16);
    assert_eq!(replay.user_info["free_vmoney"], 1500);
    assert_eq!(replay.character_list[0]["character_id"], 243001);

    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(loaded.data["user_tutorial"]["tutorial_step"], 5);
    assert_eq!(loaded.data["user_tutorial"]["skip_flag"], true);
    assert_eq!(loaded.data["mail_arrived"], true);

    let triggered = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tutorial/finish_trigger",
        &encode_request(&FinishTriggerRequest {
            viewer_id,
            tutorial_ids: vec![12, 55],
        }),
    ));
    assert_eq!(triggered.data, json!([]));
    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert!(loaded.data["user_tutorial"].is_null());
    assert!(loaded.data["user_triggered_tutorial"]
        .as_array()
        .is_some_and(|tutorials| tutorials.contains(&json!(12)) && tutorials.contains(&json!(55))));
    service.stop().expect("service stops cleanly");
}
// //// /拒绝无效教程扭蛋并完成短教程 ////

// //// 重放已处理的教程扭蛋响应 [@x380kkm 2026-08-23] ////
#[test]
fn replays_processed_tutorial_gacha() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 9 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    set_incomplete_tutorial_snapshot(root.path());

    let first = update_step(&service, viewer_id, 14, false, Some(TUTORIAL_GACHA_ID));
    let replay = update_step(&service, viewer_id, 14, false, Some(TUTORIAL_GACHA_ID));
    assert_eq!(replay.gacha, first.gacha);
    assert_eq!(replay.character_list, first.character_list);
    assert_eq!(replay.user_info["free_vmoney"], 0);
    service.stop().expect("service stops cleanly");
}
// //// /重放已处理的教程扭蛋响应 ////

fn update_step(
    service: &PersonalService,
    viewer_id: i64,
    step: i64,
    skip: bool,
    gacha_id: Option<i64>,
) -> TutorialResponse {
    let response = cn_support::send_request(
        service.port(),
        "/api/index.php/tutorial/update_step",
        &encode_request(&UpdateStepRequest {
            viewer_id,
            step,
            skip,
            gacha_id,
        }),
    );
    decode_response::<TutorialResponse>(&response).data
}
