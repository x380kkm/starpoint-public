// audience: internal
// # personal-service-cn-mail-tests
//
// 该文件验证管理员发放, CN 邮件索引, 单封领取和批量领取的持久化闭环.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, LoadRequest, SignupData, SignupRequest};
use serde::Serialize;
use serde_json::Value;
use starpoint_personal_service::PersonalService;
use support::request_with_headers;
use tempfile::TempDir;

#[derive(Serialize)]
struct MailIndexRequest {
    viewer_id: i64,
    current_page: i64,
}

#[derive(Serialize)]
struct MailReceiveRequest {
    viewer_id: i64,
    mail_id: i64,
}

#[derive(Serialize)]
struct MailReceiveAllRequest {
    viewer_id: i64,
    mail_ids: Vec<i64>,
}

fn response_json(response: &str) -> Value {
    serde_json::from_str(
        response
            .split_once("\r\n\r\n")
            .expect("JSON response has a body")
            .1,
    )
    .expect("response body is JSON")
}

// //// 验证管理员邮件发放和 CN 单封领取 [@x380kkm 2026-07-24] ////
#[test]
fn creates_lists_and_claims_a_local_mail() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 21 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let initial = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    let initial_free_vmoney = initial.data["user_info"]["free_vmoney"]
        .as_i64()
        .expect("initial free vmoney is numeric");
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let created = request_with_headers(
        service.port(),
        "POST",
        "/v1/mails",
        "application/json",
        &authorization,
        format!(
            "{{\"viewer_id\":{viewer_id},\"title\":\"Welcome gift\",\"body\":\"Offline test\",\"sender\":\"Starpoint\",\"rewards\":{{\"freeVmoney\":250}}}}"
        )
        .as_bytes(),
    );
    assert!(created.starts_with("HTTP/1.1 201 Created"));
    let created_body = created
        .split_once("\r\n\r\n")
        .expect("mail response body")
        .1;
    let created_mail: Value = serde_json::from_str(created_body).expect("mail response is JSON");
    let mail_id = created_mail["id"].as_i64().expect("mail id is numeric");

    let index = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/index",
        &encode_request(&MailIndexRequest {
            viewer_id,
            current_page: 1,
        }),
    ));
    assert_eq!(index.data["total_count"].as_i64(), Some(1));
    assert_eq!(index.data["mail"][0]["id"].as_i64(), Some(mail_id));
    assert_eq!(index.data["mail"][0]["type"].as_i64(), Some(4));
    assert_eq!(index.data["mail"][0]["number"].as_i64(), Some(250));
    assert_eq!(
        index.data["mail"][0]["receive_time"].as_str(),
        Some("0000-00-00 00:00:00")
    );

    let received = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive",
        &encode_request(&MailReceiveRequest { viewer_id, mail_id }),
    ));
    assert_eq!(
        received.data["user_info"]["free_vmoney"].as_i64(),
        Some(initial_free_vmoney + 250)
    );
    assert_eq!(received.data["total_count"].as_i64(), Some(1));
    assert!(!received.data["mail_arrived"].as_bool().unwrap_or(true));

    let repeated = cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive",
        &encode_request(&MailReceiveRequest { viewer_id, mail_id }),
    );
    assert!(repeated.starts_with("HTTP/1.1 400 Bad Request"));
    assert!(repeated.ends_with("{\"error\":\"mail_not_found\"}"));
    service.stop().expect("service stops cleanly");
}
// //// /验证管理员邮件发放和 CN 单封领取 ////

// //// 验证特殊资源邮件使用 CN 客户端邮件类型并写入玩家余额 [@x380kkm 2026-08-26] ////
#[test]
fn claims_special_resource_mail_with_reference_kind_mapping() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 221 }),
    ))
    .data_headers
    .viewer_id;
    let initial = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    let resources = [
        ("starCrumb", 7_i64, 17_i64),
        ("bond_token", 10_i64, 3_i64),
        ("bossBoostPoint", 11_i64, 2_i64),
        ("boostPoint", 12_i64, 4_i64),
        ("rankPoint", 15_i64, 5_i64),
        ("expPool", 9_i64, 13_i64),
    ];
    let mut mail_ids = Vec::new();
    for (field, _, amount) in resources {
        let created = request_with_headers(
            service.port(),
            "POST",
            "/v1/mails",
            "application/json",
            &[],
            format!(
                "{{\"viewer_id\":{viewer_id},\"title\":\"{field}\",\"body\":\"special resource\",\"sender\":\"Admin\",\"rewards\":{{\"{field}\":{amount}}}}}"
            )
            .as_bytes(),
        );
        assert!(created.starts_with("HTTP/1.1 201 Created"), "{created}");
        mail_ids.push(response_json(&created)["id"].as_i64().unwrap());
    }
    for ((field, kind, amount), mail_id) in resources.into_iter().zip(mail_ids.iter().copied()) {
        let index = decode_response::<Value>(&cn_support::send_request(
            service.port(),
            "/api/index.php/mail/index",
            &encode_request(&MailIndexRequest {
                viewer_id,
                current_page: 1,
            }),
        ));
        let mail = index.data["mail"]
            .as_array()
            .unwrap()
            .iter()
            .find(|mail| mail["id"] == mail_id)
            .unwrap();
        assert_eq!(mail["type"], kind, "mail field {field}");
        assert_eq!(mail["number"], amount, "mail field {field}");
    }
    let received = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive_all",
        &encode_request(&MailReceiveAllRequest {
            viewer_id,
            mail_ids,
        }),
    ));
    for (response_key, initial_key, amount) in [
        ("star_crumb", "star_crumb", 17_i64),
        ("bond_token", "bond_token", 3_i64),
        ("boss_boost_point", "boss_boost_point", 2_i64),
        ("boost_point", "boost_point", 4_i64),
        ("rank_point", "rank_point", 5_i64),
        ("exp_pool", "exp_pool", 13_i64),
    ] {
        assert_eq!(
            received.data["user_info"][response_key].as_i64(),
            Some(initial.data["user_info"][initial_key].as_i64().unwrap() + amount),
        );
    }
    service.stop().expect("service stops cleanly");
}
// //// /验证特殊资源邮件使用 CN 客户端邮件类型并写入玩家余额 ////

// //// 验证 CN 批量领取道具并保持管理列表可读 [@x380kkm 2026-07-24] ////
#[test]
fn claims_item_mail_in_batch_and_lists_it_for_management() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 22 }),
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
            "{{\"viewer_id\":{viewer_id},\"title\":\"Item gift\",\"body\":\"Use it\",\"sender\":\"Admin\",\"rewards\":{{\"item_list\":{{\"14018\":3}},\"freeMana\":50}}}}"
        )
        .as_bytes(),
    );
    assert!(created.starts_with("HTTP/1.1 201 Created"));
    let mail_id = response_json(&created)["id"]
        .as_i64()
        .expect("created mail id is numeric");

    let managed = request_with_headers(
        service.port(),
        "GET",
        &format!("/v1/mails/{viewer_id}"),
        "application/octet-stream",
        &authorization,
        &[],
    );
    assert!(managed.starts_with("HTTP/1.1 200 OK"));
    assert!(managed.contains("Item gift"));

    let received = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive_all",
        &encode_request(&MailReceiveAllRequest {
            viewer_id,
            mail_ids: vec![mail_id],
        }),
    ));
    assert_eq!(received.data["item_list"]["14018"].as_i64(), Some(3));
    assert_eq!(received.data["ex_boost_item_list"], serde_json::json!([]));
    assert_eq!(received.data["user_info"]["free_mana"].as_i64(), Some(1050));
    assert_eq!(received.data["mail_ids"].as_array().map(Vec::len), Some(1));
    assert_eq!(received.data["total_count"].as_i64(), Some(1));
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 批量领取道具并保持管理列表可读 ////

// //// 验证邮件新角色领取返回百科增量并跨请求保留 [@x380kkm 2026-08-28] ////
#[test]
fn claims_new_character_mail_with_encyclopedia_delta() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 224 }),
    ))
    .data_headers
    .viewer_id;
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let create_mail = |title: &str| {
        let response = request_with_headers(
            service.port(),
            "POST",
            "/v1/mails",
            "application/json",
            &authorization,
            format!(
                "{{\"viewer_id\":{viewer_id},\"title\":\"{title}\",\"body\":\"Character gift\",\"sender\":\"Admin\",\"rewards\":{{\"characterList\":[111001]}}}}"
            )
            .as_bytes(),
        );
        assert!(response.starts_with("HTTP/1.1 201 Created"), "{response}");
        response_json(&response)["id"]
            .as_i64()
            .expect("created mail id is numeric")
    };

    let first_mail_id = create_mail("New character");
    let first = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive_all",
        &encode_request(&MailReceiveAllRequest {
            viewer_id,
            mail_ids: vec![first_mail_id],
        }),
    ));
    assert_eq!(first.data["encyclopedia_info"]["111100101"]["read"], false);

    let second_mail_id = create_mail("Duplicate character");
    let second = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive_all",
        &encode_request(&MailReceiveAllRequest {
            viewer_id,
            mail_ids: vec![second_mail_id],
        }),
    ));
    assert!(second.data.get("encyclopedia_info").is_none());

    let encyclopedia = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/encyclopedia/index",
        &encode_request(&serde_json::json!({"viewer_id": viewer_id})),
    ));
    assert_eq!(
        encyclopedia.data["encyclopedia_list"]["111100101"]["read"],
        false
    );
    service.stop().expect("service stops cleanly");
}
// //// /验证邮件新角色领取返回百科增量并跨请求保留 ////

// //// 验证邮件过期后日期回退不会重复发放 [@x380kkm 2026-08-19] ////
#[test]
fn does_not_reissue_an_expired_mail_after_virtual_date_rewind() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let set_time = |iso: &str| {
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
    };
    set_time("2030-01-01T12:00:00.000Z");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 23 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let created = request_with_headers(
        service.port(),
        "POST",
        "/v1/mails",
        "application/json",
        &authorization,
        format!(
            "{{\"viewer_id\":{viewer_id},\"title\":\"Expiring gift\",\"body\":\"Rewind test\",\"sender\":\"Admin\",\"expires_at\":1893542400,\"rewards\":{{\"freeVmoney\":250}}}}"
        )
        .as_bytes(),
    );
    assert!(created.starts_with("HTTP/1.1 201 Created"));
    let created_body = created
        .split_once("\r\n\r\n")
        .expect("mail response body")
        .1;
    let mail_id = serde_json::from_str::<Value>(created_body).expect("mail response is JSON")["id"]
        .as_i64()
        .expect("mail id is numeric");

    set_time("2030-01-03T12:00:00.000Z");
    let expired = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive",
        &encode_request(&MailReceiveRequest { viewer_id, mail_id }),
    ));
    assert_eq!(expired.data["total_count"], 1);
    assert!(expired.data.get("user_info").is_none());

    set_time("2030-01-01T18:00:00.000Z");
    let index = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/index",
        &encode_request(&MailIndexRequest {
            viewer_id,
            current_page: 1,
        }),
    ));
    assert_eq!(index.data["total_count"], 1);
    assert_eq!(index.data["mail"][0]["id"], mail_id);
    assert_ne!(
        index.data["mail"][0]["receive_time"].as_str(),
        Some("0000-00-00 00:00:00")
    );
    let repeated = cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive",
        &encode_request(&MailReceiveRequest { viewer_id, mail_id }),
    );
    assert!(repeated.starts_with("HTTP/1.1 400 Bad Request"));
    service.stop().expect("service stops cleanly");
}
// //// /验证邮件过期后日期回退不会重复发放 ////

// //// 验证奖励目录使用真实抽卡价格并持久化收藏 [@x380kkm 2026-08-20] ////
#[test]
fn lists_mail_rewards_and_persists_favorites_without_local_authorization() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let catalog = request_with_headers(
        service.port(),
        "GET",
        "/v1/mail-rewards/catalog",
        "application/octet-stream",
        &[],
        &[],
    );
    assert!(catalog.starts_with("HTTP/1.1 200 OK"));
    let catalog: Value = serde_json::from_str(
        catalog
            .split_once("\r\n\r\n")
            .expect("catalog response body")
            .1,
    )
    .expect("catalog response is JSON");
    let catalog_items = catalog["items"]
        .as_array()
        .expect("catalog items are present");
    let item_catalog: Value =
        serde_json::from_str(include_str!("../../../assets/cn_item_catalog.json"))
            .expect("CN item catalog is valid JSON");
    let item_row_count = item_catalog["row_count"]
        .as_u64()
        .expect("CN item row count is present") as usize;
    assert_eq!(
        catalog_items
            .iter()
            .filter(|item| item["resource_id"].is_string())
            .count(),
        item_row_count
    );
    let free_vmoney = catalog_items
        .iter()
        .find(|item| item["key"] == "currency.free-vmoney")
        .expect("free vmoney catalog item is present");
    assert_eq!(free_vmoney["default_amount"], 1_500);
    assert_eq!(free_vmoney["rewards"]["freeVmoney"], 1);
    assert_eq!(
        free_vmoney["image_url"],
        "/manage/assets/item-icons/currency.free-vmoney.png"
    );
    assert_eq!(free_vmoney["favorite"], false);
    let craft_point = catalog_items
        .iter()
        .find(|item| item["key"] == "item.100000")
        .expect("craft point catalog item is present");
    assert_eq!(craft_point["name"], "锻造石");
    assert_eq!(craft_point["kind"], "craft");
    assert_eq!(craft_point["rewards"]["itemList"]["100000"], 1);
    let event_item = catalog_items
        .iter()
        .find(|item| item["key"] == "item.10000004")
        .expect("event catalog item is present");
    assert_eq!(event_item["name"], "摇曳水滴");
    assert_eq!(event_item["kind"], "event");
    let bond_token = catalog_items
        .iter()
        .find(|item| item["key"] == "growth.bond-token")
        .expect("bond token catalog item is present");
    assert_eq!(bond_token["rewards"]["bondToken"], 1);
    let star_crumb = catalog_items
        .iter()
        .find(|item| item["key"] == "currency.star-crumb")
        .expect("star crumb catalog item is present");
    assert_eq!(star_crumb["rewards"]["starCrumb"], 1);
    let rank_point = catalog_items
        .iter()
        .find(|item| item["key"] == "growth.rank-point")
        .expect("rank point catalog item is present");
    assert_eq!(rank_point["rewards"]["rankPoint"], 1);

    let gacha: Value = serde_json::from_str(include_str!("../../../assets/cn_gacha.json"))
        .expect("CN gacha configuration is valid JSON");
    let multi_draw_cost = gacha["1"]["multiCost"]
        .as_i64()
        .expect("CN multi-draw cost is present");
    let expected_cost = multi_draw_cost * 20;
    let quick_pull = catalog["presets"]
        .as_array()
        .expect("catalog presets are present")
        .iter()
        .find(|preset| preset["key"] == "gacha.200-pulls")
        .expect("200-pull preset is present");
    assert_eq!(quick_pull["rewards"]["vmoney"], expected_cost);
    assert_eq!(
        quick_pull["image_url"],
        "/manage/assets/item-icons/currency.free-vmoney.png"
    );
    assert_eq!(quick_pull["source"]["multi_draw_cost"], multi_draw_cost);
    let stamina_item = catalog_items
        .iter()
        .find(|item| item["key"] == "stamina.recovery.large")
        .expect("large stamina recovery item is present");
    assert_eq!(stamina_item["resource_id"], "106");
    assert_eq!(stamina_item["rewards"]["itemList"]["106"], 1);
    assert_eq!(
        stamina_item["image_url"],
        "/manage/assets/item-icons/stamina.recovery.large.png"
    );
    let stamina_preset = catalog["presets"]
        .as_array()
        .expect("catalog presets are present")
        .iter()
        .find(|preset| preset["key"] == "stamina.large-potions")
        .expect("large stamina recovery preset is present");
    assert_eq!(stamina_preset["rewards"]["itemList"]["106"], 10);
    assert_eq!(
        stamina_preset["image_url"],
        "/manage/assets/item-icons/stamina.recovery.large.png"
    );
    assert_eq!(stamina_preset["source"]["stamina_recovery"], 100);

    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 24 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let initial = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    let initial_vmoney = initial.data["user_info"]["vmoney"]
        .as_i64()
        .expect("initial paid vmoney is numeric");
    let quick_mail = serde_json::json!({
        "viewer_id": viewer_id,
        "title": "200 抽资源",
        "body": "目录快捷发放测试",
        "sender": "Starpoint",
        "rewards": quick_pull["rewards"].clone(),
    });
    let created = request_with_headers(
        service.port(),
        "POST",
        "/v1/mails",
        "application/json",
        &[],
        quick_mail.to_string().as_bytes(),
    );
    assert!(created.starts_with("HTTP/1.1 201 Created"));
    let quick_mail_id = response_json(&created)["id"]
        .as_i64()
        .expect("quick mail id is numeric");
    let received = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive_all",
        &encode_request(&MailReceiveAllRequest {
            viewer_id,
            mail_ids: vec![quick_mail_id],
        }),
    ));
    assert_eq!(
        received.data["user_info"]["vmoney"],
        initial_vmoney + expected_cost
    );
    let stamina_mail = serde_json::json!({
        "viewer_id": viewer_id,
        "title": "大体力回复药",
        "body": "目录快捷发放测试",
        "sender": "Starpoint",
        "rewards": stamina_preset["rewards"].clone(),
    });
    let created = request_with_headers(
        service.port(),
        "POST",
        "/v1/mails",
        "application/json",
        &[],
        stamina_mail.to_string().as_bytes(),
    );
    assert!(created.starts_with("HTTP/1.1 201 Created"));
    let stamina_mail_id = response_json(&created)["id"]
        .as_i64()
        .expect("stamina mail id is numeric");
    let received = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive_all",
        &encode_request(&MailReceiveAllRequest {
            viewer_id,
            mail_ids: vec![stamina_mail_id],
        }),
    ));
    assert_eq!(received.data["item_list"]["106"], 10);
    let craft_mail = serde_json::json!({
        "viewer_id": viewer_id,
        "title": "锻造石",
        "body": "目录物品发放测试",
        "sender": "Starpoint",
        "rewards": craft_point["rewards"].clone(),
    });
    let created = request_with_headers(
        service.port(),
        "POST",
        "/v1/mails",
        "application/json",
        &[],
        craft_mail.to_string().as_bytes(),
    );
    assert!(created.starts_with("HTTP/1.1 201 Created"));
    let craft_mail_id = response_json(&created)["id"]
        .as_i64()
        .expect("craft mail id is numeric");
    let received = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive_all",
        &encode_request(&MailReceiveAllRequest {
            viewer_id,
            mail_ids: vec![craft_mail_id],
        }),
    ));
    assert_eq!(received.data["item_list"]["100000"], 1);

    let favorite = request_with_headers(
        service.port(),
        "PUT",
        "/v1/mail-rewards/catalog/item.100000/favorite",
        "application/json",
        &[],
        br#"{"favorite":true}"#,
    );
    assert!(favorite.starts_with("HTTP/1.1 200 OK"));
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
    assert_eq!(
        loaded.data["user_info"]["vmoney"],
        initial_vmoney + expected_cost
    );
    assert_eq!(loaded.data["item_list"]["106"], 10);
    let catalog = request_with_headers(
        restarted.port(),
        "GET",
        "/v1/mail-rewards/catalog",
        "application/octet-stream",
        &[],
        &[],
    );
    let catalog: Value = serde_json::from_str(
        catalog
            .split_once("\r\n\r\n")
            .expect("restarted catalog response body")
            .1,
    )
    .expect("restarted catalog response is JSON");
    let craft_point = catalog["items"]
        .as_array()
        .expect("restarted catalog items are present")
        .iter()
        .find(|item| item["key"] == "item.100000")
        .expect("restarted craft point item is present");
    assert_eq!(craft_point["favorite"], true);

    let missing = request_with_headers(
        restarted.port(),
        "PUT",
        "/v1/mail-rewards/catalog/item.unknown/favorite",
        "application/json",
        &[],
        br#"{"favorite":true}"#,
    );
    assert!(missing.starts_with("HTTP/1.1 404 Not Found"));
    restarted.stop().expect("service stops cleanly");
}
// //// /验证奖励目录使用真实抽卡价格并持久化收藏 ////
