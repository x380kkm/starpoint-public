// audience: internal
// # personal-service-ai-team-tests
//
// 该文件通过真实 HTTP 请求验证两个 CN 队伍的自动选择和不可变本地快照.
// 该文件验证候选状态, 槽位隔离和快照只包含白名单游戏数据.

#[path = "support/cn.rs"]
mod cn_support;
#[path = "support/local_saves.rs"]
mod local_save_support;
mod support;

use local_save_support::{
    assert_status, authorized_request, list_local_saves, response_body, signup,
    update_slot_player_snapshot,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

// //// 管理两个不可变 AI 队伍快照 [@x380kkm 2026-08-18] ////
#[test]
fn manages_two_immutable_ai_team_snapshots() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    signup(service.port(), 6101);
    let slot_id = list_local_saves(&service)["slots"][0]["id"]
        .as_i64()
        .expect("signup creates a local save slot");
    let path = format!("/v1/local-saves/{slot_id}/ai-teams");

    let direct =
        support::request_with_headers(service.port(), "GET", &path, "application/json", &[], b"");
    assert_status(&direct, "200 OK");

    let automatically_created = authorized_request(&service, "GET", &path, None);
    assert_status(&automatically_created, "200 OK");
    let automatically_created = response_body(&automatically_created);
    assert_eq!(automatically_created["selection_status"], "ready");
    assert_eq!(automatically_created["selected_party_ids"], json!([1, 2]));
    assert_eq!(
        automatically_created["teams"].as_array().map(Vec::len),
        Some(2)
    );
    assert!(automatically_created["candidates"]
        .as_array()
        .is_some_and(|candidates| candidates.len() >= 2));

    for party_ids in [json!([1]), json!([1, 1]), json!([1, 2, 3])] {
        let invalid = authorized_request(
            &service,
            "PUT",
            &path,
            Some(&json!({ "party_ids": party_ids })),
        );
        assert_status(&invalid, "400 Bad Request");
        assert_eq!(
            response_body(&invalid)["error"].as_str(),
            Some("invalid_ai_team_party_ids")
        );
    }

    let missing = authorized_request(
        &service,
        "PUT",
        &path,
        Some(&json!({ "party_ids": [1, 9999] })),
    );
    assert_status(&missing, "422 Unprocessable Entity");
    assert_eq!(
        response_body(&missing)["error"].as_str(),
        Some("ai_team_party_not_found")
    );

    update_slot_player_snapshot(root.path(), slot_id, |data| {
        data["user_party_group_list"]["1"]["list"]["2"]
            .as_object_mut()
            .expect("party is an object")
            .remove("options");
    });
    let incomplete = authorized_request(
        &service,
        "PUT",
        &path,
        Some(&json!({ "party_ids": [1, 2] })),
    );
    assert_status(&incomplete, "422 Unprocessable Entity");
    assert_eq!(
        response_body(&incomplete)["error"].as_str(),
        Some("ai_team_party_incomplete")
    );

    update_slot_player_snapshot(root.path(), slot_id, |data| {
        let party_one = &mut data["user_party_group_list"]["1"]["list"]["1"];
        party_one["unison_character_ids"] = json!([1, null, null]);
        party_one["equipment_ids"] = json!([42, null, null]);
        party_one["ability_soul_ids"] = json!([9001, null, null]);
        party_one["options"] = json!({
            "allow_other_players_to_heal_me": true,
            "custom_ai_mode": "balanced",
        });
        data["user_equipment_list"]["42"] = json!({
            "equipment_id": 110001,
            "level": 3,
            "protection": true,
        });
        data["user_character_mana_node_list"]["1"] = json!([2201, 2202, 2203]);
        data["user_party_group_list"]["1"]["list"]["2"]["options"] = json!({
            "allow_other_players_to_heal_me": false,
        });
    });

    let created = authorized_request(
        &service,
        "PUT",
        &path,
        Some(&json!({ "party_ids": [1, 2] })),
    );
    assert_status(&created, "200 OK");
    let initial = response_body(&created);
    let teams = initial["teams"].as_array().expect("AI teams are an array");
    assert_eq!(teams.len(), 2);
    assert_eq!(teams[0]["team_index"].as_i64(), Some(0));
    assert_eq!(teams[1]["team_index"].as_i64(), Some(1));
    assert_eq!(teams[0]["party_id"].as_i64(), Some(1));
    assert_eq!(teams[1]["party_id"].as_i64(), Some(2));
    assert_eq!(
        teams[0]["source_revision_id"],
        teams[1]["source_revision_id"]
    );
    assert_eq!(teams[0]["snapshot_id"].as_str().map(str::len), Some(32));
    assert_eq!(teams[0]["data"]["character_ids"], json!([1, null, null]));
    assert_eq!(
        teams[0]["data"]["unison_character_ids"],
        json!([1, null, null])
    );
    assert_eq!(teams[0]["data"]["characters"]["1"]["exp"], 10);
    assert_eq!(teams[0]["data"]["unison_characters"]["1"]["entry_count"], 1);
    assert_eq!(teams[0]["data"]["equipment"]["42"]["level"], 3);
    assert_eq!(
        teams[0]["data"]["mana_nodes"]["1"],
        json!([2201, 2202, 2203])
    );
    assert_eq!(
        teams[0]["data"]["ability_soul_ids"],
        json!([9001, null, null])
    );
    assert_eq!(
        teams[0]["data"]["options"]["custom_ai_mode"].as_str(),
        Some("balanced")
    );
    let encoded_initial = initial.to_string();
    for forbidden in [
        "viewer_id",
        "associate_token",
        "session",
        "credential",
        "password",
    ] {
        assert!(!encoded_initial.contains(forbidden));
    }

    update_slot_player_snapshot(root.path(), slot_id, |data| {
        data["user_character_list"]["2"] = json!({
            "entry_count": 1,
            "evolution_level": 2,
            "exp": 777,
        });
        data["user_party_group_list"]["1"]["list"]["1"]["character_ids"] = json!([2, null, null]);
        data["user_party_group_list"]["1"]["list"]["1"]["options"]["custom_ai_mode"] =
            Value::from("aggressive");
    });

    let unchanged = authorized_request(&service, "GET", &path, None);
    assert_status(&unchanged, "200 OK");
    assert_eq!(response_body(&unchanged)["teams"], initial["teams"]);

    let refreshed = authorized_request(
        &service,
        "PUT",
        &path,
        Some(&json!({ "party_ids": [1, 2] })),
    );
    assert_status(&refreshed, "200 OK");
    let refreshed = response_body(&refreshed);
    assert_ne!(
        refreshed["teams"][0]["snapshot_id"],
        initial["teams"][0]["snapshot_id"]
    );
    assert_ne!(
        refreshed["teams"][0]["source_revision_id"],
        initial["teams"][0]["source_revision_id"]
    );
    assert_eq!(
        refreshed["teams"][0]["data"]["character_ids"],
        json!([2, null, null])
    );
    assert_eq!(refreshed["teams"][0]["data"]["characters"]["2"]["exp"], 777);
    assert_eq!(
        refreshed["teams"][0]["data"]["options"]["custom_ai_mode"].as_str(),
        Some("aggressive")
    );

    let database = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("service database is opened");
    let stored_count = database
        .query_row(
            "SELECT COUNT(*) FROM ai_team_snapshots WHERE slot_id = ?1",
            params![slot_id],
            |row| row.get::<_, i64>(0),
        )
        .expect("AI snapshot count is read");
    assert_eq!(stored_count, 6);
    let head_count = database
        .query_row(
            "SELECT COUNT(*) FROM ai_team_snapshot_heads WHERE slot_id = ?1",
            params![slot_id],
            |row| row.get::<_, i64>(0),
        )
        .expect("AI snapshot head count is read");
    assert_eq!(head_count, 2);
    let immutable_update = database.execute(
        "UPDATE ai_team_snapshots SET party_id = 3 WHERE id = ?1",
        params![initial["teams"][0]["snapshot_id"]
            .as_str()
            .expect("initial snapshot has an id")],
    );
    assert!(immutable_update.is_err());
    drop(database);

    service.stop().expect("service stops before restart");
    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let after_restart = authorized_request(&service, "GET", &path, None);
    assert_status(&after_restart, "200 OK");
    assert_eq!(response_body(&after_restart), refreshed);

    let deleted = authorized_request(&service, "DELETE", &path, None);
    assert_status(&deleted, "200 OK");
    assert_eq!(response_body(&deleted)["deleted"].as_bool(), Some(true));
    let after_delete = authorized_request(&service, "GET", &path, None);
    assert_status(&after_delete, "200 OK");
    assert_eq!(
        response_body(&after_delete)["teams"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    assert_eq!(
        response_body(&after_delete)["selection_status"],
        "manual_selection_required"
    );

    for method in ["GET", "DELETE"] {
        let missing_slot =
            authorized_request(&service, method, "/v1/local-saves/999999/ai-teams", None);
        assert_status(&missing_slot, "404 Not Found");
    }
    let missing_slot = authorized_request(
        &service,
        "PUT",
        "/v1/local-saves/999999/ai-teams",
        Some(&json!({ "party_ids": [1, 2] })),
    );
    assert_status(&missing_slot, "404 Not Found");
    service.stop().expect("service stops cleanly");
}
// //// /管理两个不可变 AI 队伍快照 ////

// //// 返回队伍不足时的默认模板状态 [@x380kkm 2026-08-20] ////
#[test]
fn requires_default_template_when_only_one_party_is_valid() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    signup(service.port(), 6102);
    let slot_id = list_local_saves(&service)["slots"][0]["id"]
        .as_i64()
        .expect("signup creates a local save slot");
    update_slot_player_snapshot(root.path(), slot_id, |data| {
        let groups = data["user_party_group_list"]
            .as_object_mut()
            .expect("party groups are an object");
        for (group_id, group) in groups {
            group["list"]
                .as_object_mut()
                .expect("party list is an object")
                .retain(|party_id, _| group_id == "1" && party_id == "1");
        }
    });

    let path = format!("/v1/local-saves/{slot_id}/ai-teams");
    let response = authorized_request(&service, "GET", &path, None);
    assert_status(&response, "200 OK");
    let body = response_body(&response);
    assert_eq!(body["selection_status"], "default_template_required");
    assert_eq!(body["teams"].as_array().map(Vec::len), Some(0));
    assert_eq!(body["candidates"].as_array().map(Vec::len), Some(1));
    assert_eq!(body["selected_party_ids"], json!([]));
    service.stop().expect("service stops cleanly");
}
// //// /返回队伍不足时的默认模板状态 ////

// //// 隔离不同存档槽的 AI 队伍选择 [@x380kkm 2026-08-20] ////
#[test]
fn isolates_ai_team_snapshots_between_slots_and_restarts() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    signup(service.port(), 6103);
    let first_slot_id = list_local_saves(&service)["slots"][0]["id"]
        .as_i64()
        .expect("signup creates a local save slot");
    let copied = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{first_slot_id}/copy"),
        Some(&json!({ "name": "AI team isolated copy" })),
    );
    assert_status(&copied, "201 Created");
    let second_slot_id = response_body(&copied)["id"]
        .as_i64()
        .expect("copied slot has an id");
    let first_path = format!("/v1/local-saves/{first_slot_id}/ai-teams");
    let second_path = format!("/v1/local-saves/{second_slot_id}/ai-teams");

    let first = response_body(&authorized_request(&service, "GET", &first_path, None));
    let second = response_body(&authorized_request(&service, "GET", &second_path, None));
    assert_eq!(first["selected_party_ids"], json!([1, 2]));
    assert_eq!(second["selected_party_ids"], json!([1, 2]));
    assert_ne!(
        first["teams"][0]["snapshot_id"],
        second["teams"][0]["snapshot_id"]
    );

    let reversed = authorized_request(
        &service,
        "PUT",
        &first_path,
        Some(&json!({ "party_ids": [2, 1] })),
    );
    assert_status(&reversed, "200 OK");
    assert_eq!(
        response_body(&reversed)["selected_party_ids"],
        json!([2, 1])
    );
    let unchanged_second = response_body(&authorized_request(&service, "GET", &second_path, None));
    assert_eq!(unchanged_second["teams"], second["teams"]);
    service.stop().expect("service stops before restart");

    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let restored_first = response_body(&authorized_request(&service, "GET", &first_path, None));
    let restored_second = response_body(&authorized_request(&service, "GET", &second_path, None));
    assert_eq!(restored_first["selected_party_ids"], json!([2, 1]));
    assert_eq!(restored_second["selected_party_ids"], json!([1, 2]));
    assert_eq!(restored_second["teams"], second["teams"]);
    service.stop().expect("restarted service stops cleanly");
}
// //// /隔离不同存档槽的 AI 队伍选择 ////
