// audience: internal
// # personal-service-profile-party-tests
//
// 该模块验证个人资料展示编队与战斗编队分别保存并在重启后保持各自内容.

use super::*;
use local_save_support::export_local_save;
use serde_json::json;

// //// 构造客户端编队编辑项 [@x380kkm 2026-09-20] ////
fn party_info(category: i64, party_id: i64, characters: Value) -> Value {
    json!({
        "party_edited": true,
        "party_category": category,
        "party_name": format!("Party {category}:{party_id}"),
        "party_id": party_id,
        "current_battle_power": 12345,
        "before_battle_power": 9876,
        "character_ids": characters,
        "unison_character_ids": [null, null, null],
        "equipment_ids": [null, null, null],
        "ability_soul_ids": [null, null, null],
        "options": {"allow_other_players_to_heal_me": false}
    })
}
// //// /构造客户端编队编辑项 ////

// //// 通过编队接口保存指定类别的队伍 [@x380kkm 2026-09-20] ////
fn edit_parties(service: &PersonalService, viewer_id: i64, main_id: i64, parties: Vec<Value>) {
    let response = cn_support::send_request(
        service.port(),
        "/api/index.php/party/edit",
        &encode_request(&json!({
            "viewer_id": viewer_id,
            "main_party_id": main_id,
            "use_party_group_edit": false,
            "ignore_ngword": true,
            "api_count": 1,
            "party_info_list": parties
        })),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
}
// //// /通过编队接口保存指定类别的队伍 ////

// //// 验证个人资料喜爱角色与多组战斗编队双向隔离 [@x380kkm 2026-09-20] ////
#[test]
fn saves_profile_favorites_without_replacing_battle_parties() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 399 }),
    ))
    .data_headers
    .viewer_id;
    let slot_id = list_local_saves(&service)["slots"][0]["id"]
        .as_i64()
        .expect("signup creates a local save slot");
    update_slot_player_snapshot(root.path(), slot_id, |data| {
        let character = data["user_character_list"]["1"].clone();
        data["user_character_list"]["111001"] = character.clone();
        data["user_character_list"]["231001"] = character;
    });
    edit_parties(
        &service,
        viewer_id,
        12,
        vec![
            party_info(1, 1, json!([1, 111001, 231001])),
            party_info(1, 2, json!([111001, 231001, 1])),
            party_info(1, 12, json!([231001, 1, 111001])),
        ],
    );
    let before = export_local_save(&service, slot_id)["data"].clone();
    let favorites = json!([231001, 111001, null]);
    edit_parties(
        &service,
        viewer_id,
        1,
        vec![party_info(99, 1, favorites.clone())],
    );
    let after = export_local_save(&service, slot_id)["data"].clone();
    assert_eq!(
        after["user_party_group_list"],
        before["user_party_group_list"]
    );
    assert_eq!(after["user_info"], before["user_info"]);
    assert_eq!(
        after["favorite_party_group_list"][0]["party_list"][0]["character_ids"],
        favorites
    );
    assert_eq!(
        after["profile_party_group_list"]["1"]["list"]["1"]["character_ids"],
        favorites
    );
    let mut unchanged = after.clone();
    unchanged
        .as_object_mut()
        .unwrap()
        .remove("profile_party_group_list");
    unchanged["favorite_party_group_list"] = before["favorite_party_group_list"].clone();
    assert_eq!(unchanged, before);

    edit_parties(
        &service,
        viewer_id,
        12,
        vec![party_info(1, 1, json!([111001, 1, 231001]))],
    );
    let updated = export_local_save(&service, slot_id)["data"].clone();
    assert_eq!(
        updated["profile_party_group_list"],
        after["profile_party_group_list"]
    );
    assert_eq!(
        updated["favorite_party_group_list"],
        after["favorite_party_group_list"]
    );
    assert_eq!(
        updated["user_party_group_list"]["1"]["list"]["1"]["character_ids"],
        json!([111001, 1, 231001])
    );
    service.stop().expect("service stops cleanly");
    let restarted = PersonalService::start(root.path(), 0).expect("service restarts");
    let profile = decode_response::<Value>(&cn_support::send_request(
        restarted.port(),
        "/api/index.php/profile/get_my_profile",
        &encode_request(&json!({"viewer_id": viewer_id})),
    ));
    assert_eq!(
        profile.data["user_party_group_list"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        profile.data["user_party_group_list"][0]["party_list"][0]["character_ids"],
        favorites
    );
    let public_profile = decode_response::<Value>(&cn_support::send_request(
        restarted.port(),
        "/api/index.php/profile/get_profile",
        &encode_request(&json!({"viewer_id": viewer_id, "target_viewer_id": viewer_id})),
    ));
    assert_eq!(
        public_profile.data["favorite_character"]["character_ids"],
        favorites
    );
    let loaded = decode_response::<Value>(&cn_support::send_request(
        restarted.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(
        loaded.data["user_party_group_list"],
        updated["user_party_group_list"]
    );
    assert_eq!(loaded.data["user_info"]["party_slot"], 12);
    assert_eq!(
        loaded.data["favorite_party_group_list"],
        profile.data["user_party_group_list"]
    );
    restarted.stop().expect("service stops cleanly");
}
// //// /验证个人资料喜爱角色与多组战斗编队双向隔离 ////

// //// 验证空喜爱编队的首页响应使用持有角色并保留存档选择 [@x380kkm 2026-09-30] ////
#[test]
fn projects_owned_home_character_for_empty_profile_parties() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 400 }),
    ))
    .data_headers
    .viewer_id;
    let slot_id = list_local_saves(&service)["slots"][0]["id"]
        .as_i64()
        .unwrap();
    edit_parties(
        &service,
        viewer_id,
        1,
        vec![party_info(99, 1, json!([null, null, null]))],
    );
    let stored_groups =
        export_local_save(&service, slot_id)["data"]["profile_party_group_list"].clone();
    for groups in [
        stored_groups,
        Value::Null,
        json!({}),
        json!({"1": {"color_id": 7, "list": {}}}),
        json!({"1": {"list": {"1": {"character_ids": [999999]}}}}),
    ] {
        update_slot_player_snapshot(root.path(), slot_id, |data| {
            data["profile_party_group_list"] = groups.clone();
        });
        let loaded = decode_response::<Value>(&cn_support::send_request(
            service.port(),
            "/api/index.php/load",
            &encode_request(&LoadRequest {
                keychain: viewer_id,
                viewer_id,
            }),
        ));
        assert_eq!(
            loaded.data["favorite_party_group_list"][0]["party_list"][0]["character_ids"],
            json!([1, null, null])
        );
        let profile = decode_response::<Value>(&cn_support::send_request(
            service.port(),
            "/api/index.php/profile/get_my_profile",
            &encode_request(&json!({"viewer_id": viewer_id})),
        ));
        assert_eq!(
            loaded.data["favorite_party_group_list"],
            profile.data["user_party_group_list"]
        );
        assert_eq!(
            export_local_save(&service, slot_id)["data"]["profile_party_group_list"],
            groups
        );
        let edited = decode_response::<Value>(&cn_support::send_request(
            service.port(),
            "/api/index.php/party_group/edit",
            &encode_request(&json!({
                "viewer_id":viewer_id,"party_group_edit_params_list":[{
                    "party_group_id":1,"party_category":99,"party_group_color_id":2,
                }],
            })),
        ));
        assert_eq!(edited.data_headers.result_code, 1);
        let reloaded = decode_response::<Value>(&cn_support::send_request(
            service.port(),
            "/api/index.php/load",
            &encode_request(&LoadRequest {
                keychain: viewer_id,
                viewer_id,
            }),
        ));
        assert_eq!(
            reloaded.data["favorite_party_group_list"][0]["party_group_color_id"],
            2
        );
        edit_parties(
            &service,
            viewer_id,
            1,
            vec![party_info(99, 1, json!([1, null, null]))],
        );
        assert_eq!(
            export_local_save(&service, slot_id)["data"]["profile_party_group_list"]["1"]["list"]
                ["1"]["character_ids"],
            json!([1, null, null])
        );
    }
    service.stop().expect("service stops cleanly");
}
// //// /验证空喜爱编队的首页响应使用持有角色并保留存档选择 ////

// //// 验证稀疏展示存档补齐客户端字段并保留有效偏好 [@x380kkm 2026-09-30] ////
#[test]
fn projects_sparse_profile_fields_without_changing_saved_preferences() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 401 }),
    ))
    .data_headers
    .viewer_id;
    let slot_id = list_local_saves(&service)["slots"][0]["id"]
        .as_i64()
        .unwrap();
    let stored_groups = json!({"1": {"list": {"1": {
        "name": "首页角色",
        "character_ids": [111001],
        "unison_character_ids": [1],
        "options": {"allow_other_players_to_heal_me": false}
    }}}});
    update_slot_player_snapshot(root.path(), slot_id, |data| {
        data["user_character_list"]["111001"] = data["user_character_list"]["1"].clone();
        data["profile_party_group_list"] = stored_groups.clone();
    });
    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    let favorites = &loaded.data["favorite_party_group_list"][0]["party_list"][0];
    assert_eq!(favorites["character_ids"], json!([111001, null, null]));
    assert_eq!(favorites["unison_character_ids"], json!([1, null, null]));
    assert_eq!(favorites["ability_soul_ids"], json!([null, null, null]));
    assert_eq!(favorites["equipment_ids"], json!([null, null, null]));
    assert_eq!(favorites["party_name"], "首页角色");
    assert_eq!(favorites["party_edited"], false);
    assert_eq!(
        favorites["options"]["allow_other_players_to_heal_me"],
        false
    );
    let profile = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/profile/get_my_profile",
        &encode_request(&json!({"viewer_id": viewer_id})),
    ));
    assert_eq!(
        loaded.data["favorite_party_group_list"],
        profile.data["user_party_group_list"]
    );
    assert_eq!(
        export_local_save(&service, slot_id)["data"]["profile_party_group_list"],
        stored_groups
    );
    service.stop().expect("service stops cleanly");
}
// //// /验证稀疏展示存档补齐客户端字段并保留有效偏好 ////
