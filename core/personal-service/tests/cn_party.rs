// audience: internal
// # personal-service-cn-party-tests
//
// 该文件验证 CN 编队编辑的所有权过滤、主编队选择和快照持久化.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[path = "support/local_saves.rs"]
#[allow(dead_code)]
mod local_save_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, LoadRequest, SignupData, SignupRequest};
use local_save_support::{list_local_saves, update_slot_player_snapshot};
use serde::Serialize;
use serde_json::Value;
use starpoint_personal_service::PersonalService;
use support::request_with_headers;
use tempfile::TempDir;

#[derive(Serialize)]
struct MailReceiveAllRequest {
    viewer_id: i64,
}

#[derive(Serialize)]
struct PartyEditRequest {
    viewer_id: i64,
    main_party_id: i64,
    use_party_group_edit: bool,
    ignore_ngword: bool,
    api_count: i64,
    party_info_list: Vec<PartyInfo>,
}

#[derive(Serialize)]
struct PartyInfo {
    party_edited: bool,
    party_category: i64,
    party_name: String,
    party_id: i64,
    current_battle_power: i64,
    before_battle_power: i64,
    unison_character_ids: Vec<Option<i64>>,
    equipment_ids: Vec<Option<i64>>,
    character_ids: Vec<Option<i64>>,
    ability_soul_ids: Vec<Option<i64>>,
    options: PartyOptions,
}

#[derive(Serialize)]
struct PartyOptions {
    allow_other_players_to_heal_me: bool,
}

#[derive(Serialize)]
struct PartyReferRequest {
    viewer_id: i64,
    party_code: String,
}

// //// 验证 CN 编队编辑所有权和快照持久化 [@x380kkm 2026-07-24] ////
#[test]
fn edits_party_and_filters_unowned_members() {
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
    let mail = format!(
        "{{\"viewer_id\":{viewer_id},\"title\":\"Party\",\"body\":\"Party test\",\"sender\":\"Admin\",\"rewards\":{{\"equipmentList\":{{\"5030037\":1}}}}}}"
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

    let edited = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/party/edit",
        &encode_request(&PartyEditRequest {
            viewer_id,
            main_party_id: 12,
            use_party_group_edit: false,
            ignore_ngword: true,
            api_count: 1,
            party_info_list: vec![PartyInfo {
                party_edited: true,
                party_category: 3,
                party_name: "CN Party".to_owned(),
                party_id: 12,
                current_battle_power: 12_345,
                before_battle_power: 11_111,
                unison_character_ids: vec![Some(999_999), None, None],
                equipment_ids: vec![Some(5_030_037), Some(5_040_028), None],
                character_ids: vec![Some(1), Some(999_999), None],
                ability_soul_ids: vec![Some(7), None, None],
                options: PartyOptions {
                    allow_other_players_to_heal_me: false,
                },
            }],
        }),
    ));
    assert_eq!(edited.data["mail_arrived"], false);

    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(loaded.data["user_info"]["party_slot"], 12);
    assert_eq!(
        loaded.data["user_party_group_list"]["1"]["list"]["1"]["name"],
        "Party A"
    );
    let party = &loaded.data["user_party_group_list"]["2"]["list"]["12"];
    assert_eq!(party["name"], "CN Party");
    assert_eq!(party["current_battle_power"], 12_345);
    assert_eq!(party["before_battle_power"], 11_111);
    assert_eq!(party["character_ids"], serde_json::json!([1, null, null]));
    assert_eq!(
        party["unison_character_ids"],
        serde_json::json!([null, null, null])
    );
    assert_eq!(
        party["equipment_ids"],
        serde_json::json!([5_030_037, null, null])
    );
    assert_eq!(
        party["ability_soul_ids"],
        serde_json::json!([7, null, null])
    );
    assert_eq!(party["options"]["allow_other_players_to_heal_me"], false);
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 编队编辑所有权和快照持久化 ////

// //// 验证完整编队列表按全局编号写入对应分组 [@x380kkm 2026-08-24] ////
#[test]
fn edits_all_twelve_party_groups_in_one_request() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 144 }),
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
    assert_eq!(
        initial.data["user_party_group_list"]
            .as_object()
            .map(serde_json::Map::len),
        Some(12)
    );
    assert_eq!(
        initial.data["user_party_group_list"]["12"]["list"]["120"]["name"],
        "Party J"
    );
    let party_info_list = (1..=120)
        .map(|party_id| PartyInfo {
            party_edited: true,
            party_category: 3,
            party_name: format!("Party {party_id}"),
            party_id,
            current_battle_power: party_id,
            before_battle_power: party_id - 1,
            unison_character_ids: vec![None, None, None],
            equipment_ids: vec![None, None, None],
            character_ids: vec![Some(1), None, None],
            ability_soul_ids: vec![None, None, None],
            options: PartyOptions {
                allow_other_players_to_heal_me: true,
            },
        })
        .collect();
    let edited = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/party/edit",
        &encode_request(&PartyEditRequest {
            viewer_id,
            main_party_id: 120,
            use_party_group_edit: false,
            ignore_ngword: true,
            api_count: 1,
            party_info_list,
        }),
    ));
    assert_eq!(edited.data["mail_arrived"], false);

    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(loaded.data["user_info"]["party_slot"], 120);
    for party_id in 1..=120 {
        let group_id = (party_id - 1) / 10 + 1;
        let group_key = group_id.to_string();
        let party_key = party_id.to_string();
        let expected_name = format!("Party {party_id}");
        assert_eq!(
            loaded.data["user_party_group_list"][group_key.as_str()]["list"][party_key.as_str()]
                ["name"]
                .as_str(),
            Some(expected_name.as_str())
        );
    }

    let published = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/party/publish",
        &encode_request(&serde_json::json!({"viewer_id": viewer_id})),
    ));
    let party_code = published.data["party_code"]
        .as_str()
        .expect("party code is text")
        .to_owned();
    let referred = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/party/refer",
        &encode_request(&PartyReferRequest {
            viewer_id,
            party_code,
        }),
    ));
    assert_eq!(referred.data["party_name"], "Party 120");
    service.stop().expect("service stops cleanly");
}
// //// /验证完整编队列表按全局编号写入对应分组 ////

// //// 验证旧编队快照补齐缺失分组并保留已有槽位 [@x380kkm 2026-08-28] ////
#[test]
fn completes_legacy_party_groups_without_replacing_existing_parties() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 244 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let slot_id = list_local_saves(&service)["slots"][0]["id"]
        .as_i64()
        .expect("signup creates a local save slot");
    update_slot_player_snapshot(root.path(), slot_id, |player_data| {
        let groups = player_data["user_party_group_list"]
            .as_object_mut()
            .expect("party groups are an object");
        for group_id in 7..=12 {
            groups.remove(&group_id.to_string());
        }
        groups["1"]["color_id"] = Value::from(9);
        groups["1"]["list"]["1"]["name"] = Value::String("Preserved Party".to_owned());
    });

    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    let groups = loaded.data["user_party_group_list"]
        .as_object()
        .expect("party groups are an object");
    assert_eq!(groups.len(), 12);
    assert_eq!(groups["1"]["color_id"], 9);
    assert_eq!(groups["1"]["list"]["1"]["name"], "Preserved Party");
    assert_eq!(groups["12"]["list"]["120"]["name"], "Party J");
    assert_eq!(groups["12"]["list"]["120"]["character_ids"][0], 1);
    service.stop().expect("service stops cleanly");
}
// //// /验证旧编队快照补齐缺失分组并保留已有槽位 ////

// //// 验证本地编队分享和引用 [@x380kkm 2026-08-24] ////
#[test]
fn publishes_and_refers_to_active_party() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 134 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let published = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/party/publish",
        &encode_request(&serde_json::json!({"viewer_id": viewer_id})),
    ));
    let party_code = published.data["party_code"]
        .as_str()
        .expect("party code is text")
        .to_owned();
    let referred = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/party/refer",
        &encode_request(&PartyReferRequest {
            viewer_id,
            party_code,
        }),
    ));
    assert!(referred.data["party_name"].is_string());
    assert_eq!(
        referred.data["battle_party"]["characters"]
            .as_array()
            .map(Vec::len),
        Some(3)
    );
    assert_eq!(
        referred.data["battle_party"]["equipments"]
            .as_array()
            .map(Vec::len),
        Some(3)
    );
    assert_eq!(
        referred.data["battle_party"]["unison_characters"]
            .as_array()
            .map(Vec::len),
        Some(3)
    );
    service.stop().expect("service stops cleanly");
}
// //// /验证本地编队分享和引用 ////
