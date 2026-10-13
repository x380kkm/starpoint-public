// audience: internal
// # personal-service-cn-expod-replay-tests
// 经验注入和副本转换按请求计数重放, 转换产出与领取历史共同保存, 无计数操作保持独立结算.

use super::*;

// //// 准备经验注入及单次和批量副本转换场景 [@x380kkm 2026-09-23] ////
fn cases() -> [(&'static str, Value); 3] {
    [
        ("expod/inject_exp", json!({"character_id":1,"exp":100})),
        ("expod/stack_to_exp", json!({"character_id":1,"number":1})),
        ("expod/bulk_stack_to_exp", json!({})),
    ]
}

fn prepare_expod() -> (TempDir, PersonalService, i64) {
    let (root, service, viewer) = setup();
    update_player(root.path(), |player| {
        player["user_info"]["exp_pool"] = json!(1_000);
        player["user_character_list"]["1"]["exp"] = json!(0);
        player["user_character_list"]["1"]["stack"] = json!(3);
        player["user_character_list"]["1"]["over_limit_step"] = json!(6);
    });
    (root, service, viewer)
}

fn conversion_history(root: &Path) -> Vec<(i64, Option<i64>, i64)> {
    database(root)
        .prepare(
            "SELECT kind, type_id, number FROM player_receive_history
         WHERE event_key LIKE 'expod:%' ORDER BY id",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}
// //// /准备经验注入及单次和批量副本转换场景 ////

// //// 经验操作重启重放与无计数请求分别保持对应结算语义 [@x380kkm 2026-09-23] ////
#[test]
fn replays_expod_requests_after_restart_and_records_actual_conversion_rewards() {
    for (route, mut body) in cases() {
        let (root, service, viewer) = prepare_expod();
        body["viewer_id"] = json!(viewer);
        body["api_count"] = json!(31);
        let first = decode_response::<Value>(&request(&service, route, body.clone())).data;
        let after = player_state(root.path());
        let records = conversion_history(root.path());
        match route {
            "expod/inject_exp" => {
                assert_eq!(after.snapshot["user_character_list"]["1"]["exp"], 100);
                assert_eq!(after.snapshot["user_info"]["exp_pool"], 900);
                assert!(records.is_empty());
            }
            "expod/stack_to_exp" => {
                assert_eq!(after.snapshot["user_character_list"]["1"]["stack"], 2);
                assert_eq!(records, [(9, None, 2_000)]);
            }
            _ => {
                assert_eq!(after.snapshot["user_character_list"]["1"]["stack"], 0);
                assert_eq!(records, [(9, None, 6_000)]);
            }
        }
        assert_eq!(
            decode_response::<Value>(&request(&service, route, body.clone())).data,
            first
        );
        assert!(player_state(root.path()) == after);
        if route != "expod/bulk_stack_to_exp" {
            let mut changed = body.clone();
            let field = if route == "expod/inject_exp" {
                "exp"
            } else {
                "number"
            };
            changed[field] = json!(2);
            let conflict = request(&service, route, changed);
            assert!(conflict.starts_with("HTTP/1.1 400"), "{conflict}");
            assert!(conflict.ends_with("{\"error\":\"expod_request_conflict\"}"));
            assert!(player_state(root.path()) == after);
        }
        service.stop().unwrap();
        let service = PersonalService::start(root.path(), 0).unwrap();
        assert_eq!(
            decode_response::<Value>(&request(&service, route, body.clone())).data,
            first
        );
        assert!(player_state(root.path()) == after);
        update_player(root.path(), |player| {
            player["user_character_list"]["1"]["stack"] = json!(2);
        });
        body.as_object_mut().unwrap().remove("api_count");
        decode_response::<Value>(&request(&service, route, body.clone()));
        if route == "expod/inject_exp" {
            assert_eq!(
                player_state(root.path()).snapshot["user_character_list"]["1"]["exp"],
                200
            );
            decode_response::<Value>(&request(&service, route, body));
            assert_eq!(
                player_state(root.path()).snapshot["user_character_list"]["1"]["exp"],
                300
            );
        } else {
            assert_eq!(conversion_history(root.path()).len(), 2);
        }
        let loaded = decode_response::<Value>(&request(
            &service,
            "load",
            json!({"viewer_id":viewer,"keychain":viewer}),
        ));
        assert!(loaded.data.get("expod_request_receipts").is_none());
        service.stop().unwrap();
    }
}
// //// /经验操作重启重放与无计数请求分别保持对应结算语义 ////

// //// 转换历史保存失败时保持副本和全部产出可重新结算 [@x380kkm 2026-09-23] ////
#[test]
fn rolls_back_stack_conversion_when_reward_history_fails() {
    for (route, mut body) in cases()
        .into_iter()
        .filter(|(route, _)| *route != "expod/inject_exp")
    {
        let (root, service, viewer) = prepare_expod();
        body["viewer_id"] = json!(viewer);
        body["api_count"] = json!(31);
        let before = player_state(root.path());
        database(root.path())
            .execute_batch(
                "CREATE TRIGGER reject_conversion_history BEFORE INSERT ON player_receive_history
             WHEN NEW.event_key LIKE 'expod:%'
             BEGIN SELECT RAISE(ABORT, 'conversion history failure'); END;",
            )
            .unwrap();
        let failed = request(&service, route, body.clone());
        assert!(failed.starts_with("HTTP/1.1 500"), "{failed}");
        assert!(player_state(root.path()) == before);
        database(root.path())
            .execute_batch("DROP TRIGGER reject_conversion_history")
            .unwrap();
        decode_response::<Value>(&request(&service, route, body));
        assert_eq!(conversion_history(root.path()).len(), 1);
        service.stop().unwrap();
    }
}
// //// /转换历史保存失败时保持副本和全部产出可重新结算 ////

// //// 经验超过角色上限时只消费实际提升量 [@x380kkm 2026-09-23] ////
#[test]
fn preserves_unused_experience_at_the_character_level_cap() {
    let (root, service, viewer) = prepare_expod();
    update_player(root.path(), |player| {
        player["user_character_list"]["1"]["exp"] = json!(342_400);
    });
    let body = json!({"viewer_id":viewer,"character_id":1,"exp":1_000,"api_count":31});
    let first = decode_response::<Value>(&request(&service, "expod/inject_exp", body.clone())).data;
    assert_eq!(first["add_exp_list"][0]["add_exp"], 10);
    assert_eq!(first["add_exp_list"][0]["add_exp_pool"], 990);
    assert_eq!(first["character_list"][0]["exp"], 342_410);
    assert_eq!(first["user_info"]["exp_pool"], 990);
    let after = player_state(root.path());
    assert_eq!(
        decode_response::<Value>(&request(&service, "expod/inject_exp", body)).data,
        first
    );
    assert!(player_state(root.path()) == after);
    service.stop().unwrap();
}
// //// /经验超过角色上限时只消费实际提升量 ////

// //// 角色保护同时约束单次转换与批量转换候选 [@x380kkm 2026-09-23] ////
#[test]
fn preserves_protected_character_stacks_during_single_and_bulk_conversion() {
    let (root, service, viewer) = prepare_expod();
    update_player(root.path(), |player| {
        let mut available = player["user_character_list"]["1"].clone();
        available["protection"] = json!(false);
        available["over_limit_step"] = json!(8);
        available["stack"] = json!(1);
        player["user_character_list"]["311001"] = available;
        player["user_character_list"]["1"]["protection"] = json!(true);
    });
    let before = player_state(root.path());
    let body = json!({"viewer_id":viewer,"character_id":1,"number":1,"api_count":31});
    let rejected = request(&service, "expod/stack_to_exp", body.clone());
    assert!(rejected.starts_with("HTTP/1.1 400"), "{rejected}");
    assert!(rejected.ends_with("{\"error\":\"character_protected\"}"));
    assert!(player_state(root.path()) == before);
    let bulk = decode_response::<Value>(&request(
        &service,
        "expod/bulk_stack_to_exp",
        json!({"viewer_id":viewer,"api_count":31}),
    ))
    .data;
    let after = player_state(root.path());
    assert_eq!(after.snapshot["user_character_list"]["1"]["stack"], 3);
    assert_eq!(after.snapshot["user_character_list"]["311001"]["stack"], 0);
    assert_eq!(bulk["converted_exp_info"]["add_exp"], 500);
    assert_eq!(
        conversion_history(root.path()),
        [(9, None, 500), (1, Some(990_008), 2)]
    );
    update_player(root.path(), |player| {
        player["user_character_list"]["1"]["protection"] = json!(false);
    });
    decode_response::<Value>(&request(&service, "expod/stack_to_exp", body));
    assert_eq!(
        player_state(root.path()).snapshot["user_character_list"]["1"]["stack"],
        2
    );
    service.stop().unwrap();
}
// //// /角色保护同时约束单次转换与批量转换候选 ////

// //// 同星级的抽取角色与 Boss 角色转换保持各自星粒规则 [@x380kkm 2026-09-23] ////
#[test]
fn converts_star_grain_only_for_source_zero_in_single_and_bulk_requests() {
    for route in ["expod/stack_to_exp", "expod/bulk_stack_to_exp"] {
        let (root, service, viewer) = prepare_expod();
        update_player(root.path(), |player| {
            player["user_character_list"]["1"]["stack"] = json!(0);
            let mut character = player["user_character_list"]["1"].clone();
            character["over_limit_step"] = json!(4);
            character["stack"] = json!(2);
            character["protection"] = json!(false);
            player["user_character_list"]["111001"] = character.clone();
            player["user_character_list"]["141003"] = character;
            player["item_list"]["990008"] = json!(7);
        });
        let requests = if route == "expod/stack_to_exp" {
            vec![
                json!({"viewer_id":viewer,"character_id":111001,"number":2,"api_count":81}),
                json!({"viewer_id":viewer,"character_id":141003,"number":2,"api_count":82}),
            ]
        } else {
            vec![json!({"viewer_id":viewer,"api_count":81})]
        };
        let before = player_state(root.path());
        database(root.path()).execute_batch(
            "CREATE TRIGGER reject_conversion_star_grain BEFORE INSERT ON player_receive_history
             WHEN NEW.event_key LIKE 'expod:%' AND NEW.kind=1 AND NEW.type_id=990008
             BEGIN SELECT RAISE(ABORT, 'star grain history failure'); END;",
        ).unwrap();
        let failed = request(&service, route, requests[0].clone());
        assert!(failed.starts_with("HTTP/1.1 500"), "{failed}");
        assert!(player_state(root.path()) == before);
        database(root.path())
            .execute_batch("DROP TRIGGER reject_conversion_star_grain")
            .unwrap();
        let mut responses = Vec::new();
        for body in &requests {
            let response = decode_response::<Value>(&request(&service, route, body.clone())).data;
            assert_eq!(
                response["converted_exp_info"]["add_exp"],
                if route == "expod/stack_to_exp" {
                    20_000
                } else {
                    40_000
                }
            );
            assert_eq!(response["item_list"]["990008"], 67);
            responses.push(response);
        }
        let after = player_state(root.path());
        assert_eq!(after.snapshot["user_info"]["exp_pool"], 41_000);
        assert_eq!(after.snapshot["item_list"]["990008"], 67);
        for id in ["111001", "141003"] {
            assert_eq!(after.snapshot["user_character_list"][id]["stack"], 0);
        }
        let records = conversion_history(root.path());
        assert_eq!(
            records
                .iter()
                .filter(|(kind, _, _)| *kind == 9)
                .map(|(_, _, number)| number)
                .sum::<i64>(),
            40_000
        );
        assert_eq!(
            records
                .iter()
                .filter(|(kind, _, _)| *kind == 1)
                .copied()
                .collect::<Vec<_>>(),
            [(1, Some(990_008), 60)]
        );
        service.stop().unwrap();
        let restarted = PersonalService::start(root.path(), 0).unwrap();
        for (body, response) in requests.into_iter().zip(responses) {
            assert_eq!(
                decode_response::<Value>(&request(&restarted, route, body)).data,
                response
            );
        }
        assert!(player_state(root.path()) == after);
        restarted.stop().unwrap();
    }
}
// //// /同星级的抽取角色与 Boss 角色转换保持各自星粒规则 ////
