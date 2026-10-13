// audience: internal
// # personal-service-cn-character-board-tests
// 玛纳板开放保存最高板块, 二板任务与开放状态共同提交, 迟到请求和重启保持已开放板块.

use super::*;

// //// 验证首板入口与二板开放后的迟到请求 [@x380kkm 2026-09-23] ////
#[test]
fn preserves_opened_boards_and_grants_the_opening_mission_once() {
    let (root, service, viewer) = setup();
    let mut body = json!({"viewer_id":viewer,"character_id":1,"mana_board_index":1});
    update_player(root.path(), |player| {
        player["user_character_list"]["1"]["mana_board_index"] = json!(0);
        player["user_character_list"]["1"]["exp"] = json!(0);
        player["user_character_list"]["1"]["over_limit_step"] = json!(0);
    });
    let first = decode_response::<Value>(&request(
        &service,
        "character/open_mana_board",
        body.clone(),
    ));
    assert_eq!(first.data["character_list"][0]["mana_board_index"], 1);
    update_player(root.path(), |player| {
        player["user_character_list"]["1"]["exp"] = json!(76_272);
        player["user_character_list"]["1"]["over_limit_step"] = json!(4);
        player["user_character_list"]["1"]["bond_token_list"][0]["status"] = json!(1);
    });
    let before = player_state(root.path());
    body["mana_board_index"] = json!(2);
    reject_snapshot_when(
        root.path(),
        r#"COALESCE(json_extract(NEW.data_json, '$.user_character_list."1".mana_board_index'), 0) = 2"#,
    );
    let failed = request(&service, "character/open_mana_board", body.clone());
    assert!(failed.starts_with("HTTP/1.1 500"), "{failed}");
    assert!(player_state(root.path()) == before);
    allow_snapshot(root.path());
    let opened = decode_response::<Value>(&request(
        &service,
        "character/open_mana_board",
        body.clone(),
    ));
    assert_eq!(opened.data["character_list"][0]["mana_board_index"], 2);
    assert!(has_mission_reward(&opened.data, 95, 95001));
    let after = player_state(root.path());
    assert!(after
        .counters
        .contains(&("mana_board_2nd_open_count".to_owned(), 1)));
    for requested in [1, 2, 1] {
        body["mana_board_index"] = json!(requested);
        let repeated = decode_response::<Value>(&request(
            &service,
            "character/open_mana_board",
            body.clone(),
        ));
        assert_eq!(repeated.data["character_list"][0]["mana_board_index"], 2);
        assert!(player_state(root.path()) == after);
    }
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let repeated = decode_response::<Value>(&request(&service, "character/open_mana_board", body));
    assert_eq!(repeated.data["character_list"][0]["mana_board_index"], 2);
    assert!(player_state(root.path()) == after);
    service.stop().unwrap();
}
// //// /验证首板入口与二板开放后的迟到请求 ////
