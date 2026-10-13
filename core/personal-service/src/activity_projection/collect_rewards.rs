// audience: internal
// # personal-service-collect-reward-master
// 客户端追加掉落范围和服务奖励资格共用收集活动目录.

use super::{encode_ordered_map, OrderedValue};
use crate::PersonalServiceError;
use serde::Deserialize;

pub(super) const CATALOG: &str = include_str!("../../assets/cn-additional-battle-rewards.json");

#[derive(Deserialize)]
struct Catalog {
    client_quest_relation_master: QuestRelationMaster,
}

#[derive(Deserialize)]
struct QuestRelationMaster {
    entry_path: String,
    master: Vec<(String, Vec<(String, Vec<String>)>)>,
}

// //// 将收集活动关卡范围加入客户端资源投影 [@x380kkm 2026-09-29] ////
pub(super) fn projected_entry() -> Result<(String, Vec<u8>), PersonalServiceError> {
    let catalog: Catalog = serde_json::from_str(CATALOG).map_err(|error| {
        PersonalServiceError::new(format!("invalid CN collect reward master: {error}"))
    })?;
    let master = catalog.client_quest_relation_master;
    let rows = OrderedValue::Map(
        master
            .master
            .into_iter()
            .map(|(event, relations)| {
                (
                    event,
                    OrderedValue::Map(
                        relations
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

    // //// 客户端范围顺序与服务的奖励关系保持一致 [@x380kkm 2026-09-29] ////
    #[test]
    fn client_scope_order_matches_service_reward_relations() {
        let service: Value = serde_json::from_str(CATALOG).unwrap();
        let (_, bytes) = projected_entry().unwrap();
        let OrderedValue::Map(events) = super::super::decode_ordered_map(&bytes).unwrap() else {
            panic!("collect event map");
        };
        for (event_id, entry) in events {
            let event = service["events"]
                .as_array()
                .unwrap()
                .iter()
                .find(|event| event["id"].as_i64().unwrap().to_string() == event_id)
                .unwrap();
            let OrderedValue::Map(rows) = entry else {
                panic!("collect relation map");
            };
            let expected: Vec<i64> = event["relations"]
                .as_array()
                .unwrap()
                .iter()
                .map(|relation| relation["reward_relation_id"].as_i64().unwrap())
                .collect();
            let actual: Vec<i64> = rows
                .iter()
                .map(|(_, value)| {
                    let OrderedValue::Row(row) = value else {
                        panic!("collect relation row");
                    };
                    row[5].parse().unwrap()
                })
                .collect();
            assert_eq!(actual, expected, "{event_id}");
        }
    }
}
