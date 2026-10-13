// audience: internal
// # editable-local-saves
// 明文存档的手工编辑通过独立实例导入, 登录和重启保留游戏数据.

#[path = "support/cn.rs"]
mod cn_support;
#[path = "support/local_saves.rs"]
mod local_save_support;
mod support;

use local_save_support::{
    activate_local_save, assert_status, authorized_request, export_local_save, list_local_saves,
    load, response_body, signup,
};
use serde_json::json;
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

// //// 在平台来源不同的实例之间导入编辑后的进度并保留原槽 [@x380kkm 2026-09-28] ////
#[test]
fn edited_json_roundtrips_with_stale_and_removed_digest() {
    let source_root = TempDir::new().unwrap();
    let source = PersonalService::start(source_root.path(), 0).unwrap();
    signup(source.port(), 92801);
    let source_slot = list_local_saves(&source)["slots"][0]["id"].as_i64().unwrap();
    let original = export_local_save(&source, source_slot);
    let mut edited = original.clone();
    edited["sourceClient"]["platform"] = json!("android");
    edited["data"]["user_info"]["name"] = json!("编辑后的存档");
    edited["data"]["user_info"]["free_mana"] = json!(123456);
    edited["data"]["user_character_list"]["1"]["ex_boost"] =
        json!({"status_id": 2, "ability_id_list": [4, 36]});
    edited["data"]["quest_progress"]["1"] = json!([{
        "quest_id": 12004001, "finished": true, "clear_rank": 5,
        "high_score": 54027506081_i64, "best_elapsed_time_ms": 53929
    }]);
    edited["data"]["editor_extension"] = json!({"notes": "保留扩展字段"});

    let target_root = TempDir::new().unwrap();
    let target = PersonalService::start(target_root.path(), 0).unwrap();
    let response = authorized_request(
        &target,
        "POST",
        "/v1/local-saves/import",
        Some(&json!({"name": "导入 Android 存档", "data": edited})),
    );
    assert_status(&response, "201 Created");
    let target_slot = response_body(&response)["id"].as_i64().unwrap();
    let target_export = export_local_save(&target, target_slot);
    assert_eq!(target_export["data"], edited["data"]);
    assert_ne!(target_export["payloadSha256"], original["payloadSha256"]);
    target.stop().unwrap();

    let target = PersonalService::start(target_root.path(), 0).unwrap();
    let login = signup(target.port(), 92802);
    assert_eq!(login.data.new_account, 0);
    let player = load(target.port(), login.data_headers.viewer_id, "1.4.67");
    assert_eq!(player.data["user_info"]["name"], "编辑后的存档");
    assert_eq!(player.data["user_info"]["free_mana"], 123456);
    assert_eq!(
        player.data["user_character_list"]["1"]["ex_boost"],
        edited["data"]["user_character_list"]["1"]["ex_boost"]
    );
    assert_eq!(player.data["quest_progress"]["1"][0]["high_score"], 54027506081_i64);

    let mut returned = export_local_save(&target, target_slot);
    returned["sourceClient"]["platform"] = json!("ios");
    returned["data"]["user_info"]["free_mana"] = json!(234567);
    returned.as_object_mut().unwrap().remove("payloadSha256");
    let response = authorized_request(
        &source,
        "POST",
        "/v1/local-saves/import",
        Some(&returned),
    );
    assert_status(&response, "201 Created");
    let returned_slot = response_body(&response)["id"].as_i64().unwrap();
    assert_ne!(source_slot, returned_slot);
    assert_eq!(export_local_save(&source, source_slot)["data"], original["data"]);
    assert_eq!(export_local_save(&source, returned_slot)["data"], returned["data"]);
    activate_local_save(&source, returned_slot, 92801);
    source.stop().unwrap();

    let source = PersonalService::start(source_root.path(), 0).unwrap();
    let login = signup(source.port(), 92801);
    let player = load(source.port(), login.data_headers.viewer_id, "1.4.67");
    assert_eq!(player.data["user_info"]["free_mana"], 234567);
    assert_eq!(player.data["editor_extension"], returned["data"]["editor_extension"]);
    assert_eq!(list_local_saves(&source)["slots"].as_array().unwrap().len(), 2);
    source.stop().unwrap();
    target.stop().unwrap();
}
