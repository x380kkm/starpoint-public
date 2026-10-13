// audience: internal
// # local-save-restore-occupancy
// 恢复请求保留房主和成员正在使用的玩家状态, 房间释放后恢复历史快照和版本.

#[path = "support/cn.rs"]
mod cn_support;
#[path = "support/local_saves.rs"]
mod local_save_support;
mod support;

use local_save_support::{
    assert_status, authorized_request, export_local_save, list_local_saves, load, response_body,
    signup, update_slot_player_snapshot,
};
use rusqlite::{params, Connection};
use serde_json::json;
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

#[derive(Clone, Copy)]
enum RoomRole {
    Host,
    Member,
}

// //// 建立独立的房主或成员占用状态 [@x380kkm 2026-09-27] ////
fn occupy_room(root: &Path, slot_id: i64, other_slot_id: i64, role: RoomRole) {
    let database = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    let account_id: i64 = database
        .query_row(
            "SELECT account_id FROM local_save_slots WHERE id = ?1",
            [slot_id],
            |row| row.get(0),
        )
        .unwrap();
    let host_account_id = match role {
        RoomRole::Host => account_id,
        RoomRole::Member => database
            .query_row(
                "SELECT account_id FROM local_save_slots WHERE id = ?1",
                [other_slot_id],
                |row| row.get(0),
            )
            .unwrap(),
    };
    database
        .execute(
            "INSERT INTO multiplayer_rooms (room_number, room_sequence, access_token,
         host_account_id, host_viewer_id, host_party_id, host_main_character_id,
         category_id, quest_id, raising_state, created_at_ms, host_entry_time,
         expiry_anchor_ms, battle_started, play_id, is_npc_mode)
         VALUES ('999992', 999992, 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', ?1, 993, 1, 1,
         2, 1001, 0, 4102444800000, 4102444800, 4102444800000, 1, 'save-restore', 1)",
            [host_account_id],
        )
        .unwrap();
    if matches!(role, RoomRole::Member) {
        database
            .execute(
                "INSERT INTO multiplayer_room_members (room_number, account_id, viewer_id,
             party_id, member_index, entered, ready, scene_ready)
             VALUES ('999992', ?1, 994, 1, 1, 1, 1, 1)",
                [account_id],
            )
            .unwrap();
    }
}

// //// 验证恢复在房间释放前保留当前存档与历史记录 [@x380kkm 2026-09-27] ////
#[test]
fn restores_wait_until_host_and_member_rooms_are_released() {
    for role in [RoomRole::Host, RoomRole::Member] {
        let root = TempDir::new().unwrap();
        let service = PersonalService::start(root.path(), 0).unwrap();
        signup(service.port(), 871);
        let slot_id = list_local_saves(&service)["slots"][0]["id"]
            .as_i64()
            .unwrap();
        let baseline = export_local_save(&service, slot_id);
        let revision_id = baseline["source"]["revisionId"]
            .as_str()
            .unwrap()
            .to_owned();
        let original_name = baseline["data"]["user_info"]["name"].clone();
        let snapshot = authorized_request(
            &service,
            "POST",
            &format!("/v1/local-saves/{slot_id}/snapshots"),
            Some(&json!({"label": "Restore baseline"})),
        );
        assert_status(&snapshot, "201 Created");
        let snapshot_id = response_body(&snapshot)["id"].as_i64().unwrap();
        let copy = authorized_request(
            &service,
            "POST",
            &format!("/v1/local-saves/{slot_id}/copy"),
            Some(&json!({"name": "Room companion"})),
        );
        assert_status(&copy, "201 Created");
        let other_slot_id = response_body(&copy)["id"].as_i64().unwrap();
        service.stop().unwrap();
        update_slot_player_snapshot(root.path(), slot_id, |data| {
            data["user_info"]["name"] = json!("Current room progress");
        });
        occupy_room(root.path(), slot_id, other_slot_id, role);
        let service = PersonalService::start(root.path(), 0).unwrap();
        let before = export_local_save(&service, slot_id);
        let before_slots = list_local_saves(&service);
        let snapshot_path = format!("/v1/local-saves/{slot_id}/snapshots");
        let revision_path = format!("/v1/local-saves/{slot_id}/revisions");
        let before_snapshots =
            response_body(&authorized_request(&service, "GET", &snapshot_path, None));
        let before_revisions =
            response_body(&authorized_request(&service, "GET", &revision_path, None));
        let restores = [
            format!("{snapshot_path}/{snapshot_id}/restore"),
            format!("{revision_path}/{revision_id}/restore"),
        ];
        for path in &restores {
            let response = authorized_request(&service, "POST", path, None);
            assert_status(&response, "409 Conflict");
            assert_eq!(response_body(&response)["error"], "local_save_busy");
            assert_eq!(export_local_save(&service, slot_id)["data"], before["data"]);
            assert_eq!(list_local_saves(&service), before_slots);
            assert_eq!(
                response_body(&authorized_request(&service, "GET", &snapshot_path, None)),
                before_snapshots
            );
            assert_eq!(
                response_body(&authorized_request(&service, "GET", &revision_path, None)),
                before_revisions
            );
        }
        let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
        let room_count: i64 = database
            .query_row(
                "SELECT COUNT(*) FROM multiplayer_rooms WHERE room_number = '999992'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(room_count, 1);
        database
            .execute(
                "DELETE FROM multiplayer_room_members WHERE room_number = '999992'",
                [],
            )
            .unwrap();
        database
            .execute(
                "DELETE FROM multiplayer_rooms WHERE room_number = '999992'",
                [],
            )
            .unwrap();
        for path in &restores {
            update_slot_player_snapshot(root.path(), slot_id, |data| {
                data["user_info"]["name"] = json!("Current room progress");
            });
            let response = authorized_request(&service, "POST", path, None);
            assert_status(&response, "200 OK");
            assert_eq!(response_body(&response)["restored"], true);
            let current = signup(service.port(), 871);
            let loaded = load(service.port(), current.data_headers.viewer_id, "1.4.74");
            assert_eq!(loaded.data["user_info"]["name"], original_name);
            let selected: i64 = database.query_row(
                "SELECT active.slot_id FROM active_local_save_slots AS active WHERE device_id = ?1", params![871], |row| row.get(0),
            ).unwrap();
            assert_eq!(selected, slot_id);
        }
        service.stop().unwrap();
    }
}
