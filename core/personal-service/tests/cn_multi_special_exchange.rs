// audience: internal
// # personal-service-cn-multi-special-exchange-tests
// 真实 HTTP 请求验证活动票券、候选角色、任务进度和管理历史, 临时数据库承载隔离存档.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

const CAMPAIGNS: [(i64, i64, i64); 3] = [
    (1, 980001, 111057),
    (2, 980004, 111063),
    (3, 980005, 111123),
];

fn start(root: &TempDir, cdn: &TempDir) -> PersonalService {
    let activities = CAMPAIGNS
        .iter()
        .map(|(id, _, _)| {
            json!({
                "activity_id":format!("multi-special-exchange:{id}"),
                "name":format!("Exchange {id}"),"kind":"multi-special-exchange",
            })
        })
        .collect::<Vec<_>>();
    std::fs::write(
        cdn.path().join("activity-catalog.json"),
        json!({"format_version":1,"activities":activities}).to_string(),
    )
    .unwrap();
    PersonalService::start_with_cdn_root(root.path(), 0, cdn.path()).unwrap()
}

fn manage(service: &PersonalService, method: &str, path: &str, body: Value) -> Value {
    let bearer = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        method,
        path,
        "application/json",
        &[("Authorization", &bearer)],
        body.to_string().as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}

fn send(service: &PersonalService, action: &str, body: &Value) -> String {
    cn_support::send_request(
        service.port(),
        &format!("/api/index.php/multi_special_exchange/{action}"),
        &encode_request(body),
    )
}

fn signup(service: &PersonalService, device_id: i64) -> i64 {
    decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id }),
    ))
    .data_headers
    .viewer_id
}

fn load(service: &PersonalService, viewer_id: i64) -> Value {
    decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&json!({"viewer_id":viewer_id,"keychain":viewer_id})),
    ))
    .data
}

fn open(service: &PersonalService, activity: &str) {
    manage(
        service,
        "PUT",
        &format!("/v1/activities/calendar/{activity}"),
        json!({"enabled":true,"start_at":"2020-01-01T00:00:00.000Z","end_at":"2099-01-01T00:00:00.000Z"}),
    );
}

fn set_time(service: &PersonalService, iso: &str) {
    manage(
        service,
        "PUT",
        "/v1/time",
        json!({"enabled":true,"iso":iso,"rate":1}),
    );
}

fn stored(root: &Path) -> (Value, i64) {
    let database = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    let player: String = database
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    let history = database
        .query_row("SELECT COUNT(*) FROM player_receive_history", [], |row| {
            row.get(0)
        })
        .unwrap();
    (serde_json::from_str(&player).unwrap(), history)
}

fn campaign(data: &Value, id: i64) -> &Value {
    data["multi_special_exchange_campaign_list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["campaign_id"] == id)
        .unwrap()
}

// //// 验证所有单人活动可兑换、重放、重置并保留历史与资产 [@x380kkm 2026-09-22] ////
#[test]
fn completes_all_campaigns_and_preserves_claims_across_reset_and_restart() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    let service = start(&root, &cdn);
    set_time(&service, "2030-01-01T12:00:00.000Z");
    open(&service, "active-mission:3");
    for (id, _, _) in CAMPAIGNS {
        open(&service, &format!("multi-special-exchange:{id}"));
    }
    let viewer = signup(&service, 751);
    let loaded = load(&service, viewer);
    let slots = manage(&service, "GET", "/v1/local-saves", json!({}));
    let slot = slots["slots"][0]["id"].as_i64().unwrap();
    let mut completed = Vec::new();
    for (id, ticket, character) in CAMPAIGNS {
        assert_eq!(campaign(&loaded, id)["status"], 1);
        let draw = json!({"viewer_id":viewer,"campaign_id":id,"api_count":id*10});
        let exchange = json!({"viewer_id":viewer,"campaign_id":id,"api_count":id*10+3,"ticket_item_id":ticket,"character_id":character});
        let first = send(&service, "single_draw_ticket", &draw);
        let received = decode_response::<Value>(&first);
        assert_eq!(received.data_headers.result_code, 1);
        assert_eq!(campaign(&received.data, id)["status"], 3);
        assert_eq!(received.data["item_list"][ticket.to_string()], 1);
        let granted = stored(root.path());
        assert_eq!(send(&service, "single_draw_ticket", &draw), first);
        assert_eq!(stored(root.path()), granted);

        let recovered = decode_response::<Value>(&send(
            &service,
            "multi_draw_ticket",
            &json!({"viewer_id":viewer,"campaign_id":id,"api_count":id*10+1}),
        ));
        assert_eq!(recovered.data["item_list"][ticket.to_string()], 1);
        assert_eq!(stored(root.path()), granted);
        let mut invalid = exchange.clone();
        invalid["character_id"] = json!(1);
        assert_eq!(
            decode_response::<Value>(&send(&service, "exchange_character", &invalid))
                .data_headers
                .result_code,
            4901
        );
        assert_eq!(stored(root.path()), granted);

        let delivered = send(&service, "exchange_character", &exchange);
        let result = decode_response::<Value>(&delivered);
        assert_eq!(result.data_headers.result_code, 1);
        assert_eq!(campaign(&result.data, id)["status"], 4);
        assert_eq!(result.data["item_list"][ticket.to_string()], 0);
        assert_eq!(result.data["character_list"][0]["character_id"], character);
        let settled = stored(root.path());
        assert_eq!(settled.1, granted.1 + 1);
        assert_eq!(settled.0["item_list"][ticket.to_string()], 0);
        assert_eq!(send(&service, "exchange_character", &exchange), delivered);
        assert_eq!(stored(root.path()), settled);
        assert!(send(&service, "exchange_character", &invalid)
            .ends_with("{\"error\":\"multi_special_exchange_request_conflict\"}"));
        if id == 2 {
            assert_eq!(settled.0["all_active_mission_list"]["21030"]["progress"], 1);
            assert!(result.data["active_mission_list"]
                .as_array()
                .unwrap()
                .iter()
                .any(|mission| mission["mission_id"] == 21030));
        }

        let path = format!("/v1/local-saves/{slot}/activities/multi-special-exchange:{id}");
        let history = manage(&service, "GET", &format!("{path}/history"), json!({}));
        assert_eq!(history["can_reset"], true);
        assert_eq!(
            history["entries"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|entry| entry["kind"] == "claim")
                .count(),
            2
        );
        let reset = manage(
            &service,
            "POST",
            &format!("{path}/reset"),
            json!({"expected_round":0}),
        );
        assert_eq!(reset["next_round"], 1);
        let after_reset = stored(root.path());
        assert_eq!(
            after_reset.0["user_character_list"],
            settled.0["user_character_list"]
        );
        assert_eq!(after_reset.0["item_list"], settled.0["item_list"]);
        assert_eq!(campaign(&load(&service, viewer), id)["status"], 1);
        let second_ticket = decode_response::<Value>(&send(&service, "single_draw_ticket", &draw));
        assert_eq!(second_ticket.data["item_list"][ticket.to_string()], 1);
        let second_exchange = send(&service, "exchange_character", &exchange);
        let duplicate = decode_response::<Value>(&second_exchange);
        assert_eq!(duplicate.data["character_list"][0]["stack"], 1);
        assert_eq!(duplicate.data["item_list"][ticket.to_string()], 0);
        let after = stored(root.path());
        for (item, amount) in duplicate.data["item_list"].as_object().unwrap() {
            assert_eq!(&after.0["item_list"][item], amount);
        }
        let history = manage(&service, "GET", &format!("{path}/history"), json!({}));
        let claims = history["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["kind"] == "claim")
            .collect::<Vec<_>>();
        assert_eq!(claims.len(), 4);
        assert_eq!(
            claims
                .iter()
                .filter(|entry| entry["participation_round"] == 0)
                .count(),
            2
        );
        assert_eq!(
            claims
                .iter()
                .filter(|entry| entry["participation_round"] == 1)
                .count(),
            2
        );
        completed.push((exchange, second_exchange));
    }
    let settled = stored(root.path());
    service.stop().unwrap();
    let restarted = PersonalService::start_with_cdn_root(root.path(), 0, cdn.path()).unwrap();
    set_time(&restarted, "2100-01-01T12:00:00.000Z");
    for (request, response) in completed {
        assert_eq!(send(&restarted, "exchange_character", &request), response);
    }
    assert_eq!(stored(root.path()), settled);
    restarted.stop().unwrap();
}
// //// /验证所有单人活动可兑换、重放、重置并保留历史与资产 ////

// //// 验证发券期限与兑换宽限期及管理关闭状态 [@x380kkm 2026-09-22] ////
#[test]
fn honors_campaign_end_and_exchange_grace_period() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    let service = start(&root, &cdn);
    set_time(&service, "2030-01-01T12:00:00.000Z");
    for (id, _, _) in CAMPAIGNS {
        manage(
            &service,
            "PUT",
            &format!("/v1/activities/calendar/multi-special-exchange:{id}"),
            json!({"enabled":true,"start_at":"2030-01-01T00:00:00.000Z","end_at":"2030-01-02T00:00:00.000Z"}),
        );
    }
    let viewer = signup(&service, 752);
    for (id, _, _) in &CAMPAIGNS[1..] {
        assert_eq!(
            decode_response::<Value>(&send(
                &service,
                "single_draw_ticket",
                &json!({"viewer_id":viewer,"campaign_id":id,"api_count":id})
            ))
            .data_headers
            .result_code,
            1
        );
    }
    set_time(&service, "2030-01-03T12:00:00.000Z");
    let before = stored(root.path());
    assert_eq!(
        decode_response::<Value>(&send(
            &service,
            "single_draw_ticket",
            &json!({"viewer_id":viewer,"campaign_id":1,"api_count":4})
        ))
        .data_headers
        .result_code,
        4901
    );
    assert_eq!(stored(root.path()), before);
    let eligible = decode_response::<Value>(&send(
        &service,
        "exchange_character",
        &json!({"viewer_id":viewer,"campaign_id":2,"api_count":5,"ticket_item_id":980004,"character_id":111063}),
    ));
    assert_eq!(eligible.data_headers.result_code, 1);
    assert_eq!(eligible.data["item_list"]["980004"], 0);
    set_time(&service, "2030-01-10T12:00:00.000Z");
    let expired = json!({"viewer_id":viewer,"campaign_id":3,"api_count":6,"ticket_item_id":980005,"character_id":111123});
    let before = stored(root.path());
    assert_eq!(
        decode_response::<Value>(&send(&service, "exchange_character", &expired))
            .data_headers
            .result_code,
        4901
    );
    assert_eq!(stored(root.path()), before);
    open(&service, "multi-special-exchange:3");
    assert_eq!(
        decode_response::<Value>(&send(&service, "exchange_character", &expired))
            .data_headers
            .result_code,
        1
    );
    manage(
        &service,
        "PUT",
        "/v1/activities/calendar/multi-special-exchange:1",
        json!({"enabled":false,"start_at":"2020-01-01T00:00:00.000Z","end_at":"2099-01-01T00:00:00.000Z"}),
    );
    assert_eq!(
        decode_response::<Value>(&send(
            &service,
            "single_draw_ticket",
            &json!({"viewer_id":viewer,"campaign_id":1,"api_count":7})
        ))
        .data_headers
        .result_code,
        4901
    );
    assert_eq!(
        decode_response::<Value>(&send(
            &service,
            "single_draw_ticket",
            &json!({"viewer_id":viewer,"campaign_id":5,"api_count":8})
        ))
        .data_headers
        .result_code,
        4901
    );
    service.stop().unwrap();
}
// //// /验证发券期限与兑换宽限期及管理关闭状态 ////

// //// 验证多人已决定的各组票券限制对应候选并仅发放一次 [@x380kkm 2026-09-22] ////
#[test]
fn recovers_selected_multiplayer_tickets_without_granting_unselected_tickets() {
    for (ticket, character) in [(980002, 111051), (980003, 111045)] {
        let root = TempDir::new().unwrap();
        let cdn = TempDir::new().unwrap();
        let service = start(&root, &cdn);
        open(&service, "multi-special-exchange:1");
        let viewer = signup(&service, ticket);
        load(&service, viewer);
        let request = json!({"viewer_id":viewer,"campaign_id":1,"api_count":1});
        assert_eq!(
            decode_response::<Value>(&send(&service, "multi_draw_ticket", &request))
                .data_headers
                .result_code,
            4901
        );
        let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
        let mut player = stored(root.path()).0;
        player["cn_activity_state"]["event_families"]["multi-special-exchange"]["1"] =
            json!({"status":2,"ticket_item_id":ticket,"legacy_state_imported":true});
        database
            .execute(
                "UPDATE player_snapshots SET data_json=?1",
                params![player.to_string()],
            )
            .unwrap();
        let first = send(&service, "multi_draw_ticket", &request);
        assert_eq!(
            decode_response::<Value>(&first).data["item_list"][ticket.to_string()],
            1
        );
        let granted = stored(root.path());
        assert_eq!(send(&service, "multi_draw_ticket", &request), first);
        assert_eq!(stored(root.path()), granted);
        let invalid = json!({"viewer_id":viewer,"campaign_id":1,"api_count":2,"ticket_item_id":ticket,"character_id":111057});
        assert_eq!(
            decode_response::<Value>(&send(&service, "exchange_character", &invalid))
                .data_headers
                .result_code,
            4901
        );
        assert_eq!(stored(root.path()), granted);
        let mut valid = invalid;
        valid["character_id"] = json!(character);
        let response = decode_response::<Value>(&send(&service, "exchange_character", &valid));
        assert_eq!(response.data_headers.result_code, 1);
        assert_eq!(response.data["item_list"][ticket.to_string()], 0);
        assert_eq!(
            response.data["character_list"][0]["character_id"],
            character
        );
        assert_eq!(
            decode_response::<Value>(&send(
                &service,
                "multi_draw_ticket",
                &json!({"viewer_id":viewer,"campaign_id":1})
            ))
            .data_headers
            .result_code,
            4902
        );
        service.stop().unwrap();
    }
}
// //// /验证多人已决定的各组票券限制对应候选并仅发放一次 ////
