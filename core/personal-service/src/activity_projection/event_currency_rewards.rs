// audience: internal
// # personal-service-event-currency-rewards
// 客户端奖励主表与服务奖励使用活动专用组, 关卡投影保留活动开放日程和原始掉落位置.

use super::{encode_ordered_map, OrderedValue};
use crate::PersonalServiceError;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

pub(super) const CATALOG: &str =
    include_str!("../../assets/cn-event-currency-reward-projection.json");
pub(super) const QUEST_MASTER_NAME: &str = "advent_event_quest";

#[derive(Deserialize)]
struct RewardMaster {
    entry_path: String,
    groups: Vec<(String, Vec<(String, Vec<String>)>)>,
}

#[derive(Deserialize)]
struct Catalog {
    masters: Vec<RewardMaster>,
    quest_score_groups: BTreeMap<i64, i64>,
}

// //// 读取活动货币对应的完整客户端奖励主表 [@x380kkm 2026-09-29] ////
fn catalog() -> Result<&'static Catalog, PersonalServiceError> {
    static VALUE: OnceLock<Result<Catalog, String>> = OnceLock::new();
    VALUE
        .get_or_init(|| {
            serde_json::from_str(CATALOG)
                .map_err(|error| format!("invalid CN event currency rewards: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

pub(super) fn projected_entries() -> Result<Vec<(String, Vec<u8>)>, PersonalServiceError> {
    catalog()?
        .masters
        .iter()
        .map(|master| {
            let groups = OrderedValue::Map(
                master
                    .groups
                    .iter()
                    .map(|(id, rewards)| {
                        (
                            id.clone(),
                            OrderedValue::Map(
                                rewards
                                    .iter()
                                    .map(|(position, row)| {
                                        (position.clone(), OrderedValue::Row(row.clone()))
                                    })
                                    .collect(),
                            ),
                        )
                    })
                    .collect(),
            );
            Ok((
                master.entry_path.clone(),
                encode_ordered_map(&groups).map_err(PersonalServiceError::new)?,
            ))
        })
        .collect()
}
// //// /读取活动货币对应的完整客户端奖励主表 ////

// //// 将活动关卡关联到本期货币奖励组 [@x380kkm 2026-09-29] ////
pub(super) fn project_quests(value: &mut OrderedValue) -> Result<(), PersonalServiceError> {
    let OrderedValue::Map(events) = value else {
        return Ok(());
    };
    let catalog = catalog()?;
    for (_, event) in events {
        let OrderedValue::Map(quests) = event else {
            continue;
        };
        for (_, quest) in quests {
            let OrderedValue::Row(row) = quest else {
                continue;
            };
            let group = row
                .first()
                .and_then(|id| id.parse().ok())
                .and_then(|id| catalog.quest_score_groups.get(&id));
            if let Some(group) = group {
                let field = row.get_mut(76).ok_or_else(|| {
                    PersonalServiceError::new("CN advent quest reward group field is missing")
                })?;
                *field = group.to_string();
            }
        }
    }
    Ok(())
}
// //// /将活动关卡关联到本期货币奖励组 ////

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity_projection::{decode_ordered_map, master_seed};

    // //// 核对客户端结算位置与服务端发奖引用保持一致 [@x380kkm 2026-09-29] ////
    #[test]
    fn client_reward_groups_and_server_quests_share_currency_projection() {
        let fixture = crate::cn_battle_assets::load_battle_fixture().unwrap();
        let mut quests = decode_ordered_map(master_seed(QUEST_MASTER_NAME).unwrap()).unwrap();
        project_quests(&mut quests).unwrap();
        let OrderedValue::Map(events) = quests else {
            panic!("advent event map");
        };
        for (_, event) in events {
            let OrderedValue::Map(quests) = event else {
                panic!("advent quest map");
            };
            for (_, quest) in quests {
                let OrderedValue::Row(row) = quest else {
                    panic!("advent quest row");
                };
                let quest_id = row[0].parse::<i64>().unwrap();
                if let Some(group) = catalog().unwrap().quest_score_groups.get(&quest_id) {
                    let server = &fixture.quests[&format!("7:{quest_id}")];
                    assert_eq!(server.score_reward_group_id, Some(*group));
                    assert_eq!(row[76], group.to_string());
                    let client_groups = &catalog().unwrap().masters[0].groups;
                    let rows = &client_groups
                        .iter()
                        .find(|(id, _)| id == &group.to_string())
                        .unwrap()
                        .1;
                    for reward in &server.score_rewards {
                        let position = reward.position.unwrap().to_string();
                        let client = &rows.iter().find(|(id, _)| id == &position).unwrap().1;
                        if reward.kind == 0 {
                            assert_eq!(reward.id, client[3].parse().ok());
                            assert_eq!(reward.count, client[4].parse().ok());
                            assert_eq!(reward.field5, client[5].parse().ok());
                        } else {
                            assert_eq!(reward.id, client[6].parse().ok());
                            assert_eq!(reward.rarity, client[7].parse().ok());
                            let rare_groups = &catalog().unwrap().masters[1].groups;
                            let client_rare = &rare_groups
                                .iter()
                                .find(|(id, _)| id == &client[6])
                                .unwrap()
                                .1;
                            for entry in &fixture.rare_reward_groups[&client[6]] {
                                let position = entry.position.to_string();
                                let row = &client_rare
                                    .iter()
                                    .find(|(id, _)| id == &position)
                                    .unwrap()
                                    .1;
                                assert_eq!(entry.reward.kind, row[1].parse::<i64>().unwrap());
                                assert_eq!(entry.reward.id, row[2].parse().ok());
                                assert_eq!(entry.reward.count, row[3].parse().ok());
                                assert_eq!(entry.reward.rarity, row[4].parse().ok());
                            }
                        }
                    }
                }
            }
        }
    }
}
