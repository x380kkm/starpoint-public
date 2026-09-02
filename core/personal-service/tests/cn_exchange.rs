// audience: internal
// # personal-service-cn-exchange-tests
//
// 该文件验证 CN 星屑兑换读取正式目录并校验玩家余额.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::Value;
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

#[derive(Serialize)]
struct ExchangeRequest {
    viewer_id: i64,
    exchange_id: i64,
    api_count: i64,
}

#[derive(Serialize)]
struct BondTokenExchangeRequest {
    viewer_id: i64,
    equipment_id: i64,
}

// //// 验证 CN 星屑兑换使用正式费用并拒绝不足余额 [@x380kkm 2026-08-22] ////
#[test]
fn rejects_exchange_when_star_crumb_is_insufficient() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = cn_support::decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 73 }),
    ));
    let response = cn_support::send_request(
        service.port(),
        "/api/index.php/exchange/star_crumb",
        &encode_request(&ExchangeRequest {
            viewer_id: signup.data_headers.viewer_id,
            exchange_id: 1,
            api_count: 1,
        }),
    );
    assert!(response.ends_with("{\"error\":\"not_enough_star_crumb\"}"));
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 星屑兑换使用正式费用并拒绝不足余额 ////

// //// 验证羁绊证兑换目录和装备状态持久化 [@x380kkm 2026-08-29] ////
#[test]
fn persists_bond_token_exchange() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 133 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;

    let list = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/exchange/get_bond_token_exchange_list",
        &encode_request(&serde_json::json!({"viewer_id": viewer_id})),
    ));
    assert_eq!(
        list.data,
        serde_json::json!([
            {"equipment_id": 5010005, "exchange_count": 0},
            {"equipment_id": 5030005, "exchange_count": 0}
        ])
    );

    {
        let database = Connection::open(root.path().join("personal-service.sqlite3"))
            .expect("service database is opened");
        let (account_id, serialized) = database
            .query_row(
                "SELECT sessions.account_id, player_snapshots.data_json
                 FROM sessions
                 JOIN player_snapshots ON player_snapshots.account_id = sessions.account_id
                 WHERE sessions.token = ?1",
                params![viewer_id.to_string()],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .expect("player snapshot is read");
        let mut player_data: Value =
            serde_json::from_str(&serialized).expect("player snapshot is JSON");
        player_data["user_info"]["bond_token"] = Value::from(50);
        database
            .execute(
                "UPDATE player_snapshots SET data_json = ?1 WHERE account_id = ?2",
                params![
                    serde_json::to_string(&player_data).expect("player snapshot is encoded"),
                    account_id,
                ],
            )
            .expect("player snapshot is updated");
    }

    let exchange = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/exchange/bond_token",
        &encode_request(&BondTokenExchangeRequest {
            viewer_id,
            equipment_id: 5010005,
        }),
    ));
    assert_eq!(exchange.data["user_info"]["bond_token"], 0);
    assert_eq!(exchange.data["equipment_list"][0]["equipment_id"], 5010005);
    assert_eq!(exchange.data["equipment_list"][0]["stack"], 0);

    let list_after_exchange = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/exchange/get_bond_token_exchange_list",
        &encode_request(&serde_json::json!({"viewer_id": viewer_id})),
    ));
    assert_eq!(
        list_after_exchange.data,
        serde_json::json!([
            {"equipment_id": 5010005, "exchange_count": 1},
            {"equipment_id": 5030005, "exchange_count": 0}
        ])
    );

    let second_exchange = cn_support::send_request(
        service.port(),
        "/api/index.php/exchange/bond_token",
        &encode_request(&BondTokenExchangeRequest {
            viewer_id,
            equipment_id: 5010005,
        }),
    );
    assert!(second_exchange.ends_with("{\"error\":\"shop_out_of_stock\"}"));
    service.stop().expect("service stops cleanly");
}
// //// /验证羁绊证兑换目录和装备状态持久化 ////
