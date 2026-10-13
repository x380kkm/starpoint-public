// audience: internal
// # personal-service-cn-currency-exchange-bond-claim-tests
// 角色领取的羁绊证可用于装备兑换, 两次资产变动分别保留到账记录.

use super::*;

// //// 从角色领证到装备兑换保留余额和管理台记录 [@x380kkm 2026-09-23] ////
#[test]
fn exchanges_a_claimed_character_bond_token() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 107);
    update(root.path(), viewer, |data| {
        data["user_info"]["bond_token"] = json!(49);
        data["user_character_list"]["1"]["bond_token_list"][0]["status"] = json!(1);
        data["user_character_list"]["1"]["exp"] = json!(76271);
        data["user_character_list"]["1"]["over_limit_step"] = json!(4);
    });
    let claim = json!({"viewer_id":viewer,"character_id":1,"mana_board_index":1});
    let claimed = send(&service, "character/receive_bond_token", &claim);
    assert_eq!(claimed["user_info"]["bond_token"], 50);
    let body = bond_request(viewer, 5010005, 1);
    let exchanged = send(&service, "exchange/bond_token", &body);
    assert_eq!(exchanged["user_info"]["bond_token"], 0);
    assert_eq!(exchanged["equipment_list"][0]["equipment_id"], 5010005);
    let received = history(&service);
    for (source, kind, target) in [("reward", 10, None), ("exchange", 6, Some(5010005))] {
        assert!(
            received["entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["source"] == source
                    && entry["kind"] == kind
                    && entry["type_id"].as_i64() == target
                    && entry["number"] == 1),
            "{received}"
        );
    }
    let before = stored(root.path(), viewer);
    assert_eq!(send(&service, "exchange/bond_token", &body), exchanged);
    assert_eq!(stored(root.path(), viewer), before);
    assert_eq!(
        send(&service, "character/receive_bond_token", &claim)["user_info"]["bond_token"],
        0
    );
    assert_eq!(history(&service), received);
    service.stop().unwrap();
}
// //// /从角色领证到装备兑换保留余额和管理台记录 ////
