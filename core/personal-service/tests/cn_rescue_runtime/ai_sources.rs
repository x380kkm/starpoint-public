// audience: internal
// # personal-service-cn-rescue-ai-source-tests
// 内置模板支持队伍不足和清空 AI 选择, 保存快照与玩家编队保持各自的持久化内容.

use super::*;

fn manage_json(service: &PersonalService, method: &str, path: &str) -> Value {
    let token = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        method,
        path,
        "application/json",
        &[("Authorization", &token)],
        &[],
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}

fn party_groups(db: &Connection) -> Value {
    let serialized: String = db
        .query_row(
            "SELECT json_extract(data_json,'$.user_party_group_list') FROM player_snapshots",
            [],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str(&serialized).unwrap()
}

fn clear_parties(
    service: &PersonalService,
    db: &Connection,
    viewer: i64,
    retained_party: Option<i64>,
) {
    let groups = party_groups(db);
    let parties = groups
        .as_object()
        .unwrap()
        .values()
        .flat_map(|group| group["list"].as_object().unwrap().keys())
        .map(|id| id.parse::<i64>().unwrap())
        .filter(|id| Some(*id) != retained_party)
        .map(|id| {
            json!({
                "party_id":id,"party_category":1,"party_edited":true,"party_name":"",
                "character_ids":[null,null,null],"unison_character_ids":[null,null,null],
                "equipment_ids":[null,null,null],"ability_soul_ids":[null,null,null],
                "options":{"allow_other_players_to_heal_me":true}
            })
        })
        .collect::<Vec<_>>();
    data(
        service,
        "party/edit",
        json!({
            "viewer_id":viewer,"main_party_id":1,"use_party_group_edit":false,
            "ignore_ngword":false,"api_count":1,"party_info_list":parties
        }),
    );
}

fn wind_dragon_offer(service: &PersonalService, db: &Connection, viewer: i64) -> Value {
    unlock_attention(db);
    management(
        service,
        "/v1/time",
        json!({"enabled":true,"unix_time_ms":1752292800000_i64,"rate":1.0}),
    );
    management(
        service,
        "/v1/rescue-settings",
        json!({
            "enabled":true,"spawn_interval_seconds":60,"room_lifetime_seconds":180,"max_rooms":1,
            "quests":[{"category_id":2,"quest_id":1028001}]
        }),
    );
    let offer = data(service, "attention/check", json!({"viewer_id":viewer}))["multi"][0].clone();
    assert!(offer["attention_key"].is_string(), "{offer}");
    let sources: Vec<(Option<String>, Option<i64>)> = db
        .prepare("SELECT snapshot_id,template_index FROM multiplayer_ai_mates ORDER BY position")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(sources, vec![(None, Some(0)), (None, Some(1))]);
    offer
}

// //// 单队伍通过默认 BOT 完成风龙救援并重复读取领取回执 [@x380kkm 2026-09-27] ////
#[test]
fn completes_wind_dragon_rescue_with_one_valid_player_party() {
    let root = TempDir::new().unwrap();
    let (service, viewer) = start_player(root.path());
    let db = database(root.path());
    clear_parties(&service, &db, viewer, Some(1));
    let expected_parties = party_groups(&db);
    let offer = wind_dragon_offer(&service, &db, viewer);
    let room = data(
        &service,
        "multi_battle_quest/select_room",
        json!({
            "viewer_id":viewer,"category":2,"quest_id":1028001,"party_id":1,
            "accepted_type":2,"room_number":offer["quest_info"]["room_number"]
        }),
    );
    assert_eq!(room["raising_state"], 1);
    let (lobby, connection) = enter_lobby(&service, viewer, &room);
    let started = start(
        &service,
        viewer,
        &room,
        &offer["attention_key"],
        "single-party-rescue",
    );
    assert_eq!(started["category_id"], 2);
    let mut battle = Socket::connect(room["port"].as_u64().unwrap() as u16);
    battle.send(json!({"socklet":"cooperation_battle","roomNumber":room["room_number"],"connectionId":connection,"reconnected":0}));
    assert_eq!(battle.receive()[0], 0);
    battle.send(json!([0, [0]]));
    assert_eq!(battle.receive(), json!([1, [1]]));
    drop(lobby);
    let body = json!({
        "viewer_id":viewer,"category":2,"quest_id":1028001,"play_id":"single-party-rescue",
        "is_accomplished":true,"elapsed_time_ms":30000,"score":1000,"add_mana":0,
        "statistics":{"clear_phase":1,"zones":[],"party":{"characters":[{"id":1},null,null],
            "unison_characters":[null,null,null],"equipments":[null,null,null],"ability_soul_ids":[null,null,null]}}
    });
    let result = data(&service, "multi_battle_quest/finish", body.clone());
    assert_eq!(data(&service, "multi_battle_quest/finish", body), result);
    assert!(result["item_list"].is_object());
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM ai_team_snapshot_heads", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(party_groups(&db), expected_parties);
    drop(battle);
    service.stop().unwrap();
}

// //// 全部编队为空时仍提供内置救援房 [@x380kkm 2026-09-27] ////
#[test]
fn creates_default_rescue_when_all_player_parties_are_empty() {
    let root = TempDir::new().unwrap();
    let (service, viewer) = start_player(root.path());
    let db = database(root.path());
    clear_parties(&service, &db, viewer, None);
    let expected_parties = party_groups(&db);
    wind_dragon_offer(&service, &db, viewer);
    assert_eq!(party_groups(&db), expected_parties);
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM ai_team_snapshots", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    service.stop().unwrap();
}

// //// 清空 AI 选择后保持历史快照并使用内置救援房 [@x380kkm 2026-09-27] ////
#[test]
fn preserves_cleared_ai_selection_while_serving_default_rescue() {
    let root = TempDir::new().unwrap();
    let (service, viewer) = start_player(root.path());
    let db = database(root.path());
    let slot: i64 = db
        .query_row("SELECT id FROM local_save_slots", [], |row| row.get(0))
        .unwrap();
    let path = format!("/v1/local-saves/{slot}/ai-teams");
    assert_eq!(
        manage_json(&service, "GET", &path)["selected_party_ids"],
        json!([1, 2])
    );
    let expected_parties = party_groups(&db);
    let saved: Vec<(String, String)> = db
        .prepare("SELECT id,data_json FROM ai_team_snapshots ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    manage_json(&service, "DELETE", &path);
    wind_dragon_offer(&service, &db, viewer);
    let selection = manage_json(&service, "GET", &path);
    assert_eq!(selection["selected_party_ids"], json!([]));
    assert_eq!(selection["selection_status"], "manual_selection_required");
    let retained: Vec<(String, String)> = db
        .prepare("SELECT id,data_json FROM ai_team_snapshots ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(retained, saved);
    assert_eq!(party_groups(&db), expected_parties);
    service.stop().unwrap();
}
