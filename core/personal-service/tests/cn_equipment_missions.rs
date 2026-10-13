// audience: internal
// # personal-service-cn-equipment-mission-api-tests
// 隔离存档通过装备接口验证副本和星铁钢消耗, 魂珠, 任务领取, 原子恢复和跨重启请求重放.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

const FIRST: i64 = 5_030_037;
const SECOND: i64 = 5_040_028;
const FOUR_STAR: i64 = 4_010_003;
const FIXED: i64 = 100_001;

// //// 构造真实奖励入账并读取装备与任务的持久化状态 [@x380kkm 2026-09-23] ////
fn send(service: &PersonalService, route: &str, body: &Value) -> String {
    cn_support::send_request(
        service.port(),
        &format!("/api/index.php/{route}"),
        &encode_request(body),
    )
}

fn data(service: &PersonalService, route: &str, body: &Value) -> Value {
    let response = send(service, route, body);
    assert!(response.starts_with("HTTP/1.1 200"), "{route}: {response}");
    decode_response::<Value>(&response).data
}

fn signup(service: &PersonalService, device: i64) -> i64 {
    decode_response::<SignupData>(&send(service, "tool/signup", &json!({"device_id":device})))
        .data_headers
        .viewer_id
}

fn manage(service: &PersonalService, method: &str, route: &str, body: Value) {
    let token = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        method,
        route,
        "application/json",
        &[("Authorization", token.as_str())],
        body.to_string().as_bytes(),
    );
    assert!(
        response.starts_with("HTTP/1.1 200") || response.starts_with("HTTP/1.1 201"),
        "{response}"
    );
}

fn grant(service: &PersonalService, viewer: i64, equipment: Value, items: Value) {
    manage(
        service,
        "POST",
        "/v1/mails",
        json!({
            "viewer_id":viewer,"title":"装备奖励","body":"装备与强化材料.","sender":"Starpoint",
            "rewards":{"equipmentList":equipment,"itemList":items},
        }),
    );
    data(service, "mail/receive_all", &json!({"viewer_id":viewer}));
}

fn prepare(service: &PersonalService, device: i64, equipment: Value) -> i64 {
    let viewer = signup(service, device);
    manage(
        service,
        "PUT",
        "/v1/time",
        json!({"enabled":true,"iso":"2026-09-23T12:00:00.000Z","rate":1}),
    );
    load(service, viewer);
    grant(
        service,
        viewer,
        equipment,
        json!({"100000":1000,"12001":10,"12002":10}),
    );
    viewer
}

fn load(service: &PersonalService, viewer: i64) -> Value {
    data(
        service,
        "load",
        &json!({"viewer_id":viewer,"keychain":viewer}),
    )
}

fn database(root: &Path) -> Connection {
    Connection::open(root.join("personal-service.sqlite3")).unwrap()
}

fn state(root: &Path) -> Value {
    let db = database(root);
    let snapshot: String = db
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    let mut rows = db.prepare("SELECT category,mission_id,value FROM player_mission_progress ORDER BY category,mission_id").unwrap();
    let progress = rows
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let mut rows = db
        .prepare("SELECT pattern,value FROM player_mission_counters ORDER BY pattern")
        .unwrap();
    let counters = rows
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let history: i64 = db
        .query_row("SELECT COUNT(*) FROM player_receive_history", [], |row| {
            row.get(0)
        })
        .unwrap();
    json!({"player":serde_json::from_str::<Value>(&snapshot).unwrap(),"progress":progress,"counters":counters,"history":history})
}

fn upgrade(viewer: i64, equipment: i64, count: i64, api_count: i64) -> Value {
    json!({"viewer_id":viewer,"equipment_id":equipment,"upgrade_count":count,"use_stack":true,"api_count":api_count})
}

fn item(player: &Value, id: i64) -> i64 {
    player["item_list"][id.to_string()]
        .as_i64()
        .unwrap_or_default()
}

fn level(player: &Value, id: i64) -> i64 {
    player["user_equipment_list"][id.to_string()]["level"]
        .as_i64()
        .unwrap()
}

fn stack(player: &Value, id: i64) -> i64 {
    player["user_equipment_list"][id.to_string()]["stack"]
        .as_i64()
        .unwrap()
}

fn progress(service: &PersonalService, viewer: i64, mission: i64) -> i64 {
    let response = data(
        service,
        "mission/get_mission_progress",
        &json!({"viewer_id":viewer,"api_count":1,"category_list":[{"category":1}]}),
    );
    response["mission_progress_list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["mission_id"] == mission)
        .unwrap()["progress_value"]
        .as_i64()
        .unwrap()
}

fn has_reward(response: &Value, mission: i64) -> bool {
    response["mission_info"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["mission_category_id"] == 1 && row["mission_id"] == mission)
}
// //// /构造真实奖励入账并读取装备与任务的持久化状态 ////

// //// 单件觉醒按请求计数连续结算并跨会话保存材料和任务奖励 [@x380kkm 2026-09-23] ////
#[test]
fn upgrades_with_stack_and_crystal_and_replays_after_restart() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = prepare(
        &service,
        923401,
        json!({FIRST.to_string():5,FOUR_STAR.to_string():3}),
    );
    let initial = state(root.path())["player"].clone();
    let request = upgrade(viewer, FIRST, 1, 10);
    let first_raw = send(&service, "equipment/upgrade", &request);
    let first = decode_response::<Value>(&first_raw).data;
    assert!(has_reward(&first, 67));
    let after_first = state(root.path());
    assert_eq!(level(&after_first["player"], FIRST), 2);
    assert_eq!(stack(&after_first["player"], FIRST), 3);
    assert_eq!(
        item(&after_first["player"], 100000),
        item(&initial, 100000) - 25
    );
    assert_eq!(
        item(&after_first["player"], FIRST),
        item(&initial, FIRST) + 1
    );
    assert_eq!(send(&service, "equipment/upgrade", &request), first_raw);
    assert_eq!(state(root.path()), after_first);

    data(
        &service,
        "equipment/upgrade",
        &upgrade(viewer, FIRST, 1, 11),
    );
    let mut crystal = upgrade(viewer, FIRST, 2, 12);
    crystal["use_stack"] = json!(false);
    crystal["item_id"] = json!(12002);
    let full = data(&service, "equipment/upgrade", &crystal);
    assert!(has_reward(&full, 68));
    let complete = state(root.path());
    assert_eq!(level(&complete["player"], FIRST), 5);
    assert_eq!(stack(&complete["player"], FIRST), 2);
    assert_eq!(
        item(&complete["player"], 100000),
        item(&initial, 100000) - 100
    );
    assert_eq!(item(&complete["player"], 12002), item(&initial, 12002) - 2);
    assert_eq!(item(&complete["player"], FIRST), item(&initial, FIRST) + 4);
    assert_eq!(progress(&service, viewer, 67), 4);
    assert_eq!(progress(&service, viewer, 68), 1);
    assert_eq!(
        complete["player"]["all_active_mission_list"]["11030"]["progress"],
        4
    );
    let claim =
        json!({"viewer_id":viewer,"active_mission_list":[{"mission_id":11030,"stages":[1]}]});
    let before_claim = state(root.path());
    data(&service, "active_mission/receive", &claim);
    let rewarded = state(root.path());
    assert_eq!(
        item(&rewarded["player"], 100000),
        item(&before_claim["player"], 100000) + 150
    );
    data(&service, "active_mission/receive", &claim);
    assert_eq!(state(root.path()), rewarded);
    let mut conflict = request.clone();
    conflict["upgrade_count"] = json!(2);
    assert!(send(&service, "equipment/upgrade", &conflict).contains("equipment_request_conflict"));
    assert_eq!(state(root.path()), rewarded);
    assert!(load(&service, viewer)
        .get("equipment_request_receipts")
        .is_none());
    let loaded = state(root.path());
    service.stop().unwrap();

    let service = PersonalService::start(root.path(), 0).unwrap();
    assert_eq!(send(&service, "equipment/upgrade", &request), first_raw);
    assert_eq!(state(root.path()), loaded);
    let new_viewer = signup(&service, 923401);
    assert_ne!(new_viewer, viewer);
    data(
        &service,
        "equipment/upgrade",
        &upgrade(new_viewer, FOUR_STAR, 1, 10),
    );
    assert_eq!(level(&state(root.path())["player"], FOUR_STAR), 2);
    service.stop().unwrap();
}
// //// /单件觉醒按请求计数连续结算并跨会话保存材料和任务奖励 ////

// //// 批量觉醒累计实际层数并以当前完全觉醒库存完成任务 [@x380kkm 2026-09-23] ////
#[test]
fn bulk_upgrades_every_eligible_copy_once_and_completes_inventory_missions() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = prepare(
        &service,
        923402,
        json!({
            FIRST.to_string():2,SECOND.to_string():3,FOUR_STAR.to_string():5,FIXED.to_string():2,
        }),
    );
    let initial = state(root.path())["player"].clone();
    let request = json!({"viewer_id":viewer,"api_count":20,"equipment_ids":[FIRST,SECOND,FIRST,FOUR_STAR,FIXED]});
    let first_raw = send(&service, "equipment/bulk_upgrade", &request);
    let response = decode_response::<Value>(&first_raw).data;
    assert_eq!(response["equipment_list"].as_array().unwrap().len(), 3);
    assert!(has_reward(&response, 67) && has_reward(&response, 68));
    let first = state(root.path())["player"].clone();
    assert_eq!(
        (
            level(&first, FIRST),
            level(&first, SECOND),
            level(&first, FOUR_STAR)
        ),
        (2, 3, 5)
    );
    assert_eq!((level(&first, FIXED), stack(&first, FIXED)), (1, 1));
    assert_eq!(item(&first, 100000), item(&initial, 100000) - 155);
    assert_eq!(
        (
            item(&first, FIRST),
            item(&first, SECOND),
            item(&first, FOUR_STAR)
        ),
        (1, 2, 4)
    );
    assert_eq!(progress(&service, viewer, 67), 7);
    grant(
        &service,
        viewer,
        json!({FIRST.to_string():3,SECOND.to_string():2}),
        json!({}),
    );
    let replenished = state(root.path());
    assert_eq!(
        send(&service, "equipment/bulk_upgrade", &request),
        first_raw
    );
    assert_eq!(state(root.path()), replenished);

    let mut next = request.clone();
    next["api_count"] = json!(21);
    let response = data(&service, "equipment/bulk_upgrade", &next);
    assert!(has_reward(&response, 67) && has_reward(&response, 68));
    let complete = state(root.path());
    assert_eq!(
        (
            level(&complete["player"], FIRST),
            level(&complete["player"], SECOND)
        ),
        (5, 5)
    );
    assert_eq!(
        item(&complete["player"], 100000),
        item(&replenished["player"], 100000) - 125
    );
    assert_eq!(progress(&service, viewer, 67), 12);
    assert_eq!(progress(&service, viewer, 68), 3);
    assert!(complete["player"]["degree_ids"]
        .as_array()
        .unwrap()
        .contains(&json!(42000)));
    assert_eq!(
        complete["player"]["all_active_mission_list"]["14050"]["progress"],
        3
    );
    let souls: i64 = database(root.path()).query_row(
        "SELECT SUM(number) FROM player_receive_history WHERE event_key LIKE 'equipment:%' AND kind=1",
        [], |row| row.get(0)).unwrap();
    assert_eq!(souls, 12);
    let before_restart = state(root.path());
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    assert_eq!(
        send(&service, "equipment/bulk_upgrade", &request),
        first_raw
    );
    assert_eq!(state(root.path()), before_restart);
    service.stop().unwrap();
}
// //// /批量觉醒累计实际层数并以当前完全觉醒库存完成任务 ////

// //// 材料和等级限制保留资产且旧无计数请求可连续执行 [@x380kkm 2026-09-23] ////
#[test]
fn validates_awakening_materials_and_preserves_requests_without_counters() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = prepare(
        &service,
        923403,
        json!({FIRST.to_string():5,FOUR_STAR.to_string():4,FIXED.to_string():2}),
    );
    let before = state(root.path());
    for item_id in [Value::Null, json!(12001), json!(100000)] {
        let mut request = upgrade(viewer, FIRST, 1, 30);
        request["use_stack"] = json!(false);
        request["item_id"] = item_id;
        assert!(send(&service, "equipment/upgrade", &request).contains("invalid_awaking_item"));
        assert_eq!(state(root.path()), before);
    }
    assert!(send(
        &service,
        "equipment/upgrade",
        &upgrade(viewer, FIXED, 1, 31)
    )
    .contains("equipment_level_limit"));
    assert!(send(
        &service,
        "equipment/upgrade",
        &upgrade(viewer, FIRST, 0, 32)
    )
    .contains("invalid_request_body"));
    assert_eq!(state(root.path()), before);
    let mut legacy = upgrade(viewer, FOUR_STAR, 1, 33);
    legacy.as_object_mut().unwrap().remove("api_count");
    data(&service, "equipment/upgrade", &legacy);
    data(&service, "equipment/upgrade", &legacy);
    let after = state(root.path())["player"].clone();
    assert_eq!(level(&after, FOUR_STAR), 3);
    assert_eq!(stack(&after, FOUR_STAR), 1);
    assert_eq!(item(&after, 100000), item(&before["player"], 100000) - 40);
    let history: i64 = database(root.path())
        .query_row(
            "SELECT COUNT(*) FROM player_receive_history WHERE event_key LIKE 'equipment:%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(history, 2);
    let mut crystal = upgrade(viewer, FOUR_STAR, 1, 34);
    crystal["use_stack"] = json!(false);
    crystal["item_id"] = json!(12001);
    data(&service, "equipment/upgrade", &crystal);
    let after_crystal = state(root.path())["player"].clone();
    assert_eq!(level(&after_crystal, FOUR_STAR), 4);
    assert_eq!(stack(&after_crystal, FOUR_STAR), 1);
    assert_eq!(item(&after_crystal, 12001), item(&after, 12001) - 1);
    assert_eq!(item(&after_crystal, 100000), item(&after, 100000) - 20);
    service.stop().unwrap();
}
// //// /材料和等级限制保留资产且旧无计数请求可连续执行 ////

// //// 最终领取历史写入失败时回滚装备和任务并允许同一请求重试 [@x380kkm 2026-09-23] ////
#[test]
fn rolls_back_single_and_bulk_upgrade_rewards_before_retry() {
    for bulk in [false, true] {
        let root = TempDir::new().unwrap();
        let service = PersonalService::start(root.path(), 0).unwrap();
        let viewer = prepare(
            &service,
            923404,
            json!({FIRST.to_string():5,SECOND.to_string():5}),
        );
        let request = if bulk {
            json!({"viewer_id":viewer,"api_count":40,"equipment_ids":[FIRST,SECOND]})
        } else {
            upgrade(viewer, FIRST, 1, 40)
        };
        let route = if bulk {
            "equipment/bulk_upgrade"
        } else {
            "equipment/upgrade"
        };
        let before = state(root.path());
        database(root.path()).execute_batch(
            "CREATE TRIGGER reject_equipment_history BEFORE INSERT ON player_receive_history
             WHEN NEW.event_key LIKE 'equipment:%' BEGIN SELECT RAISE(ABORT,'history unavailable'); END;").unwrap();
        let failed = send(&service, route, &request);
        assert!(failed.starts_with("HTTP/1.1 500"), "{failed}");
        assert_eq!(state(root.path()), before);
        database(root.path())
            .execute_batch("DROP TRIGGER reject_equipment_history;")
            .unwrap();
        let response = send(&service, route, &request);
        decode_response::<Value>(&response);
        assert_eq!(progress(&service, viewer, 67), if bulk { 8 } else { 1 });
        let committed = state(root.path());
        assert!(
            committed["player"]["equipment_request_receipts"]
                .as_object()
                .unwrap()
                .len()
                == 1
        );
        assert_eq!(send(&service, route, &request), response);
        assert_eq!(state(root.path()), committed);
        service.stop().unwrap();
    }
}
// //// /最终领取历史写入失败时回滚装备和任务并允许同一请求重试 ////
