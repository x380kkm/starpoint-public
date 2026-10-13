// audience: internal
// # personal-service-cn-gacha-replay-tests
// 抽取通过真实 HTTP 入口执行, 存档与领取记录来自隔离测试数据库.

use super::*;
use rusqlite::Connection;
use std::path::Path;

struct DrawCase {
    gacha_id: i64,
    payment_type: i64,
    draw_type: i64,
    executions: i64,
    pulls: i64,
    free_cost: i64,
    paid_cost: i64,
    ticket_id: Option<i64>,
    equipment: bool,
}

// //// 读取持久化资产、任务进度和领取数量 [@x380kkm 2026-09-21] ////
pub(super) fn stored_state(root: &Path) -> (Value, i64) {
    let database = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    let data: String = database
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    let history = database
        .query_row("SELECT COUNT(*) FROM player_receive_history", [], |row| {
            row.get(0)
        })
        .unwrap();
    (serde_json::from_str(&data).unwrap(), history)
}
// //// /读取持久化资产、任务进度和领取数量 ////

// //// 开放抽取次数和活动抽取对应的任务 [@x380kkm 2026-09-21] ////
fn open_draw_missions(service: &PersonalService) {
    let bearer = format!("Bearer {}", service.management_token());
    for event_id in [3, 150] {
        let response = request_with_headers(
            service.port(),
            "PUT",
            &format!("/v1/activities/calendar/active-mission%3A{event_id}"),
            "application/json",
            &[("Authorization", &bearer)],
            br#"{"enabled":true,"start_at_ms":0,"end_at_ms":253402300799000}"#,
        );
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    }
}
// //// /开放抽取次数和活动抽取对应的任务 ////

fn mission_progress(player: &Value, mission_id: i64) -> i64 {
    player["all_active_mission_list"][mission_id.to_string()]["progress"]
        .as_i64()
        .unwrap_or_default()
}

fn execute(service: &PersonalService, request: &Value) -> String {
    cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(request),
    )
}

// //// 验证各支付分支重放原结果且资产与任务仅结算一次 [@x380kkm 2026-09-21] ////
#[test]
fn replays_all_payment_results_before_availability_checks_and_after_restart() {
    let cases = [
        (1, 1, 1, 1, 1, 150, 0, None, false),
        (1, 1, 2, 1, 10, 1500, 0, None, false),
        (5009, 1, 1, 1, 1, 75, 0, None, true),
        (5009, 1, 2, 1, 10, 750, 0, None, true),
        (1, 2, 5, 1, 1, 0, 50, None, false),
        (5009, 2, 5, 1, 1, 0, 25, None, true),
        (800_000, 2, 7, 1, 10, 0, 1500, None, false),
        (2, 3, 3, 1, 1, 0, 0, Some(20003), false),
        (9, 3, 4, 1, 10, 0, 0, Some(20015), false),
        (57, 3, 3, 1, 1, 0, 0, Some(20065), false),
        (57, 3, 4, 1, 10, 0, 0, Some(20064), false),
        (80_000, 3, 10, 3, 3, 0, 0, Some(999003), false),
        (80_000, 3, 9, 2, 20, 0, 0, Some(999001), false),
        (80_000, 3, 20, 1, 1, 0, 0, Some(999008), false),
        (5009, 3, 12, 2, 2, 0, 0, Some(999005), true),
        (5009, 3, 13, 2, 20, 0, 0, Some(999004), true),
        (147, 4, 8, 1, 10, 0, 0, None, false),
        (46, 4, 7, 1, 1, 0, 0, None, false),
        (47, 4, 11, 1, 1, 0, 0, None, false),
    ]
    .map(
        |(
            gacha_id,
            payment_type,
            draw_type,
            executions,
            pulls,
            free_cost,
            paid_cost,
            ticket_id,
            equipment,
        )| DrawCase {
            gacha_id,
            payment_type,
            draw_type,
            executions,
            pulls,
            free_cost,
            paid_cost,
            ticket_id,
            equipment,
        },
    );
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    set_virtual_time(&service, "2022-12-01T12:00:00.000Z");
    campaigns::set_campaign_enabled(&service, 2, true);
    for case in &cases {
        open_gacha(&service, case.gacha_id);
    }
    open_draw_missions(&service);
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 701 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    grant_mail_rewards(
        &service,
        viewer_id,
        json!({
            "freeVmoney": 10000,
            "vmoney": 5000,
            "itemList": {
                "20003": 1, "20015": 1, "20065": 1, "20064": 1, "999012": 1,
                "999003": 3, "999001": 2, "999008": 1, "999005": 2, "999004": 2
            }
        }),
    );
    decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));

    let mut completed = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let body = json!({
            "api_count": index + 1, "viewer_id": viewer_id,
            "gacha_id": case.gacha_id, "payment_type": case.payment_type,
            "type": case.draw_type, "number_of_exec": case.executions,
        });
        let (before, history_before) = stored_state(root.path());
        let response = execute(&service, &body);
        let result = decode_response::<Value>(&response).data;
        let (after, history_after) = stored_state(root.path());
        assert_eq!(
            result["user_info"]["free_vmoney"],
            before["user_info"]["free_vmoney"].as_i64().unwrap() - case.free_cost
        );
        assert_eq!(
            result["user_info"]["vmoney"],
            before["user_info"]["vmoney"].as_i64().unwrap() - case.paid_cost
        );
        let draws_field = if case.equipment {
            "draw_equipment"
        } else {
            "draw"
        };
        assert_eq!(
            result[draws_field].as_array().unwrap().len(),
            case.pulls as usize
        );
        assert_eq!(history_after - history_before, case.pulls);
        assert_eq!(
            mission_progress(&after, 25001) - mission_progress(&before, 25001),
            if case.equipment { 0 } else { case.pulls }
        );
        let campaign_action = result["gacha_campaign_list"]
            .as_array()
            .is_some_and(|campaigns| {
                campaigns
                    .iter()
                    .any(|campaign| campaign["campaign_id"] == 18)
            });
        assert_eq!(
            mission_progress(&after, 21080) - mission_progress(&before, 21080),
            i64::from(campaign_action),
            "campaign mission for pool {}",
            case.gacha_id
        );
        if let Some(ticket_id) = case.ticket_id {
            assert_eq!(after["item_list"][ticket_id.to_string()], 0);
            assert_eq!(result["item_list"][ticket_id.to_string()], 0);
        }
        let mut retry = body.clone();
        retry["retry_count"] = json!(1);
        assert_eq!(
            execute(&service, &retry),
            response,
            "draw {} is replayed",
            index + 1
        );
        assert_eq!(stored_state(root.path()), (after, history_after));
        completed.push((body, response));
    }

    let settled = stored_state(root.path());
    for (field, value) in [
        ("gacha_id", 5009),
        ("payment_type", 2),
        ("type", 2),
        ("number_of_exec", 2),
    ] {
        let mut conflict = completed[0].0.clone();
        conflict[field] = json!(value);
        assert!(execute(&service, &conflict).ends_with("{\"error\":\"gacha_request_conflict\"}"));
        assert_eq!(stored_state(root.path()), settled);
    }
    for case in &cases {
        close_gacha(&service, case.gacha_id);
    }
    set_virtual_time(&service, "2030-01-02T12:00:00.000Z");
    service.stop().unwrap();
    let restarted = PersonalService::start(root.path(), 0).unwrap();
    for (body, response) in completed {
        assert_eq!(execute(&restarted, &body), response);
    }
    assert_eq!(stored_state(root.path()), settled);
    let loaded = decode_response::<Value>(&cn_support::send_request(
        restarted.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert!(loaded.data.get("gacha_request_receipts").is_none());
    restarted.stop().unwrap();
}
// //// /验证各支付分支重放原结果且资产与任务仅结算一次 ////

// //// 验证有偿余额补足、失败重试和新会话请求分别结算 [@x380kkm 2026-09-21] ////
#[test]
fn retries_failed_payment_and_reuses_counter_in_a_new_viewer_session() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    open_gacha(&service, 1);
    let signup_request = encode_request(&SignupRequest { device_id: 702 });
    let viewer_id = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &signup_request,
    ))
    .data_headers
    .viewer_id;
    let mut body = json!({"api_count":1,"viewer_id":viewer_id,"gacha_id":1,"payment_type":1,"type":2,"number_of_exec":1});
    let initial = decode_response::<Value>(&execute(&service, &body));
    assert_eq!(initial.data["user_info"]["free_vmoney"], 0);
    body["api_count"] = json!(2);
    body["type"] = json!(1);
    let before = stored_state(root.path());
    assert!(execute(&service, &body).ends_with("{\"error\":\"not_enough_vmoney\"}"));
    assert_eq!(stored_state(root.path()), before);
    grant_mail_rewards(&service, viewer_id, json!({"vmoney":400}));
    let response = execute(&service, &body);
    assert_eq!(
        decode_response::<Value>(&response).data["user_info"]["vmoney"],
        250
    );
    let settled = stored_state(root.path());
    service.stop().unwrap();
    let restarted = PersonalService::start(root.path(), 0).unwrap();
    assert_eq!(execute(&restarted, &body), response);
    assert_eq!(stored_state(root.path()), settled);
    let next_viewer = decode_response::<SignupData>(&cn_support::send_request(
        restarted.port(),
        "/api/index.php/tool/signup",
        &signup_request,
    ))
    .data_headers
    .viewer_id;
    assert_ne!(next_viewer, viewer_id);
    assert!(execute(&restarted, &body).ends_with("{\"error\":\"invalid_viewer_session\"}"));
    body["viewer_id"] = json!(next_viewer);
    let next_response = decode_response::<Value>(&execute(&restarted, &body));
    assert_eq!(next_response.data["user_info"]["vmoney"], 100);
    assert_eq!(stored_state(root.path()).1, settled.1 + 1);
    restarted.stop().unwrap();
}
// //// /验证有偿余额补足、失败重试和新会话请求分别结算 ////
