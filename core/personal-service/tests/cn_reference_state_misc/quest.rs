// audience: internal
// # personal-service-cn-quest-unlock-tests
// 一次性解锁消耗按分类隔离, 重复请求和重启均沿用已保存的解锁状态.

use super::*;

// //// 解锁费用与持久化状态保持一致 [@x380kkm 2026-09-23] ////
#[test]
fn charges_world_story_unlock_once_and_preserves_other_categories() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 923_618 }),
    ))
    .data_headers
    .viewer_id;
    service.stop().unwrap();
    update_player(root.path(), |player| {
        player["item_list"] = json!({"60000": 1, "60001": 1});
    });
    let service = PersonalService::start(root.path(), 0).unwrap();
    for category in [11, 14] {
        let response = decode_response::<Value>(&cn_support::send_request(
            service.port(),
            "/api/index.php/quest/unlock",
            &encode_request(&QuestUnlockRequest {
                viewer_id,
                category,
                quest_id: 1001,
                api_count: category,
            }),
        ));
        assert_eq!(response.data["item_list"], json!({}));
    }
    let request = encode_request(&QuestUnlockRequest {
        viewer_id,
        category: 18,
        quest_id: 400_001_102,
        api_count: 18,
    });
    for _ in 0..2 {
        let response = decode_response::<Value>(&cn_support::send_request(
            service.port(),
            "/api/index.php/quest/unlock",
            &request,
        ));
        assert_eq!(response.data["item_list"]["60001"], 0);
    }
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let replay = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/quest/unlock",
        &request,
    ));
    assert_eq!(replay.data["item_list"]["60001"], 0);
    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(loaded.data["item_list"]["60000"], 1);
    assert_eq!(loaded.data["item_list"]["60001"], 0);
    let progress = loaded.data["quest_progress"]["18"].as_array().unwrap();
    assert!(progress
        .iter()
        .any(|entry| { entry["quest_id"] == 400_001_102 && entry["unlocked"] == true }));
    service.stop().unwrap();
}
// //// /解锁费用与持久化状态保持一致 ////
