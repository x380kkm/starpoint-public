// audience: internal
// # personal-service-cn-character-bond-token-tests
// 羁绊之证领取共同保存板块状态, 资产, 任务和历史, 玩家与管理台读取同一到账记录.

use super::*;

// //// 读取玩家与管理台中的羁绊之证到账记录 [@x380kkm 2026-09-23] ////
fn player_bond_history(service: &PersonalService, viewer: i64) -> Vec<Value> {
    decode_response::<Value>(&request(
        service,
        "history/receive",
        json!({"viewer_id":viewer,"page":1}),
    ))
    .data["history"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["type"] == 10)
        .cloned()
        .collect()
}

fn management_bond_history(service: &PersonalService) -> Vec<Value> {
    let slot = local_save_support::list_local_saves(service)["slots"][0]["id"]
        .as_i64()
        .unwrap();
    let response = local_save_support::authorized_request(
        service,
        "GET",
        &format!("/v1/local-saves/{slot}/reward-history"),
        None,
    );
    local_save_support::assert_status(&response, "200 OK");
    local_save_support::response_body(&response)["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["source"] == "reward" && entry["kind"] == 10)
        .cloned()
        .collect()
}
// //// /读取玩家与管理台中的羁绊之证到账记录 ////

// //// 羁绊之证记录失败时回滚所有状态并支持重试和重启 [@x380kkm 2026-09-23] ////
#[test]
fn settles_each_board_token_with_missions_and_shared_reward_history() {
    let (root, service, viewer) = setup();
    let body = json!({"viewer_id":viewer,"character_id":1,"mana_board_index":1});
    let locked = player_state(root.path());
    let response = request(&service, "character/receive_bond_token", body.clone());
    assert!(response.starts_with("HTTP/1.1 400"), "{response}");
    assert!(player_state(root.path()) == locked);
    update_player(root.path(), |player| {
        player["user_character_list"]["1"]["mana_board_index"] = json!(2);
        player["user_character_list"]["1"]["bond_token_list"] = json!([
            {"mana_board_index":1,"status":1},
            {"mana_board_index":2,"status":1}
        ]);
    });
    let before = player_state(root.path());
    database(root.path())
        .execute_batch(
            "CREATE TRIGGER reject_bond_history BEFORE INSERT ON player_receive_history
         WHEN NEW.event_key LIKE 'bond-token:%'
         BEGIN SELECT RAISE(ABORT, 'bond history failure'); END;",
        )
        .unwrap();
    let failed = request(&service, "character/receive_bond_token", body.clone());
    assert!(failed.starts_with("HTTP/1.1 500"), "{failed}");
    assert!(player_state(root.path()) == before);
    database(root.path())
        .execute_batch("DROP TRIGGER reject_bond_history")
        .unwrap();

    for board in 1..=2 {
        let mut claim = body.clone();
        claim["mana_board_index"] = json!(board);
        let response = decode_response::<Value>(&request(
            &service,
            "character/receive_bond_token",
            claim.clone(),
        ));
        let after = player_state(root.path());
        assert_eq!(
            response.data["user_info"]["bond_token"],
            before.snapshot["user_info"]["bond_token"].as_i64().unwrap() + board
        );
        assert_eq!(
            after.snapshot["user_character_list"]["1"]["bond_token_list"][(board - 1) as usize]
                ["status"],
            2
        );
        let retry =
            decode_response::<Value>(&request(&service, "character/receive_bond_token", claim));
        assert_eq!(
            retry.data["user_info"]["bond_token"],
            response.data["user_info"]["bond_token"]
        );
        assert!(player_state(root.path()) == after);
    }
    let after = player_state(root.path());
    assert!(after
        .counters
        .contains(&("total_obtained_bond_token_count".to_owned(), 2)));
    let player_history = player_bond_history(&service, viewer);
    assert_eq!(player_history.len(), 2);
    assert!(player_history
        .iter()
        .all(|entry| entry["number"] == 1 && entry["type_id"].is_null()));
    let management_history = management_bond_history(&service);
    assert_eq!(management_history.len(), 2);
    assert!(management_history
        .iter()
        .all(|entry| entry["kind"] == 10 && entry["number"] == 1));
    assert!(player_state(root.path()) == after);
    service.stop().unwrap();

    let service = PersonalService::start(root.path(), 0).unwrap();
    decode_response::<Value>(&request(&service, "character/receive_bond_token", body));
    assert!(player_state(root.path()) == after);
    assert_eq!(player_bond_history(&service, viewer), player_history);
    assert_eq!(management_bond_history(&service), management_history);
    service.stop().unwrap();
}
// //// /羁绊之证记录失败时回滚所有状态并支持重试和重启 ////
