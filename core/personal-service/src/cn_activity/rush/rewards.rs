// audience: internal
// # personal-service-cn-rush-rewards
//
// 该模块解析 rush folder 和排名奖励主数据.

use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::Value;
use std::sync::OnceLock;

const FOLDER_ASSET: &str = include_str!("../../../../../assets/rush_event_quest_folder.json");
const RANKING_ASSET: &str = include_str!("../../../../../assets/rush_event_ranking_reward.json");
static FOLDERS: OnceLock<Result<Value, String>> = OnceLock::new();
static RANKING_REWARDS: OnceLock<Result<Value, String>> = OnceLock::new();

#[derive(Deserialize)]
pub(super) struct RankingReward {
    pub(super) kind: i64,
    #[serde(rename = "kindId")]
    pub(super) kind_id: i64,
    pub(super) number: i64,
}

pub(super) fn folder_rewards(
    event_id: i64,
    folder_id: i64,
) -> Result<Option<Vec<(i64, i64)>>, PersonalServiceError> {
    let document = FOLDERS.get_or_init(|| {
        serde_json::from_str::<Value>(FOLDER_ASSET)
            .map_err(|error| format!("failed to decode CN rush folders: {error}"))
    });
    let document = document
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    let Some(rewards) = document
        .get(event_id.to_string())
        .and_then(|event| event.get(folder_id.to_string()))
        .and_then(Value::as_array)
    else {
        return Ok(None);
    };
    rewards
        .iter()
        .map(|reward| {
            if reward.get("type").and_then(Value::as_i64) != Some(0) {
                return Err(PersonalServiceError::new(
                    "CN rush folder reward type is invalid",
                ));
            }
            let item_id = reward
                .get("id")
                .and_then(Value::as_i64)
                .filter(|item_id| *item_id > 0)
                .ok_or_else(|| PersonalServiceError::new("CN rush folder reward id is invalid"))?;
            let count = reward
                .get("count")
                .and_then(Value::as_i64)
                .filter(|count| *count > 0)
                .ok_or_else(|| {
                    PersonalServiceError::new("CN rush folder reward count is invalid")
                })?;
            Ok((item_id, count))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

pub(super) fn ranking_rewards(
    event_id: i64,
    rank_number: i64,
) -> Result<Vec<RankingReward>, PersonalServiceError> {
    let document = RANKING_REWARDS.get_or_init(|| {
        serde_json::from_str::<Value>(RANKING_ASSET)
            .map_err(|error| format!("failed to decode CN rush ranking rewards: {error}"))
    });
    let document = document
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    let Some(groups) = document
        .get(event_id.to_string())
        .and_then(Value::as_object)
    else {
        return Ok(Vec::new());
    };
    let mut rewards = Vec::new();
    for entries in groups.values().filter_map(Value::as_array) {
        let Some(reward) = entries.iter().find(|reward| {
            reward
                .get("fromRank")
                .and_then(Value::as_i64)
                .is_some_and(|from| rank_number >= from)
                && reward
                    .get("toRank")
                    .and_then(Value::as_i64)
                    .is_some_and(|to| rank_number <= to)
        }) else {
            continue;
        };
        rewards.push(
            serde_json::from_value::<RankingReward>(reward.clone()).map_err(|error| {
                PersonalServiceError::new(format!("CN rush ranking reward is invalid: {error}"))
            })?,
        );
    }
    Ok(rewards)
}
