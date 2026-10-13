// audience: internal
// # personal-service-cn-battle-character-reward-tests
// 关卡角色主表经统一奖励结构结算后, 首次通关只发放该角色的一个实例.

use super::*;

// //// 通过真实入场和结算验证赠角关卡的完整发奖数量 [@x380kkm 2026-09-26] ////
#[test]
fn grants_one_character_from_each_clear_reward_master_record() {
    for (category, quest, activity, character) in [
        (10, 200031018, "story:200031", 131182),
        (7, 100006001, "advent:100006", 213007),
        (7, 300000001, "advent:300000", 263015),
        (7, 300001001, "advent:300001", 123001),
    ] {
        let root = TempDir::new().unwrap();
        let service = PersonalService::start(root.path(), 0).unwrap();
        let viewer = decode_response::<SignupData>(&send_request(
            service.port(),
            "/api/index.php/tool/signup",
            &encode_request(&SignupRequest { device_id: quest }),
        ))
        .data_headers
        .viewer_id;
        set_virtual_time(&service, "2025-01-02T12:00:00.000Z");
        set_activity_always(&service, activity);
        let started = decode_response::<Value>(&start_battle_for(
            service.port(),
            viewer,
            category,
            quest,
            "character-clear",
            false,
        ));
        assert_eq!(
            started.data_headers.result_code, 1,
            "{quest}: {}",
            started.data
        );
        let response = finish_battle_for(
            service.port(),
            viewer,
            category,
            quest,
            "character-clear",
            1000,
            1000,
        );
        let result = decode_response::<Value>(&response);
        assert_eq!(
            result.data_headers.result_code, 1,
            "{quest}: {}",
            result.data
        );
        assert_eq!(result.data["joined_character_id_list"], json!([character]));
        let granted = result.data["character_list"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["character_id"] == character)
            .unwrap();
        assert_eq!(granted["entry_count"], 1);
        let connection = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
        let stored: String = connection
            .query_row("SELECT data_json FROM player_snapshots", [], |row| {
                row.get(0)
            })
            .unwrap();
        let loaded: Value = serde_json::from_str(&stored).unwrap();
        assert_eq!(
            loaded["user_character_list"][character.to_string()]["stack"],
            0
        );
        assert_eq!(
            loaded["user_character_list"][character.to_string()]["entry_count"],
            1
        );
        let replay = decode_response::<Value>(&finish_battle_for(
            service.port(),
            viewer,
            category,
            quest,
            "character-clear",
            1000,
            1000,
        ));
        assert_eq!(replay.data_headers.result_code, 1);
        assert_eq!(replay.data, result.data);
        let replayed: String = connection
            .query_row("SELECT data_json FROM player_snapshots", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(replayed, stored);
        service.stop().unwrap();
    }
}
// //// /通过真实入场和结算验证赠角关卡的完整发奖数量 ////
