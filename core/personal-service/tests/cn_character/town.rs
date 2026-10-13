// audience: internal
// # personal-service-cn-town-character-tests
//
// 剧情首次通关的角色入队和城镇领取都通过真实 HTTP 入口执行. 玩家状态和领取记录来自隔离测试数据库.
// CANDIDATES 第一项是 town 类型, 其余两项是按关卡加入的 quest 类型.

use super::*;
use serde_json::json;

const CANDIDATES: [(i64, i64, &str, usize); 3] = [
    (512_001, 1_008_004, "151200101", 1),
    (10, 2_009_007, "100001001", 2),
    (213_013, 10_015_003, "121301301", 2),
];

fn signup(service: &PersonalService) -> i64 {
    decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 44 }),
    ))
    .data_headers
    .viewer_id
}

fn receive(service: &PersonalService, viewer_id: i64, character_id: i64, api_count: i64) -> String {
    cn_support::send_request(
        service.port(),
        "/api/index.php/character/add_character_from_town",
        &encode_request(
            &json!({"viewer_id": viewer_id, "character_id": character_id, "api_count": api_count}),
        ),
    )
}

fn stored_state(root: &Path) -> (Value, i64) {
    let database = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    let data: String = database
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    let history = database
        .query_row("SELECT COUNT(*) FROM player_receive_history", [], |row| {
            row.get(0)
        })
        .unwrap();
    (serde_json::from_str(&data).unwrap(), history)
}

fn finish_story(service: &PersonalService, viewer_id: i64, quest_id: i64) -> Value {
    decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/story_quest/finish",
        &encode_request(
            &json!({"viewer_id": viewer_id, "category": 1, "quest_id": quest_id, "party_id": 1}),
        ),
    ))
    .data
}

fn mark_quest_finished(root: &Path, quest_id: i64) {
    update_player_snapshot(root, |data| {
        if !data["quest_progress"].is_object() {
            data["quest_progress"] = json!({});
        }
        let progress = data["quest_progress"].as_object_mut().unwrap();
        let entries = progress.entry("1").or_insert_with(|| json!([]));
        entries.as_array_mut().unwrap().push(json!({"quest_id": quest_id, "finished": true}));
    });
}

// //// 仅主表角色对应分类的完成记录授予领取资格 [@x380kkm 2026-09-22] ////
#[test]
fn rejects_non_master_characters_and_incomplete_prerequisite_quests() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service);
    let before = stored_state(root.path());
    for character_id in [1, 111_001, 999_999] {
        let response = receive(&service, viewer_id, character_id, 1);
        assert!(
            response.starts_with("HTTP/1.1 400 Bad Request"),
            "{response}"
        );
        assert!(response.contains("character_not_available_from_town"));
        assert_eq!(stored_state(root.path()), before);
    }
    for (character_id, quest_id, _, _) in CANDIDATES {
        for progress in [
            json!({}),
            json!({"14": [{"quest_id": quest_id, "finished": true}]}),
            json!({"1": [{"quest_id": quest_id, "finished": false, "unlocked": true, "high_score": 9999}]}),
            json!({"1": [{"quest_id": quest_id, "unlocked": true}]}),
            json!({"1": [{"quest_id": quest_id + 1, "finished": true}]}),
        ] {
            update_player_snapshot(root.path(), |data| data["quest_progress"] = progress);
            let before = stored_state(root.path());
            let response = receive(&service, viewer_id, character_id, 2);
            assert!(
                response.starts_with("HTTP/1.1 400 Bad Request"),
                "{response}"
            );
            assert!(response.contains("town_quest_not_cleared"));
            assert_eq!(stored_state(root.path()), before);
        }
    }
    service.stop().unwrap();
}
// //// /仅主表角色对应分类的完成记录授予领取资格 ////

// //// 三个剧情角色在关卡已完成而未入队时由城镇补领并持久化图鉴任务和单次领取记录 [@x380kkm 2026-10-07] ////
#[test]
fn persists_town_character_encyclopedia_delta_and_retries_without_rewards() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service);
    for (index, (character_id, quest_id, encyclopedia_id, board_count)) in
        CANDIDATES.into_iter().enumerate()
    {
        mark_quest_finished(root.path(), quest_id);
        update_player_snapshot(root.path(), |data| {
            data["all_active_mission_list"] = json!({})
        });
        let (before, history_before) = stored_state(root.path());
        let response = decode_response::<Value>(&receive(&service, viewer_id, character_id, 3));
        let character = &response.data["character_list"][0];
        assert_eq!(character["character_id"], character_id);
        assert_eq!(character["entry_count"], 1);
        assert_eq!(character["exp"], 0);
        assert_eq!(character["mana_board_index"], 1);
        assert_eq!(
            character["bond_token_list"].as_array().unwrap().len(),
            board_count
        );
        assert_eq!(
            response.data["encyclopedia_info"],
            json!({encyclopedia_id: {"read": false}})
        );
        assert!(response.data["active_mission_list"]
            .as_array()
            .unwrap()
            .iter()
            .any(|mission| { mission["mission_id"] == 12030 && mission["progress_value"] == 1 }));
        let (after, history_after) = stored_state(root.path());
        assert_eq!(history_after, history_before + 1);
        assert_eq!(history_after, index as i64 + 1);
        assert_eq!(after["item_list"], before["item_list"]);
        assert_eq!(
            after["user_character_list"][character_id.to_string()]["stack"],
            0
        );
        assert_eq!(after["encyclopedia_list"][encyclopedia_id]["read"], false);
        assert_eq!(after["all_active_mission_list"]["12030"]["progress"], 1);
        assert!(after["all_active_mission_list"].get("14080").is_none());
        for api_count in [3, 4] {
            let retry =
                decode_response::<Value>(&receive(&service, viewer_id, character_id, api_count));
            assert_eq!(retry.data["character_list"][0]["stack"], 0);
            assert_eq!(retry.data["encyclopedia_info"], json!({}));
            assert_eq!(stored_state(root.path()), (after.clone(), history_after));
        }
    }
    service.stop().unwrap();

    let service = PersonalService::start(root.path(), 0).unwrap();
    let before = stored_state(root.path());
    for (character_id, _, _, _) in CANDIDATES {
        decode_response::<Value>(&receive(&service, viewer_id, character_id, 5));
        assert_eq!(stored_state(root.path()), before);
    }
    let history = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/history/receive",
        &encode_request(&json!({"viewer_id":viewer_id})),
    ));
    assert_eq!(history.data["total_count"], 3);
    for (character_id, _, encyclopedia_id, _) in CANDIDATES {
        assert!(history.data["history"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| {
                entry["type"] == 5 && entry["type_id"] == character_id && entry["number"] == 1
            }));
        assert_eq!(
            before.0["user_character_list"][character_id.to_string()]["entry_count"],
            1
        );
        assert_eq!(
            before.0["encyclopedia_list"][encyclopedia_id]["read"],
            false
        );
    }
    assert!(before.0["encyclopedia_list"].get("11001").is_none());
    service.stop().unwrap();
}
// //// /三个剧情角色在关卡已完成而未入队时由城镇补领并持久化图鉴任务和单次领取记录 ////

// //// 按关卡加入的剧情角色在首次通关时入队且只记录一次 [@x380kkm 2026-10-07] ////
#[test]
fn joins_quest_story_characters_on_first_clear() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service);
    let town_only = finish_story(&service, viewer_id, CANDIDATES[0].1);
    assert!(town_only.get("story_join_character_id_list").is_none());
    assert!(stored_state(root.path()).0["user_character_list"].get(CANDIDATES[0].0.to_string()).is_none());
    for (character_id, quest_id, encyclopedia_id, _) in CANDIDATES.into_iter().skip(1) {
        let (_, history_before) = stored_state(root.path());
        let response = finish_story(&service, viewer_id, quest_id);
        assert_eq!(response["story_join_character_id_list"], json!([character_id]));
        assert!(response["character_list"].as_array().unwrap().iter()
            .any(|character| character["character_id"] == character_id && character["entry_count"] == 1));
        assert_eq!(response["encyclopedia_info"][encyclopedia_id], json!({"read": false}));
        let (after, history_after) = stored_state(root.path());
        assert_eq!(history_after, history_before + 1);
        assert_eq!(after["user_character_list"][character_id.to_string()]["entry_count"], 1);

        let replay = finish_story(&service, viewer_id, quest_id);
        assert!(replay.get("story_join_character_id_list").is_none());
        assert_eq!(stored_state(root.path()), (after.clone(), history_after));
        decode_response::<Value>(&receive(&service, viewer_id, character_id, 9));
        assert_eq!(stored_state(root.path()), (after, history_after));
    }
    service.stop().unwrap();
}
// //// /按关卡加入的剧情角色在首次通关时入队且只记录一次 ////

// //// 通关前已拥有的剧情角色不重复入队 [@x380kkm 2026-10-07] ////
#[test]
fn keeps_owned_story_characters_on_first_clear() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service);
    let (character_id, quest_id, _, _) = CANDIDATES[2];
    update_player_snapshot(root.path(), |data| {
        let mut character = data["user_character_list"]["1"].clone();
        character["stack"] = json!(7);
        data["user_character_list"][character_id.to_string()] = character;
    });
    let (_, history_before) = stored_state(root.path());
    let response = finish_story(&service, viewer_id, quest_id);
    assert!(response.get("story_join_character_id_list").is_none());
    let (after, history_after) = stored_state(root.path());
    assert_eq!(history_after, history_before);
    assert_eq!(after["user_character_list"][character_id.to_string()]["stack"], 7);
    service.stop().unwrap();
}
// //// /通关前已拥有的剧情角色不重复入队 ////

// //// 已从其它途径取得的剧情角色保持资产和历史原样 [@x380kkm 2026-09-22] ////
#[test]
fn returns_owned_story_characters_without_prerequisite_or_mutation() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service);
    update_player_snapshot(root.path(), |data| {
        data["quest_progress"] = json!({});
        for (character_id, _, _, _) in CANDIDATES {
            let mut character = data["user_character_list"]["1"].clone();
            character["stack"] = json!(7);
            data["user_character_list"][character_id.to_string()] = character;
        }
    });
    let before = stored_state(root.path());
    for (character_id, _, _, _) in CANDIDATES {
        let response = decode_response::<Value>(&receive(&service, viewer_id, character_id, 1));
        assert_eq!(response.data["character_list"][0]["stack"], 7);
        assert_eq!(stored_state(root.path()), before);
    }
    service.stop().unwrap();
}
// //// /已从其它途径取得的剧情角色保持资产和历史原样 ////

// //// 领取记录写入失败时角色图鉴和任务状态一起回滚 [@x380kkm 2026-09-22] ////
#[test]
fn rolls_back_character_encyclopedia_and_missions_when_history_save_fails() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service);
    finish_story(&service, viewer_id, CANDIDATES[0].1);
    update_player_snapshot(root.path(), |data| {
        data["all_active_mission_list"] = json!({})
    });
    let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    database.execute_batch("CREATE TRIGGER reject_town_history BEFORE INSERT ON player_receive_history WHEN NEW.event_key LIKE 'town-character:%' BEGIN SELECT RAISE(ABORT, 'town history failure'); END;").unwrap();
    let before = stored_state(root.path());
    let response = receive(&service, viewer_id, CANDIDATES[0].0, 1);
    assert!(
        response.starts_with("HTTP/1.1 500 Internal Server Error"),
        "{response}"
    );
    assert_eq!(stored_state(root.path()), before);
    database
        .execute_batch("DROP TRIGGER reject_town_history")
        .unwrap();
    decode_response::<Value>(&receive(&service, viewer_id, CANDIDATES[0].0, 1));
    let after = stored_state(root.path());
    assert_eq!(after.1, before.1 + 1);
    assert_eq!(after.0["all_active_mission_list"]["12030"]["progress"], 1);
    assert_eq!(after.0["encyclopedia_list"][CANDIDATES[0].2]["read"], false);
    service.stop().unwrap();
}
// //// /领取记录写入失败时角色图鉴和任务状态一起回滚 ////
