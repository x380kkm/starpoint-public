// audience: internal
// # personal-service-cn-gacha-tests
//
// 该文件验证 CN 角色和装备扭蛋的开放条件, 支付, 发放和状态持久化.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, LoadRequest, SignupData, SignupRequest};
use serde::Serialize;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::collections::HashSet;
use std::sync::OnceLock;
use support::request_with_headers;
use tempfile::TempDir;

static MOVIE_SEEDS: OnceLock<Value> = OnceLock::new();
static NORMAL_MOVIE_SEEDS: OnceLock<Value> = OnceLock::new();
static FES_MOVIE_SEEDS: OnceLock<Value> = OnceLock::new();
static NORMAL_GUARANTEE_MOVIE_SEEDS: OnceLock<Value> = OnceLock::new();
static FES_GUARANTEE_MOVIE_SEEDS: OnceLock<Value> = OnceLock::new();
static CHARACTER_ASSETS: OnceLock<Value> = OnceLock::new();

#[derive(Serialize)]
struct ExecuteRequest {
    api_count: i64,
    payment_type: i64,
    number_of_exec: i64,
    viewer_id: i64,
    gacha_id: i64,
    r#type: i64,
}

#[derive(Serialize)]
struct MailReceiveAllRequest {
    viewer_id: i64,
}

fn assert_valid_movie_seeds(data: &Value) {
    let character_assets = CHARACTER_ASSETS.get_or_init(|| {
        serde_json::from_str(include_str!("../../../assets/character.json"))
            .expect("CN character assets are JSON")
    });
    let seeds = MOVIE_SEEDS.get_or_init(|| {
        serde_json::from_str(include_str!("../../../assets/gacha_movie_seeds.json"))
            .expect("CN gacha movie seeds are JSON")
    });
    for draw in data["draw"].as_array().expect("draw list is an array") {
        let character_id = draw["character_id"]
            .as_i64()
            .expect("draw character id is numeric");
        let rarity = character_assets[character_id.to_string()]["rarity"]
            .as_i64()
            .expect("character rarity is numeric");
        let rarity_key = (6 - rarity).to_string();
        let movie_id = draw["movie_id"].as_str().expect("movie id is text");
        if movie_id == "rarity_5_guarantee" {
            assert_eq!(draw["seed"], character_id * 1_000);
            continue;
        }
        let seeds = match movie_id {
            "normal" => NORMAL_MOVIE_SEEDS.get_or_init(|| {
                serde_json::from_str(include_str!(
                    "../../../assets/gacha_movie_seeds_normal.json"
                ))
                .expect("normal movie seeds are JSON")
            }),
            "fes" => FES_MOVIE_SEEDS.get_or_init(|| {
                serde_json::from_str(include_str!("../../../assets/gacha_movie_seeds_fes.json"))
                    .expect("fes movie seeds are JSON")
            }),
            "normal_guarantee" => NORMAL_GUARANTEE_MOVIE_SEEDS.get_or_init(|| {
                serde_json::from_str(include_str!(
                    "../../../assets/gacha_movie_seeds_normal_guarantee.json"
                ))
                .expect("normal guarantee movie seeds are JSON")
            }),
            "fes_guarantee" => FES_GUARANTEE_MOVIE_SEEDS.get_or_init(|| {
                serde_json::from_str(include_str!(
                    "../../../assets/gacha_movie_seeds_fes_guarantee.json"
                ))
                .expect("fes guarantee movie seeds are JSON")
            }),
            _ => seeds,
        };
        let valid_seeds = seeds[&rarity_key]["0"]
            .as_array()
            .expect("movie seed rarity exists");
        assert!(valid_seeds.iter().any(|seed| seed == &draw["seed"]));
    }
}

// //// 验证同一批重复角色保留完整状态和重复素材 [@x380kkm 2026-08-26] ////
fn has_valid_repeated_character_response(data: &Value) -> bool {
    let draws = data["draw"].as_array().expect("draw list is an array");
    let Some(duplicate_draw) = draws
        .iter()
        .find(|draw| draw.get("ex_boost_item").is_some())
    else {
        return false;
    };
    let character_id = duplicate_draw["character_id"]
        .as_i64()
        .expect("draw character id is numeric");
    let character = data["character_list"]
        .as_array()
        .expect("character list is an array")
        .iter()
        .find(|character| character["character_id"] == character_id)
        .expect("repeated character is present in character list");
    for field in [
        "entry_count",
        "exp",
        "exp_total",
        "bond_token_list",
        "mana_board_index",
        "stack",
        "create_time",
        "update_time",
        "join_time",
    ] {
        assert!(
            !character[field].is_null(),
            "character field {field} is present"
        );
    }
    let duplicate_item_id = duplicate_draw["ex_boost_item"]["id"]
        .as_i64()
        .expect("duplicate material id is numeric");
    assert!(data["item_list"][duplicate_item_id.to_string()]
        .as_i64()
        .is_some_and(|count| count > 0));
    true
}
// //// /验证同一批重复角色保留完整状态和重复素材 ////

#[derive(Serialize)]
struct ExchangeCharacterRequest {
    character_id: i64,
    api_count: i64,
    gacha_id: i64,
    viewer_id: i64,
}

#[derive(Serialize)]
struct ExchangeEquipmentRequest {
    equipment_id: i64,
    api_count: i64,
    gacha_id: i64,
    viewer_id: i64,
}

// //// 设置测试服务的 UTC 虚拟日期 [@x380kkm 2026-08-18] ////
fn set_virtual_time(service: &PersonalService, iso: &str) {
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let body = format!(r#"{{"enabled":true,"iso":"{iso}","rate":1.0}}"#);
    let response = request_with_headers(
        service.port(),
        "PUT",
        "/v1/time",
        "application/json",
        &authorization,
        body.as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"));
}
// //// /设置测试服务的 UTC 虚拟日期 ////

// //// 显式开放测试使用的卡池 [@x380kkm 2026-08-24] ////
fn open_gacha(service: &PersonalService, gacha_id: i64) {
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let path = format!("/v1/activities/calendar/gacha%3A{gacha_id}");
    let response = request_with_headers(
        service.port(),
        "PUT",
        &path,
        "application/json",
        &authorization,
        br#"{"enabled":true,"start_at_ms":0,"end_at_ms":253402300799000}"#,
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
}
// //// /显式开放测试使用的卡池 ////

// //// 显式关闭测试使用的卡池 [@x380kkm 2026-08-24] ////
fn close_gacha(service: &PersonalService, gacha_id: i64) {
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let path = format!("/v1/activities/calendar/gacha%3A{gacha_id}");
    let response = request_with_headers(
        service.port(),
        "PUT",
        &path,
        "application/json",
        &authorization,
        br#"{"enabled":false,"start_at_ms":0,"end_at_ms":253402300799000}"#,
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"));
}
// //// /显式关闭测试使用的卡池 ////

// //// 临时开放测试使用的卡池 [@x380kkm 2026-08-29] ////
fn temporarily_open_gacha(service: &PersonalService, gacha_id: i64) {
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let path = format!("/v1/activities/gacha%3A{gacha_id}/temporary-open");
    let response = request_with_headers(
        service.port(),
        "POST",
        &path,
        "application/json",
        &authorization,
        br#"{}"#,
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
}

fn end_temporary_gacha(service: &PersonalService, gacha_id: i64) {
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let path = format!("/v1/activities/gacha%3A{gacha_id}/temporary-open");
    let response = request_with_headers(
        service.port(),
        "DELETE",
        &path,
        "application/json",
        &authorization,
        &[],
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
}
// //// /临时开放测试使用的卡池 ////

// //// 通过管理邮件向测试账号发放扭蛋支付资源 [@x380kkm 2026-08-24] ////
fn grant_mail_rewards(service: &PersonalService, viewer_id: i64, rewards: Value) {
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let mail = json!({
        "viewer_id": viewer_id,
        "title": "Gacha resources",
        "body": "Gacha payment resources",
        "sender": "Admin",
        "rewards": rewards,
    })
    .to_string();
    let created = request_with_headers(
        service.port(),
        "POST",
        "/v1/mails",
        "application/json",
        &authorization,
        mail.as_bytes(),
    );
    assert!(created.starts_with("HTTP/1.1 201 Created"));
    let received = cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive_all",
        &encode_request(&MailReceiveAllRequest { viewer_id }),
    );
    assert!(received.starts_with("HTTP/1.1 200 OK"));
}
// //// /通过管理邮件向测试账号发放扭蛋支付资源 ////

// //// 请求一次 CN 每日付费扭蛋 [@x380kkm 2026-08-18] ////
fn request_daily_draw(service: &PersonalService, viewer_id: i64) -> String {
    cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count: 1,
            payment_type: 2,
            number_of_exec: 1,
            viewer_id,
            gacha_id: 1,
            r#type: 5,
        }),
    )
}
// //// /请求一次 CN 每日付费扭蛋 ////

// //// 验证 CN 普通扭蛋扣费, 发放角色和持久化 [@x380kkm 2026-07-24] ////
#[test]
fn draws_a_character_and_persists_gacha_info() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    open_gacha(&service, 1);
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 31 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;

    let invalid = cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count: 1,
            payment_type: 1,
            number_of_exec: 1,
            viewer_id,
            gacha_id: 9999,
            r#type: 1,
        }),
    );
    assert!(invalid.starts_with("HTTP/1.1 400 Bad Request"));

    let insufficient_paid = cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count: 1,
            payment_type: 2,
            number_of_exec: 1,
            viewer_id,
            gacha_id: 1,
            r#type: 5,
        }),
    );
    assert!(insufficient_paid.ends_with("{\"error\":\"not_enough_vmoney\"}"));

    let response = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count: 1,
            payment_type: 1,
            number_of_exec: 1,
            viewer_id,
            gacha_id: 1,
            r#type: 1,
        }),
    ));
    assert_eq!(
        response.data["user_info"]["free_vmoney"].as_i64(),
        Some(1350)
    );
    assert_eq!(response.data["draw"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        response.data["character_list"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(response.data["gacha_info_list"][0]["gacha_id"], 1);
    assert_eq!(response.data["gacha_info_list"][0]["is_daily_first"], true);
    assert_eq!(response.data["gacha_info_list"][0]["daily_one_count"], 1);
    assert_eq!(response.data["gacha_info_list"][0]["daily_ten_count"], 0);
    assert_eq!(response.data["gacha_info_list"][0]["crazy_draw_count"], 0);
    assert_eq!(
        response.data["gacha_info_list"][0]["gacha_exchange_point"],
        1
    );
    let character_id = response.data["draw"][0]["character_id"]
        .as_i64()
        .expect("draw contains a character id");
    assert!(character_id > 100_000);
    let encyclopedia_id = format!("1{character_id}01");
    assert_eq!(
        response.data["encyclopedia_info"][&encyclopedia_id]["read"],
        false
    );

    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(loaded.data["user_info"]["free_vmoney"].as_i64(), Some(1350));
    assert_eq!(
        loaded.data["gacha_info_list"]
            .as_array()
            .and_then(|list| list.iter().find(|item| item["gacha_id"] == 1))
            .and_then(|item| item["is_daily_first"].as_bool()),
        Some(true)
    );
    assert_eq!(
        loaded.data["gacha_info_list"]
            .as_array()
            .and_then(|list| list.iter().find(|item| item["gacha_id"] == 1))
            .and_then(|item| item["gacha_exchange_point"].as_i64()),
        Some(1)
    );
    assert_eq!(
        loaded.data["gacha_info_list"]
            .as_array()
            .and_then(|list| list.iter().find(|item| item["gacha_id"] == 1))
            .and_then(|item| item["daily_one_count"].as_i64()),
        Some(1)
    );
    assert!(loaded.data["user_character_list"]
        .get(character_id.to_string())
        .is_some());
    let encyclopedia = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/encyclopedia/index",
        &encode_request(&json!({"viewer_id": viewer_id})),
    ));
    assert_eq!(
        encyclopedia.data["encyclopedia_list"][&encyclopedia_id]["read"],
        false
    );

    let insufficient = cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count: 1,
            payment_type: 1,
            number_of_exec: 1,
            viewer_id,
            gacha_id: 1,
            r#type: 2,
        }),
    );
    assert!(insufficient.ends_with("{\"error\":\"not_enough_vmoney\"}"));

    grant_mail_rewards(&service, viewer_id, json!({"freeVmoney": 1500}));
    let multi = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count: 2,
            payment_type: 1,
            number_of_exec: 1,
            viewer_id,
            gacha_id: 1,
            r#type: 2,
        }),
    ));
    assert_eq!(multi.data["gacha_info_list"][0]["daily_one_count"], 1);
    assert_eq!(multi.data["gacha_info_list"][0]["daily_ten_count"], 1);
    assert_eq!(multi.data["gacha_info_list"][0]["crazy_draw_count"], 0);
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 普通扭蛋扣费, 发放角色和持久化 ////

// //// 验证 CN 卡池默认时间与所选候选范围 [@x380kkm 2026-08-24] ////
#[test]
fn draws_from_requested_master_gacha() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 38 }),
    ));
    set_virtual_time(&service, "2026-08-21T12:00:00.000Z");
    grant_mail_rewards(
        &service,
        signup.data_headers.viewer_id,
        json!({"itemList": {"20003": 1}}),
    );
    let public_response = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count: 1,
            payment_type: 3,
            number_of_exec: 1,
            viewer_id: signup.data_headers.viewer_id,
            gacha_id: 2,
            r#type: 3,
        }),
    ));
    assert_eq!(public_response.data["gacha_info_list"][0]["gacha_id"], 2);

    let request = encode_request(&ExecuteRequest {
        api_count: 1,
        payment_type: 1,
        number_of_exec: 1,
        viewer_id: signup.data_headers.viewer_id,
        gacha_id: 5,
        r#type: 1,
    });
    let response = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &request,
    ));
    assert_eq!(response.data["gacha_info_list"][0]["gacha_id"], 5);
    assert_eq!(response.data["draw"].as_array().map(Vec::len), Some(1));
    let character_id = response.data["draw"][0]["character_id"]
        .as_i64()
        .expect("draw character id is numeric");
    let master_json = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/gacha.json"),
    )
    .expect("CN gacha master can be loaded");
    let master = serde_json::from_str::<Value>(&master_json).expect("CN gacha master can be read");
    let public_character_id = public_response.data["draw"][0]["character_id"]
        .as_i64()
        .expect("public draw character id is numeric");
    for (gacha_id, drawn_id) in [("2", public_character_id), ("5", character_id)] {
        assert!(["1", "2", "3"]
            .iter()
            .any(|rank| master[gacha_id]["pool"][rank]
                .as_array()
                .is_some_and(|pool| pool.iter().any(|entry| entry["id"] == drawn_id))));
    }
    open_gacha(&service, 157);
    let fes_response = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count: 2,
            payment_type: 1,
            number_of_exec: 1,
            viewer_id: signup.data_headers.viewer_id,
            gacha_id: 157,
            r#type: 1,
        }),
    ));
    assert!(matches!(
        fes_response.data["draw"][0]["movie_id"].as_str(),
        Some("fes" | "fes_guarantee")
    ));
    assert_valid_movie_seeds(&fes_response.data);
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 卡池默认时间与所选候选范围 ////

// //// 验证载入响应按活动状态生成群星和回归卡池有效期 [@x380kkm 2026-08-24] ////
#[test]
fn loads_dynamic_stars_and_comeback_campaign_periods() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    set_virtual_time(&service, "2026-08-21T12:00:00.000Z");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 49 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let load = || {
        decode_response::<Value>(&cn_support::send_request(
            service.port(),
            "/api/index.php/load",
            &encode_request(&LoadRequest {
                keychain: viewer_id,
                viewer_id,
            }),
        ))
    };

    let initial = load();
    let initial_list = initial.data["gacha_info_list"]
        .as_array()
        .expect("gacha info list is an array");
    let stars = initial_list
        .iter()
        .find(|info| info["gacha_id"] == 80_000)
        .expect("active stars gacha is advertised");
    assert_eq!(stars["is_account_first"], true);
    assert_eq!(
        stars["stars_campaign"]["period_start_time"],
        1_693_450_800_i64
    );
    assert_eq!(
        stars["stars_campaign"]["period_end_time"],
        7_255_943_999_i64
    );
    assert_eq!(
        initial_list
            .iter()
            .find(|info| info["gacha_id"] == 800_209)
            .map(|info| &info["is_account_first"]),
        Some(&Value::Bool(true))
    );
    assert!(initial_list.iter().all(|info| info["gacha_id"] != 700_000));

    open_gacha(&service, 700_000);
    let opened = load();
    let comeback = opened.data["gacha_info_list"]
        .as_array()
        .and_then(|list| list.iter().find(|info| info["gacha_id"] == 700_000))
        .expect("manually opened comeback gacha is advertised");
    let period = comeback["comeback_campaign"]
        .as_object()
        .expect("comeback campaign period is an object");
    assert_eq!(comeback["is_account_first"], true);
    assert!(
        period["period_start_time"]
            .as_i64()
            .expect("campaign start time is numeric")
            <= 1_787_313_600
    );
    assert!(
        period["period_end_time"]
            .as_i64()
            .expect("campaign end time is numeric")
            > 1_787_313_600
    );

    close_gacha(&service, 700_000);
    let closed = load();
    let closed_comeback = closed.data["gacha_info_list"]
        .as_array()
        .and_then(|list| list.iter().find(|info| info["gacha_id"] == 700_000));
    assert!(match closed_comeback {
        Some(info) => info.get("comeback_campaign").is_none(),
        None => true,
    });
    service.stop().expect("service stops cleanly");
}
// //// /验证载入响应按活动状态生成群星和回归卡池有效期 ////

// //// 验证账号首次有偿十连只可执行一次并跨重启保存 [@x380kkm 2026-08-24] ////
#[test]
fn consumes_account_first_paid_multi_once_and_persists_state() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    open_gacha(&service, 800_000);
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 46 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    grant_mail_rewards(&service, viewer_id, json!({"vmoney": 3000}));

    let request = |api_count| {
        cn_support::send_request(
            service.port(),
            "/api/index.php/gacha/exec",
            &encode_request(&ExecuteRequest {
                api_count,
                payment_type: 2,
                number_of_exec: 1,
                viewer_id,
                gacha_id: 800_000,
                r#type: 7,
            }),
        )
    };
    let first = decode_response::<Value>(&request(1));
    assert_eq!(first.data["draw"].as_array().map(Vec::len), Some(10));
    assert_eq!(first.data["user_info"]["vmoney"], 1500);
    assert_eq!(first.data["gacha_info_list"][0]["is_account_first"], false);
    assert_eq!(first.data["gacha_info_list"][0]["is_daily_first"], false);
    assert_eq!(
        first.data["gacha_info_list"][0]
            .as_object()
            .map(serde_json::Map::len),
        Some(7)
    );
    assert!(request(2).ends_with("{\"error\":\"account_first_gacha_already_used\"}"));

    service.stop().expect("service stops cleanly");
    let restarted = PersonalService::start(root.path(), 0).expect("service restarts");
    open_gacha(&restarted, 800_000);
    let repeated = cn_support::send_request(
        restarted.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count: 3,
            payment_type: 2,
            number_of_exec: 1,
            viewer_id,
            gacha_id: 800_000,
            r#type: 7,
        }),
    );
    assert!(repeated.ends_with("{\"error\":\"account_first_gacha_already_used\"}"));
    restarted.stop().expect("restarted service stops cleanly");
}
// //// /验证账号首次有偿十连只可执行一次并跨重启保存 ////

// //// 验证池内票券在卡池结束后延长到票券期限 [@x380kkm 2026-08-24] ////
#[test]
fn extends_expired_gacha_only_while_pool_ticket_is_held() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    set_virtual_time(&service, "2020-06-01T00:00:00.000Z");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 47 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let request = |api_count| {
        cn_support::send_request(
            service.port(),
            "/api/index.php/gacha/exec",
            &encode_request(&ExecuteRequest {
                api_count,
                payment_type: 3,
                number_of_exec: 1,
                viewer_id,
                gacha_id: 1,
                r#type: 3,
            }),
        )
    };
    assert!(request(1).ends_with("{\"error\":\"not_enough_tickets\"}"));

    grant_mail_rewards(&service, viewer_id, json!({"itemList": {"20001": 1}}));
    let draw = decode_response::<Value>(&request(2));
    assert_eq!(draw.data["draw"].as_array().map(Vec::len), Some(1));
    assert_eq!(draw.data["item_list"]["20001"], 0);
    assert!(request(3).ends_with("{\"error\":\"not_enough_tickets\"}"));
    service.stop().expect("service stops cleanly");
}
// //// /验证池内票券在卡池结束后延长到票券期限 ////

// //// 验证特殊页面和通用票券按客户端抽取类型扣除真实道具 [@x380kkm 2026-08-24] ////
#[test]
fn consumes_pool_and_wildcard_tickets_by_client_draw_type() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    for gacha_id in [2, 9, 57, 100, 5009, 80_000] {
        open_gacha(&service, gacha_id);
    }
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 48 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    grant_mail_rewards(
        &service,
        viewer_id,
        json!({
            "vmoney": 100,
            "itemList": {
                "20003": 1,
                "20015": 1,
                "20064": 1,
                "20065": 1,
                "999001": 1,
                "999003": 1,
                "999004": 1,
                "999005": 1,
                "999008": 1,
                "999012": 1
            }
        }),
    );
    let execute_ticket = |api_count, gacha_id, draw_type| {
        cn_support::send_request(
            service.port(),
            "/api/index.php/gacha/exec",
            &encode_request(&ExecuteRequest {
                api_count,
                payment_type: 3,
                number_of_exec: 1,
                viewer_id,
                gacha_id,
                r#type: draw_type,
            }),
        )
    };

    let wrong_pool_ticket = execute_ticket(1, 2, 4);
    assert!(wrong_pool_ticket.ends_with("{\"error\":\"unsupported_gacha_type\"}"));
    let single_pool = decode_response::<Value>(&execute_ticket(2, 2, 3));
    assert_eq!(single_pool.data["draw"].as_array().map(Vec::len), Some(1));
    assert_eq!(single_pool.data["item_list"]["20003"], 0);
    let multi_pool = decode_response::<Value>(&execute_ticket(3, 9, 4));
    assert_eq!(multi_pool.data["draw"].as_array().map(Vec::len), Some(10));
    assert_eq!(multi_pool.data["item_list"]["20015"], 0);
    let mixed_single = decode_response::<Value>(&execute_ticket(4, 57, 3));
    assert_eq!(mixed_single.data["item_list"]["20065"], 0);
    let mixed_multi = decode_response::<Value>(&execute_ticket(5, 57, 4));
    assert_eq!(mixed_multi.data["draw"].as_array().map(Vec::len), Some(10));
    assert_eq!(mixed_multi.data["item_list"]["20064"], 0);
    let crazy = decode_response::<Value>(&execute_ticket(6, 100, 14));
    assert_eq!(crazy.data["draw"].as_array().map(Vec::len), Some(10));
    assert_eq!(crazy.data["item_list"]["999012"], 0);
    assert_eq!(crazy.data["gacha_info_list"][0]["crazy_draw_count"], 1);

    let character_single = decode_response::<Value>(&execute_ticket(7, 80_000, 10));
    assert_eq!(character_single.data["item_list"]["999003"], 0);
    let character_multi = decode_response::<Value>(&execute_ticket(8, 80_000, 9));
    assert_eq!(
        character_multi.data["draw"].as_array().map(Vec::len),
        Some(10)
    );
    assert_eq!(character_multi.data["item_list"]["999001"], 0);
    let guaranteed = decode_response::<Value>(&execute_ticket(9, 80_000, 20));
    let guaranteed_character_id = guaranteed.data["draw"][0]["character_id"]
        .as_i64()
        .expect("guaranteed draw contains a character id");
    assert!(guaranteed_character_id / 100_000 <= 2);
    assert_eq!(guaranteed.data["item_list"]["999008"], 0);

    let equipment_single = decode_response::<Value>(&execute_ticket(10, 5009, 12));
    assert_eq!(
        equipment_single.data["draw_equipment"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(equipment_single.data["item_list"]["999005"], 0);
    let equipment_multi = decode_response::<Value>(&execute_ticket(11, 5009, 13));
    assert_eq!(
        equipment_multi.data["draw_equipment"]
            .as_array()
            .map(Vec::len),
        Some(10)
    );
    assert_eq!(equipment_multi.data["item_list"]["999004"], 0);

    let daily_on_stars = cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count: 12,
            payment_type: 2,
            number_of_exec: 1,
            viewer_id,
            gacha_id: 80_000,
            r#type: 5,
        }),
    );
    assert!(daily_on_stars.ends_with("{\"error\":\"invalid_gacha_request\"}"));
    service.stop().expect("service stops cleanly");
}
// //// /验证特殊页面和通用票券按客户端抽取类型扣除真实道具 ////

// //// 验证 CN 每日付费和角色票券扭蛋 [@x380kkm 2026-07-24] ////
#[test]
fn consumes_daily_payment_and_character_ticket_once() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    open_gacha(&service, 1);
    open_gacha(&service, 29);
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 32 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let mail = format!(
        "{{\"viewer_id\":{viewer_id},\"title\":\"Gacha tickets\",\"body\":\"Daily and ticket test\",\"sender\":\"Admin\",\"rewards\":{{\"vmoney\":100,\"itemList\":{{\"999003\":1}}}}}}"
    );
    let created = request_with_headers(
        service.port(),
        "POST",
        "/v1/mails",
        "application/json",
        &authorization,
        mail.as_bytes(),
    );
    assert!(created.starts_with("HTTP/1.1 201 Created"));
    let received = cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive_all",
        &encode_request(&MailReceiveAllRequest { viewer_id }),
    );
    assert!(received.starts_with("HTTP/1.1 200 OK"));

    let daily = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count: 1,
            payment_type: 2,
            number_of_exec: 1,
            viewer_id,
            gacha_id: 1,
            r#type: 5,
        }),
    ));
    assert_eq!(daily.data["user_info"]["vmoney"].as_i64(), Some(50));
    assert_eq!(daily.data["gacha_info_list"][0]["is_daily_first"], false);
    assert_eq!(daily.data["gacha_info_list"][0]["daily_one_count"], 0);
    assert_eq!(daily.data["gacha_info_list"][0]["daily_ten_count"], 0);
    assert_eq!(daily.data["gacha_info_list"][0]["crazy_draw_count"], 0);

    let repeated_daily = cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count: 1,
            payment_type: 2,
            number_of_exec: 1,
            viewer_id,
            gacha_id: 1,
            r#type: 5,
        }),
    );
    assert!(repeated_daily.starts_with("HTTP/1.1 400 Bad Request"));

    let unsupported_ticket = cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count: 2,
            payment_type: 3,
            number_of_exec: 1,
            viewer_id,
            gacha_id: 1,
            r#type: 10,
        }),
    );
    assert!(unsupported_ticket.ends_with("{\"error\":\"unsupported_gacha_type\"}"));

    let ticket = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count: 3,
            payment_type: 3,
            number_of_exec: 1,
            viewer_id,
            gacha_id: 29,
            r#type: 10,
        }),
    ));
    assert_eq!(ticket.data["item_list"]["999003"].as_i64(), Some(0));
    assert_eq!(ticket.data["draw"].as_array().map(Vec::len), Some(1));
    assert_eq!(ticket.data["user_info"]["vmoney"].as_i64(), Some(50));
    let insufficient_ticket = cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count: 4,
            payment_type: 3,
            number_of_exec: 1,
            viewer_id,
            gacha_id: 29,
            r#type: 10,
        }),
    );
    assert!(insufficient_ticket.ends_with("{\"error\":\"not_enough_tickets\"}"));
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 每日付费和角色票券扭蛋 ////

// //// 验证 CN 每日扭蛋按 UTC 虚拟日期重置并跨重启保留 [@x380kkm 2026-08-18] ////
#[test]
fn resets_daily_payment_by_virtual_utc_date_and_persists_consumption() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    open_gacha(&service, 1);
    set_virtual_time(&service, "2030-01-01T12:00:00.000Z");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 34 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let mail = format!(
        "{{\"viewer_id\":{viewer_id},\"title\":\"Daily gacha currency\",\"body\":\"Virtual date test\",\"sender\":\"Admin\",\"rewards\":{{\"vmoney\":300}}}}"
    );
    let created = request_with_headers(
        service.port(),
        "POST",
        "/v1/mails",
        "application/json",
        &authorization,
        mail.as_bytes(),
    );
    assert!(created.starts_with("HTTP/1.1 201 Created"));
    let received = cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive_all",
        &encode_request(&MailReceiveAllRequest { viewer_id }),
    );
    assert!(received.starts_with("HTTP/1.1 200 OK"));

    let first = decode_response::<Value>(&request_daily_draw(&service, viewer_id));
    assert_eq!(first.data["user_info"]["vmoney"].as_i64(), Some(250));
    let repeated = request_daily_draw(&service, viewer_id);
    assert!(repeated.ends_with("{\"error\":\"daily_gacha_already_used\"}"));

    set_virtual_time(&service, "2030-01-02T12:00:00.000Z");
    let next_day_load = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(
        next_day_load.data["gacha_info_list"][0]["is_daily_first"].as_bool(),
        Some(true)
    );
    let next_day = decode_response::<Value>(&request_daily_draw(&service, viewer_id));
    assert_eq!(next_day.data["user_info"]["vmoney"].as_i64(), Some(200));

    set_virtual_time(&service, "2030-01-01T18:00:00.000Z");
    let rewound_load = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(
        rewound_load.data["gacha_info_list"][0]["is_daily_first"].as_bool(),
        Some(false)
    );
    let rewound = request_daily_draw(&service, viewer_id);
    assert!(rewound.ends_with("{\"error\":\"daily_gacha_already_used\"}"));

    set_virtual_time(&service, "2030-01-03T12:00:00.000Z");
    let fast_forwarded = decode_response::<Value>(&request_daily_draw(&service, viewer_id));
    assert_eq!(
        fast_forwarded.data["user_info"]["vmoney"].as_i64(),
        Some(150)
    );
    service.stop().expect("service stops cleanly");

    let restarted = PersonalService::start(root.path(), 0).expect("service restarts");
    let repeated_after_restart = request_daily_draw(&restarted, viewer_id);
    assert!(repeated_after_restart.ends_with("{\"error\":\"daily_gacha_already_used\"}"));
    set_virtual_time(&restarted, "2030-01-04T12:00:00.000Z");
    let after_restart = decode_response::<Value>(&request_daily_draw(&restarted, viewer_id));
    assert_eq!(
        after_restart.data["user_info"]["vmoney"].as_i64(),
        Some(100)
    );
    restarted.stop().expect("restarted service stops cleanly");
}
// //// /验证 CN 每日扭蛋按 UTC 虚拟日期重置并跨重启保留 ////

// //// 验证 CN 扭蛋角色兑换的积分边界和持久化 [@x380kkm 2026-07-24] ////
#[test]
fn exchanges_a_character_with_gacha_points_once() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    open_gacha(&service, 1);
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 33 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let request = |character_id| {
        encode_request(&ExchangeCharacterRequest {
            character_id,
            api_count: 1,
            gacha_id: 1,
            viewer_id,
        })
    };

    let missing_info = cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exchange_character",
        &request(111001),
    );
    assert!(missing_info.ends_with("{\"error\":\"no_gacha_info\"}"));

    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let created = request_with_headers(
        service.port(),
        "POST",
        "/v1/mails",
        "application/json",
        &authorization,
        format!(
            "{{\"viewer_id\":{viewer_id},\"title\":\"Gacha resources\",\"body\":\"Protocol test\",\"sender\":\"Admin\",\"rewards\":{{\"freeVmoney\":37500}}}}"
        )
        .as_bytes(),
    );
    assert!(created.starts_with("HTTP/1.1 201 Created"));
    let received = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive_all",
        &encode_request(&MailReceiveAllRequest { viewer_id }),
    ));
    let initial_free_vmoney = received.data["user_info"]["free_vmoney"]
        .as_i64()
        .expect("granted free vmoney is numeric");
    assert!(initial_free_vmoney >= 37_500);

    let mut observed_repeated_character = false;
    for draw_number in 1..=24 {
        let draw = decode_response::<Value>(&cn_support::send_request(
            service.port(),
            "/api/index.php/gacha/exec",
            &encode_request(&ExecuteRequest {
                api_count: draw_number,
                payment_type: 1,
                number_of_exec: 1,
                viewer_id,
                gacha_id: 1,
                r#type: 2,
            }),
        ));
        assert_eq!(
            draw.data["user_info"]["free_vmoney"],
            initial_free_vmoney - draw_number * 1500
        );
        assert_eq!(draw.data["draw"].as_array().map(Vec::len), Some(10));
        assert_valid_movie_seeds(&draw.data);
        observed_repeated_character |= has_valid_repeated_character_response(&draw.data);
        let character_list = draw.data["character_list"]
            .as_array()
            .expect("character list is an array");
        let character_ids = character_list
            .iter()
            .map(|character| {
                character["character_id"]
                    .as_i64()
                    .expect("character id is numeric")
            })
            .collect::<HashSet<_>>();
        assert_eq!(character_ids.len(), character_list.len());
        for draw_entry in draw.data["draw"].as_array().expect("draw list is an array") {
            assert!(character_ids.contains(
                &draw_entry["character_id"]
                    .as_i64()
                    .expect("draw character id is numeric")
            ));
        }
        for character_entry in character_list {
            if let Some(response_viewer_id) = character_entry.get("viewer_id") {
                assert_eq!(response_viewer_id, 0);
            }
        }
        assert_eq!(
            draw.data["gacha_info_list"][0]["gacha_exchange_point"],
            draw_number * 10
        );
    }

    let insufficient = cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exchange_character",
        &request(111001),
    );
    assert!(insufficient.ends_with("{\"error\":\"not_enough_exchange_points\"}"));

    let final_draw = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count: 25,
            payment_type: 1,
            number_of_exec: 1,
            viewer_id,
            gacha_id: 1,
            r#type: 2,
        }),
    ));
    assert_eq!(
        final_draw.data["user_info"]["free_vmoney"],
        initial_free_vmoney - 25 * 1500
    );
    assert_eq!(final_draw.data["draw"].as_array().map(Vec::len), Some(10));
    observed_repeated_character |= has_valid_repeated_character_response(&final_draw.data);
    assert!(observed_repeated_character);
    assert_eq!(
        final_draw.data["gacha_info_list"][0]["gacha_exchange_point"],
        250
    );

    let unknown_character = cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exchange_character",
        &request(999999),
    );
    assert!(unknown_character.ends_with("{\"error\":\"character_not_in_gacha\"}"));

    let exchanged = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exchange_character",
        &request(111001),
    ));
    assert_eq!(
        exchanged.data["gacha_info_list"][0]["gacha_exchange_point"],
        0
    );
    assert_eq!(exchanged.data["character_list"][0]["character_id"], 111001);

    let repeated = cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exchange_character",
        &request(111001),
    );
    assert!(repeated.ends_with("{\"error\":\"not_enough_exchange_points\"}"));
    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(loaded.data["gacha_info_list"][0]["gacha_exchange_point"], 0);
    assert!(loaded.data["user_character_list"].get("111001").is_some());
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 扭蛋角色兑换的积分边界和持久化 ////

// //// 验证 CN 扭蛋装备兑换和积分持久化 [@x380kkm 2026-07-24] ////
#[test]
fn exchanges_an_equipment_with_gacha_points_once() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    open_gacha(&service, 1);
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 42 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let created = request_with_headers(
        service.port(),
        "POST",
        "/v1/mails",
        "application/json",
        &authorization,
        format!(
            "{{\"viewer_id\":{viewer_id},\"title\":\"Gacha resources\",\"body\":\"Equipment exchange test\",\"sender\":\"Admin\",\"rewards\":{{\"freeVmoney\":37500}}}}"
        )
        .as_bytes(),
    );
    assert!(created.starts_with("HTTP/1.1 201 Created"));
    let received = cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive_all",
        &encode_request(&MailReceiveAllRequest { viewer_id }),
    );
    assert!(received.starts_with("HTTP/1.1 200 OK"));

    for draw_number in 1..=25 {
        let draw = decode_response::<Value>(&cn_support::send_request(
            service.port(),
            "/api/index.php/gacha/exec",
            &encode_request(&ExecuteRequest {
                api_count: draw_number,
                payment_type: 1,
                number_of_exec: 1,
                viewer_id,
                gacha_id: 1,
                r#type: 2,
            }),
        ));
        assert_eq!(
            draw.data["gacha_info_list"][0]["gacha_exchange_point"],
            draw_number * 10
        );
    }

    let exchanged = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exchange_equipment",
        &encode_request(&ExchangeEquipmentRequest {
            equipment_id: 5_030_037,
            api_count: 26,
            gacha_id: 1,
            viewer_id,
        }),
    ));
    assert_eq!(
        exchanged.data["gacha_info_list"][0]["gacha_exchange_point"],
        0
    );
    assert_eq!(
        exchanged.data["equipment_list"][0]["equipment_id"],
        5_030_037
    );
    assert_eq!(exchanged.data["equipment_list"][0]["stack"], 0);

    let repeated = cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exchange_equipment",
        &encode_request(&ExchangeEquipmentRequest {
            equipment_id: 5_030_037,
            api_count: 27,
            gacha_id: 1,
            viewer_id,
        }),
    );
    assert!(repeated.ends_with("{\"error\":\"not_enough_exchange_points\"}"));
    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(loaded.data["user_equipment_list"]["5030037"]["stack"], 0);
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 扭蛋装备兑换和积分持久化 ////

// //// 验证 CN 装备扭蛋的扣费, 抽取和持久化 [@x380kkm 2026-08-22] ////
#[test]
fn draws_equipment_and_persists_payment_inventory_and_gacha_info() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    open_gacha(&service, 3);
    open_gacha(&service, 5009);
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 43 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;

    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let mail = format!(
        "{{\"viewer_id\":{viewer_id},\"title\":\"Equipment gacha\",\"body\":\"Equipment payment resources\",\"sender\":\"Admin\",\"rewards\":{{\"vmoney\":25,\"itemList\":{{\"999004\":1}}}}}}"
    );
    let created = request_with_headers(
        service.port(),
        "POST",
        "/v1/mails",
        "application/json",
        &authorization,
        mail.as_bytes(),
    );
    assert!(created.starts_with("HTTP/1.1 201 Created"));
    let received = cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive_all",
        &encode_request(&MailReceiveAllRequest { viewer_id }),
    );
    assert!(received.starts_with("HTTP/1.1 200 OK"));

    let daily_draw = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count: 1,
            payment_type: 2,
            number_of_exec: 1,
            viewer_id,
            gacha_id: 3,
            r#type: 5,
        }),
    ));
    assert_eq!(daily_draw.data["user_info"]["vmoney"], 0);
    assert_eq!(
        daily_draw.data["gacha_info_list"][0]["is_daily_first"],
        false
    );

    let free_draw = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count: 2,
            payment_type: 1,
            number_of_exec: 1,
            viewer_id,
            gacha_id: 3,
            r#type: 1,
        }),
    ));
    let free_data = free_draw
        .data
        .as_object()
        .expect("equipment gacha response data is an object");
    assert_eq!(free_data.len(), 8);
    assert_eq!(free_data["user_info"]["free_vmoney"], 1425);
    assert_eq!(free_data["is_erupt"], false);
    assert_eq!(
        free_data["draw_equipment"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        free_data["equipment_list"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(free_data["gacha_info_list"][0]["gacha_id"], 3);
    assert_eq!(free_data["gacha_info_list"][0]["gacha_exchange_point"], 2);
    assert_eq!(free_data["gacha_info_list"][0]["is_daily_first"], false);
    let first_draw = free_data["draw_equipment"][0]
        .as_object()
        .expect("equipment draw is an object");
    assert_eq!(first_draw.len(), 2);
    assert_eq!(first_draw["treasure_up_type"], 0);
    let first_equipment = free_data["equipment_list"][0]
        .as_object()
        .expect("equipment response is an object");
    assert_eq!(first_equipment.len(), 5);
    assert!(!first_equipment.contains_key("null"));
    assert!(!first_equipment.contains_key("viewer_id"));

    let unsupported_ticket = cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count: 3,
            payment_type: 3,
            number_of_exec: 1,
            viewer_id,
            gacha_id: 3,
            r#type: 13,
        }),
    );
    assert!(unsupported_ticket.ends_with("{\"error\":\"unsupported_gacha_type\"}"));

    let ticket_draw = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count: 4,
            payment_type: 3,
            number_of_exec: 1,
            viewer_id,
            gacha_id: 5009,
            r#type: 13,
        }),
    ));
    let draws = ticket_draw.data["draw_equipment"]
        .as_array()
        .expect("equipment ticket draw is an array");
    assert_eq!(draws.len(), 10);
    assert_eq!(ticket_draw.data["item_list"]["999004"], 0);
    assert_eq!(
        ticket_draw.data["gacha_info_list"][0]["gacha_exchange_point"],
        10
    );
    for draw in draws {
        let draw = draw.as_object().expect("equipment draw is an object");
        assert_eq!(draw.len(), 2);
        assert!(draw["equipment_id"].as_i64().is_some());
        assert_eq!(draw["treasure_up_type"], 0);
    }
    for equipment in ticket_draw.data["equipment_list"]
        .as_array()
        .expect("equipment list is an array")
    {
        let equipment = equipment
            .as_object()
            .expect("equipment response is an object");
        assert_eq!(equipment.len(), 5);
        assert!(equipment["equipment_id"].as_i64().is_some());
        assert!(equipment["protection"].as_bool().is_some());
        assert!(equipment["level"].as_i64().is_some());
        assert!(equipment["enhancement_level"].as_i64().is_some());
        assert!(equipment["stack"].as_i64().is_some());
    }

    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(loaded.data["user_info"]["free_vmoney"], 1425);
    assert_eq!(loaded.data["user_info"]["vmoney"], 0);
    assert_eq!(loaded.data["item_list"]["999004"], 0);
    let equipment_gacha_info = loaded.data["gacha_info_list"]
        .as_array()
        .and_then(|list| list.iter().find(|info| info["gacha_id"] == 3))
        .expect("equipment gacha info is persisted");
    assert_eq!(equipment_gacha_info["gacha_exchange_point"], 2);
    assert_eq!(equipment_gacha_info["is_daily_first"], false);
    let ticket_gacha_info = loaded.data["gacha_info_list"]
        .as_array()
        .and_then(|list| list.iter().find(|info| info["gacha_id"] == 5009))
        .expect("equipment ticket gacha info is persisted");
    assert_eq!(ticket_gacha_info["gacha_exchange_point"], 10);
    for draw in draws {
        let equipment_id = draw["equipment_id"]
            .as_i64()
            .expect("equipment draw id is numeric");
        assert!(loaded.data["user_equipment_list"]
            .get(equipment_id.to_string())
            .is_some());
    }
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 装备扭蛋的扣费, 抽取和持久化 ////

// //// 验证活动开关统一限制扭蛋执行 [@x380kkm 2026-08-22] ////
#[test]
fn follows_manual_activity_state_for_gacha_execution() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 44 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let closed = request_with_headers(
        service.port(),
        "POST",
        "/v1/activities/gacha%3A1/close",
        "application/json",
        &authorization,
        br#"{}"#,
    );
    assert!(closed.starts_with("HTTP/1.1 200 OK"));

    let request = encode_request(&ExecuteRequest {
        api_count: 1,
        payment_type: 1,
        number_of_exec: 1,
        viewer_id,
        gacha_id: 1,
        r#type: 1,
    });
    let blocked = cn_support::send_request(service.port(), "/api/index.php/gacha/exec", &request);
    assert!(blocked.ends_with("{\"error\":\"activity_disabled\"}"));

    let opened = request_with_headers(
        service.port(),
        "POST",
        "/v1/activities/gacha%3A1/open",
        "application/json",
        &authorization,
        br#"{}"#,
    );
    assert!(opened.starts_with("HTTP/1.1 200 OK"), "{opened}");
    let executed = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &request,
    ));
    assert_eq!(executed.data["draw"].as_array().map(Vec::len), Some(1));
    service.stop().expect("service stops cleanly");
}
// //// /验证活动开关统一限制扭蛋执行 ////

// //// 验证活动免费抽取和领取记录跨重启保持一致 [@x380kkm 2026-08-22] ////
#[test]
fn redeems_campaign_draw_once_and_persists_history() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    open_gacha(&service, 29);
    open_gacha(&service, 30);
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 45 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    open_gacha(&service, 28);
    let initial = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    let available_campaign = initial.data["gacha_campaign_list"]
        .as_array()
        .and_then(|campaigns| campaigns.iter().find(|campaign| campaign["gacha_id"] == 28))
        .expect("campaign is advertised by load");
    assert_eq!(available_campaign["count"], 1);
    let campaign_request = |api_count| {
        cn_support::send_request(
            service.port(),
            "/api/index.php/gacha/exec",
            &encode_request(&ExecuteRequest {
                api_count,
                payment_type: 4,
                number_of_exec: 1,
                viewer_id,
                gacha_id: 28,
                r#type: 8,
            }),
        )
    };
    let campaign = decode_response::<Value>(&campaign_request(1));
    assert_eq!(campaign.data["draw"].as_array().map(Vec::len), Some(10));
    assert_eq!(campaign.data["gacha_campaign_list"][0]["campaign_id"], 1);
    assert_eq!(campaign.data["gacha_campaign_list"][0]["count"], 0);
    let history = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/history/receive",
        &encode_request(&serde_json::json!({"viewer_id": viewer_id})),
    ));
    assert_eq!(history.data["total_count"], 10);
    assert!(history.data["history"]
        .as_array()
        .expect("history is an array")
        .iter()
        .all(|entry| entry["type"] == 5));
    assert!(campaign_request(2).starts_with("HTTP/1.1 400 Bad Request"));

    let legacy_single_campaign = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count: 3,
            payment_type: 4,
            number_of_exec: 1,
            viewer_id,
            gacha_id: 29,
            r#type: 7,
        }),
    ));
    assert_eq!(
        legacy_single_campaign.data["draw"].as_array().map(Vec::len),
        Some(1)
    );

    let single_campaign = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count: 4,
            payment_type: 4,
            number_of_exec: 1,
            viewer_id,
            gacha_id: 30,
            r#type: 11,
        }),
    ));
    assert_eq!(
        single_campaign.data["draw"].as_array().map(Vec::len),
        Some(1)
    );

    service.stop().expect("service stops cleanly");
    let restarted = PersonalService::start(root.path(), 0).expect("service restarts");
    let loaded = decode_response::<Value>(&cn_support::send_request(
        restarted.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    let stored_campaign = loaded.data["gacha_campaign_list"]
        .as_array()
        .and_then(|campaigns| campaigns.iter().find(|campaign| campaign["gacha_id"] == 28))
        .expect("campaign state is persisted");
    assert_eq!(stored_campaign["count"], 0);
    let restarted_history = decode_response::<Value>(&cn_support::send_request(
        restarted.port(),
        "/api/index.php/history/receive",
        &encode_request(&serde_json::json!({"viewer_id": viewer_id})),
    ));
    assert_eq!(restarted_history.data["total_count"], 12);
    restarted.stop().expect("service stops cleanly");
}
// //// /验证活动免费抽取和领取记录跨重启保持一致 ////

// //// 验证临时卡池沿用真实卡池的免费次数 [@x380kkm 2026-08-24] ////
#[test]
fn projects_temporary_alias_campaign_without_alias_save_state() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    set_virtual_time(&service, "2030-01-01T12:00:00.000Z");
    open_gacha(&service, 28);
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let opened = request_with_headers(
        service.port(),
        "POST",
        "/v1/activities/gacha%3A28/temporary-open",
        "application/json",
        &authorization,
        br#"{}"#,
    );
    assert!(opened.starts_with("HTTP/1.1 200 OK"), "{opened}");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 49 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let temporary_gacha_id = 1_000_028;
    let initial = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    let temporary_info = initial.data["gacha_info_list"]
        .as_array()
        .and_then(|list| {
            list.iter()
                .find(|info| info["gacha_id"] == temporary_gacha_id)
        })
        .expect("temporary gacha is advertised by load");
    assert_eq!(
        initial.data["gacha_info_list"]
            .as_array()
            .expect("gacha info list is returned")
            .iter()
            .filter(|info| { info["gacha_id"].as_i64().is_some_and(|id| id >= 1_000_000) })
            .count(),
        1
    );
    assert!(temporary_info["comeback_campaign"]["period_end_time"]
        .as_i64()
        .is_some());
    let temporary_campaign = initial.data["gacha_campaign_list"]
        .as_array()
        .and_then(|campaigns| {
            campaigns
                .iter()
                .find(|campaign| campaign["gacha_id"] == temporary_gacha_id)
        })
        .expect("temporary gacha campaign is advertised by load");
    assert_eq!(
        initial.data["gacha_campaign_list"]
            .as_array()
            .expect("gacha campaigns are returned")
            .iter()
            .filter(|campaign| {
                campaign["gacha_id"]
                    .as_i64()
                    .is_some_and(|id| id >= 1_000_000)
            })
            .count(),
        1
    );
    assert_eq!(temporary_campaign["campaign_id"], temporary_gacha_id);
    assert_eq!(temporary_campaign["count"], 1);

    let draw = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count: 1,
            payment_type: 4,
            number_of_exec: 1,
            viewer_id,
            gacha_id: temporary_gacha_id,
            r#type: 8,
        }),
    ));
    assert_eq!(draw.data["draw"].as_array().map(Vec::len), Some(10));
    assert_eq!(
        draw.data["gacha_info_list"][0]["gacha_id"],
        temporary_gacha_id
    );
    assert_eq!(
        draw.data["gacha_campaign_list"][0]["gacha_id"],
        temporary_gacha_id
    );
    assert_eq!(
        draw.data["gacha_campaign_list"][0]["campaign_id"],
        temporary_gacha_id
    );
    assert_eq!(draw.data["gacha_campaign_list"][0]["count"], 0);

    service.stop().expect("service stops cleanly");
    let restarted = PersonalService::start(root.path(), 0).expect("service restarts");
    let loaded = decode_response::<Value>(&cn_support::send_request(
        restarted.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    let campaigns = loaded.data["gacha_campaign_list"]
        .as_array()
        .expect("gacha campaigns are returned");
    assert_eq!(
        campaigns
            .iter()
            .find(|campaign| campaign["gacha_id"] == 28)
            .and_then(|campaign| campaign["count"].as_i64()),
        Some(0)
    );
    assert_eq!(
        campaigns
            .iter()
            .find(|campaign| campaign["gacha_id"] == temporary_gacha_id)
            .and_then(|campaign| campaign["count"].as_i64()),
        Some(0)
    );
    restarted.stop().expect("service stops cleanly");
}
// //// /验证临时卡池沿用真实卡池的免费次数 ////

// //// 验证特殊票券卡池临时开放只投影目标池 [@x380kkm 2026-08-29] ////
#[test]
fn projects_temporary_special_ticket_pool_without_cross_banner_state() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    set_virtual_time(&service, "2030-01-01T12:00:00.000Z");
    open_gacha(&service, 57);
    temporarily_open_gacha(&service, 57);
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 50 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let load = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    let temporary_id = 1_000_057;
    let temporary_info = load.data["gacha_info_list"]
        .as_array()
        .expect("gacha info list is an array")
        .iter()
        .find(|info| info["gacha_id"] == temporary_id)
        .expect("temporary special gacha is advertised");
    assert_eq!(
        temporary_info["comeback_campaign"]["period_start_time"].is_i64(),
        true
    );
    assert_eq!(
        temporary_info["comeback_campaign"]["period_end_time"].is_i64(),
        true
    );
    assert_eq!(
        load.data["gacha_info_list"]
            .as_array()
            .expect("gacha info list is an array")
            .iter()
            .filter(|info| info["gacha_id"].as_i64().is_some_and(|id| id >= 1_000_000))
            .count(),
        1
    );

    grant_mail_rewards(&service, viewer_id, json!({"itemList": {"20065": 1}}));
    let draw = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count: 1,
            payment_type: 3,
            number_of_exec: 1,
            viewer_id,
            gacha_id: temporary_id,
            r#type: 3,
        }),
    ));
    assert_eq!(draw.data["draw"].as_array().map(Vec::len), Some(1));
    assert_eq!(draw.data["item_list"]["20065"], 0);
    assert_eq!(draw.data["gacha_info_list"][0]["gacha_id"], temporary_id);
    assert_eq!(
        draw.data["gacha_info_list"][0]["comeback_campaign"]["period_end_time"].is_i64(),
        true
    );

    end_temporary_gacha(&service, 57);
    service.stop().expect("service stops cleanly");
}
// //// /验证特殊票券卡池临时开放只投影目标池 ////

// //// 验证地区覆盖卡池抽取响应保留客户端有效期 [@x380kkm 2026-08-29] ////
#[test]
fn preserves_coverage_period_in_gacha_draw_response() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    set_virtual_time(&service, "2021-10-30T12:00:00.000Z");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 51 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let load = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    let coverage_info = load.data["gacha_info_list"]
        .as_array()
        .expect("gacha info list is an array")
        .iter()
        .find(|info| info["gacha_id"] == 61)
        .expect("coverage gacha is advertised");
    assert!(coverage_info["comeback_campaign"]["period_start_time"].is_i64());
    assert!(coverage_info["comeback_campaign"]["period_end_time"].is_i64());

    grant_mail_rewards(&service, viewer_id, json!({"freeVmoney": 150}));
    let draw = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exec",
        &encode_request(&ExecuteRequest {
            api_count: 1,
            payment_type: 1,
            number_of_exec: 1,
            viewer_id,
            gacha_id: 61,
            r#type: 1,
        }),
    ));
    assert_eq!(draw.data["gacha_info_list"][0]["gacha_id"], 61);
    assert!(draw.data["gacha_info_list"][0]["comeback_campaign"]["period_end_time"].is_i64());
    service.stop().expect("service stops cleanly");
}
// //// /验证地区覆盖卡池抽取响应保留客户端有效期 ////
