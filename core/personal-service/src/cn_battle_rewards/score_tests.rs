// audience: internal
// # personal-service-cn-score-reward-tests
// 此文件验证普通掉落的权重边界, 稀有奖励的概率边界和各类奖励的实际发放数量.

use super::*;
use crate::cn_battle_assets::{load_battle_fixture, ScoreReward};

fn rare_reward(probability: f64, position: i64) -> RareScoreReward {
    RareScoreReward {
        reward: Reward {
            kind: 0,
            id: Some(position),
            count: Some(1),
            rarity: Some(probability),
        },
        position,
        element_rarity: None,
    }
}

// //// 验证零权重和普通奖励累计权重的选择边界 [@x380kkm 2026-09-22] ////
#[test]
fn draws_common_rewards_by_weight_and_allows_repeated_entries() {
    let fixture = load_battle_fixture().unwrap();
    let mut quest = fixture.quests["1:1001002"].clone();
    quest.score_rewards = [(0, 0), (2, 1), (3, 3)]
        .into_iter()
        .map(|(position, weight)| ScoreReward {
            kind: 0,
            reward_type: Some(0),
            id: Some(13),
            element_rarity: None,
            count: Some(1),
            rarity: None,
            position: Some(position),
            field5: Some(weight),
        })
        .collect();
    let mut next = 0;
    let draws = draw_common_rewards(&quest, 4, |upper| {
        assert_eq!(upper, 4);
        let draw = next;
        next += 1;
        Ok(draw)
    })
    .unwrap();
    assert_eq!(draws, vec![1, 2, 2, 2]);
    quest.score_rewards.clear();
    assert!(
        draw_common_rewards(&quest, 4, |_| panic!("empty pools consume no draws"))
            .unwrap()
            .is_empty()
    );
}

// //// 验证稀有概率的零值, 单位值和主表位置 [@x380kkm 2026-09-22] ////
#[test]
fn uses_exact_probability_boundaries_and_sparse_rare_positions() {
    assert!(!wins_probability(0.0, 0.0).unwrap());
    assert!(wins_probability(1.0, 1.0 - f64::EPSILON).unwrap());
    assert!(wins_probability(0.1, 0.099999).unwrap());
    assert!(!wins_probability(0.1, 0.1).unwrap());
    let pool = [
        rare_reward(0.0, 1),
        rare_reward(0.25, 4),
        rare_reward(0.75, 9),
    ];
    assert_eq!(choose_rare_reward(&pool, 0.0).unwrap().position, 4);
    assert_eq!(choose_rare_reward(&pool, 0.25).unwrap().position, 9);
    assert_eq!(
        choose_rare_reward(&pool, 1.0 - f64::EPSILON)
            .unwrap()
            .position,
        9
    );
}

// //// 多人普通掉落按抽取次数向上取整并保留每项数量和稀有抽取次数 [@x380kkm 2026-09-28] ////
#[test]
fn rounds_multiplayer_common_draws_without_scaling_each_reward() {
    let fixture = load_battle_fixture().unwrap();
    let mut quest = fixture.quests["1:1001002"].clone();
    quest
        .score_rewards
        .iter_mut()
        .filter(|reward| reward.kind == 1)
        .for_each(|reward| reward.rarity = Some(1.0));
    let rare_groups = BTreeMap::from([("101000".to_owned(), vec![rare_reward(1.0, 19)])]);
    for (clear_rank, single_count, multi_count) in [(1, 1, 2), (4, 3, 5), (5, 4, 6)] {
        for (multiplier, expected) in [
            (1.0, single_count),
            (
                fixture
                    .reward_rules
                    .common_reward_multiplier_by_multi_play_mode,
                multi_count,
            ),
        ] {
            let mut player: Value =
                serde_json::from_str(include_str!("../../assets/cn-default-player.json")).unwrap();
            let initial_items = player["item_list"]["13"].as_i64().unwrap_or_default();
            let context = ScoreRewardContext {
                clear_rank,
                common_reward_count_multiplier: multiplier,
                boost_point_used: false,
                drop_multiplier: 3,
                server_time: 100,
                reward_elements: &fixture.reward_rules.reward_element_map,
            };
            let result = apply_score_rewards(
                player.as_object_mut().unwrap(),
                &rare_groups,
                &quest,
                &context,
            )
            .unwrap();
            assert_eq!(result.drop_score_reward_ids.len(), expected);
            assert!(result
                .drop_score_reward_ids
                .iter()
                .all(|reward| reward["number"] == 3));
            assert_eq!(
                player["item_list"]["13"],
                initial_items + expected as i64 * 3
            );
            assert_eq!(result.drop_rare_reward_ids.len(), 1);
        }
    }
}

// //// 验证角色和星导石保持数量且加成覆盖普通和稀有资源 [@x380kkm 2026-09-22] ////
#[test]
fn scales_resource_rewards_by_boost_and_item_settings() {
    let elements = BTreeMap::new();
    let context = ScoreRewardContext {
        clear_rank: 5,
        common_reward_count_multiplier: 1.0,
        boost_point_used: true,
        drop_multiplier: 3,
        server_time: 100,
        reward_elements: &elements,
    };
    for (kind, expected) in [
        (0, 60),
        (1, 20),
        (2, 1),
        (3, 10),
        (4, 20),
        (5, 20),
        (6, 60),
        (7, 60),
    ] {
        let reward = Reward {
            kind,
            id: Some(1),
            count: Some(10),
            rarity: None,
        };
        assert_eq!(
            scaled_reward(&reward, &context).unwrap().count,
            Some(expected),
            "{kind}"
        );
    }
    let fixture = load_battle_fixture().unwrap();
    for (kind, rarity, expected) in [(6, 4, 8), (7, 3, 53)] {
        let selected = RareScoreReward {
            reward: Reward {
                kind,
                id: Some(rarity),
                count: Some(1),
                rarity: Some(1.0),
            },
            position: 3,
            element_rarity: Some(rarity),
        };
        assert_eq!(
            resolve_element_reward(
                &selected.reward,
                selected.element_rarity,
                2,
                &fixture.reward_rules.reward_element_map
            )
            .unwrap()
            .id,
            Some(expected)
        );
    }
}

// //// 按各关卡类别的五个档位核对掉落数量和库存变更 [@x380kkm 2026-09-22] ////
#[test]
fn grants_ranked_drops_for_every_reward_bearing_category() {
    let fixture = load_battle_fixture().unwrap();
    let mut quests = BTreeMap::new();
    for quest in fixture
        .quests
        .values()
        .filter(|quest| !quest.score_rewards.is_empty())
    {
        quests.entry(quest.category).or_insert(quest);
    }
    assert_eq!(
        quests.keys().copied().collect::<Vec<_>>(),
        vec![1, 2, 4, 6, 7, 10, 13, 14, 18, 19, 20, 21, 23, 24, 25, 26]
    );
    for quest in quests.values() {
        for clear_rank in 1..=5 {
            for boost_point_used in [false, true] {
                let mut player: Value =
                    serde_json::from_str(include_str!("../../assets/cn-default-player.json"))
                        .unwrap();
                let root = player.as_object_mut().unwrap();
                let context = ScoreRewardContext {
                    clear_rank,
                    common_reward_count_multiplier: 1.0,
                    boost_point_used,
                    drop_multiplier: 3,
                    server_time: 100,
                    reward_elements: &fixture.reward_rules.reward_element_map,
                };
                let result =
                    apply_score_rewards(root, &fixture.rare_reward_groups, quest, &context)
                        .unwrap();
                assert_eq!(
                    result.drop_score_reward_ids.len(),
                    if quest.score_rewards.iter().any(|reward| reward.kind == 0) {
                        quest.score_reward_counts[clear_rank as usize - 1]
                    } else {
                        0
                    },
                    "category {}",
                    quest.category
                );
                for drop in result.drop_score_reward_ids {
                    let selected = quest
                        .score_rewards
                        .iter()
                        .find(|reward| reward.position == drop["index"].as_i64())
                        .unwrap();
                    let expected = scaled_reward(
                        &Reward {
                            kind: selected.reward_type.unwrap(),
                            id: selected.id,
                            count: selected.count,
                            rarity: None,
                        },
                        &context,
                    )
                    .unwrap();
                    assert_eq!(drop["number"], expected.count.unwrap());
                }
                for (id, count) in result.rewards.items {
                    assert_eq!(
                        root["item_list"][&id], count,
                        "category {} item {id}",
                        quest.category
                    );
                }
            }
        }
    }
}
