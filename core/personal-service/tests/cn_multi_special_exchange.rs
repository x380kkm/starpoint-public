// audience: internal
// # personal-service-cn-multi-special-exchange-tests
//
// 该文件验证 CN 多选角色兑换的活动状态, 票券余额和重复角色 stack 持久化.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{encode_request, SignupData, SignupRequest};
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

const CAMPAIGN_ID: i64 = 5;
const TICKET_ITEM_ID: i64 = 980_007;
const CHARACTER_ID: i64 = 111_015;

#[derive(Serialize)]
struct SingleDrawTicketRequest {
    viewer_id: i64,
    campaign_id: i64,
    api_count: i64,
}

#[derive(Serialize)]
struct MultiDrawTicketRequest {
    viewer_id: i64,
    campaign_id: i64,
}

#[derive(Serialize)]
struct ExchangeCharacterRequest {
    viewer_id: i64,
    campaign_id: i64,
    character_id: i64,
    ticket_item_id: i64,
    api_count: i64,
}

// //// 验证 CN 多选角色兑换完整状态转换 [@x380kkm 2026-08-23] ////
#[test]
fn exchanges_ticket_for_new_and_duplicate_characters() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = cn_support::decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 75 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    set_exchange_state(root.path(), 2, 0);

    let ticket = cn_support::decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/multi_special_exchange/single_draw_ticket",
        &encode_request(&SingleDrawTicketRequest {
            viewer_id,
            campaign_id: CAMPAIGN_ID,
            api_count: 1,
        }),
    ));
    assert_eq!(ticket.data_headers.result_code, 1);
    assert_eq!(
        ticket.data["multi_special_exchange_campaign_list"][0]["status"],
        3
    );
    assert_eq!(ticket.data["item_list"][TICKET_ITEM_ID.to_string()], 1);
    assert_eq!(ticket.data["mail_arrived"], false);

    let repeated_ticket = cn_support::decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/multi_special_exchange/single_draw_ticket",
        &encode_request(&SingleDrawTicketRequest {
            viewer_id,
            campaign_id: CAMPAIGN_ID,
            api_count: 2,
        }),
    ));
    assert_eq!(repeated_ticket.data_headers.result_code, 4902);

    let character = exchange_character(service.port(), viewer_id, 3);
    assert_eq!(character.data_headers.result_code, 1);
    assert_eq!(
        character.data["multi_special_exchange_campaign_list"][0]["status"],
        4
    );
    assert_eq!(
        character.data["character_list"][0]["character_id"],
        CHARACTER_ID
    );
    assert!(character.data["character_list"][0]["stack"].is_null());
    assert_eq!(character.data["item_list"][TICKET_ITEM_ID.to_string()], 0);
    assert_eq!(
        character.data["encyclopedia_info"],
        json!({"111101501": {"read": false}})
    );
    assert_eq!(character.data["mail_arrived"], false);

    let repeated_character = exchange_character(service.port(), viewer_id, 4);
    assert_eq!(repeated_character.data_headers.result_code, 4902);

    set_exchange_state(root.path(), 3, 0);
    let missing_ticket = exchange_character(service.port(), viewer_id, 5);
    assert_eq!(missing_ticket.data_headers.result_code, 4901);

    set_exchange_state(root.path(), 3, 1);
    let duplicate = exchange_character(service.port(), viewer_id, 6);
    assert_eq!(duplicate.data_headers.result_code, 1);
    assert_eq!(
        duplicate.data["character_list"][0]["character_id"],
        CHARACTER_ID
    );
    assert_eq!(duplicate.data["character_list"][0]["stack"], 1);
    assert_eq!(duplicate.data["encyclopedia_info"], json!({}));
    assert_eq!(stored_character_stack(root.path()), 1);

    set_exchange_state(root.path(), 1, 0);
    let inactive_campaign = cn_support::decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/multi_special_exchange/single_draw_ticket",
        &encode_request(&SingleDrawTicketRequest {
            viewer_id,
            campaign_id: CAMPAIGN_ID,
            api_count: 7,
        }),
    ));
    assert_eq!(inactive_campaign.data_headers.result_code, 4901);
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 多选角色兑换完整状态转换 ////

// //// 验证 CN 多抽票券状态转换 [@x380kkm 2026-08-24] ////
#[test]
fn grants_multi_draw_ticket() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = cn_support::decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 76 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    set_exchange_state(root.path(), 2, 0);

    let ticket = cn_support::decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/multi_special_exchange/multi_draw_ticket",
        &encode_request(&MultiDrawTicketRequest {
            viewer_id,
            campaign_id: CAMPAIGN_ID,
        }),
    ));
    assert_eq!(ticket.data_headers.result_code, 1);
    assert_eq!(
        ticket.data["multi_special_exchange_campaign_list"][0]["status"],
        3
    );
    assert_eq!(ticket.data["item_list"][TICKET_ITEM_ID.to_string()], 1);
}
// //// /验证 CN 多抽票券状态转换 ////

fn exchange_character(port: u16, viewer_id: i64, api_count: i64) -> cn_support::Envelope<Value> {
    cn_support::decode_response::<Value>(&cn_support::send_request(
        port,
        "/api/index.php/multi_special_exchange/exchange_character",
        &encode_request(&ExchangeCharacterRequest {
            viewer_id,
            campaign_id: CAMPAIGN_ID,
            character_id: CHARACTER_ID,
            ticket_item_id: TICKET_ITEM_ID,
            api_count,
        }),
    ))
}

fn set_exchange_state(root: &Path, status: i64, ticket_count: i64) {
    let connection = Connection::open(root.join("personal-service.sqlite3"))
        .expect("personal service database opens");
    let serialized: String = connection
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .expect("player snapshot exists");
    let mut player_data = serde_json::from_str::<Value>(&serialized).expect("snapshot is JSON");
    player_data["multi_special_exchange_campaign_list"] = if status == 3 {
        json!([{
            "campaign_id": CAMPAIGN_ID,
            "status": status,
            "ticket_item_id": TICKET_ITEM_ID,
        }])
    } else {
        json!([{"campaign_id": CAMPAIGN_ID, "status": status}])
    };
    player_data["item_list"][TICKET_ITEM_ID.to_string()] = Value::from(ticket_count);
    connection
        .execute(
            "UPDATE player_snapshots SET data_json = ?1",
            params![serde_json::to_string(&player_data).expect("snapshot encodes")],
        )
        .expect("exchange state is saved");
}

fn stored_character_stack(root: &Path) -> i64 {
    let connection = Connection::open(root.join("personal-service.sqlite3"))
        .expect("personal service database opens");
    let serialized: String = connection
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .expect("player snapshot exists");
    let player_data = serde_json::from_str::<Value>(&serialized).expect("snapshot is JSON");
    player_data["user_character_list"][CHARACTER_ID.to_string()]["stack"]
        .as_i64()
        .expect("character stack is stored")
}
