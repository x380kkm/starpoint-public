// audience: internal
// # personal-service-cn-item-use
// 客户端自选序号从 1 开始, 同一批请求的库存, 素材, 邮件余量和重放回执共同提交.

use crate::cn::{
    decode_request, deserialize_i64, deserialize_optional_i64, msgpack_response_at,
    msgpack_result_code_response_at, server_time,
};
use crate::cn_request_receipt::RequestReceipt;
use crate::cn_reward_overflow::{self, Conversion};
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, player_snapshot, require_object, require_root,
};
use crate::database::{MailReward, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

const MAX_STAMINA_OVERFLOW: i64 = 999;

#[derive(Deserialize)]
struct ItemUseRequest {
    #[serde(deserialize_with = "deserialize_i64")]
    viewer_id: i64,
    #[serde(default, deserialize_with = "deserialize_optional_i64")]
    api_count: Option<i64>,
    items: Vec<ItemUseEntry>,
}

#[derive(Deserialize)]
struct ItemUseEntry {
    #[serde(deserialize_with = "deserialize_i64")]
    id: i64,
    #[serde(deserialize_with = "deserialize_i64")]
    number: i64,
    #[serde(default, rename = "selectIndex", deserialize_with = "deserialize_i64")]
    select_index: i64,
}

#[derive(Deserialize)]
struct ItemEffect {
    #[serde(rename = "effectKind")]
    kind: i64,
    #[serde(rename = "effectValue")]
    value: i64,
    start_time: Option<i64>,
    end_time: Option<i64>,
}

#[derive(Deserialize)]
struct Choice {
    item_id: i64,
    amount: i64,
}

#[derive(Deserialize)]
struct SelectionItem {
    name: String,
    start_time: i64,
    end_time: i64,
    choices: Vec<Choice>,
}

struct Catalog {
    effects: BTreeMap<i64, ItemEffect>,
    selections: BTreeMap<i64, SelectionItem>,
}

struct UsePlan {
    items: BTreeMap<i64, i64>,
    selections: BTreeMap<(i64, i64), i64>,
    recovery: i64,
}

static CATALOG: OnceLock<Result<Catalog, String>> = OnceLock::new();

// //// 读取体力道具和自选素材目录 [@x380kkm 2026-09-28] ////
fn catalog() -> Result<&'static Catalog, PersonalServiceError> {
    CATALOG
        .get_or_init(|| {
            Ok(Catalog {
                effects: serde_json::from_str(include_str!("../../assets/cn-item-effects.json"))
                    .map_err(|error| error.to_string())?,
                selections: serde_json::from_str(include_str!(
                    "../../assets/cn-item-selection.json"
                ))
                .map_err(|error| error.to_string())?,
            })
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

// //// 合并库存用量并保留每个素材选项的数量 [@x380kkm 2026-09-28] ////
fn prepare_use(items: &[ItemUseEntry], catalog: &Catalog) -> Result<UsePlan, &'static str> {
    if items.is_empty() {
        return Err("invalid_request_body");
    }
    let mut plan = UsePlan {
        items: BTreeMap::new(),
        selections: BTreeMap::new(),
        recovery: 0,
    };
    for item in items {
        if item.id <= 0 || item.number <= 0 {
            return Err("invalid_item_use");
        }
        if let Some(selection) = catalog.selections.get(&item.id) {
            let index = usize::try_from(item.select_index).map_err(|_| "invalid_item_selection")?;
            if index == 0 || index > selection.choices.len() {
                return Err("invalid_item_selection");
            }
            selection.choices[index - 1]
                .amount
                .checked_mul(item.number)
                .ok_or("invalid_item_number")?;
            let count = plan
                .selections
                .entry((item.id, item.select_index))
                .or_default();
            *count = count
                .checked_add(item.number)
                .ok_or("invalid_item_number")?;
        } else {
            let effect = catalog.effects.get(&item.id).ok_or("item_not_usable")?;
            if item.select_index != 0 || !matches!(effect.kind, 2 | 3) {
                return Err("invalid_item_selection");
            }
            let per_item = if effect.kind == 2 {
                effect.value
            } else {
                MAX_STAMINA_OVERFLOW * effect.value / 100
            };
            let recovery = per_item
                .checked_mul(item.number)
                .ok_or("invalid_item_number")?;
            plan.recovery = plan
                .recovery
                .checked_add(recovery)
                .ok_or("invalid_item_number")?;
        }
        let count = plan.items.entry(item.id).or_default();
        *count = count
            .checked_add(item.number)
            .ok_or("invalid_item_number")?;
    }
    Ok(plan)
}
// //// /合并库存用量并保留每个素材选项的数量 ////

// //// 生成与请求排列无关的重放参数 [@x380kkm 2026-09-28] ////
fn receipt_request(plan: &UsePlan) -> Value {
    let mut request = json!({"items":plan.items});
    if !plan.selections.is_empty() {
        request["selections"] = json!(plan
            .selections
            .iter()
            .map(|(&(id, index), &number)| [id, index, number])
            .collect::<Vec<_>>());
    }
    request
}

// //// 按选择序号合并各类素材奖励 [@x380kkm 2026-09-28] ////
fn selection_rewards(plan: &UsePlan, catalog: &Catalog) -> Result<MailReward, &'static str> {
    let mut reward = MailReward::default();
    for (&(id, index), &number) in &plan.selections {
        let choice = &catalog.selections[&id].choices[index as usize - 1];
        let amount = choice
            .amount
            .checked_mul(number)
            .ok_or("invalid_item_number")?;
        let total = reward
            .item_list
            .entry(choice.item_id.to_string())
            .or_default();
        *total = total.checked_add(amount).ok_or("invalid_item_number")?;
    }
    Ok(reward)
}

// //// 在同一快照中结算道具使用和素材发放 [@x380kkm 2026-09-28] ////
pub(super) fn use_items(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ItemUseRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        _ => {
            return Ok(super::error_response(
                "400 Bad Request",
                "invalid_request_body",
            ))
        }
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let catalog = catalog()?;
    let plan = match prepare_use(&body.items, catalog) {
        Ok(plan) => plan,
        Err(code) => return Ok(super::error_response("400 Bad Request", code)),
    };
    let rewards = match selection_rewards(&plan, catalog) {
        Ok(rewards) => rewards,
        Err(code) => return Ok(super::error_response("400 Bad Request", code)),
    };
    let mut player = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player)?;
    let receipt = body.api_count.map(|count| {
        RequestReceipt::new("item", "use", body.viewer_id, count, receipt_request(&plan))
    });
    if let Some(receipt) = &receipt {
        if let Some(response) = receipt.replay(root)? {
            return Ok(response);
        }
    }
    let now = server_time(database)?;
    if plan.items.keys().any(|id| {
        if let Some(selection) = catalog.selections.get(id) {
            now < selection.start_time || now > selection.end_time
        } else {
            let effect = &catalog.effects[id];
            effect.start_time.is_some_and(|start| now < start)
                || effect.end_time.is_some_and(|end| now > end)
        }
    }) {
        return msgpack_result_code_response_at(body.viewer_id, now, 2057);
    }
    let mut response = json!({"item_list":{},"mail_arrived":false});
    if plan.recovery > 0 {
        let current = crate::cn_stamina::current_stamina(root, now)?;
        if current >= MAX_STAMINA_OVERFLOW {
            return msgpack_result_code_response_at(body.viewer_id, now, 2102);
        }
        let next = current
            .saturating_add(plan.recovery)
            .min(MAX_STAMINA_OVERFLOW);
        response["user_info"] = json!({"stamina":next,"stamina_heal_time":now});
    }
    let mut updates = Map::new();
    for (&id, &number) in &plan.items {
        let held = crate::cn_reward_capacity::held_count(root, "item_list", &id.to_string());
        if held < number {
            return Ok(super::error_response(
                "400 Bad Request",
                "insufficient_items",
            ));
        }
        updates.insert(id.to_string(), json!(held - number));
    }
    require_object(root, "item_list")?.extend(updates.clone());
    response["item_list"] = Value::Object(updates);
    if let Some(user) = response.get("user_info").and_then(Value::as_object) {
        require_object(root, "user_info")?.extend(user.clone());
    }
    if !rewards.item_list.is_empty() {
        let names = plan
            .items
            .keys()
            .filter_map(|id| catalog.selections.get(id))
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        cn_reward_overflow::grant(
            root,
            Conversion {
                created_at: now,
                subject: "自选素材奖励".to_owned(),
                description: names,
                notice: None,
                reward: rewards,
            },
        )?;
        cn_reward_overflow::apply_response(&mut response, root)?;
    }
    if let Some(receipt) = receipt {
        receipt.record(root, &response, now)?;
    }
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player)?)?;
    msgpack_response_at(body.viewer_id, false, now, response)
}
// //// /在同一快照中结算道具使用和素材发放 ////
