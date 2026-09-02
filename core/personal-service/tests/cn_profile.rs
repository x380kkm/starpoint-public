// audience: internal
// # personal-service-cn-profile-tests
//
// 该文件验证 CN 个人资料响应来自当前玩家快照并校验 viewer session.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, LoadRequest, SignupData, SignupRequest};
use serde::Serialize;
use serde_json::Value;
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

#[derive(Serialize)]
struct ProfileRequest {
    viewer_id: i64,
}

#[derive(Serialize)]
struct TargetProfileRequest {
    viewer_id: i64,
    target_viewer_id: i64,
}

#[derive(Serialize)]
struct UpdateDegreeRequest {
    viewer_id: i64,
    degree_id: i64,
}

#[derive(Serialize)]
struct UpdateSettingsRequest {
    viewer_id: i64,
    profile_settings: ProfileSettings,
}

#[derive(Serialize)]
struct ProfileSettings {
    show_opened_mana_board_second_count: bool,
    show_owned_character_count: bool,
    show_owned_degree_count: bool,
}

#[derive(Serialize)]
struct TextRequest {
    viewer_id: i64,
    name: String,
    comment: String,
}

// //// 验证 CN 个人资料映射和 viewer session [@x380kkm 2026-08-22] ////
#[test]
fn returns_profile_from_the_current_player_snapshot() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 68 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let profile = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/profile/get_my_profile",
        &encode_request(&ProfileRequest { viewer_id }),
    ));
    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    let owned_character_count = loaded.data["user_character_list"]
        .as_object()
        .map(|characters| characters.len() as i64)
        .expect("loaded character list is an object");
    assert_eq!(
        profile.data["profile_info"]["owned_character_count"],
        owned_character_count
    );
    assert_eq!(
        profile.data["profile_info"]["max_owned_character_count"],
        owned_character_count
    );
    assert_eq!(
        profile.data["user_party_group_list"][0]["party_group_color_id"],
        loaded.data["user_party_group_list"]["1"]["color_id"]
    );
    assert_eq!(
        profile.data["user_party_group_list"][0]["party_list"][0]["character_ids"],
        loaded.data["user_party_group_list"]["1"]["list"]["1"]["character_ids"]
    );

    let degree = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/profile/update_degree",
        &encode_request(&UpdateDegreeRequest {
            viewer_id,
            degree_id: 2,
        }),
    ));
    assert_eq!(degree.data["user_info"]["degree_id"], 2);
    let settings = ProfileSettings {
        show_opened_mana_board_second_count: true,
        show_owned_character_count: false,
        show_owned_degree_count: true,
    };
    decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/profile/update_profile_settings",
        &encode_request(&UpdateSettingsRequest {
            viewer_id,
            profile_settings: settings,
        }),
    ));
    let text = TextRequest {
        viewer_id,
        name: "Offline Player".to_owned(),
        comment: "Local profile".to_owned(),
    };
    decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/profile/rename",
        &encode_request(&text),
    ));
    decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/profile/update_comment",
        &encode_request(&text),
    ));
    let updated_profile = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/profile/get_my_profile",
        &encode_request(&ProfileRequest { viewer_id }),
    ));
    assert_eq!(
        updated_profile.data["profile_settings"]["show_owned_character_count"],
        false
    );
    let updated_player = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(updated_player.data["user_info"]["degree_id"], 2);
    assert_eq!(updated_player.data["user_info"]["name"], "Offline Player");
    assert_eq!(updated_player.data["user_info"]["comment"], "Local profile");

    let target_profile = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/profile/get_profile",
        &encode_request(&TargetProfileRequest {
            viewer_id,
            target_viewer_id: viewer_id,
        }),
    ));
    assert_eq!(
        target_profile.data["target_user_info"]["name"],
        "Offline Player"
    );
    assert_eq!(target_profile.data["target_user_info"]["degree_id"], 2);
    assert!(target_profile.data["favorite_character"]["character_ids"]
        .as_array()
        .is_some_and(|characters| !characters.is_empty()));

    let invalid = cn_support::send_request(
        service.port(),
        "/api/index.php/profile/get_my_profile",
        &encode_request(&ProfileRequest {
            viewer_id: 999_999_999,
        }),
    );
    assert!(invalid.starts_with("HTTP/1.1 400 Bad Request"));
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 个人资料映射和 viewer session ////
