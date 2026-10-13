// audience: internal
// # personal-service-cn-exchange-overflow-tests
// 兑换响应的邮件状态同时包含已有邮件和当前事务的新投递.

use super::*;

// //// 验证角色赠礼的转换邮件立即通知且重放保留通知 [@x380kkm 2026-09-28] ////
#[test]
fn announces_pending_exchange_mail_before_snapshot_commit() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    set_time(&service, 1_735_790_400);
    send(&service, "special_exchange/enter_campaign", &json!({"viewer_id":viewer,"campaign_id":50013,"api_count":1}));
    send(&service, "special_exchange/exchange_character", &json!({"viewer_id":viewer,"campaign_id":50013,"character_id":111135,"api_count":2}));
    update(root.path(), |data| {
        data["special_exchange_campaign_list"] = json!([{"campaign_id":50013,"status":2}]);
        data["user_character_list"]["111135"]["stack"] = json!(99);
        data["user_info"]["exp_pool"] = json!(2_000_000_000_i64);
        data["item_list"]["990008"] = json!(999999);
        data["item_list"]["999009"] = json!(1);
    });
    let body = json!({"viewer_id":viewer,"campaign_id":50013,"character_id":111135,"api_count":3});
    let response = send(&service, "special_exchange/exchange_character", &body);
    assert_eq!(response["mail_arrived"], true);
    assert_eq!(send(&service, "special_exchange/exchange_character", &body), response);
    let mail_count: i64 = db(root.path()).query_row("SELECT COUNT(*) FROM player_mails", [], |row| row.get(0)).unwrap();
    assert_eq!(mail_count, 1);
    service.stop().unwrap();
}
