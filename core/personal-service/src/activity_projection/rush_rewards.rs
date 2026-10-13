// audience: internal
// # personal-service-rush-reward-master
// 客户端排名档位和服务奖励由同一份主表生成, 首档包含榜首.

use super::{encode_ordered_map, OrderedValue};
use crate::PersonalServiceError;
use serde::Deserialize;
use std::collections::BTreeMap;

pub(super) const CATALOG: &str = include_str!("../../assets/cn-rush-ranking-master.json");

#[derive(Deserialize)]
struct Master {
    entry_path: String,
    master: BTreeMap<String, BTreeMap<String, Vec<String>>>,
}

// //// 将排名奖励主表加入客户端资源投影 [@x380kkm 2026-09-28] ////
pub(super) fn projected_entry() -> Result<(String, Vec<u8>), PersonalServiceError> {
    let master: Master = serde_json::from_str(CATALOG).map_err(|error| {
        PersonalServiceError::new(format!("invalid CN rush reward master: {error}"))
    })?;
    let rows = OrderedValue::Map(
        master
            .master
            .into_iter()
            .map(|(event, rewards)| {
                (
                    event,
                    OrderedValue::Map(
                        rewards
                            .into_iter()
                            .map(|(id, row)| (id, OrderedValue::Row(row)))
                            .collect(),
                    ),
                )
            })
            .collect(),
    );
    Ok((
        master.entry_path,
        encode_ordered_map(&rows).map_err(PersonalServiceError::new)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    // //// 客户端和服务使用相同的完整排名奖励区间 [@x380kkm 2026-09-28] ////
    #[test]
    fn client_and_service_cover_first_rank_with_the_same_rewards() {
        let server: Value =
            serde_json::from_str(include_str!("../../assets/cn-raid-rush-rewards.json")).unwrap();
        let (_, bytes) = projected_entry().unwrap();
        let OrderedValue::Map(events) = super::super::decode_ordered_map(&bytes).unwrap() else {
            panic!("event map");
        };
        for (event, rewards) in events {
            let OrderedValue::Map(rows) = rewards else {
                panic!("reward map");
            };
            let server_tiers = server["rush"][event.as_str()]["ranking_rewards"]
                .as_array()
                .unwrap();
            assert_eq!(rows.len(), server_tiers.len());
            let mut next = 1;
            for ((_, row), tier) in rows.iter().zip(server_tiers) {
                let OrderedValue::Row(row) = row else {
                    panic!("reward row");
                };
                assert_eq!(row[0].parse::<i64>().unwrap(), next);
                assert_eq!(tier["rank_start"], next);
                assert_eq!(tier["rank_end"], row[1].parse::<i64>().unwrap());
                assert_eq!(tier["rewards"][0]["id"], row[4].parse::<i64>().unwrap());
                next = tier["rank_end"].as_i64().unwrap() + 1;
            }
        }
    }
}
