// audience: internal
// # personal-service-multiplayer-presentation-tests
// 旧联机结算回执按当前玩家存档补全展示角色, 持久化资产和回执保持原样.

use super::*;

// //// 重放旧联机活动结算时使用当前队伍并保持奖励幂等 [@x380kkm 2026-09-28] ////
#[test]
fn replays_cached_finish_with_current_character_presentation() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 9287401);
    let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let (account_id, serialized): (i64, String) = database.query_row(
        "SELECT account_id,data_json FROM player_snapshots",
        [], |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap();
    let mut player: Value = serde_json::from_str(&serialized).unwrap();
    player["user_character_list"]["111001"] = player["user_character_list"]["1"].clone();
    player["user_party_group_list"]["1"]["list"]["1"]["character_ids"] = json!([111001,null,null]);
    let player = player.to_string();
    database.execute(
        "UPDATE player_snapshots SET data_json=?1 WHERE account_id=?2",
        rusqlite::params![player, account_id],
    ).unwrap();
    let cached = json!({
        "host_finished":false,"contribution_score":721,
        "user_info":{"free_vmoney":9123},"item_list":{"49100":42},
        "carnival_event":{"leader_character_id":0,"score":{"time_bonus":61000},"reward_ids":[11]},
        "score_attack_event":null,"solo_time_attack_event":null,
    });
    database.execute(
        "INSERT INTO multiplayer_battle_action_receipts
         (account_id,room_number,action,play_id,api_count,response_time,response_json)
         VALUES (?1,'100001','finish','cached-activity-finish',-1,1750000000,?2)",
        rusqlite::params![account_id, cached.to_string()],
    ).unwrap();
    let body = json!({
        "viewer_id":viewer,"category":22,"quest_id":1001,"play_id":"cached-activity-finish",
    });
    let mut expected = cached.clone();
    expected["carnival_event"]["leader_character_id"] = json!(111001);
    for _ in 0..2 {
        assert_eq!(send(&service, "/api/index.php/multi_battle_quest/finish", body.clone()), expected);
    }
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    assert_eq!(send(&service, "/api/index.php/multi_battle_quest/finish", body), expected);
    let persisted: String = database.query_row(
        "SELECT data_json FROM player_snapshots WHERE account_id=?1",
        [account_id], |row| row.get(0),
    ).unwrap();
    assert_eq!(persisted, player);
    let receipt: String = database.query_row(
        "SELECT response_json FROM multiplayer_battle_action_receipts WHERE account_id=?1",
        [account_id], |row| row.get(0),
    ).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&receipt).unwrap(), cached);
    assert_eq!(database.query_row(
        "SELECT COUNT(*) FROM player_mails", [], |row| row.get::<_, i64>(0),
    ).unwrap(), 0);
    assert_eq!(database.query_row(
        "SELECT COUNT(*) FROM multiplayer_battle_action_receipts", [], |row| row.get::<_, i64>(0),
    ).unwrap(), 1);
    service.stop().unwrap();
}
