// audience: internal
// # personal-service-multiplayer-battle-state-tests
//
// 该模块验证成员独立身份以及继续和结算事务的重放契约.

use super::{
    MultiplayerBattleContinue, MultiplayerBattleFinish, MultiplayerBattleIdentity,
    MultiplayerBattleStart,
};
use crate::database::ServiceDatabase;
use rusqlite::params;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use tempfile::TempDir;

const CATEGORY_ID: i64 = 1;
const QUEST_ID: i64 = 1_001_002;

fn open_database() -> (TempDir, ServiceDatabase) {
    let root = TempDir::new().expect("temporary service directory is created");
    let database = ServiceDatabase::open(root.path()).expect("service database opens");
    (root, database)
}

fn insert_player(database: &ServiceDatabase, account_id: i64, viewer_id: i64) {
    database
        .connection
        .execute(
            "INSERT INTO accounts (
                 id, app_id, first_login_time, idp_alias, idp_code,
                 idp_id, reg_time, last_login_time, status
             ) VALUES (?1, 'app', 'now', 'local', 'local', ?2, 'now', 'now', 'active')",
            params![account_id, format!("battle-state-{viewer_id}")],
        )
        .expect("test account is inserted");
    database
        .connection
        .execute(
            "INSERT INTO players (account_id) VALUES (?1)",
            params![account_id],
        )
        .expect("test player is inserted");
    database
        .connection
        .execute(
            "INSERT INTO player_snapshots (account_id, data_json) VALUES (?1, ?2)",
            params![
                account_id,
                json!({ "user_info": { "free_vmoney": 100, "vmoney": 0 } }).to_string(),
            ],
        )
        .expect("test player snapshot is inserted");
}

fn response_value(receipt: &super::MultiplayerBattleReceipt) -> Value {
    serde_json::from_str(&receipt.response_json).expect("battle receipt contains JSON")
}

// //// 验证成员身份, 重试收据和房间完成共享同一持久状态 [@x380kkm 2026-08-23] ////
#[test]
fn keeps_member_play_ids_and_replays_atomic_battle_actions() {
    let (root, mut database) = open_database();
    insert_player(&database, 1, 101);
    insert_player(&database, 2, 202);
    let room = database
        .create_multiplayer_room(1, 101, 1, 1, CATEGORY_ID, QUEST_ID, 1, 1)
        .expect("test room is created");
    database
        .join_multiplayer_room(&room.room_number, 2, 202, 1)
        .expect("guest room lookup succeeds")
        .expect("guest joins the room");
    database
        .connection
        .execute(
            "UPDATE multiplayer_room_members SET entered = 1 WHERE room_number = ?1",
            params![room.room_number],
        )
        .expect("room members enter the lobby");
    assert!(database
        .stage_multiplayer_battle_expected_viewers(
            &room.room_number,
            room.room_sequence,
            1,
            &BTreeSet::from([101, 202]),
        )
        .expect("battle members are staged"));
    assert!(database
        .set_multiplayer_lobby_started(&room.room_number, 1)
        .expect("lobby state is stored"));
    drop(database);
    let mut database = ServiceDatabase::open(root.path()).expect("service database reopens");

    let guest_start_json = json!({ "member": "guest" }).to_string();
    database
        .start_multiplayer_battle_member(MultiplayerBattleStart {
            identity: MultiplayerBattleIdentity {
                account_id: 2,
                room_number: &room.room_number,
                play_id: "guest-play",
                category_id: CATEGORY_ID,
                quest_id: QUEST_ID,
                api_count: Some(10),
            },
            use_boss_boost_point: false,
            use_boost_point: true,
            is_auto_start_mode: false,
            response_time: 10,
            response_json: &guest_start_json,
        })
        .expect("guest battle start is stored")
        .expect("guest battle start matches the room");
    let host_start_json = json!({ "member": "host" }).to_string();
    database
        .start_multiplayer_battle_member(MultiplayerBattleStart {
            identity: MultiplayerBattleIdentity {
                account_id: 1,
                room_number: &room.room_number,
                play_id: "host-play",
                category_id: CATEGORY_ID,
                quest_id: QUEST_ID,
                api_count: Some(11),
            },
            use_boss_boost_point: true,
            use_boost_point: false,
            is_auto_start_mode: true,
            response_time: 11,
            response_json: &host_start_json,
        })
        .expect("host battle start is stored")
        .expect("host battle start matches the room");

    let guest = database
        .get_active_single_quest(2)
        .expect("guest active battle is read")
        .expect("guest has an active battle");
    assert_eq!(guest.play_id, "guest-play");
    assert!(!guest.use_boss_boost_point);
    assert!(guest.use_boost_point);
    assert!(!guest.is_auto_start_mode);
    let host = database
        .get_active_single_quest(1)
        .expect("host active battle is read")
        .expect("host has an active battle");
    assert_eq!(host.play_id, "host-play");
    assert!(host.use_boss_boost_point);
    assert!(!host.use_boost_point);
    assert!(host.is_auto_start_mode);
    let room_play_id = database
        .connection
        .query_row(
            "SELECT play_id FROM multiplayer_rooms WHERE room_number = ?1",
            params![room.room_number],
            |row| row.get::<_, Option<String>>(0),
        )
        .expect("room battle identity is read");
    assert_eq!(room_play_id, None);

    let continued_snapshot = json!({ "user_info": { "free_vmoney": 50, "vmoney": 0 } }).to_string();
    let continue_response = json!({
        "user_info": { "free_vmoney": 50, "vmoney": 0 },
        "mail_arrived": false,
    });
    let first_continue = database
        .continue_multiplayer_battle_member(MultiplayerBattleContinue {
            identity: MultiplayerBattleIdentity {
                account_id: 2,
                room_number: &room.room_number,
                play_id: "guest-play",
                category_id: CATEGORY_ID,
                quest_id: QUEST_ID,
                api_count: Some(20),
            },
            snapshot: &continued_snapshot,
            response_time: 20,
            response: &continue_response,
        })
        .expect("guest continue is stored")
        .expect("guest continue matches the active battle");
    assert_eq!(response_value(&first_continue)["continue_count"], 1);
    let replayed_continue = database
        .continue_multiplayer_battle_member(MultiplayerBattleContinue {
            identity: MultiplayerBattleIdentity {
                account_id: 2,
                room_number: &room.room_number,
                play_id: "guest-play",
                category_id: CATEGORY_ID,
                quest_id: QUEST_ID,
                api_count: Some(20),
            },
            snapshot: "ignored-on-replay",
            response_time: 99,
            response: &json!({}),
        })
        .expect("guest continue retry is read")
        .expect("guest continue retry has a receipt");
    assert_eq!(replayed_continue, first_continue);
    let counts = database
        .connection
        .query_row(
            "SELECT quests.continue_count, members.continue_count
             FROM active_single_quests AS quests
             JOIN multiplayer_room_members AS members ON members.account_id = quests.account_id
             WHERE quests.account_id = 2 AND members.room_number = ?1",
            params![room.room_number],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .expect("guest continue counts are read");
    assert_eq!(counts, (1, 1));
    let stored_snapshot = database
        .connection
        .query_row(
            "SELECT data_json FROM player_snapshots WHERE account_id = 2",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("continued guest snapshot is read");
    assert_eq!(stored_snapshot, continued_snapshot);

    let second_snapshot = json!({ "user_info": { "free_vmoney": 0, "vmoney": 0 } }).to_string();
    database
        .continue_multiplayer_battle_member(MultiplayerBattleContinue {
            identity: MultiplayerBattleIdentity {
                account_id: 2,
                room_number: &room.room_number,
                play_id: "guest-play",
                category_id: CATEGORY_ID,
                quest_id: QUEST_ID,
                api_count: Some(21),
            },
            snapshot: &second_snapshot,
            response_time: 21,
            response: &json!({ "user_info": { "free_vmoney": 0, "vmoney": 0 } }),
        })
        .expect("second guest continue is stored")
        .expect("second guest continue matches the active battle");
    let continue_receipt_count = database
        .connection
        .query_row(
            "SELECT COUNT(*) FROM multiplayer_battle_action_receipts
             WHERE account_id = 2 AND action = 'continue' AND play_id = 'guest-play'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("guest continue receipts are counted");
    assert_eq!(continue_receipt_count, 2);

    let finish_response = json!({ "is_multi": "multi" });
    let guest_finish_snapshot = json!({
        "quest_progress": {
            "1": [{ "quest_id": QUEST_ID }],
        },
    })
    .to_string();
    let guest_finish = database
        .finish_multiplayer_battle_member(MultiplayerBattleFinish {
            identity: MultiplayerBattleIdentity {
                account_id: 2,
                room_number: &room.room_number,
                play_id: "guest-play",
                category_id: CATEGORY_ID,
                quest_id: QUEST_ID,
                api_count: Some(30),
            },
            snapshot: &guest_finish_snapshot,
            expiry_anchor_ms: 30,
            response_time: 30,
            response: &finish_response,
        })
        .expect("guest finish is stored")
        .expect("guest finish matches the active battle");
    assert_eq!(response_value(&guest_finish)["host_finished"], false);
    let guest_stored_snapshot = database
        .connection
        .query_row(
            "SELECT data_json FROM player_snapshots WHERE account_id = 2",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("finished guest snapshot is read");
    assert!(serde_json::from_str::<Value>(&guest_stored_snapshot)
        .expect("finished guest snapshot contains JSON")["quest_progress"]["1"][0]
        .get("host_finished")
        .is_none());
    let replayed_finish = database
        .finish_multiplayer_battle_member(MultiplayerBattleFinish {
            identity: MultiplayerBattleIdentity {
                account_id: 2,
                room_number: &room.room_number,
                play_id: "guest-play",
                category_id: CATEGORY_ID,
                quest_id: QUEST_ID,
                api_count: Some(30),
            },
            snapshot: "ignored-on-replay",
            expiry_anchor_ms: 99,
            response_time: 99,
            response: &json!({}),
        })
        .expect("guest finish retry is read")
        .expect("guest finish retry has a receipt");
    assert_eq!(replayed_finish, guest_finish);
    assert!(database
        .finish_multiplayer_battle_member(MultiplayerBattleFinish {
            identity: MultiplayerBattleIdentity {
                account_id: 1,
                room_number: &room.room_number,
                play_id: "wrong-host-play",
                category_id: CATEGORY_ID,
                quest_id: QUEST_ID,
                api_count: Some(31),
            },
            snapshot: "wrong-host-finish",
            expiry_anchor_ms: 31,
            response_time: 31,
            response: &finish_response,
        })
        .expect("wrong host finish is rejected without storage failure")
        .is_none());
    let host_finish_snapshot = json!({
        "quest_progress": {
            "1": [{ "quest_id": QUEST_ID }],
        },
    })
    .to_string();
    let host_finish = database
        .finish_multiplayer_battle_member(MultiplayerBattleFinish {
            identity: MultiplayerBattleIdentity {
                account_id: 1,
                room_number: &room.room_number,
                play_id: "host-play",
                category_id: CATEGORY_ID,
                quest_id: QUEST_ID,
                api_count: Some(32),
            },
            snapshot: &host_finish_snapshot,
            expiry_anchor_ms: 32,
            response_time: 32,
            response: &finish_response,
        })
        .expect("host finish is stored")
        .expect("host finish matches the active battle");
    assert_eq!(response_value(&host_finish)["host_finished"], true);
    let host_stored_snapshot = database
        .connection
        .query_row(
            "SELECT data_json FROM player_snapshots WHERE account_id = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("finished host snapshot is read");
    assert!(serde_json::from_str::<Value>(&host_stored_snapshot)
        .expect("finished host snapshot contains JSON")["quest_progress"]["1"][0]
        .get("host_finished")
        .is_none());
    let room_state = database
        .connection
        .query_row(
            "SELECT raising_state, battle_started, lobby_started, expiry_anchor_ms
             FROM multiplayer_rooms WHERE room_number = ?1",
            params![room.room_number],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, bool>(1)?,
                    row.get::<_, bool>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .expect("completed room state is read");
    assert_eq!(room_state, (1, false, false, 32));
}
// //// /验证成员身份, 重试收据和房间完成共享同一持久状态 ////

// //// 迁移房主已结算的旧联机战斗 [@x380kkm 2026-08-23] ////
#[test]
fn migrates_legacy_guest_battle_after_host_finish() {
    let (_root, mut database) = open_database();
    insert_player(&database, 1, 101);
    insert_player(&database, 2, 202);
    let room = database
        .create_multiplayer_room(1, 101, 1, 1, CATEGORY_ID, QUEST_ID, 1, 1)
        .expect("legacy room is created");
    database
        .join_multiplayer_room(&room.room_number, 2, 202, 1)
        .expect("legacy guest room lookup succeeds")
        .expect("legacy guest joins the room");
    database
        .connection
        .execute(
            "UPDATE multiplayer_rooms
             SET battle_started = 1, lobby_started = 1, play_id = 'legacy-host-play'
             WHERE room_number = ?1",
            params![room.room_number],
        )
        .expect("legacy room battle state is stored");
    database
        .connection
        .execute(
            "INSERT INTO active_single_quests (
                 account_id, play_id, quest_id, category,
                 use_boss_boost_point, use_boost_point, is_auto_start_mode, continue_count
             ) VALUES (2, 'legacy-guest-play', ?1, ?2, 0, 0, 0, 1)",
            params![QUEST_ID, CATEGORY_ID],
        )
        .expect("legacy guest active battle is stored");

    super::battle_state::migrate(&database.connection).expect("legacy battle state is migrated");

    let guest_play_id = database
        .connection
        .query_row(
            "SELECT play_id FROM multiplayer_battle_players WHERE account_id = 2",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .expect("migrated guest battle identity is read");
    assert_eq!(guest_play_id.as_deref(), Some("legacy-guest-play"));
    let guest_active_count = database
        .connection
        .query_row(
            "SELECT COUNT(*) FROM active_single_quests
             WHERE account_id = 2 AND play_id = 'legacy-guest-play'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("migrated guest active battle is counted");
    assert_eq!(guest_active_count, 1);
    let room_play_id = database
        .connection
        .query_row(
            "SELECT play_id FROM multiplayer_rooms WHERE room_number = ?1",
            params![room.room_number],
            |row| row.get::<_, Option<String>>(0),
        )
        .expect("migrated room identity is read");
    assert_eq!(room_play_id, None);
}
// //// /迁移房主已结算的旧联机战斗 ////
