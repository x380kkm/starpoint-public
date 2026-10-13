// audience: internal
// # personal-service-cn-raid-party-member-tests
// 共斗编队覆盖初始化, 旧组读取与批量编辑中的主副位角色占用, 普通编队保持独立.

use super::slots::*;
use super::*;
use serde_json::json;

fn raid_groups(service: &PersonalService, viewer: i64) -> Value {
    data(service, "event/raid/party", json!({"viewer_id": viewer}))["user_party_group_list"].clone()
}

fn raid_party(id: i64, character: Option<i64>) -> Value {
    let mut value = party(3, id);
    value["character_ids"] = json!([character, null, null]);
    value["unison_character_ids"] = json!([null, null, null]);
    value["ability_soul_ids"] = json!([100001, null, null]);
    value
}

// //// 验证共斗初始化和旧组读取保留首队并补齐去重后的队长 [@x380kkm 2026-09-28] ////
#[test]
fn initializes_and_reads_raid_groups_with_unique_character_references() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let (viewer, slot) = prepare(&root, &service);
    update_slot_player_snapshot(root.path(), slot, |state| {
        let parties = &mut state["user_party_group_list"]["1"]["list"];
        parties["1"]["character_ids"] = json!([1, 111001, null]);
        parties["1"]["unison_character_ids"] = json!([231001, null, null]);
        parties["1"]["equipment_ids"] = json!([5030037, null, null]);
        parties["1"]["ability_soul_ids"] = json!([100001, null, null]);
        parties["2"]["character_ids"] = json!([111001, null, null]);
        parties["2"]["equipment_ids"] = json!([5030037, null, null]);
        parties["2"]["ability_soul_ids"] = json!([100001, null, null]);
        parties["3"]["character_ids"] = json!([231001, null, null]);
    });
    let normal = player(&service, slot)["user_party_group_list"].clone();
    let initial = raid_groups(&service, viewer);
    let parties = initial[0]["party_list"].as_array().unwrap();
    assert_eq!(parties.len(), 3);
    assert_eq!(parties[0]["character_ids"], json!([1, 111001, null]));
    assert_eq!(
        parties[0]["unison_character_ids"],
        json!([231001, null, null])
    );
    assert_eq!(parties[0]["equipment_ids"], json!([5030037, null, null]));
    let inventory = player(&service, slot)["user_character_list"].clone();
    let mut used_leaders = std::collections::BTreeSet::from([1]);
    for other in &parties[1..] {
        let leader = other["character_ids"][0].as_i64().unwrap();
        assert!(inventory.get(leader.to_string()).is_some());
        assert!(![1, 111001, 231001].contains(&leader));
        assert!(used_leaders.insert(leader));
        assert_eq!(other["equipment_ids"], json!([null, null, null]));
        assert_eq!(other["ability_soul_ids"], json!([null, null, null]));
    }
    assert_eq!(player(&service, slot)["user_party_group_list"], normal);
    update_slot_player_snapshot(root.path(), slot, |state| {
        let parties = &mut state["cn_activity_state"]["event_families"]["raid"]["0"]
            ["party_group_list"][0]["party_list"];
        parties[1]["character_ids"] = json!([231001, null, null]);
        parties[1]["unison_character_ids"] = json!([111001, null, null]);
        parties[1]["equipment_ids"] = json!([5030037, null, null]);
        parties[1]["ability_soul_ids"] = json!([100001, null, null]);
        parties[1]["party_name"] = json!("保留队名");
        parties.as_array_mut().unwrap().truncate(2);
    });
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let restored = raid_groups(&service, viewer);
    let parties = restored[0]["party_list"].as_array().unwrap();
    assert_eq!(parties[0], initial[0]["party_list"][0]);
    assert_eq!(parties[1]["party_name"], "保留队名");
    assert_eq!(parties[1]["character_ids"], initial[0]["party_list"][1]["character_ids"]);
    assert_eq!(
        parties[1]["unison_character_ids"],
        json!([null, null, null])
    );
    assert_eq!(parties[1]["equipment_ids"], json!([null, null, null]));
    assert_eq!(parties[1]["ability_soul_ids"], json!([null, null, null]));
    let added_leader = parties[2]["character_ids"][0].as_i64().unwrap();
    assert!(![1, 111001, 231001].contains(&added_leader));
    assert_eq!(player(&service, slot)["user_party_group_list"], normal);
    service.stop().unwrap();
}

// //// 验证共斗批量编辑保护未编辑队伍并补齐空队长 [@x380kkm 2026-09-28] ////
#[test]
fn edits_raid_parties_as_one_batch_and_preserves_other_members() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let (viewer, slot) = prepare(&root, &service);
    update_slot_player_snapshot(root.path(), slot, |state| {
        let parties = &mut state["user_party_group_list"]["1"]["list"];
        for (id, character) in [("1", 1), ("2", 111001), ("3", 231001)] {
            parties[id]["character_ids"] = json!([character, null, null]);
        }
    });
    let normal = player(&service, slot)["user_party_group_list"].clone();
    let initial = raid_groups(&service, viewer);
    let mut conflict = raid_party(1, Some(231001));
    conflict["unison_character_ids"] = json!([1, null, null]);
    data(
        &service,
        "party/edit",
        edit_body(viewer, vec![conflict, raid_party(2, Some(1))]),
    );
    let groups = &player(&service, slot)["cn_activity_state"]["event_families"]["raid"]["0"]
        ["party_group_list"];
    assert_eq!(groups[0]["party_list"][2], initial[0]["party_list"][2]);
    assert_eq!(
        groups[0]["party_list"][0]["character_ids"],
        json!([1, null, null])
    );
    assert_eq!(
        groups[0]["party_list"][0]["equipment_ids"],
        json!([null, null, null])
    );
    assert_eq!(
        groups[0]["party_list"][0]["ability_soul_ids"],
        json!([null, null, null])
    );
    assert_eq!(
        groups[0]["party_list"][0]["unison_character_ids"],
        json!([null, null, null])
    );
    assert_eq!(
        groups[0]["party_list"][1]["character_ids"],
        json!([111001, null, null])
    );
    data(
        &service,
        "party/edit",
        edit_body(
            viewer,
            vec![
                raid_party(3, Some(1)),
                raid_party(1, Some(111001)),
                raid_party(2, Some(231001)),
            ],
        ),
    );
    let swapped = raid_groups(&service, viewer);
    for (index, character) in [(0, 111001), (1, 231001), (2, 1)] {
        assert_eq!(
            swapped[0]["party_list"][index]["character_ids"],
            json!([character, null, null])
        );
        assert_eq!(
            swapped[0]["party_list"][index]["ability_soul_ids"],
            json!([100001, null, null])
        );
    }
    data(
        &service,
        "party/edit",
        edit_body(viewer, vec![raid_party(3, None)]),
    );
    let empty = raid_groups(&service, viewer);
    assert_eq!(
        empty[0]["party_list"][2]["character_ids"],
        json!([1, null, null])
    );
    assert_eq!(
        empty[0]["party_list"][2]["ability_soul_ids"],
        json!([null, null, null])
    );
    assert_eq!(empty[0]["party_list"][0], swapped[0]["party_list"][0]);
    assert_eq!(empty[0]["party_list"][1], swapped[0]["party_list"][1]);
    assert_eq!(player(&service, slot)["user_party_group_list"], normal);
    service.stop().unwrap();
}

// //// 三个持有角色集中首队的旧档分配真实队长并保留装配 [@x380kkm 2026-09-28] ////
#[test]
fn repairs_raid_leaders_when_owned_characters_are_concentrated_in_one_party() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let (viewer, slot) = prepare(&root, &service);
    update_slot_player_snapshot(root.path(), slot, |state| {
        state["user_character_list"].as_object_mut().unwrap()
            .retain(|id, _| ["1", "111001", "231001"].contains(&id.as_str()));
        let parties = &mut state["user_party_group_list"]["1"]["list"];
        parties["1"]["character_ids"] = json!([1, 111001, 231001]);
        parties["1"]["unison_character_ids"] = json!([null, null, null]);
        parties["1"]["equipment_ids"] = json!([null, 5030037, null]);
        parties["1"]["ability_soul_ids"] = json!([100001, 100001, 100002]);
        parties["2"]["character_ids"] = json!([1, null, null]);
        parties["3"]["character_ids"] = json!([1, null, null]);
    });
    let before = player(&service, slot);
    let groups = raid_groups(&service, viewer);
    let parties = groups[0]["party_list"].as_array().unwrap();
    for (index, leader) in [(0, 1), (1, 231001), (2, 111001)] {
        assert_eq!(parties[index]["character_ids"], json!([leader, null, null]));
    }
    assert_eq!(parties[0]["equipment_ids"], json!([null, null, null]));
    assert_eq!(parties[2]["equipment_ids"], json!([5030037, null, null]));
    assert_eq!(parties[2]["ability_soul_ids"], json!([100001, null, null]));
    assert_eq!(raid_groups(&service, viewer), groups);
    let after = player(&service, slot);
    for field in ["user_character_list", "user_equipment_list", "item_list", "user_party_group_list"] {
        assert_eq!(after[field], before[field], "{field}");
    }
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    assert_eq!(raid_groups(&service, viewer), groups);
    service.stop().unwrap();
}
