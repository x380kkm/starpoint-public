// audience: internal
// # personal-service-cn-equipment-dissolve-settlement
// 分解产出按物品上限入库, 锻造玉溢出转为玛纳. 装备, 任务, 领取历史和请求凭据共同提交.

use super::catalog::{self, EquipmentDefinition};
use super::Sale;
use crate::cn::{msgpack_response_at, server_time};
use crate::cn_equipment::{add_item, serialize_equipment, WRIGHTPIECE_ITEM_ID};
use crate::cn_request_receipt::RequestReceipt;
use crate::cn_tutorial::{encode_player_data, require_object, require_root};
use crate::database::{ReceiveHistoryEntry, ServiceDatabase};
use crate::http::HttpResponse;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

const STAR_GRAIN_ITEM_ID: i64 = 990_008;

#[derive(Default)]
struct ItemReward {
    amount: i64,
    max_count: i64,
}

struct Rewards {
    items: Map<String, Value>,
    acquired: BTreeMap<i64, i64>,
    history: Vec<ReceiveHistoryEntry>,
}

// //// 汇总装备产出并按真实持有上限入库 [@x380kkm 2026-09-23] ////
fn grant(
    root: &mut Map<String, Value>,
    sales: &[Sale],
    definitions: &BTreeMap<i64, EquipmentDefinition>,
) -> Result<Rewards, PersonalServiceError> {
    let rules = catalog::item_rules()?;
    let mut rewards = BTreeMap::<i64, ItemReward>::new();
    for sale in sales {
        let definition = definitions.get(&sale.equipment_id).ok_or_else(|| {
            PersonalServiceError::new("CN equipment dissolve definition is missing")
        })?;
        let mut items = vec![
            (
                WRIGHTPIECE_ITEM_ID,
                definition.craft_points,
                rules[&WRIGHTPIECE_ITEM_ID].max_count,
            ),
            (
                STAR_GRAIN_ITEM_ID,
                definition.star_grains,
                rules[&STAR_GRAIN_ITEM_ID].max_count,
            ),
        ];
        if definition.generate_ability_soul {
            items.push((
                definition.ability_soul_id,
                1,
                definition.ability_soul_max_count,
            ));
        }
        for (id, amount, max_count) in items {
            let reward = rewards.entry(id).or_default();
            reward.amount = amount
                .checked_mul(sale.count)
                .and_then(|amount| reward.amount.checked_add(amount))
                .ok_or_else(|| {
                    PersonalServiceError::new("CN equipment dissolve reward exceeds range")
                })?;
            reward.max_count = max_count;
        }
    }
    let mut result = Rewards {
        items: Map::new(),
        acquired: BTreeMap::new(),
        history: Vec::new(),
    };
    let mut mana = 0_i64;
    let item_list = require_object(root, "item_list")?;
    for (id, reward) in rewards {
        if reward.amount == 0 {
            continue;
        }
        let current = item_list
            .get(&id.to_string())
            .and_then(Value::as_i64)
            .unwrap_or_default();
        if current < 0 {
            return Err(PersonalServiceError::new(
                "stored CN item count is negative",
            ));
        }
        let acquired = reward
            .amount
            .min(reward.max_count.saturating_sub(current).max(0));
        add_item(item_list, &mut result.items, id, acquired)?;
        result
            .items
            .insert(id.to_string(), json!(current + acquired));
        if acquired > 0 {
            result.acquired.insert(id, acquired);
            result
                .history
                .push(ReceiveHistoryEntry::reward(1, Some(id), acquired));
        }
        if id == WRIGHTPIECE_ITEM_ID {
            mana = (reward.amount - acquired)
                .checked_mul(rules[&id].sale_price)
                .ok_or_else(|| PersonalServiceError::new("CN dissolve mana exceeds range"))?;
        }
    }
    if mana > 0 {
        let user = require_object(root, "user_info")?;
        let current = user
            .get("free_mana")
            .and_then(Value::as_i64)
            .ok_or_else(|| PersonalServiceError::new("stored CN free mana is invalid"))?;
        let total = current
            .checked_add(mana)
            .ok_or_else(|| PersonalServiceError::new("CN dissolve mana exceeds range"))?;
        user.insert("free_mana".to_owned(), json!(total));
        result
            .history
            .push(ReceiveHistoryEntry::reward(8, None, mana));
    }
    Ok(result)
}
// //// /汇总装备产出并按真实持有上限入库 ////

// //// 保存本体或副本的消耗并返回装备列表 [@x380kkm 2026-09-23] ////
fn consume(
    root: &mut Map<String, Value>,
    sales: &[Sale],
) -> Result<Vec<Value>, PersonalServiceError> {
    let equipment_list = require_object(root, "user_equipment_list")?;
    for sale in sales {
        match sale.remaining_stack {
            Some(stack) => {
                let equipment = equipment_list
                    .get_mut(&sale.equipment_id.to_string())
                    .and_then(Value::as_object_mut)
                    .ok_or_else(|| {
                        PersonalServiceError::new("stored CN equipment data is invalid")
                    })?;
                equipment.insert("stack".to_owned(), json!(stack));
            }
            None => {
                equipment_list.remove(&sale.equipment_id.to_string());
            }
        }
    }
    equipment_list
        .iter()
        .map(|(id, equipment)| {
            let id = id
                .parse::<i64>()
                .map_err(|_| PersonalServiceError::new("stored CN equipment id is invalid"))?;
            let equipment = equipment
                .as_object()
                .ok_or_else(|| PersonalServiceError::new("stored CN equipment data is invalid"))?;
            Ok(serialize_equipment(id, equipment))
        })
        .collect()
}
// //// /保存本体或副本的消耗并返回装备列表 ////

// //// 将装备消耗, 奖励, 任务与领取凭据整体保存 [@x380kkm 2026-09-23] ////
pub(super) fn settle(
    database: &mut ServiceDatabase,
    account_id: i64,
    viewer_id: i64,
    player_data: &mut Value,
    sales: &[Sale],
    definitions: &BTreeMap<i64, EquipmentDefinition>,
    receipt: Option<RequestReceipt>,
) -> Result<HttpResponse, PersonalServiceError> {
    let response_time = server_time(database)?;
    let collect_plan = crate::cn_mission::plan_collect_item_missions(database)?;
    database.with_player_settlement(|database| {
        let root = require_root(player_data)?;
        let before_mana = crate::cn_mission::total_mana(root)?;
        let rewards = grant(root, sales, definitions)?;
        let response_equipment = consume(root, sales)?;
        let mut missions = crate::cn_mission::record_collected_item_rewards(root, &collect_plan, &rewards.acquired, response_time)?;
        let acquired_mana = crate::cn_mission::total_mana(root)?.saturating_sub(before_mana);
        missions.extend(crate::cn_mission::record_acquired_mana(root, database, account_id, acquired_mana, response_time)?);
        let mut response = json!({
            "equipment_list":response_equipment, "item_list":rewards.items, "mail_arrived":false,
        });
        if acquired_mana > 0 {
            response["user_info"] = json!({"free_mana":root["user_info"]["free_mana"]});
        }
        crate::cn_mission::apply_mission_reward_response(&mut response, root, &missions, viewer_id, response_time)?;
        if !missions.is_empty() { response["mission_info"] = json!(missions); }
        let event_key = match receipt {
            Some(receipt) => receipt.record(root, &response, response_time)?,
            None => {
                let mut bytes = [0_u8; 16];
                getrandom::getrandom(&mut bytes).map_err(|error| PersonalServiceError::new(format!("CN dissolve history id generation failed: {error}")))?;
                let id = bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
                format!("equipment:dissolve:{viewer_id}:{id}")
            }
        };
        database.save_player_snapshot_with_receive_history(account_id, &encode_player_data(player_data)?, &event_key, response_time, &rewards.history)?;
        Ok(Some(msgpack_response_at(viewer_id, false, response_time, response)?))
    })?.ok_or_else(|| PersonalServiceError::new("CN dissolve settlement did not commit"))
}
// //// /将装备消耗, 奖励, 任务与领取凭据整体保存 ////
