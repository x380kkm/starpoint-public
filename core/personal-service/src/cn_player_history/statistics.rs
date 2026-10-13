// audience: internal
// # personal-service-cn-player-history-statistics
// 数值来自玩家持有物, 任务计数和活动成绩. 里程碑日期来自保存的完成凭据.
// 客户端为每个主题读取固定位置的 Option 数组, 缺失的历史事实使用协议的 null 值.

mod events;

use super::catalog;
use crate::cn_mission::conditions::{character_level, fully_awakened_equipment_count};
use crate::cn_tutorial::format_client_time;
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

// 客户端 guild_girl_equipment_enhancement 的六个固定图标按装备编号选择未持有画面.
const GUILD_EQUIPMENT_IDS: [i64; 6] = [5040020, 5100011, 5030028, 5010032, 5010056, 5010045];

#[derive(Default, Serialize)]
struct TopicValues {
    boss_id_values: Option<Vec<Option<i64>>>,
    character_id_values: Option<Vec<Option<i64>>>,
    date_values: Option<Vec<Option<String>>>,
    equipment_id_values: Option<Vec<Option<i64>>>,
    int_values: Option<Vec<Option<i64>>>,
    quest_values: Option<Vec<Option<Value>>>,
    string_values: Option<Vec<Option<String>>>,
}

struct Statistics<'a> {
    root: &'a Map<String, Value>,
    counters: BTreeMap<String, i64>,
    definitions: &'static catalog::Definitions,
}

// //// 为主表全部主题构造具有真实编号和完整位置的字段 [@x380kkm 2026-09-26] ////
pub(super) fn topic_values(
    root: &Map<String, Value>,
    database: &ServiceDatabase,
    account_id: i64,
    history_id: i64,
) -> Result<Map<String, Value>, PersonalServiceError> {
    let data = catalog::definitions()?;
    let topics = data
        .topics
        .get(&history_id)
        .ok_or_else(|| PersonalServiceError::new("CN player history topics are missing"))?;
    let stats = Statistics {
        root,
        counters: database.mission_counters(account_id)?,
        definitions: data,
    };
    topics
        .iter()
        .map(|topic| {
            Ok((
                topic.index.to_string(),
                json!(stats.values(topic.aggregation_target)?),
            ))
        })
        .collect()
}

impl Statistics<'_> {
    // //// 按客户端聚合种类读取相应存档事实 [@x380kkm 2026-09-26] ////
    fn values(&self, kind: i64) -> Result<TopicValues, PersonalServiceError> {
        let root = self.root;
        let mut values = TopicValues::default();
        match kind {
            0 => {
                values.date_values = Some(vec![root
                    .get("user_info")
                    .and_then(|user| user.get("create_time"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)])
            }
            1 => {
                values.int_values = Some(vec![root
                    .get("mission_login_days")
                    .and_then(Value::as_array)
                    .map(|days| {
                        days.iter()
                            .filter_map(Value::as_i64)
                            .collect::<BTreeSet<_>>()
                            .len() as i64
                    })])
            }
            2 | 3 => {
                let first = if kind == 2 { 1 } else { 7 };
                values.date_values = Some(
                    (first..first + 6)
                        .map(|chapter| {
                            self.milestone_time("chapter_clear", Some(chapter))
                                .map(date)
                        })
                        .collect(),
                );
            }
            4 => {
                let milestone = self
                    .definitions
                    .milestones
                    .iter()
                    .filter(|milestone| milestone.pattern == "mana_board_2nd_complete_count")
                    .filter_map(|milestone| {
                        receipt_time(
                            root,
                            milestone.category,
                            milestone.mission_id,
                            milestone.stage,
                        )
                        .map(|time| (time, milestone.character_id))
                    })
                    .min_by_key(|entry| entry.0);
                values.date_values = Some(vec![milestone.map(|entry| date(entry.0))]);
                values.character_id_values = Some(vec![milestone.and_then(|entry| entry.1)]);
            }
            5 => {
                values.int_values = Some(vec![Some(
                    object(root, "user_character_list")
                        .into_iter()
                        .flatten()
                        .filter_map(|(id, _)| id.parse::<i64>().ok())
                        .filter(|id| character_level(root, *id) >= 100)
                        .count() as i64,
                )])
            }
            6 => values.int_values = Some(vec![self.counter("total_obtained_bond_token_count")]),
            7 => {
                values.date_values = Some(vec![self
                    .milestone_time("characters_count", None)
                    .map(date)])
            }
            8 => {
                values.date_values = Some(vec![self
                    .milestone_time("player_rank_achievement", None)
                    .map(date)])
            }
            9 => values.int_values = Some(vec![receipt_count(root, &[1])]),
            10 => values.int_values = Some(vec![receipt_count(root, &[3, 4])]),
            11 => values.int_values = Some(vec![self.counter("multi_battle_mvp_count")]),
            12 => {
                values.int_values = Some(vec![
                    self.counter("multi_battle_host_count"),
                    self.counter("multi_battle_guest_count"),
                ])
            }
            13 => {
                values.int_values = Some(vec![
                    object(root, "user_equipment_list").map(|items| items.len() as i64)
                ])
            }
            14 => values.int_values = Some(vec![Some(fully_awakened_equipment_count(root)?)]),
            15 => {
                values.int_values = Some(vec![object(root, "user_equipment_list").map(|items| {
                    items
                        .iter()
                        .filter(|(id, item)| {
                            id.parse::<i64>()
                                .ok()
                                .and_then(|id| self.definitions.equipment_max_enhancement.get(&id))
                                .is_some_and(|maximum| {
                                    item.get("enhancement_level")
                                        .and_then(Value::as_i64)
                                        .is_some_and(|level| level >= *maximum)
                                })
                        })
                        .count() as i64
                })])
            }
            16 => {
                values.equipment_id_values =
                    Some(GUILD_EQUIPMENT_IDS.iter().copied().map(Some).collect());
                values.int_values = Some(
                    GUILD_EQUIPMENT_IDS
                        .iter()
                        .flat_map(|id| {
                            let item = object(root, "user_equipment_list")
                                .and_then(|items| items.get(&id.to_string()));
                            [
                                item.map(|item| {
                                    item.get("level").and_then(Value::as_i64).unwrap_or(1)
                                }),
                                item.map(|item| {
                                    item.get("enhancement_level")
                                        .and_then(Value::as_i64)
                                        .unwrap_or_default()
                                }),
                            ]
                        })
                        .collect(),
                );
            }
            17 => {
                let record = events::best_rush(root);
                values.int_values = Some(vec![
                    record.map(|entry| entry.0),
                    record.map(|entry| entry.1),
                ]);
                let characters = record.and_then(|entry| {
                    entry
                        .2
                        .get("endless_battle_max_round_character_ids")
                        .and_then(Value::as_array)
                });
                values.character_id_values = Some(
                    (0..3)
                        .map(|index| {
                            characters
                                .and_then(|ids| ids.get(index))
                                .and_then(Value::as_i64)
                        })
                        .collect(),
                );
            }
            18 => {
                let record = events::best_carnival(root);
                values.int_values = Some(vec![
                    record.map(|entry| entry.0),
                    record.map(|entry| entry.1),
                ]);
            }
            19 => values.int_values = Some(events::solo_degree_counts(root)?),
            20 => values.int_values = Some(events::score_attack_borders(root)?),
            21 => values.int_values = Some(vec![events::expert_quest_count(root)?]),
            22 => values.int_values = Some(vec![events::side_story_count(root)?]),
            23 => values.int_values = Some(vec![events::tower_floor_count(root)?]),
            24 => {
                values.int_values = Some(vec![root
                    .get("mission_economic_counters")
                    .and_then(|counters| counters.get("total_used_mana_count"))
                    .and_then(Value::as_i64)
                    .or_else(|| self.counter("total_used_mana_count"))])
            }
            25 => {
                values.int_values = Some(vec![object(root, "encyclopedia_list").map(|items| {
                    items
                        .values()
                        .filter(|item| item.get("read").and_then(Value::as_bool) == Some(true))
                        .count() as i64
                })])
            }
            26 => {
                let record = self
                    .definitions
                    .challenge_single_bosses
                    .iter()
                    .filter_map(|boss| {
                        receipt_time(root, 5, boss.degree_mission_id, 1).map(|time| (time, boss.id))
                    })
                    .min();
                values.date_values = Some(vec![record.map(|record| date(record.0))]);
                values.boss_id_values = Some(vec![record.map(|record| record.1)]);
            }
            _ => {
                return Err(PersonalServiceError::new(
                    "CN player history aggregation kind is invalid",
                ))
            }
        }
        if let Some(numbers) = &mut values.int_values {
            for value in numbers {
                *value = value.filter(|number| i32::try_from(*number).is_ok());
            }
        }
        Ok(values)
    }

    fn counter(&self, pattern: &str) -> Option<i64> {
        self.counters.get(pattern).copied()
    }

    fn milestone_time(&self, pattern: &str, chapter: Option<i64>) -> Option<i64> {
        self.definitions
            .milestones
            .iter()
            .filter(|milestone| milestone.pattern == pattern && milestone.chapter_id == chapter)
            .filter_map(|milestone| {
                receipt_time(
                    self.root,
                    milestone.category,
                    milestone.mission_id,
                    milestone.stage,
                )
            })
            .min()
    }
}

fn object<'a>(root: &'a Map<String, Value>, field: &str) -> Option<&'a Map<String, Value>> {
    root.get(field).and_then(Value::as_object)
}

fn date(time: i64) -> String {
    format_client_time(time.saturating_add(8 * 3600))
}

fn receipt_time(root: &Map<String, Value>, category: i64, mission: i64, stage: i64) -> Option<i64> {
    let prefix = format!("{category}:{mission}:");
    let suffix = format!(":{stage}");
    object(root, "mission_stage_receipts")
        .into_iter()
        .flatten()
        .filter(|(key, _)| key.starts_with(&prefix) && key.ends_with(&suffix))
        .filter_map(|(_, receipt)| receipt.get("received_at").and_then(Value::as_i64))
        .min()
}

fn receipt_count(root: &Map<String, Value>, categories: &[i64]) -> Option<i64> {
    object(root, "mission_stage_receipts").map(|receipts| {
        receipts
            .iter()
            .filter(|(key, value)| {
                value.is_object()
                    && key
                        .split(':')
                        .next()
                        .and_then(|category| category.parse::<i64>().ok())
                        .is_some_and(|category| categories.contains(&category))
            })
            .count() as i64
    })
}
