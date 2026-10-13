// audience: internal
// # transfer-binding-safety
// HTTP 验证覆盖调度方向, 冲突策略和在途战斗的存档保护.

use super::fixture::{occupy_source, Fixture, Occupancy};
use super::*;
use local_save_support::assert_status;

// //// 按冲突策略执行允许方向并保留人工冲突 [@x380kkm 2026-09-26] ////
#[test]
fn interval_conflicts_follow_policy_and_direction() {
    for (policy, upload, pull, expected) in [
        ("local_wins", "interval", "manual", "uploaded"),
        ("remote_wins", "manual", "interval", "downloaded"),
        ("ask", "interval", "interval", "conflict"),
        ("local_wins", "manual", "interval", "deferred"),
        ("remote_wins", "interval", "manual", "deferred"),
    ] {
        let fixture = Fixture::new(policy, upload, pull);
        fixture.advance_source();
        fixture.advance_target();
        let source_before = fixture.source_export();
        let target_before = fixture.target_export();
        let state = fixture.run_interval();
        let source_after = fixture.source_export();
        let target_after = fixture.target_export();
        match expected {
            "uploaded" => {
                assert_eq!(source_after["data"], source_before["data"]);
                assert_eq!(target_after["data"], source_before["data"]);
                assert_eq!(state["last_common_etag"], source_before["payloadSha256"]);
                assert_eq!(state["last_error"], Value::Null);
            }
            "downloaded" => {
                assert_eq!(source_after["data"], target_before["data"]);
                assert_eq!(target_after["data"], target_before["data"]);
                assert_eq!(state["last_common_etag"], target_before["payloadSha256"]);
                assert_eq!(state["last_error"], Value::Null);
            }
            _ => {
                assert_eq!(source_after["data"], source_before["data"]);
                assert_eq!(target_after["data"], target_before["data"]);
                if expected == "conflict" {
                    assert_eq!(state["pending_direction"], "conflict");
                    assert_eq!(state["last_error"], "transfer_conflict");
                } else {
                    assert_eq!(state["last_error"], "transfer_direction_not_scheduled");
                }
            }
        }
        fixture.stop();
    }
}
// //// /按冲突策略执行允许方向并保留人工冲突 ////

// //// 保护活动战斗的授权上传和定时下载 [@x380kkm 2026-09-26] ////
#[test]
fn busy_saves_block_authorized_uploads_and_interval_pulls() {
    for occupancy in [Occupancy::SingleQuest, Occupancy::Host, Occupancy::Member] {
        let fixture = Fixture::new("ask", "manual", "interval");
        fixture.advance_target();
        occupy_source(&fixture, occupancy);
        let source_before = fixture.source_export();
        let target = fixture.target_export();
        let access = issue_target_slot_token(&fixture.source, fixture.source_slot_id);
        let authorization = format!("Bearer {}", access.token);
        let etag = format!("\"{}\"", source_before["payloadSha256"].as_str().unwrap());
        let upload = support::request_with_headers(
            fixture.source.port(),
            "PUT",
            &format!("/v1/transfer/v1/slots/{}", fixture.source_slot_id),
            "application/json",
            &[("Authorization", &authorization), ("If-Match", &etag)],
            target.to_string().as_bytes(),
        );
        assert_status(&upload, "409 Conflict");
        assert_eq!(response_body(&upload)["error"], "local_save_import_blocked");
        let state = fixture.run_interval();
        assert_eq!(state["last_error"], "local_save_import_blocked");
        assert_eq!(fixture.source_export()["data"], source_before["data"]);
        let activity_count: i64 = fixture
            .source_database()
            .query_row(
                "SELECT (SELECT COUNT(*) FROM active_single_quests)
                 + (SELECT COUNT(*) FROM multiplayer_rooms)
                 + (SELECT COUNT(*) FROM multiplayer_room_members)",
                [],
                |row| row.get(0),
            )
            .expect("battle state remains readable");
        assert!(activity_count > 0, "{occupancy:?} remains active");
        fixture.stop();
    }
}
// //// /保护活动战斗的授权上传和定时下载 ////

// //// 保护冲突解决期间仍在救援中的存档 [@x380kkm 2026-09-26] ////
#[test]
fn remote_conflict_resolution_keeps_active_room_save() {
    let fixture = Fixture::new("ask", "manual", "manual");
    fixture.advance_source();
    fixture.advance_target();
    let conflict = authorized_request(
        &fixture.source,
        "POST",
        &format!("{}/sync", fixture.binding_path),
        None,
    );
    assert_status(&conflict, "409 Conflict");
    let conflict_id = response_body(&conflict)["conflict"]["conflict_id"]
        .as_str()
        .unwrap()
        .to_owned();
    occupy_source(&fixture, Occupancy::Member);
    let before = fixture.source_export();
    let resolution = authorized_request(
        &fixture.source,
        "POST",
        &format!("{}/conflicts/{conflict_id}/resolve", fixture.binding_path),
        Some(&json!({ "resolution": "remote_wins" })),
    );
    assert_status(&resolution, "409 Conflict");
    assert_eq!(
        response_body(&resolution)["error"],
        "local_save_import_blocked"
    );
    assert_eq!(fixture.source_export()["data"], before["data"]);
    let conflicts = authorized_request(
        &fixture.source,
        "GET",
        &format!("{}/conflicts", fixture.binding_path),
        None,
    );
    assert_eq!(response_body(&conflicts)[0]["status"], "open");
    fixture
        .source_database()
        .execute_batch(
            "DELETE FROM multiplayer_room_members WHERE room_number = '999991';
         DELETE FROM multiplayer_rooms WHERE room_number = '999991';",
        )
        .expect("isolated room closes");
    let resolution = authorized_request(
        &fixture.source,
        "POST",
        &format!("{}/conflicts/{conflict_id}/resolve", fixture.binding_path),
        Some(&json!({ "resolution": "remote_wins" })),
    );
    assert_status(&resolution, "200 OK");
    assert_eq!(
        fixture.source_export()["data"],
        fixture.target_export()["data"]
    );
    assert_eq!(
        response_body(&resolution)["conflict"]["status"],
        "resolved_remote_wins"
    );
    fixture.stop();
}
// //// /保护冲突解决期间仍在救援中的存档 ////
