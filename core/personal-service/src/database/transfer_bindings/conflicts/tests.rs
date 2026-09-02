// audience: internal
// # transfer-binding-conflict-tests
//
// 该文件验证本地下载覆盖与绑定状态在同一 SQLite 事务提交.

use super::*;
use tempfile::{tempdir, TempDir};

const BINDING_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CONFLICT_ID: &str = "cccccccccccccccccccccccccccccccc";
const TARGET_ETAG: &str = "2222222222222222222222222222222222222222222222222222222222222222";

// //// 拒绝变化或停用后的绑定覆盖本地存档 [@x380kkm 2026-08-03] ////
#[test]
fn stale_or_disabled_binding_does_not_replace_local_save() {
    let (_root, mut database, slot_id, _initial_etag) = create_transfer_binding_fixture();
    let before = read_local_save_state(&database.connection, slot_id);
    database
        .connection
        .execute(
            "UPDATE transfer_bindings
             SET enabled = 0, revision = revision + 1
             WHERE id = ?1",
            params![BINDING_ID],
        )
        .expect("binding is disabled");

    let stale = database.commit_transfer_binding_download(
        BINDING_ID,
        0,
        slot_id,
        r#"{"value":2}"#,
        TARGET_ETAG,
    );
    assert!(matches!(
        stale,
        Err(TransferBindingStoreError::StaleBinding)
    ));
    assert_eq!(read_local_save_state(&database.connection, slot_id), before);

    let disabled = database.commit_transfer_binding_download(
        BINDING_ID,
        1,
        slot_id,
        r#"{"value":2}"#,
        TARGET_ETAG,
    );
    assert!(matches!(
        disabled,
        Err(TransferBindingStoreError::StaleBinding)
    ));
    assert_eq!(read_local_save_state(&database.connection, slot_id), before);

    database
        .connection
        .execute(
            "DELETE FROM transfer_bindings WHERE id = ?1",
            params![BINDING_ID],
        )
        .expect("binding is deleted");
    let deleted = database.commit_transfer_binding_download(
        BINDING_ID,
        1,
        slot_id,
        r#"{"value":2}"#,
        TARGET_ETAG,
    );
    assert!(matches!(
        deleted,
        Err(TransferBindingStoreError::BindingNotFound)
    ));
    assert_eq!(read_local_save_state(&database.connection, slot_id), before);
}
// //// /拒绝变化或停用后的绑定覆盖本地存档 ////

// //// 拒绝战斗中的存档覆盖 [@x380kkm 2026-08-03] ////
#[test]
fn active_single_quest_blocks_transfer_download() {
    let (_root, mut database, slot_id, _initial_etag) = create_transfer_binding_fixture();
    let before = read_local_save_state(&database.connection, slot_id);
    database
        .connection
        .execute(
            "INSERT INTO active_single_quests (
                 account_id, quest_id, category, use_boss_boost_point,
                 use_boost_point, is_auto_start_mode
             ) SELECT account_id, 1, 1, 0, 0, 0
               FROM local_save_slots WHERE id = ?1",
            params![slot_id],
        )
        .expect("active single quest is created");

    let result = database.commit_transfer_binding_download(
        BINDING_ID,
        0,
        slot_id,
        r#"{"value":2}"#,
        TARGET_ETAG,
    );
    assert!(matches!(
        result,
        Err(TransferBindingStoreError::LocalSaveBusy)
    ));
    assert_eq!(read_local_save_state(&database.connection, slot_id), before);
    let active_quest_count = database
        .connection
        .query_row("SELECT COUNT(*) FROM active_single_quests", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("active single quest remains readable");
    assert_eq!(active_quest_count, 1);
}
// //// /拒绝战斗中的存档覆盖 ////

// //// 拒绝过期冲突解决覆盖本地存档 [@x380kkm 2026-08-03] ////
#[test]
fn stale_remote_resolution_keeps_local_save_and_conflict_open() {
    let (_root, mut database, slot_id, initial_etag) = create_transfer_binding_fixture();
    database
        .connection
        .execute(
            "INSERT INTO transfer_conflicts (
                 id, binding_id, source_revision_id, source_etag,
                 target_revision_id, target_etag, detected_at, status, resolved_at
             ) VALUES (?1, ?2, 'dddddddddddddddddddddddddddddddd', ?3,
                       'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee', ?4,
                       '2026-08-03T00:00:00.000Z', 'open', NULL)",
            params![CONFLICT_ID, BINDING_ID, initial_etag, TARGET_ETAG],
        )
        .expect("open conflict is created");
    let before = read_local_save_state(&database.connection, slot_id);
    database
        .connection
        .execute(
            "UPDATE transfer_bindings SET revision = revision + 1 WHERE id = ?1",
            params![BINDING_ID],
        )
        .expect("binding revision changes");

    let result = database.resolve_transfer_conflict_with_download(
        BINDING_ID,
        CONFLICT_ID,
        0,
        slot_id,
        &initial_etag,
        r#"{"value":2}"#,
        TARGET_ETAG,
    );
    assert!(matches!(
        result,
        Err(TransferBindingStoreError::StaleBinding)
    ));
    assert_eq!(read_local_save_state(&database.connection, slot_id), before);
    let conflict_status = database
        .connection
        .query_row(
            "SELECT status FROM transfer_conflicts WHERE id = ?1",
            params![CONFLICT_ID],
            |row| row.get::<_, String>(0),
        )
        .expect("conflict remains readable");
    assert_eq!(conflict_status, "open");
}
// //// /拒绝过期冲突解决覆盖本地存档 ////

// //// 拒绝远端覆盖冲突后变化的本地存档 [@x380kkm 2026-08-03] ////
#[test]
fn remote_resolution_keeps_local_changes_after_conflict() {
    let (_root, mut database, slot_id, initial_etag) = create_transfer_binding_fixture();
    database
        .connection
        .execute(
            "INSERT INTO transfer_conflicts (
                 id, binding_id, source_revision_id, source_etag,
                 target_revision_id, target_etag, detected_at, status, resolved_at
             ) VALUES (?1, ?2, 'dddddddddddddddddddddddddddddddd', ?3,
                       'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee', ?4,
                       '2026-08-03T00:00:00.000Z', 'open', NULL)",
            params![CONFLICT_ID, BINDING_ID, initial_etag, TARGET_ETAG],
        )
        .expect("open conflict is created");
    database
        .connection
        .execute(
            "UPDATE player_snapshots
             SET data_json = '{\"value\":3}'
             WHERE account_id = (
                 SELECT account_id FROM local_save_slots WHERE id = ?1
             )",
            params![slot_id],
        )
        .expect("local save advances after conflict detection");
    let before = read_local_save_state(&database.connection, slot_id);

    let result = database.resolve_transfer_conflict_with_download(
        BINDING_ID,
        CONFLICT_ID,
        0,
        slot_id,
        &initial_etag,
        r#"{"value":2}"#,
        TARGET_ETAG,
    );
    assert!(matches!(
        result,
        Err(TransferBindingStoreError::ConflictChanged)
    ));
    assert_eq!(read_local_save_state(&database.connection, slot_id), before);
    let conflict_status = database
        .connection
        .query_row(
            "SELECT status FROM transfer_conflicts WHERE id = ?1",
            params![CONFLICT_ID],
            |row| row.get::<_, String>(0),
        )
        .expect("conflict remains readable");
    assert_eq!(conflict_status, "open");
}
// //// /拒绝远端覆盖冲突后变化的本地存档 ////

// //// 创建绑定下载测试数据并读取本地存档状态 [@x380kkm 2026-08-03] ////
fn create_transfer_binding_fixture() -> (TempDir, ServiceDatabase, i64, String) {
    let root = tempdir().expect("temporary database directory is created");
    let mut database = ServiceDatabase::open(root.path()).expect("database opens");
    database
        .connection
        .execute_batch(
            r#"INSERT INTO accounts (
                 app_id, first_login_time, idp_alias, idp_code, idp_id,
                 reg_time, last_login_time, status
             ) VALUES (
                 'wf_cn', '2026-08-03T00:00:00.000Z', 'atomic-transfer',
                 'local', 'atomic-transfer', '2026-08-03T00:00:00.000Z',
                 '2026-08-03T00:00:00.000Z', 'normal'
             );
             INSERT INTO players (account_id) VALUES (last_insert_rowid());
             INSERT INTO player_snapshots (account_id, data_json)
             SELECT id, '{"value":1}' FROM accounts WHERE idp_id = 'atomic-transfer';
             INSERT INTO local_save_slots (account_id, name, created_at, updated_at)
             SELECT id, 'Atomic transfer', '2026-08-03T00:00:00.000Z',
                    '2026-08-03T00:00:00.000Z'
             FROM accounts WHERE idp_id = 'atomic-transfer';
             INSERT INTO server_profiles (
                 name, mode, scheme, host, port, is_builtin, created_at, updated_at
             ) VALUES (
                 'Atomic transfer target', 'remote', 'http', '127.0.0.1', 1, 0,
                 '2026-08-03T00:00:00.000Z', '2026-08-03T00:00:00.000Z'
             );"#,
        )
        .expect("local slot and remote profile are created");
    let slot_id = database
        .connection
        .query_row(
            "SELECT id FROM local_save_slots WHERE name = 'Atomic transfer'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("local slot is readable");
    if database
        .ensure_local_save_revision(slot_id, "Initial")
        .is_err()
    {
        panic!("initial revision is created");
    }
    let initial_etag = database
        .connection
        .query_row(
            "SELECT revisions.payload_sha256
             FROM local_save_heads AS heads
             JOIN local_save_revisions AS revisions ON revisions.id = heads.revision_id
             WHERE heads.slot_id = ?1",
            params![slot_id],
            |row| row.get::<_, String>(0),
        )
        .expect("initial revision ETag is readable");
    database
        .connection
        .execute(
            "INSERT INTO transfer_bindings (
                 id, source_slot_id, target_profile_id, target_instance_kind,
                 target_instance_id, target_shell_id, target_slot_id, target_token,
                 upload_mode, pull_mode, conflict_policy, interval_seconds, enabled,
                 last_common_etag, last_source_etag, last_target_etag,
                 pending_direction, next_run_at, last_synced_at, last_error,
                 revision, created_at, updated_at
             ) SELECT
                 ?1, ?2, id, 'remote', 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
                 'target-shell', 1, 'target-token', 'manual', 'manual', 'ask',
                 900, 1, NULL, ?3, ?3, 'none', '2026-08-03T00:15:00.000Z',
                 NULL, NULL, 0, '2026-08-03T00:00:00.000Z',
                 '2026-08-03T00:00:00.000Z'
             FROM server_profiles WHERE name = 'Atomic transfer target'",
            params![BINDING_ID, slot_id, initial_etag],
        )
        .expect("transfer binding is created");
    (root, database, slot_id, initial_etag)
}

fn read_local_save_state(connection: &Connection, slot_id: i64) -> (String, String, i64) {
    connection
        .query_row(
            "SELECT snapshots.data_json, heads.revision_id,
                    (SELECT COUNT(*) FROM local_save_revisions WHERE slot_id = slots.id)
             FROM local_save_slots AS slots
             JOIN player_snapshots AS snapshots ON snapshots.account_id = slots.account_id
             JOIN local_save_heads AS heads ON heads.slot_id = slots.id
             WHERE slots.id = ?1",
            params![slot_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("local save state remains readable")
}
// //// /创建绑定下载测试数据并读取本地存档状态 ////
