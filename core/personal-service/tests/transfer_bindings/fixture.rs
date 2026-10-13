// audience: internal
// # transfer-binding-fixture
// 绑定验证使用两个 loopback 服务和独立临时数据库.

use super::*;
use local_save_support::{assert_status, export_local_save, update_slot_player_snapshot};
use tempfile::TempDir;

pub(super) struct Fixture {
    pub(super) source: PersonalService,
    pub(super) target: PersonalService,
    pub(super) source_slot_id: i64,
    target_slot_id: i64,
    pub(super) binding_path: String,
    binding_id: String,
    root: TempDir,
}

// //// 建立共同基线并驱动真实绑定调度 [@x380kkm 2026-09-26] ////
impl Fixture {
    pub(super) fn new(policy: &str, upload: &str, pull: &str) -> Self {
        let root = tempdir().expect("temporary service root is created");
        let source =
            PersonalService::start(root.path().join("source"), 0).expect("source service starts");
        let target =
            PersonalService::start(root.path().join("target"), 0).expect("target service starts");
        signup(source.port(), 97101);
        signup(target.port(), 97102);
        let source_slot_id = first_slot_id(&source);
        let target_slot_id = first_slot_id(&target);
        let access = issue_target_slot_token(&target, target_slot_id);
        let profile = authorized_request(
            &source,
            "POST",
            "/v1/server-profiles",
            Some(&json!({
                "name": "Safety target", "scheme": "http", "host": "127.0.0.1",
                "port": target.port(),
            })),
        );
        assert_status(&profile, "201 Created");
        let binding = authorized_request(
            &source,
            "POST",
            &format!("/v1/local-saves/{source_slot_id}/transfer-bindings"),
            Some(&json!({
                "target_profile_id": response_body(&profile)["id"],
                "target_instance_kind": "local", "target_instance_id": access.instance_id,
                "target_slot_id": target_slot_id, "target_token": access.token,
                "upload_mode": upload, "pull_mode": pull, "conflict_policy": policy,
                "interval_seconds": 60, "enabled": true,
            })),
        );
        assert_status(&binding, "201 Created");
        let binding_id = response_body(&binding)["binding_id"]
            .as_str()
            .expect("binding ID is returned")
            .to_owned();
        let binding_path =
            format!("/v1/local-saves/{source_slot_id}/transfer-bindings/{binding_id}");
        let synchronized =
            authorized_request(&source, "POST", &format!("{binding_path}/sync"), None);
        assert_status(&synchronized, "200 OK");
        Self {
            source,
            target,
            source_slot_id,
            target_slot_id,
            binding_path,
            binding_id,
            root,
        }
    }

    pub(super) fn advance_source(&self) {
        update_slot_player_snapshot(
            &self.root.path().join("source"),
            self.source_slot_id,
            |data| {
                data["user_info"]["name"] = json!("Source changes");
            },
        );
    }

    pub(super) fn advance_target(&self) {
        update_slot_player_snapshot(
            &self.root.path().join("target"),
            self.target_slot_id,
            |data| {
                data["user_info"]["name"] = json!("Target changes");
            },
        );
    }

    pub(super) fn source_export(&self) -> Value {
        export_local_save(&self.source, self.source_slot_id)
    }

    pub(super) fn target_export(&self) -> Value {
        export_local_save(&self.target, self.target_slot_id)
    }

    pub(super) fn source_database(&self) -> Connection {
        Connection::open(self.root.path().join("source/personal-service.sqlite3"))
            .expect("isolated source database opens")
    }

    pub(super) fn run_interval(&self) -> Value {
        self.source_database().execute(
            "UPDATE transfer_bindings SET next_run_at = '1970-01-01T00:00:00.000Z' WHERE id = ?1",
            params![self.binding_id],
        ).expect("binding is made due");
        for _ in 0..60 {
            let response = authorized_request(&self.source, "GET", &self.binding_path, None);
            assert_status(&response, "200 OK");
            let state = response_body(&response);
            if state["pending_direction"] == "conflict"
                || state["next_run_at"] != "1970-01-01T00:00:00.000Z"
            {
                return state;
            }
            thread::sleep(Duration::from_millis(100));
        }
        panic!("interval binding did not complete");
    }

    pub(super) fn stop(self) {
        self.source.stop().expect("source service stops");
        self.target.stop().expect("target service stops");
    }
}
// //// /建立共同基线并驱动真实绑定调度 ////

#[derive(Clone, Copy, Debug)]
pub(super) enum Occupancy {
    SingleQuest,
    Host,
    Member,
}

// //// 建立独立的单机战斗或房间占用 [@x380kkm 2026-09-26] ////
pub(super) fn occupy_source(fixture: &Fixture, occupancy: Occupancy) {
    signup(fixture.source.port(), 97103);
    let database = fixture.source_database();
    let account_id: i64 = database
        .query_row(
            "SELECT account_id FROM local_save_slots WHERE id = ?1",
            params![fixture.source_slot_id],
            |row| row.get(0),
        )
        .expect("source account exists");
    let host_account_id = match occupancy {
        Occupancy::SingleQuest => {
            database
                .execute(
                    "INSERT INTO active_single_quests (account_id, quest_id, category,
                 use_boss_boost_point, use_boost_point, is_auto_start_mode)
                 VALUES (?1, 1001002, 1, 0, 0, 0)",
                    params![account_id],
                )
                .expect("single quest is active");
            return;
        }
        Occupancy::Host => account_id,
        Occupancy::Member => database
            .query_row(
                "SELECT id FROM accounts WHERE id != ?1 LIMIT 1",
                params![account_id],
                |row| row.get(0),
            )
            .expect("another account hosts the room"),
    };
    database
        .execute(
            "INSERT INTO multiplayer_rooms (room_number, room_sequence, access_token,
         host_account_id, host_viewer_id, host_party_id, host_main_character_id,
         category_id, quest_id, raising_state, created_at_ms, host_entry_time,
         expiry_anchor_ms, battle_started, play_id, is_npc_mode)
         VALUES ('999991', 999991, 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', ?1, 991, 1, 1,
         2, 1001, 0, 4102444800000, 4102444800, 4102444800000, 1, 'save-safety', 1)",
            params![host_account_id],
        )
        .expect("battle room is active");
    if matches!(occupancy, Occupancy::Member) {
        database
            .execute(
                "INSERT INTO multiplayer_room_members (room_number, account_id, viewer_id,
             party_id, member_index, entered, ready, scene_ready)
             VALUES ('999991', ?1, 992, 1, 1, 1, 1, 1)",
                params![account_id],
            )
            .expect("source joins the battle room");
    }
}
// //// /建立独立的单机战斗或房间占用 ////
