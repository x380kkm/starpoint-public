// audience: internal
// # personal-service-cn-party-category-tests
// 普通, 资料与活动编队分别持久化. 分组中的队伍编号在同一活动内唯一.

use super::slots::*;
use super::*;
use serde_json::json;
use std::collections::BTreeSet;

// //// 验证活动编队编辑与颜色按类别隔离并保持全局编号 [@x380kkm 2026-09-23] ////
#[test]
fn persists_each_party_category_without_replacing_normal_parties() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let (viewer, slot) = prepare(&root, &service);
    data(
        &service,
        "party/edit",
        edit_body(viewer, vec![party(1, 12)]),
    );
    let before = player(&service, slot);
    let routes = [
        "carnival_event/get_party",
        "event/raid/party",
        "event/rush/party",
    ];
    for route in routes {
        data(&service, route, json!({"viewer_id":viewer}));
    }
    let mut raid = party(3, 2);
    raid["character_ids"] = json!([111001, null, null]);
    raid["ability_soul_ids"] = json!([100001, null, null]);
    data(
        &service,
        "party/edit",
        edit_body(viewer, vec![party(2, 12), raid, party(4, 12), party(99, 1)]),
    );
    data(
        &service,
        "party_group/edit",
        json!({"viewer_id":viewer,"party_group_edit_params_list":[
            {"party_category":2,"party_group_id":2,"party_group_color_id":6},
            {"party_category":3,"party_group_id":1,"party_group_color_id":7},
            {"party_category":4,"party_group_id":2,"party_group_color_id":8},
            {"party_category":99,"party_group_id":1,"party_group_color_id":9}
        ]}),
    );
    let after = player(&service, slot);
    assert_eq!(
        after["user_party_group_list"],
        before["user_party_group_list"]
    );
    assert_eq!(after["user_info"]["party_slot"], 12);
    assert_eq!(after["profile_party_group_list"]["1"]["color_id"], 9);
    for (family, group, position, category, color) in [
        ("carnival", 1, 1, 2, 6),
        ("raid", 0, 1, 3, 7),
        ("rush", 1, 1, 4, 8),
    ] {
        let event = if family == "carnival" { "1" } else { "0" };
        let groups =
            &after["cn_activity_state"]["event_families"][family][event]["party_group_list"];
        assert_eq!(groups[group]["party_group_color_id"], color);
        assert_eq!(
            groups[group]["party_list"][position]["party_name"],
            format!("编队 {category}:{}", if category == 3 { 2 } else { 12 })
        );
        assert_eq!(
            groups[group]["party_list"][position]["ability_soul_ids"],
            if category == 3 {
                json!([100001, null, null])
            } else {
                json!([100001, 100001, 100002])
            }
        );
    }
    let before_rejected = player(&service, slot);
    let rejected = send(
        &service,
        "party/edit",
        edit_body(viewer, vec![party(1, 1), party(8, 1)]),
    );
    assert!(rejected.contains("invalid_party_category"));
    assert_eq!(player(&service, slot), before_rejected);
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    for (route, color, group) in [(routes[0], 6, 1), (routes[1], 7, 0), (routes[2], 8, 1)] {
        let loaded = data(&service, route, json!({"viewer_id":viewer}));
        let groups = loaded["user_party_group_list"].as_array().unwrap();
        assert_eq!(groups[group]["party_group_color_id"], color);
        let ids = groups
            .iter()
            .flat_map(|group| group["party_list"].as_array().unwrap())
            .map(|party| party["party_id"].as_i64().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            ids.iter().copied().collect::<BTreeSet<_>>().len(),
            ids.len()
        );
    }
    service.stop().unwrap();
}
