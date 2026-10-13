// audience: internal
// # personal-service-cn-client-result-contract
// BattleQuestFinishRealRemote 和 QuestClearResult 按字段类型及奖励主表键读取结算结果.

use serde_json::Value;

// //// 验证客户端整数与战斗结果必需字段 [@x380kkm 2026-09-27] ////
fn integer(value: &Value, path: &str, key: &str) {
    assert!(value.as_i64().is_some_and(|number| i32::try_from(number).is_ok()), "{key}: client Int {path} = {value}");
}

fn integers(value: &Value, fields: &[&str], key: &str) {
    for field in fields { integer(&value[*field], field, key); }
}

pub(super) fn assert_result(data: &Value, category: i64, quest: &Value, fixture: &Value, rules: &Value, key: &str) {
    integers(data, &["before_rank_point", "clear_rank"], key);
    assert!(data["old_high_score"].is_number(), "{key}: old_high_score");
    integers(&data["rewards"], &["field_mana", "overflow_pool_exp", "reward_mana", "reward_pool_exp"], key);
    for field in ["joined_character_id_list", "presigned_quest_category"] {
        for value in data[field].as_array().unwrap_or_else(|| panic!("{key}: {field}")) { integer(value, field, key); }
    }
    for value in data["add_exp_list"].as_array().unwrap() { integers(value, &["add_exp", "after_exp", "character_id"], key); }
    for (id, value) in data["bond_token_status_list"].as_object().unwrap() {
        assert!(id.parse::<i32>().is_ok());
        for field in ["before", "after"] {
            for entry in value[field].as_array().unwrap() { integers(entry, &["mana_board_index", "status"], key); }
        }
    }
    for field in ["drop_score_reward_ids", "drop_rare_reward_ids", "drop_additional_reward_ids", "drop_periodic_reward_ids"] {
        for drop in data[field].as_array().unwrap_or_else(|| panic!("{key}: {field}")) {
            integers(drop, &["group_id", "index", "number"], key);
            assert!(drop["number"].as_i64().unwrap() > 0, "{key}: {drop}");
            let group = drop["group_id"].as_i64().unwrap().to_string();
            let candidates = if field == "drop_score_reward_ids" {
                if let Some(phases) = rules["phase_quests"][key]["phases"].as_array() {
                    &phases.iter().find(|phase| phase["score_reward_group_id"] == drop["group_id"]).unwrap()["score_rewards"]
                } else {
                    assert_eq!(quest["score_reward_group_id"], drop["group_id"], "{key}: reward group");
                    &quest["score_rewards"]
                }
            } else if field == "drop_rare_reward_ids" {
                rules["phase_quests"][key]["rare_reward_groups"].get(&group).unwrap_or(&fixture["rare_reward_groups"][&group])
            } else { continue };
            let row = candidates.as_array().and_then(|rows| rows.iter().find(|row| row["position"] == drop["index"])).unwrap_or_else(|| panic!("{key}: client reward master lookup {field}: {drop}"));
            if field == "drop_score_reward_ids" { assert_eq!(row["type"], 0, "{key}: rare key used as common reward"); }
        }
    }
    assert_event_result(data, category, key);
}
// //// /验证客户端整数与战斗结果必需字段 ////

// //// 验证各活动结果分支的必需结构 [@x380kkm 2026-09-27] ////
fn assert_event_result(data: &Value, category: i64, key: &str) {
    match category {
        11 => {
            let event = &data["ranking_event"];
            assert!(event["is_record_valid"].is_boolean(), "{key}: ranking_event");
            for field in ["best_record", "previous"] {
                if event[field].is_null() { continue; }
                assert!(event[field]["is_accomplished"].is_boolean());
                for value in ["elapsed_time_ms", "score"] { assert!(event[field][value].is_number()); }
            }
        }
        22 => {
            let event = &data["carnival_event"];
            assert!(event["is_record_valid"].is_boolean(), "{key}: carnival_event");
            integer(&event["leader_character_id"], "leader_character_id", key);
            for field in ["new_degree_ids", "reward_ids"] {
                for id in event[field].as_array().unwrap() { integer(id, field, key); }
            }
            assert!(event["previous_total_best_score"].is_number());
            for field in ["difficulty_bonus", "time_bonus"] { assert!(event["score"][field].is_number()); }
        }
        23 => {
            let event = &data["raid_event"];
            integer(&event["auto_start_point"], "auto_start_point", key);
            assert!(event["is_out_of_period"].is_boolean());
            integer(&event["quest_boss"]["kill_count"], "kill_count", key);
            integer(&event["raid_boss"]["total_kill_count"], "total_kill_count", key);
            assert!(event["raid_boss"]["hp_percentage"].is_number());
        }
        24 => {
            let event = &data["rush_event"];
            assert!(event["is_out_of_period"].is_boolean(), "{key}: rush_event");
            for reward in event["rush_battle_reward_list"].as_array().unwrap() {
                integers(reward, &["kind", "number"], key);
                if !reward["kind_id"].is_null() { integer(&reward["kind_id"], "kind_id", key); }
            }
        }
        25 => {
            let event = &data["solo_time_attack_event"];
            assert!(event["elapsed_time_ms"].is_number(), "{key}: solo_time_attack_event");
            for field in ["new_degree_ids", "reward_ids"] {
                for id in event[field].as_array().unwrap() { integer(id, field, key); }
            }
            for id in event["main_character_ids"].as_object().unwrap().values() { integer(id, "main_character_ids", key); }
        }
        27 => {
            let event = &data["score_attack_event"];
            for id in event["reward_ids"].as_array().unwrap_or_else(|| panic!("{key}: score_attack_event")) { integer(id, "reward_ids", key); }
            for id in event["main_character_ids"].as_object().unwrap().values() { integer(id, "main_character_ids", key); }
        }
        _ => {}
    }
}
// //// /验证各活动结果分支的必需结构 ////
