// audience: internal
// # personal-service-cn-phase-reward-tests
// 此文件验证失败战斗, 剧情败北战和竞速已完成阶段的实际库存结算.

#[path = "support/cn_rewards.rs"]
#[allow(dead_code)]
mod cn_reward_support;
#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;
#[path = "cn_phase_rewards/score_attack.rs"]
mod score_attack;
#[path = "cn_phase_rewards/master_fields.rs"]
mod master_fields;
#[path = "cn_phase_rewards/active_rank.rs"]
mod active_rank;

use cn_reward_support::mission_currency;
use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::collections::BTreeMap;
use std::path::Path;
use tempfile::TempDir;

// //// 读取隔离存档并发送结算请求 [@x380kkm 2026-09-21] ////
fn request(service: &PersonalService, path: &str, body: Value) -> Value {
    decode_response::<Value>(&cn_support::send_request(
        service.port(),
        path,
        &encode_request(&body),
    ))
    .data
}

fn signup(service: &PersonalService) -> i64 {
    decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 783 }),
    ))
    .data_headers
    .viewer_id
}

fn refill_stamina(root: &Path) {
    let mut player = snapshot(root);
    player["user_info"]["stamina"] = json!(1000);
    let db = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    db.execute(
        "UPDATE player_snapshots SET data_json=?1",
        [player.to_string()],
    )
    .unwrap();
}

fn snapshot(root: &Path) -> Value {
    let db = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    let data: String = db
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    serde_json::from_str(&data).unwrap()
}

fn manage(service: &PersonalService, path: &str, body: Value) {
    let authorization = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        "PUT",
        path,
        "application/json",
        &[("Authorization", &authorization)],
        body.to_string().as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

fn open_activity(service: &PersonalService, activity: &str) {
    manage(
        service,
        &format!("/v1/activities/calendar/{activity}"),
        json!({"enabled":true,"start_at":"2030-01-01T00:00:00.000Z","end_at":"2030-01-02T00:00:00.000Z"}),
    );
    manage(
        service,
        "/v1/time",
        json!({"enabled":true,"iso":"2030-01-01T12:00:00.000Z","rate":1}),
    );
}

fn start(service: &PersonalService, viewer: i64, category: i64, quest: i64, play: &str) {
    request(
        service,
        "/api/index.php/single_battle_quest/start",
        json!({
            "viewer_id": viewer,"quest_id": quest,"category": category,"play_id": play,"party_id":1,
            "use_boss_boost_point":false,"use_boost_point":false,"is_auto_start_mode":false,
        }),
    );
}

fn finish_body(viewer: i64, category: i64, quest: i64, play: &str) -> Value {
    json!({"viewer_id":viewer,"quest_id":quest,"category":category,"play_id":play,
        "elapsed_time_ms":100_000,"score":5000,"add_mana":7,"is_accomplished":false,"is_lose":false,
        "statistics":{"clear_phase":0,"zones":[],"party":{"characters":[{"id":1}],
            "unison_characters":[],"equipments":[],"ability_soul_ids":[]}}})
}

fn finish(service: &PersonalService, body: Value) -> Value {
    request(service, "/api/index.php/single_battle_quest/finish", body)
}

fn quest_progress(player: &Value, category: i64, quest: i64) -> Option<&Value> {
    player["quest_progress"][category.to_string()]
        .as_array()
        .and_then(|entries| entries.iter().find(|entry| entry["quest_id"] == quest))
}

// //// /读取隔离存档并发送结算请求 ////

// //// 普通失败保留首次通关奖励且剧情败北战继续主线 [@x380kkm 2026-09-21] ////
#[test]
fn distinguishes_failed_quests_from_scripted_defeats() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    refill_stamina(root.path());
    start(&service, viewer, 1, 1002002, "normal-failure");
    let before = snapshot(root.path());
    let mut failed_body = finish_body(viewer, 1, 1002002, "normal-failure");
    failed_body["is_lose"] = json!(true);
    failed_body["statistics"]["clear_phase"] = json!(99);
    let failed = finish(&service, failed_body.clone());
    let after = snapshot(root.path());
    for key in ["free_mana", "exp_pool", "rank_point", "free_vmoney"] {
        assert_eq!(after["user_info"][key], before["user_info"][key], "{key}");
    }
    for key in ["item_list", "quest_progress"] {
        assert_eq!(after[key], before[key], "{key}");
    }
    for key in [
        "exp",
        "evolution_level",
        "over_limit_step",
        "stack",
        "bond_token_list",
    ] {
        assert_eq!(
            after["user_character_list"]["1"][key], before["user_character_list"]["1"][key],
            "{key}"
        );
    }
    assert_eq!(failed["rewards"]["reward_mana"], 0);
    assert_eq!(failed["rewards"]["field_mana"], 0);
    assert_eq!(failed["drop_score_reward_ids"], json!([]));
    assert_eq!(finish(&service, failed_body), failed);
    assert_eq!(snapshot(root.path()), after);

    start(&service, viewer, 1, 1002002, "normal-clear");
    let mut cleared_body = finish_body(viewer, 1, 1002002, "normal-clear");
    cleared_body["is_accomplished"] = json!(true);
    let cleared_response = finish(&service, cleared_body);
    let cleared = snapshot(root.path());
    assert!(
        cleared["user_info"]["free_vmoney"].as_i64() > before["user_info"]["free_vmoney"].as_i64(),
        "before={}, after={}, response={cleared_response}",
        before["user_info"]["free_vmoney"],
        cleared["user_info"]["free_vmoney"]
    );
    assert!(quest_progress(&cleared, 1, 1002002).is_some());

    start(&service, viewer, 1, 7014002, "story-defeat");
    let before_defeat = snapshot(root.path());
    let mut defeated_body = finish_body(viewer, 1, 7014002, "story-defeat");
    defeated_body["is_lose"] = json!(true);
    finish(&service, defeated_body);
    let defeated = snapshot(root.path());
    assert!(quest_progress(&defeated, 1, 7014002).is_some());
    assert!(
        defeated["user_info"]["free_vmoney"].as_i64()
            > before_defeat["user_info"]["free_vmoney"].as_i64()
    );
    service.stop().unwrap();
}

// //// 核对全部竞速关卡的阶段收益和重复结算 [@x380kkm 2026-09-21] ////
#[test]
fn settles_completed_phases_for_every_ranking_quest() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let rules: Value =
        serde_json::from_str(include_str!("../assets/cn-battle-reward-rules.json")).unwrap();
    let quests = [
        (1001, 1),
        (2001, 2),
        (3001, 3),
        (4001, 4),
        (5001, 5),
        (1000001, 1000),
        (1001001, 1001),
    ];
    for (_, event_id) in quests {
        open_activity(&service, &format!("ranking:{event_id}"));
    }
    let viewer = signup(&service);
    refill_stamina(root.path());
    for (quest_id, _) in quests {
        for (completed, accomplished) in [(0, false), (1, false), (2, false), (3, false), (1, true)]
        {
            let play = format!("ranking-{quest_id}-{completed}-{accomplished}");
            start(&service, viewer, 11, quest_id, &play);
            let before = snapshot(root.path());
            let mut body = finish_body(viewer, 11, quest_id, &play);
            body["statistics"]["clear_phase"] = json!(completed);
            body["is_accomplished"] = json!(accomplished);
            let response = finish(&service, body.clone());
            let after = snapshot(root.path());
            let phase = if accomplished { 4 } else { completed };
            let (mana, exp, rank) = match phase {
                0 => (0, 0, 0),
                1 => (780, 266, 93),
                2 => (1200, 420, 247),
                3 => (1770, 875, 340),
                4 => (2460, 1650, 588),
                _ => unreachable!(),
            };
            assert_eq!(response["rewards"]["reward_mana"], mana);
            assert_eq!(response["rewards"]["reward_pool_exp"], exp);
            assert_eq!(
                after["user_info"]["free_mana"].as_i64(),
                Some(
                    before["user_info"]["free_mana"].as_i64().unwrap()
                        + mana
                        + 7
                        + mission_currency(&response, 3)
                )
            );
            assert_eq!(
                after["user_info"]["rank_point"].as_i64(),
                Some(before["user_info"]["rank_point"].as_i64().unwrap() + rank)
            );
            assert_eq!(
                after["user_info"]["exp_pool"].as_i64(),
                Some(
                    before["user_info"]["exp_pool"].as_i64().unwrap()
                        + exp
                        + mission_currency(&response, 5)
                )
            );
            assert_eq!(
                after["user_character_list"]["1"]["exp"].as_i64(),
                Some(before["user_character_list"]["1"]["exp"].as_i64().unwrap() + exp)
            );
            assert_eq!(quest_progress(&after, 11, quest_id).is_some(), accomplished);
            let quest_rules = &rules["phase_quests"][format!("11:{quest_id}")];
            assert_phase_drops(&response, &before, &after, quest_rules, phase);
            assert_eq!(finish(&service, body), response);
            assert_eq!(snapshot(root.path()), after);
        }
    }
    service.stop().unwrap();
    let restarted = PersonalService::start(root.path(), 0).unwrap();
    let player = snapshot(root.path());
    assert_eq!(
        player["quest_progress"]["11"].as_array().unwrap().len(),
        quests.len()
    );
    restarted.stop().unwrap();
}

// //// 核对掉落抽取次数和实际入库数量 [@x380kkm 2026-09-21] ////
fn assert_phase_drops(response: &Value, before: &Value, after: &Value, rules: &Value, phase: i64) {
    let common = response["drop_score_reward_ids"].as_array().unwrap();
    let rare = response["drop_rare_reward_ids"].as_array().unwrap();
    if phase == 0 {
        assert!(common.is_empty());
        assert!(rare.is_empty());
        return;
    }
    assert_eq!(common.len(), if phase == 4 { 5 } else { 4 });
    let phase_rule = &rules["phases"][phase as usize - 1];
    let mut deltas = BTreeMap::<String, i64>::new();
    for drop in common {
        assert_eq!(drop["group_id"], phase_rule["score_reward_group_id"]);
        let reward = phase_rule["score_rewards"]
            .as_array()
            .unwrap()
            .iter()
            .find(|reward| reward["type"] == 0 && reward["position"] == drop["index"])
            .unwrap();
        let id = reward["id"].as_i64().unwrap().to_string();
        *deltas.entry(id).or_default() += drop["number"].as_i64().unwrap();
    }
    for drop in rare {
        let reward = &rules["rare_reward_groups"][drop["group_id"].as_i64().unwrap().to_string()]
            [drop["index"].as_u64().unwrap() as usize - 1];
        let id = reward["id"].as_i64().unwrap().to_string();
        *deltas.entry(id).or_default() += drop["number"].as_i64().unwrap();
    }
    for (item, delta) in deltas {
        assert_eq!(
            after["item_list"][&item].as_i64(),
            Some(before["item_list"][&item].as_i64().unwrap_or_default() + delta),
            "{item}"
        );
    }
}

// //// 计分活动超时按成绩发奖并保留未达标记录边界 [@x380kkm 2026-09-21] ////
#[test]
fn rewards_score_attack_results_when_the_timer_expires() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    open_activity(&service, "score-attack:1");
    start(&service, viewer, 27, 1001, "score-timeout-low");
    let low = finish_body(viewer, 27, 1001, "score-timeout-low");
    finish(&service, low);
    let below = snapshot(root.path());
    assert!(quest_progress(&below, 27, 1001).is_none());
    start(&service, viewer, 27, 1001, "score-timeout");
    let before = snapshot(root.path());
    let mut body = finish_body(viewer, 27, 1001, "score-timeout");
    body["score"] = json!(1_000_000_000);
    let response = finish(&service, body.clone());
    let after = snapshot(root.path());
    assert!(response["mission_info"]
        .as_array()
        .unwrap()
        .iter()
        .any(|mission| mission["mission_id"] == 10075));
    assert_eq!(
        after["item_list"]["40501"].as_i64(),
        Some(before["item_list"]["40501"].as_i64().unwrap_or_default() + 23 + 6)
    );
    assert_eq!(after["item_list"]["40502"].as_i64(),
        Some(before["item_list"]["40502"].as_i64().unwrap_or_default() + 40));
    assert_eq!(response["clear_rank"], 4);
    assert_eq!(response["rewards"]["reward_mana"], 15);
    assert_eq!(response["rewards"]["reward_pool_exp"], 15);
    assert_eq!(after["user_info"]["rank_point"].as_i64().unwrap(),
        before["user_info"]["rank_point"].as_i64().unwrap() + 10);
    assert_eq!(after["user_info"]["free_vmoney"].as_i64().unwrap(),
        before["user_info"]["free_vmoney"].as_i64().unwrap() + mission_currency(&response, 0));
    assert_eq!(after["score_attack_reward_receipts"].as_object().unwrap().len(), 51);
    assert!(quest_progress(&after, 27, 1001).is_some());
    assert_eq!(finish(&service, body), response);
    assert_eq!(snapshot(root.path()), after);
    service.stop().unwrap();
}
