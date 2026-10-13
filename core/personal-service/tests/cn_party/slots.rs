// audience: internal
// # personal-service-cn-party-slot-tests
// 编队装配通过实际接口保存, 快照核对资产持有量, 引用和重启后的内容.

use super::*;
use local_save_support::export_local_save;
use serde_json::json;

pub(super) fn send(service: &PersonalService, route: &str, body: Value) -> String {
    cn_support::send_request(
        service.port(),
        &format!("/api/index.php/{route}"),
        &encode_request(&body),
    )
}

pub(super) fn data(service: &PersonalService, route: &str, body: Value) -> Value {
    let response = send(service, route, body);
    assert!(response.starts_with("HTTP/1.1 200"), "{route}: {response}");
    decode_response::<Value>(&response).data
}

pub(super) fn player(service: &PersonalService, slot: i64) -> Value {
    export_local_save(service, slot)["data"].clone()
}

pub(super) fn prepare(root: &TempDir, service: &PersonalService) -> (i64, i64) {
    let viewer =
        decode_response::<SignupData>(&send(service, "tool/signup", json!({"device_id": 1011})))
            .data_headers
            .viewer_id;
    let slot = list_local_saves(service)["slots"][0]["id"]
        .as_i64()
        .unwrap();
    let authorization = format!("Bearer {}", service.management_token());
    let granted = request_with_headers(service.port(), "POST", "/v1/mails", "application/json",
        &[("Authorization", authorization.as_str())],
        json!({"viewer_id": viewer, "title":"编队", "body":"编队素材.", "sender":"Starpoint",
            "rewards":{"equipmentList":{"5030037":1},"itemList":{"100001":2,"100002":1,"5090042":1}}}).to_string().as_bytes());
    assert!(granted.starts_with("HTTP/1.1 201"), "{granted}");
    data(service, "mail/receive_all", json!({"viewer_id":viewer}));
    update_slot_player_snapshot(root.path(), slot, |state| {
        let character = state["user_character_list"]["1"].clone();
        state["user_character_list"]["111001"] = character.clone();
        state["user_character_list"]["231001"] = character;
    });
    (viewer, slot)
}

pub(super) fn party(category: i64, id: i64) -> Value {
    json!({
        "party_category":category,"party_id":id,"party_name":format!("编队 {category}:{id}"),
        "party_edited":true,"current_battle_power":123,"before_battle_power":100,
        "character_ids":[1,111001,231001],"unison_character_ids":[null,null,null],
        "equipment_ids":[5030037,null,null],"ability_soul_ids":[100001,100001,100002],
        "options":{"allow_other_players_to_heal_me":false}
    })
}

pub(super) fn edit_body(viewer: i64, parties: Vec<Value>) -> Value {
    json!({"viewer_id":viewer,"main_party_id":12,"use_party_group_edit":false,
        "ignore_ngword":true,"api_count":1,"party_info_list":parties})
}

// //// 验证魂珠装配和替换保留库存并限制同队数量 [@x380kkm 2026-09-23] ////
#[test]
fn equips_swaps_and_removes_souls_without_spending_inventory() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let (viewer, slot) = prepare(&root, &service);
    let initial = player(&service, slot);
    data(
        &service,
        "party/edit",
        edit_body(viewer, vec![party(1, 1), party(1, 12)]),
    );
    let equipped = player(&service, slot);
    assert_eq!(equipped["item_list"], initial["item_list"]);
    assert_eq!(equipped["all_active_mission_list"]["14060"]["progress"], 1);
    assert_eq!(
        equipped["user_party_group_list"]["1"]["list"]["1"]["ability_soul_ids"],
        json!([100001, 100001, 100002])
    );
    assert_eq!(
        equipped["user_party_group_list"]["2"]["list"]["12"]["ability_soul_ids"],
        json!([100001, 100001, 100002])
    );
    let mut changed = party(1, 1);
    changed["ability_soul_ids"] = json!([100002, null, 100001]);
    data(&service, "party/edit", edit_body(viewer, vec![changed]));
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let loaded = data(
        &service,
        "load",
        json!({"viewer_id":viewer,"keychain":viewer}),
    );
    assert_eq!(
        loaded["user_party_group_list"]["1"]["list"]["1"]["ability_soul_ids"],
        json!([100002, null, 100001])
    );
    for id in ["100001", "100002", "5090042"] {
        assert_eq!(loaded["item_list"][id], initial["item_list"][id]);
    }
    let mut excess = party(1, 1);
    excess["ability_soul_ids"] = json!([100001, 100001, 100001]);
    excess["equipment_ids"] = json!([5030037, 5030037, null]);
    data(&service, "party/edit", edit_body(viewer, vec![excess]));
    let saved = player(&service, slot);
    let saved_party = &saved["user_party_group_list"]["1"]["list"]["1"];
    assert_eq!(
        saved_party["ability_soul_ids"],
        json!([100001, 100001, null])
    );
    assert_eq!(saved_party["equipment_ids"], json!([5030037, null, null]));
    assert_eq!(saved["item_list"], loaded["item_list"]);
    service.stop().unwrap();
}

// //// 验证非法引用和旧存档槽位按客户端规则整理 [@x380kkm 2026-09-23] ////
#[test]
fn normalizes_invalid_references_and_keeps_empty_parties() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let (viewer, slot) = prepare(&root, &service);
    let mut invalid = party(1, 1);
    invalid["character_ids"] = json!([1, 1]);
    invalid["unison_character_ids"] = json!([1, 111001, 999999]);
    invalid["equipment_ids"] = json!([5030037, 5030037, 5030037]);
    invalid["ability_soul_ids"] = json!([5090042, 100001, 7]);
    data(&service, "party/edit", edit_body(viewer, vec![invalid]));
    let saved = player(&service, slot);
    let normalized = &saved["user_party_group_list"]["1"]["list"]["1"];
    assert_eq!(
        saved["all_active_mission_list"]["14060"]["progress"]
            .as_i64()
            .unwrap_or_default(),
        0
    );
    assert_eq!(normalized["character_ids"], json!([1, null, null]));
    assert_eq!(
        normalized["unison_character_ids"],
        json!([null, 111001, null])
    );
    assert_eq!(normalized["equipment_ids"], json!([5030037, null, null]));
    assert_eq!(normalized["ability_soul_ids"], json!([null, null, null]));
    update_slot_player_snapshot(root.path(), slot, |state| {
        let group = &mut state["user_party_group_list"]["1"];
        group["color_id"] = json!(999);
        group["list"]["2"]["character_ids"] = json!([]);
        group["list"]["2"]["equipment_ids"] = json!([5030037]);
        group["list"]["2"]["ability_soul_ids"] = json!([100001]);
        group["list"]["2"]["name"] = json!("空队");
    });
    let loaded = data(
        &service,
        "load",
        json!({"viewer_id":viewer,"keychain":viewer}),
    );
    let group = &loaded["user_party_group_list"]["1"];
    assert_eq!(group["color_id"], 15);
    let empty = &group["list"]["2"];
    assert_eq!(empty["name"], "空队");
    assert_eq!(empty["character_ids"], json!([null, null, null]));
    assert_eq!(empty["equipment_ids"], json!([null, null, null]));
    assert_eq!(empty["ability_soul_ids"], json!([null, null, null]));
    assert_eq!(
        player(&service, slot)["user_party_group_list"]["1"]["list"]["2"],
        *empty
    );
    service.stop().unwrap();
}
