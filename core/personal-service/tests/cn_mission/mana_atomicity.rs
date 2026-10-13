// audience: internal
// # personal-service-cn-mana-atomicity-tests
//
// 节点动作的任务阶段保存与经济奖励保存共享事务. 末尾记录失败保留原有资产和节点状态.

use super::*;

fn stored_row_counts(root: &Path) -> Vec<i64> {
    let database = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    ["player_mission_counters", "player_mission_progress", "player_receive_history", "player_activity_history"].into_iter()
        .map(|table| database.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0)).unwrap()).collect()
}

// //// 在经济奖励记录失败时回滚节点动作并允许正常重试 [@x380kkm 2026-09-22] ////
#[test]
fn rolls_back_mana_nodes_and_earlier_mission_writes_after_late_history_failure() {
    for (device, action, spent) in [(922111, "learn_mana_node", 1340), (922112, "awake_mana_node", 1600)] {
        let root = TempDir::new().unwrap();
        let service = PersonalService::start(root.path(), 0).unwrap();
        let viewer_id = signup(&service, device);
        open_activity(&service, "collect-item:10003");
        update_player(root.path(), |player| {
            player["user_info"]["free_mana"] = json!(100);
            player["user_info"]["paid_mana"] = json!(2000);
            for id in ["1", "2", "3", "4", "99"] { player["item_list"][id] = json!(1000); }
            player["item_list"]["70047"] = json!(30);
            if action == "awake_mana_node" {
                player["user_character_mana_node_list"]["1"] = json!([{"multiplied_id":2201,"awake_level":0}]);
                player["mission_window_progress"] = json!({"9:11":3,"9:12":100,"9:13":97});
            }
        });
        let request = if action == "learn_mana_node" {
            json!({"viewer_id":viewer_id,"character_id":1,"api_count":1,"mana_node_multiplied_id_list":(2201..=2207).collect::<Vec<_>>()})
        } else {
            json!({"viewer_id":viewer_id,"character_id":1,"mana_node_multiplied_id_list":[2201],"awake_level":1})
        };
        let path = format!("/api/index.php/character/{action}");
        let before = read_player(root.path());
        let rows_before = stored_row_counts(root.path());
        let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
        database.execute_batch("CREATE TRIGGER reject_mana_economic_history BEFORE INSERT ON player_receive_history WHEN NEW.kind=8 AND NEW.number=150000 BEGIN SELECT RAISE(ABORT,'mana economic history failure'); END;").unwrap();
        assert!(send(&service, &path, request.clone()).starts_with("HTTP/1.1 500"));
        let rejected = read_player(root.path());
        assert_eq!(rejected["user_info"]["free_mana"], before["user_info"]["free_mana"], "{action}");
        assert_eq!(rejected, before, "{action}");
        assert_eq!(stored_row_counts(root.path()), rows_before, "{action}");
        database.execute_batch("DROP TRIGGER reject_mana_economic_history;").unwrap();
        let response = decode_response::<Value>(&send(&service, &path, request.clone())).data;
        assert_eq!(response["user_info"]["free_mana"], 150000);
        assert_eq!(response["user_info"]["paid_mana"], 2100 - spent);
        let completed = read_player(root.path());
        assert_eq!(completed["mission_economic_counters"]["total_used_mana_count"], spent);
        assert_eq!(database.query_row::<i64, _, _>("SELECT COUNT(*) FROM player_receive_history WHERE kind=8 AND number=150000", [], |row| row.get(0)).unwrap(), 1);
        let repeated = send(&service, &path, request);
        decode_response::<Value>(&repeated);
        assert_eq!(read_player(root.path()), completed);
        service.stop().unwrap();
    }
}
// //// /在经济奖励记录失败时回滚节点动作并允许正常重试 ////
