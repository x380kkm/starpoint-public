// audience: internal
// # personal-service-multiplayer-lifecycle-tests
//
// 该模块使用注入的真实墙钟值验证房间生命周期存储契约.

use super::{MultiplayerRoomEvent, MultiplayerRoomEventKind};
use crate::database::ServiceDatabase;
use rusqlite::params;
use tempfile::TempDir;

const INCOMPLETE_ROOM_LIFETIME_MS: i64 = 15 * 60 * 1_000;
const FULL_ROOM_LIFETIME_MS: i64 = 30 * 60 * 1_000;
const REMAINING_NOTICE_MS: i64 = 30 * 1_000;

fn open_database() -> (TempDir, ServiceDatabase) {
    let root = TempDir::new().expect("temporary service directory is created");
    let database = ServiceDatabase::open(root.path()).expect("service database opens");
    (root, database)
}

fn insert_player(database: &ServiceDatabase, account_id: i64) {
    database
        .connection
        .execute(
            "INSERT INTO accounts (
                 id, app_id, first_login_time, idp_alias, idp_code,
                 idp_id, reg_time, last_login_time, status
             ) VALUES (?1, 'app', 'now', 'local', 'local', ?2, 'now', 'now', 'active')",
            params![account_id, format!("test-{account_id}")],
        )
        .expect("test account is inserted");
    database
        .connection
        .execute(
            "INSERT INTO players (account_id) VALUES (?1)",
            params![account_id],
        )
        .expect("test player is inserted");
}

fn create_room(
    database: &mut ServiceDatabase,
    account_id: i64,
    viewer_id: i64,
    protocol_time_ms: i64,
    expiry_anchor_ms: i64,
) -> (String, i64) {
    let room = database
        .create_multiplayer_room(
            account_id,
            viewer_id,
            1,
            1,
            1,
            1_001_002,
            protocol_time_ms,
            expiry_anchor_ms,
        )
        .expect("test multiplayer room is created");
    (room.room_number, room.room_sequence)
}

// //// 验证账号只保留最后创建的房间 [@x380kkm 2026-08-25] ////
#[test]
fn replaces_existing_room_for_same_host() {
    let (_root, mut database) = open_database();
    insert_player(&database, 1);
    let (first_room_number, first_sequence) = create_room(&mut database, 1, 101, 1, 1);
    let (second_room_number, second_sequence) = create_room(&mut database, 1, 101, 2, 2);

    assert_ne!(first_room_number, second_room_number);
    assert!(second_sequence > first_sequence);
    assert!(database
        .multiplayer_room(&first_room_number)
        .expect("replaced room lookup succeeds")
        .is_none());
    assert!(database
        .multiplayer_room(&second_room_number)
        .expect("current room lookup succeeds")
        .is_some());
}
// //// /验证账号只保留最后创建的房间 ////

// //// 验证真实墙钟房间期限和投递确认 [@x380kkm 2026-08-23] ////
#[test]
fn uses_wall_clock_deadlines_and_requires_delivery_acknowledgement() {
    let (_root, mut database) = open_database();
    insert_player(&database, 1);
    let anchor_ms = 1_000_000;
    let (room_number, room_sequence) = create_room(&mut database, 1, 101, 9_000_000_000, anchor_ms);
    database
        .connection
        .execute(
            "UPDATE multiplayer_room_members SET entered = 1 WHERE room_number = ?1",
            params![room_number],
        )
        .expect("host enters the test room");
    let deadline_ms = anchor_ms + INCOMPLETE_ROOM_LIFETIME_MS;
    let expected_notice = MultiplayerRoomEvent {
        room_number: room_number.clone(),
        room_sequence,
        kind: MultiplayerRoomEventKind::Remaining {
            seconds: 30,
            deadline_ms,
        },
    };
    assert_eq!(
        database
            .poll_multiplayer_room_events(deadline_ms - REMAINING_NOTICE_MS)
            .expect("room events are polled"),
        vec![expected_notice.clone()]
    );
    assert_eq!(
        database
            .poll_multiplayer_room_events(deadline_ms - REMAINING_NOTICE_MS)
            .expect("undelivered room notice is polled again"),
        vec![expected_notice]
    );
    assert!(database
        .mark_multiplayer_remaining_notified(&room_number, room_sequence, deadline_ms)
        .expect("delivered room notice is recorded"));
    assert!(database
        .poll_multiplayer_room_events(deadline_ms - 1)
        .expect("acknowledged room notice is suppressed")
        .is_empty());
    assert_eq!(
        database
            .poll_multiplayer_room_events(deadline_ms)
            .expect("expired room is polled"),
        vec![MultiplayerRoomEvent {
            room_number: room_number.clone(),
            room_sequence,
            kind: MultiplayerRoomEventKind::Dismissed,
        }]
    );
    assert!(database
        .multiplayer_room(&room_number)
        .expect("expired room lookup succeeds")
        .is_none());

    insert_player(&database, 2);
    insert_player(&database, 3);
    insert_player(&database, 4);
    let full_anchor_ms = 2_000_000;
    let (full_room_number, full_room_sequence) =
        create_room(&mut database, 2, 202, 1, full_anchor_ms);
    database
        .join_multiplayer_room(&full_room_number, 3, 303, 1)
        .expect("first guest joins")
        .expect("first guest has a room");
    database
        .join_multiplayer_room(&full_room_number, 4, 404, 1)
        .expect("second guest joins")
        .expect("second guest has a room");
    database
        .connection
        .execute(
            "UPDATE multiplayer_room_members SET entered = 1 WHERE room_number = ?1",
            params![full_room_number],
        )
        .expect("full room members enter");
    assert!(database
        .poll_multiplayer_room_events(full_anchor_ms + INCOMPLETE_ROOM_LIFETIME_MS)
        .expect("full room survives the incomplete deadline")
        .is_empty());
    let full_deadline_ms = full_anchor_ms + FULL_ROOM_LIFETIME_MS;
    assert_eq!(
        database
            .poll_multiplayer_room_events(full_deadline_ms - REMAINING_NOTICE_MS)
            .expect("full room notice is polled"),
        vec![MultiplayerRoomEvent {
            room_number: full_room_number,
            room_sequence: full_room_sequence,
            kind: MultiplayerRoomEventKind::Remaining {
                seconds: 30,
                deadline_ms: full_deadline_ms,
            },
        }]
    );
}
// //// /验证真实墙钟房间期限和投递确认 ////

// //// 验证战斗房跳过期限并在完成时重置锚点 [@x380kkm 2026-08-23] ////
#[test]
fn skips_battle_rooms_and_resets_expiry_anchor_on_completion() {
    let (_root, mut database) = open_database();
    insert_player(&database, 1);
    let anchor_ms = 1_000_000;
    let (room_number, room_sequence) = create_room(&mut database, 1, 101, 1, anchor_ms);
    database
        .connection
        .execute(
            "UPDATE multiplayer_rooms
             SET raising_state = 4, battle_started = 1, lobby_started = 1,
                 play_id = 'battle'
             WHERE room_number = ?1",
            params![room_number],
        )
        .expect("test room enters battle");
    assert!(database
        .poll_multiplayer_room_events(anchor_ms + FULL_ROOM_LIFETIME_MS * 10)
        .expect("battle room lifecycle is polled")
        .is_empty());
    assert!(database
        .multiplayer_room(&room_number)
        .expect("battle room lookup succeeds")
        .is_some());

    let reset_anchor_ms = 8_000_000;
    assert!(database
        .finish_multiplayer_room_if_complete(&room_number, reset_anchor_ms)
        .expect("completed battle returns to lobby"));
    let state = database
        .connection
        .query_row(
            "SELECT raising_state, battle_started, lobby_started, play_id, expiry_anchor_ms
             FROM multiplayer_rooms WHERE room_number = ?1",
            params![room_number],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, bool>(1)?,
                    row.get::<_, bool>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .expect("returned lobby state is read");
    assert_eq!(state, (1, false, false, None, reset_anchor_ms));
    let deadline_ms = reset_anchor_ms + INCOMPLETE_ROOM_LIFETIME_MS;
    assert_eq!(
        database
            .poll_multiplayer_room_events(deadline_ms - REMAINING_NOTICE_MS)
            .expect("returned lobby notice is polled"),
        vec![MultiplayerRoomEvent {
            room_number,
            room_sequence,
            kind: MultiplayerRoomEventKind::Remaining {
                seconds: 30,
                deadline_ms,
            },
        }]
    );
}
// //// /验证战斗房跳过期限并在完成时重置锚点 ////

// //// 验证房间序号跨删除和重启单调递增 [@x380kkm 2026-08-23] ////
#[test]
fn persists_monotonic_room_sequence_across_deletion_and_restart() {
    let (root, mut database) = open_database();
    insert_player(&database, 1);
    let (first_room_number, first_sequence) = create_room(&mut database, 1, 101, 1, 1);
    assert!(database
        .disband_multiplayer_room(&first_room_number, 1)
        .expect("first room is disbanded"));
    database
        .poll_multiplayer_room_events(1)
        .expect("queued dismissal is drained");
    drop(database);

    let mut reopened = ServiceDatabase::open(root.path()).expect("service database reopens");
    let (_, second_sequence) = create_room(&mut reopened, 1, 101, 1, 2);
    assert!(second_sequence > first_sequence);
}
// //// /验证房间序号跨删除和重启单调递增 ////
