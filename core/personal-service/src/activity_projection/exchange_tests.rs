// audience: internal
// # activity-projection-exchange-tests
// 兑换活动的客户端截止时间与服务端主表宽限期保持一致.

use super::*;
use tempfile::TempDir;

// //// 核对兑换期内的活动投影及结束后的关闭状态 [@x380kkm 2026-09-22] ////
#[test]
fn keeps_exchange_projection_until_its_master_deadline() {
    let root = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(root.path()).unwrap();
    let start = 1_900_000_000_000;
    let end = start + MILLIS_PER_DAY;
    let master = projection_manifest()
        .unwrap()
        .masters
        .iter()
        .find(|master| master.name == "multi_special_exchange_campaign")
        .unwrap();
    for campaign_id in 1..=3 {
        let activity_id = format!("multi-special-exchange:{campaign_id}");
        database
            .upsert_activity_schedule(&activity_id, true, start, end)
            .unwrap();
        database
            .set_virtual_time(true, end + MILLIS_PER_DAY, 1.0)
            .unwrap();
        let windows = active_windows(&database).unwrap();
        let window = windows
            .get(&activity_id)
            .expect("exchange period stays visible");
        let delay = crate::cn_multi_special_exchange::reward_period_millis(&activity_id)
            .unwrap()
            .unwrap();
        assert_eq!(window.end_at_ms, end);
        assert_eq!(window.reward_delay_ms, delay);
        let mut value = decode_ordered_map(master_seed(&master.name).unwrap()).unwrap();
        assert!(project_master_value(
            &mut value,
            master,
            &campaign_id.to_string(),
            window
        ));
        let OrderedValue::Map(entries) = value else {
            panic!("campaign master contains rows")
        };
        let OrderedValue::Row(row) = &entries
            .iter()
            .find(|(id, _)| *id == campaign_id.to_string())
            .unwrap()
            .1
        else {
            panic!("campaign row contains scalar fields")
        };
        assert_eq!(
            row[3],
            format_client_time((start + MASTER_TIMEZONE_OFFSET_MS) / 1_000)
        );
        assert_eq!(
            row[4],
            format_client_time((end + MASTER_TIMEZONE_OFFSET_MS) / 1_000)
        );
        assert_eq!(
            row[5],
            format_client_time((end + delay + MASTER_TIMEZONE_OFFSET_MS) / 1_000)
        );
        database
            .set_virtual_time(true, end + delay + 1_000, 1.0)
            .unwrap();
        let expired = active_windows(&database).unwrap();
        assert_eq!(expired[&activity_id].end_at_ms, end);
        assert!(
            expired[&activity_id].end_at_ms + expired[&activity_id].reward_delay_ms
                < database.current_server_time_millis().unwrap()
        );
    }
}
// //// /核对兑换期内的活动投影及结束后的关闭状态 ////

// //// 核对全部自选主表的购买期与兑换期并保留其他字段 [@x380kkm 2026-09-23] ////
#[test]
fn projects_optional_exchange_windows_without_changing_candidates() {
    let root = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(root.path()).unwrap();
    let start = 1_900_000_000_000;
    let end = start + MILLIS_PER_DAY;
    for (master_name, prefix, start_index, end_index) in [
        ("special_exchange_campaign", "special-exchange:", 4, 5),
        (
            "treasure_vault_exchange_campaign",
            "treasure-vault-exchange:",
            4,
            5,
        ),
        ("start_dash_exchange_campaign", "start-dash-exchange:", 2, 3),
    ] {
        let master = projection_manifest()
            .unwrap()
            .masters
            .iter()
            .find(|master| master.name == master_name)
            .unwrap();
        let original = decode_ordered_map(master_seed(master_name).unwrap()).unwrap();
        let OrderedValue::Map(entries) = &original else {
            panic!("campaign master contains rows")
        };
        for (id, source) in entries {
            let OrderedValue::Row(source_row) = source else {
                panic!("campaign row contains fields")
            };
            let activity_id = format!("{prefix}{id}");
            let delay = crate::cn_optional_exchange::reward_period_millis(&activity_id)
                .unwrap()
                .unwrap();
            database
                .upsert_activity_schedule(&activity_id, true, start, end)
                .unwrap();
            database
                .set_virtual_time(
                    true,
                    if delay > 0 {
                        end + delay / 2
                    } else {
                        start + 1_000
                    },
                    1.0,
                )
                .unwrap();
            let windows = active_windows(&database).unwrap();
            let window = windows
                .get(&activity_id)
                .expect("purchase or exchange period remains visible");
            let mut projected = original.clone();
            assert!(project_master_value(&mut projected, master, id, window));
            let OrderedValue::Map(projected_entries) = &projected else {
                panic!("projected master contains rows")
            };
            let OrderedValue::Row(row) = &projected_entries
                .iter()
                .find(|(key, _)| key == id)
                .unwrap()
                .1
            else {
                panic!("projected campaign contains fields")
            };
            for (index, original_field) in source_row.iter().enumerate() {
                let expected = if index == start_index || (start_index == 4 && index == 6) {
                    projected_time(TimeKind::Start, window)
                } else if index == end_index {
                    projected_time(TimeKind::End, window)
                } else if start_index == 4 && index == 7 {
                    projected_time(TimeKind::Reward, window)
                } else {
                    original_field.clone()
                };
                assert_eq!(row[index], expected, "{activity_id} field {index}");
            }
            for (other_id, other_row) in projected_entries {
                if other_id != id {
                    assert_eq!(
                        Some(other_row),
                        entries
                            .iter()
                            .find(|(key, _)| key == other_id)
                            .map(|(_, row)| row)
                    );
                }
            }
            let encoded = encode_ordered_map(&projected).unwrap();
            assert_eq!(decode_ordered_map(&encoded).unwrap(), projected);
            database
                .set_virtual_time(true, end + delay + 1_000, 1.0)
                .unwrap();
            let expired = active_windows(&database).unwrap();
            assert_eq!(expired[&activity_id].end_at_ms, end);
            assert!(
                expired[&activity_id].end_at_ms + expired[&activity_id].reward_delay_ms
                    < database.current_server_time_millis().unwrap()
            );
        }
    }
}
// //// /核对全部自选主表的购买期与兑换期并保留其他字段 ////
