// audience: internal
// # personal-service-cn-box-capacity-tests
// 尾箱批量抽取把普通奖励按余量存入库存和邮件, 重放与重启保留同一结算.

use super::*;

// //// 验证末箱同时发放道具, 玛纳和经验的容量与邮件领取 [@x380kkm 2026-09-28] ////
#[test]
fn settles_ordinary_reward_overflow_once_and_claims_the_mail() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = decode_response::<SignupData>(&cn_support::send_request(
        service.port(), "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 92841 }),
    )).data_headers.viewer_id;
    set_remaining_rewards(&root, 1, 1, &["1000101004", "1000101009", "1000101010"]);
    update_player_snapshot(&root, |data| {
        data["item_list"]["30101"] = json!(30);
        data["item_list"]["49"] = json!(9999);
        data["user_info"]["paid_mana"] = json!(1000);
        data["user_info"]["free_mana"] = json!(999_998_949_i64);
        data["user_info"]["exp_pool"] = json!(1_999_999_500_i64);
        data["user_info"]["exp_pooled_time"] = json!(4_070_908_800_i64);
    });
    let request = encode_request(&BoxExecRequest {
        viewer_id: viewer, box_gacha_id: 1, box_id: 1, number: 9999,
        stop_on_featured_rewards: false, api_count: Some(92841),
    });
    let response = cn_support::send_request(service.port(), "/api/index.php/box_gacha/exec", &request);
    let result = decode_response::<Value>(&response).data;
    assert_eq!(result["mail_arrived"], true);
    assert_eq!(result["item_list"]["49"], 9999);
    assert_eq!(result["item_list"]["30101"], 0);
    assert_eq!(result["user_info"]["free_mana"], 999_998_999_i64);
    assert_eq!(result["user_info"]["exp_pool"], 2_000_000_000_i64);
    assert!(result["over_max"].as_array().unwrap().contains(&json!({"process_type":1,"free_mana":350})));
    assert_eq!(result["all_box_info"].as_array().unwrap().last().unwrap()["is_closed"], true);
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let rewards: String = db.query_row("SELECT rewards_json FROM player_mails", [], |row| row.get(0)).unwrap();
    let rewards: Value = serde_json::from_str(&rewards).unwrap();
    assert_eq!(rewards["itemList"]["49"], 1);
    assert_eq!(rewards["freeMana"], 350);
    assert_eq!(rewards["expPool"], 1500);
    let settled = read_player_snapshot(&root);
    assert_eq!(cn_support::send_request(service.port(), "/api/index.php/box_gacha/exec", &request), response);
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    assert_eq!(cn_support::send_request(service.port(), "/api/index.php/box_gacha/exec", &request), response);
    assert_eq!(read_player_snapshot(&root), settled);
    let mails: i64 = db.query_row("SELECT COUNT(*) FROM player_mails", [], |row| row.get(0)).unwrap();
    assert_eq!(mails, 1);
    update_player_snapshot(&root, |data| {
        data["item_list"]["49"] = json!(9998);
        data["user_info"]["free_mana"] = json!(999_998_649_i64);
        data["user_info"]["exp_pool"] = json!(1_999_998_500_i64);
        data["user_info"]["exp_pooled_time"] = json!(4_070_908_800_i64);
    });
    let claimed = decode_response::<Value>(&cn_support::send_request(
        service.port(), "/api/index.php/mail/receive_all",
        &encode_request(&json!({"viewer_id":viewer})),
    )).data;
    assert_eq!(claimed["item_list"]["49"], 9999);
    assert_eq!(claimed["user_info"]["free_mana"], 999_998_999_i64);
    assert_eq!(claimed["user_info"]["exp_pool"], 2_000_000_000_i64);
    service.stop().unwrap();
}
