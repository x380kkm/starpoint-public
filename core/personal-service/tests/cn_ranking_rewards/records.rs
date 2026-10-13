// audience: internal
// # personal-service-cn-ranking-record-api-tests
//
// 真实竞速结算验证部分成绩, 通关优先级, 最佳记录持久化和参与档奖励到账.

use super::*;

fn open_event(service: &PersonalService, root: &Path) {
    set_time(service, "2030-01-01T12:00:00.000Z");
    manage(service, "PUT", "/v1/activities/calendar/ranking:1",
        json!({"enabled":true,"start_at":"2030-01-01T00:00:00.000Z","end_at":"2030-01-02T00:00:00.000Z"}));
    update_snapshot(root, |player| player["user_info"]["stamina"] = json!(1000));
}

fn attempt(service: &PersonalService, viewer: i64, play: &str, accomplished: bool, elapsed: i64, score: i64) -> (Value, Value) {
    decode_response::<Value>(&send(service, "/api/index.php/single_battle_quest/start", json!({
        "viewer_id":viewer,"category":11,"quest_id":1001,"play_id":play,"party_id":1,
        "use_boss_boost_point":false,"use_boost_point":false,"is_auto_start_mode":false,
    })));
    let request = json!({
        "viewer_id":viewer,"category":11,"quest_id":1001,"play_id":play,"is_accomplished":accomplished,
        "elapsed_time_ms":elapsed,"score":score,"add_mana":0,
        "statistics":{"clear_phase":1,"zones":[],"party":{
            "characters":[{"id":1},null,null],"unison_characters":[null,null,null],
            "equipments":[null,null,null],"ability_soul_ids":[null,null,null],
        }},
    });
    let response = decode_response::<Value>(&send(service, "/api/index.php/single_battle_quest/finish", request.clone())).data;
    (request, response)
}

fn stored(service: &PersonalService, viewer: i64) -> Value {
    decode_response::<Value>(&send(service, "/api/index.php/load", json!({"viewer_id":viewer,"keychain":viewer}))).data
}

// //// 依客户端规则比较部分和完整成绩并重放同一次战斗 [@x380kkm 2026-09-21] ////
#[test]
fn preserves_partial_records_and_prefers_complete_then_faster_runs() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    open_event(&service, root.path());
    let mut previous = Value::Null;
    let mut best_record = Value::Null;
    let mut latest_finish = None;
    for (index, (complete, elapsed, score, best)) in [
        (false, 100000, 100, (false, 100000, 100)),
        (false, 90000, 50, (false, 100000, 100)),
        (false, 110000, 200, (false, 110000, 200)),
        (true, 120000, 50, (true, 120000, 50)),
        (true, 100000, 10, (true, 100000, 10)),
        (true, 140000, 900, (true, 100000, 10)),
    ].into_iter().enumerate() {
        let (request, response) = attempt(&service, viewer, &format!("ranking-record-{index}"), complete, elapsed, score);
        let ranking = &response["ranking_event"];
        assert_eq!(ranking["is_record_valid"], true);
        assert_eq!(ranking["previous"], previous);
        let expected = json!({"elapsed_time_ms":best.1,"is_accomplished":best.0,"score":best.2});
        assert_eq!(ranking["best_record"], expected);
        let summary = decode_response::<Value>(&send(&service, "/api/index.php/ranking_event/get_summary", ranking_request(viewer, 1, 1))).data;
        assert_eq!(summary["best_record"], expected);
        previous = json!({"elapsed_time_ms":elapsed,"is_accomplished":complete,"score":score});
        best_record = expected;
        latest_finish = Some((request.clone(), response.clone()));
        let assets = stored(&service, viewer)["item_list"].clone();
        let replay = decode_response::<Value>(&send(&service, "/api/index.php/single_battle_quest/finish", request)).data;
        assert_eq!(replay, response);
        assert_eq!(stored(&service, viewer)["item_list"], assets);
    }
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let summary = decode_response::<Value>(&send(&service, "/api/index.php/ranking_event/get_summary", ranking_request(viewer, 1, 2))).data;
    assert_eq!(summary["best_record"], best_record);
    let (latest_request, latest_response) = latest_finish.unwrap();
    let replay = decode_response::<Value>(&send(&service, "/api/index.php/single_battle_quest/finish", latest_request)).data;
    assert_eq!(replay, latest_response);
    let (_, next) = attempt(&service, viewer, "ranking-after-restart", true, 130000, 800);
    assert_eq!(next["ranking_event"]["previous"], previous);
    assert_eq!(next["ranking_event"]["best_record"], best_record);
    service.stop().unwrap();
}
// //// /依客户端规则比较部分和完整成绩并重放同一次战斗 ////

// //// 只有历史最佳成绩的存档从实际新结算建立最近一局记录 [@x380kkm 2026-09-27] ////
#[test]
fn keeps_legacy_best_records_without_inventing_a_previous_run() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    open_event(&service, root.path());
    update_snapshot(root.path(), |player| {
        player["quest_progress"]["11"] = json!([{"quest_id":1001,"finished":true,
            "best_elapsed_time_ms":60000,"best_elapsed_score":100,"high_score":100,
            "best_leader_character_id":1}]);
    });
    let (_, first) = attempt(&service, viewer, "ranking-legacy", true, 120000, 300);
    assert!(first["ranking_event"]["previous"].is_null());
    assert_eq!(first["ranking_event"]["best_record"],
        json!({"elapsed_time_ms":60000,"is_accomplished":true,"score":100}));
    let (_, second) = attempt(&service, viewer, "ranking-legacy-next", true, 130000, 400);
    assert_eq!(second["ranking_event"]["previous"],
        json!({"elapsed_time_ms":120000,"is_accomplished":true,"score":300}));
    service.stop().unwrap();
}
// //// /只有历史最佳成绩的存档从实际新结算建立最近一局记录 ////

// //// 部分成绩按参与档投递奖励并保持通关状态独立 [@x380kkm 2026-09-21] ////
#[test]
fn delivers_participation_tier_for_a_valid_partial_record() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    open_event(&service, root.path());
    attempt(&service, viewer, "ranking-partial-reward", false, 100000, 100);
    let data = stored(&service, viewer);
    assert!(data["quest_progress"]["11"].as_array().into_iter().flatten().all(|entry| entry["quest_id"] != 1001));
    let stones = data["user_info"]["free_vmoney"].as_i64().unwrap();
    set_time(&service, "2030-01-05T00:00:01.000Z");
    let reward = decode_response::<Value>(&send(&service, "/api/index.php/ranking_event/receive_reward", ranking_request(viewer, 1, 1))).data;
    assert_eq!(reward["status"], 1);
    assert_eq!(reward["rank_percentage"], 100.0);
    assert_eq!(reward["degree_list"][0]["degree_id"], 54000);
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let mail: i64 = db.query_row("SELECT id FROM player_mails WHERE title LIKE '%排名奖励%'", [], |row| row.get(0)).unwrap();
    decode_response::<Value>(&send(&service, "/api/index.php/mail/receive", json!({"viewer_id":viewer,"mail_id":mail})));
    assert_eq!(stored(&service, viewer)["user_info"]["free_vmoney"], stones + 150);
    assert_eq!(stored(&service, viewer)["item_list"]["49100"], data["item_list"]["49100"].as_i64().unwrap_or_default() + 5);
    let repeated = decode_response::<Value>(&send(&service, "/api/index.php/ranking_event/receive_reward", ranking_request(viewer, 1, 2))).data;
    assert_eq!(repeated["status"], 2);
    assert_eq!(stored(&service, viewer)["user_info"]["free_vmoney"], stones + 150);
    service.stop().unwrap();
}
// //// /部分成绩按参与档投递奖励并保持通关状态独立 ////
