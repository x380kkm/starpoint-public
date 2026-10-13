// audience: internal
// # personal-service-cn-expod-regeneration-tests
// 自然增长的经验在强化时参与扣除, 请求重放和服务重启保持同一余额.

use super::*;
use serde_json::json;

// //// 使用在线累积经验强化并保留不足一档的计时 [@x380kkm 2026-09-26] ////
#[test]
fn injects_accrued_exp_and_preserves_the_remaining_interval() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 412 }),
    ))
    .data_headers
    .viewer_id;
    let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let read_player = || {
        let data: String = database
            .query_row("SELECT data_json FROM player_snapshots", [], |row| {
                row.get(0)
            })
            .unwrap();
        serde_json::from_str::<Value>(&data).unwrap()
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let mut player = read_player();
    player["user_info"]["exp_pool"] = json!(0);
    player["user_info"]["exp_pooled_time"] = json!(now - 1_500);
    let initial_exp = player["user_character_list"]["1"]["exp"].as_i64().unwrap();
    database
        .execute(
            "UPDATE player_snapshots SET data_json=?1",
            [player.to_string()],
        )
        .unwrap();
    let body = encode_request(&InjectExpRequest {
        character_id: 1,
        viewer_id,
        exp: 20,
        api_count: 410,
    });
    let response =
        cn_support::send_request(service.port(), "/api/index.php/expod/inject_exp", &body);
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let received = decode_response::<Value>(&response);
    assert_eq!(received.data["user_info"]["exp_pool"], 0);
    assert_eq!(received.data["user_info"]["exp_pooled_time"], now - 300);
    assert_eq!(received.data["character_list"][0]["exp"], initial_exp + 20);
    let replay = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/expod/inject_exp",
        &body,
    ));
    assert_eq!(replay.data, received.data);
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(loaded.data["user_info"]["exp_pool"], 0);
    assert_eq!(loaded.data["user_info"]["exp_pooled_time"], now - 300);
    assert_eq!(
        read_player()["user_character_list"]["1"]["exp"],
        initial_exp + 20
    );
    service.stop().unwrap();
}
// //// /使用在线累积经验强化并保留不足一档的计时 ////
