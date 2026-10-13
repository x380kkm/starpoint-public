// audience: internal
// # personal-service-regional-economy-tests
// 隔离实例验证日服原期的战斗货币到账, 商品兑换及改号活动的共享库存.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

struct BattleExchange {
    at: &'static str,
    category: i64,
    quest_id: i64,
    event_type: i64,
    event_id: i64,
    shop_item_id: i64,
    currency_id: i64,
    reward_id: i64,
}

// //// 发送实际游戏接口并保留失败路径 [@x380kkm 2026-09-29] ////
fn send(service: &PersonalService, route: &str, body: Value) -> Value {
    let response = cn_support::send_request(
        service.port(),
        &format!("/api/index.php/{route}"),
        &encode_request(&body),
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{route}: {response}");
    let decoded = decode_response::<Value>(&response);
    assert_eq!(decoded.data_headers.result_code, 1, "{route}: {response}");
    decoded.data
}

fn signup(service: &PersonalService) -> i64 {
    let response = cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 9294701 }),
    );
    decode_response::<SignupData>(&response)
        .data_headers
        .viewer_id
}

fn set_time(service: &PersonalService, iso: &str) {
    let token = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        "PUT",
        "/v1/time",
        "application/json",
        &[("Authorization", token.as_str())],
        json!({"enabled":true,"iso":iso,"rate":1})
            .to_string()
            .as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

fn event_sales(
    service: &PersonalService,
    viewer: i64,
    event_type: i64,
    event_id: i64,
) -> Vec<Value> {
    send(
        service,
        "shop/get_sales_list",
        json!({
            "viewer_id":viewer,"shop_types":[4],"boss_coin_shop_category_ids":[],
            "equipment_enhancement_shop_category_ids":[],"browse_treasure_flag":false,
            "event_list":[{"event_type":event_type,"event_ids":[event_id]}]
        }),
    )["sales_list"]
        .as_array()
        .unwrap()
        .clone()
}

fn product(sales: &[Value], item_id: i64) -> &Value {
    sales
        .iter()
        .find(|item| item["shop_item_id"] == item_id)
        .unwrap_or_else(|| {
            let listed = sales
                .iter()
                .map(|item| &item["shop_item_id"])
                .collect::<Vec<_>>();
            panic!("requested shop item {item_id} is absent: {listed:?}")
        })
}

fn snapshot(database: &Connection) -> Value {
    let stored: String = database
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    serde_json::from_str(&stored).unwrap()
}

fn quantity(player: &Value, item_id: i64) -> i64 {
    player["item_list"][item_id.to_string()]
        .as_i64()
        .unwrap_or_default()
}

fn buy(service: &PersonalService, viewer: i64, item_id: i64, number: i64) -> Value {
    send(
        service,
        "shop/buy",
        json!({"viewer_id":viewer,"shop_type":4,"shop_item_id":item_id,"number":number}),
    )
}

// //// 用一次真实战斗获得货币并兑换同活动商品 [@x380kkm 2026-09-29] ////
fn complete_battle_and_exchange(scenario: BattleExchange) {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    set_time(&service, scenario.at);
    let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let initial = snapshot(&database);
    assert_eq!(quantity(&initial, scenario.currency_id), 0);
    let start = json!({
        "viewer_id":viewer,"category":scenario.category,"quest_id":scenario.quest_id,
        "party_id":1,"play_id":"regional-earned-currency","is_auto_start_mode":false,
        "use_boss_boost_point":false,"use_boost_point":false,"api_count":1
    });
    send(&service, "single_battle_quest/start", start);
    let finish = json!({
        "viewer_id":viewer,"category":scenario.category,"quest_id":scenario.quest_id,
        "play_id":"regional-earned-currency","api_count":2,"is_restored":false,
        "continue_count":0,"elapsed_time_ms":30000,"score":1000,"add_mana":0,"is_accomplished":true,
        "statistics":{"clear_phase":1,"zones":[],"party":{
            "characters":[{"id":1},null,null],"unison_characters":[null,null,null],
            "equipments":[null,null,null],"ability_soul_ids":[null,null,null]
        }}
    });
    let settled = send(&service, "single_battle_quest/finish", finish.clone());
    let after_battle = snapshot(&database);
    let earned = quantity(&after_battle, scenario.currency_id);
    assert!(
        earned >= 2,
        "battle must supply the two exchanges: {settled}"
    );
    assert_eq!(quantity(&settled, scenario.currency_id), earned);
    assert_eq!(
        send(&service, "single_battle_quest/finish", finish),
        settled
    );
    assert_eq!(snapshot(&database), after_battle);

    let sale = event_sales(&service, viewer, scenario.event_type, scenario.event_id);
    assert_eq!(
        product(&sale, scenario.shop_item_id)["total_purchase_num"],
        0
    );
    let bought = buy(&service, viewer, scenario.shop_item_id, 2);
    assert_eq!(quantity(&bought, scenario.currency_id), earned - 2);
    assert_eq!(
        quantity(&bought, scenario.reward_id),
        quantity(&after_battle, scenario.reward_id) + 2
    );
    let saved = snapshot(&database);
    assert_eq!(quantity(&saved, scenario.currency_id), earned - 2);
    assert_eq!(
        quantity(&saved, scenario.reward_id),
        quantity(&after_battle, scenario.reward_id) + 2
    );
    assert_eq!(
        saved["shop_purchase_counts"][format!("4:{}", scenario.shop_item_id)],
        2
    );
    let sale = event_sales(&service, viewer, scenario.event_type, scenario.event_id);
    assert_eq!(
        product(&sale, scenario.shop_item_id)["total_purchase_num"],
        2
    );
    service.stop().unwrap();
}

// //// 原期黑雷讨伐的银币直接兑换素材 [@x380kkm 2026-09-29] ////
#[test]
fn black_thunder_original_battle_currency_buys_shop_materials() {
    complete_battle_and_exchange(BattleExchange {
        at: "2019-12-26T04:00:00Z",
        category: 7,
        quest_id: 1002,
        event_type: 0,
        event_id: 1,
        shop_item_id: 203016,
        currency_id: 40210,
        reward_id: 9,
    });
}

// //// 原期圣夜关卡的货币直接兑换素材 [@x380kkm 2026-09-29] ////
#[test]
fn christmas_original_battle_currency_buys_shop_materials() {
    complete_battle_and_exchange(BattleExchange {
        at: "2019-12-26T04:00:00Z",
        category: 10,
        quest_id: 100001002,
        event_type: 2,
        event_id: 100001,
        shop_item_id: 310013,
        currency_id: 50002,
        reward_id: 5,
    });
}

// //// 复刻光荒龙战斗使用本期银币兑换素材 [@x380kkm 2026-09-29] ////
#[test]
fn light_dragon_rerun_battle_currency_buys_its_own_shop_materials() {
    complete_battle_and_exchange(BattleExchange {
        at: "2022-11-25T04:00:00Z",
        category: 7,
        quest_id: 3001002,
        event_type: 0,
        event_id: 3001,
        shop_item_id: 2214015,
        currency_id: 999803,
        reward_id: 46,
    });
}

// //// 收集活动原编号与共用商品目录保留同一购买次数 [@x380kkm 2026-09-29] ////
#[test]
fn collection_original_id_shares_shop_currency_and_purchase_counts() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    set_time(&service, "2020-12-24T04:00:00Z");
    let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let original = event_sales(&service, viewer, 9, 5);
    let canonical = event_sales(&service, viewer, 9, 80004);
    assert_eq!(original.len(), 41);
    assert_eq!(original, canonical);
    assert_eq!(product(&original, 802010)["total_purchase_num"], 0);
    let before = snapshot(&database);
    database
        .execute(
            "UPDATE player_snapshots SET data_json=json_set(data_json,'$.item_list.\"80102\"',100)",
            [],
        )
        .unwrap();
    let bought = buy(&service, viewer, 802010, 3);
    assert_eq!(quantity(&bought, 80102), 70);
    assert_eq!(quantity(&bought, 1), quantity(&before, 1) + 3);
    assert_eq!(snapshot(&database)["shop_purchase_counts"]["4:802010"], 3);
    for event_id in [5, 80004] {
        let sale = event_sales(&service, viewer, 9, event_id);
        assert_eq!(product(&sale, 802010)["total_purchase_num"], 3);
    }
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    let sale = event_sales(&service, viewer, 9, 5);
    assert_eq!(product(&sale, 802010)["total_purchase_num"], 3);
    let bought = buy(&service, viewer, 802010, 2);
    assert_eq!(quantity(&bought, 80102), 50);
    assert_eq!(quantity(&bought, 1), quantity(&before, 1) + 5);
    assert_eq!(snapshot(&database)["shop_purchase_counts"]["4:802010"], 5);
    service.stop().unwrap();
}
