// audience: internal
// # personal-service-box-gacha-projection-tests
//
// 箱子主表按箱池编号分组, 客户端只展示期间覆盖当前时间的箱子, 期间与所属箱池使用同一开放区间.

use super::ordered_map::decode_ordered_map;
use super::{
    master_seed, project_master_value, projected_time, projection_manifest, projection_window,
    OrderedValue, TimeKind,
};

// //// 验证每个箱池只把自己的箱子期间同步到开放区间 [@x380kkm 2026-10-04] ////
#[test]
fn projects_box_windows_by_parent_box_gacha() {
    let master = projection_manifest()
        .unwrap()
        .masters
        .iter()
        .find(|master| master.name == "box")
        .unwrap();
    let seed = decode_ordered_map(master_seed(&master.name).unwrap()).unwrap();
    let OrderedValue::Map(seed_groups) = &seed else {
        panic!("box seed is not a map");
    };
    let window = projection_window(2_524_608_000_000, 2_524_694_400_000, i64::MAX);
    for (box_gacha_id, _) in seed_groups {
        let mut projected = seed.clone();
        assert!(project_master_value(
            &mut projected,
            master,
            box_gacha_id,
            &window
        ));
        let OrderedValue::Map(groups) = projected else {
            panic!("projected box master is not a map");
        };
        for ((group_id, group), (_, original_group)) in groups.iter().zip(seed_groups) {
            let (OrderedValue::Map(boxes), OrderedValue::Map(original_boxes)) =
                (group, original_group)
            else {
                panic!("box group {group_id} is not a map");
            };
            for ((box_id, value), (_, original)) in boxes.iter().zip(original_boxes) {
                let (OrderedValue::Row(row), OrderedValue::Row(original)) = (value, original)
                else {
                    panic!("box {group_id}/{box_id} is not a row");
                };
                for (index, field) in row.iter().enumerate() {
                    let expected = match (group_id == box_gacha_id, index) {
                        (true, 13) => projected_time(TimeKind::Start, &window),
                        (true, 14) => projected_time(TimeKind::End, &window),
                        _ => original[index].clone(),
                    };
                    assert_eq!(
                        *field, expected,
                        "box gacha {box_gacha_id}, box {group_id}/{box_id}, field {index}"
                    );
                }
            }
        }
    }
}
// //// /验证每个箱池只把自己的箱子期间同步到开放区间 ////
