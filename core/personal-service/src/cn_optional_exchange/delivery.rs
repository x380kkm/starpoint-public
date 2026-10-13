// audience: internal
// # personal-service-cn-optional-exchange-delivery
// 普通自选发放一件目标, 宝库发放五件装备; 重复角色沿用统一角色奖励规则.

use super::{catalog::Campaign, state, Map, PersonalServiceError, Value};
use crate::cn_battle_assets::Reward;
use crate::cn_character_reward::{grant_character, record_character_encyclopedia_state};
use crate::cn_tutorial::require_object;
use crate::database::ReceiveHistoryEntry;
use serde_json::json;

pub(super) struct Delivery {
    pub(super) history: Vec<ReceiveHistoryEntry>,
    pub(super) costs: Value,
}

// //// 构造保留现有可选状态的兑换响应 [@x380kkm 2026-09-23] ////
pub(super) fn response(root: &Map<String, Value>) -> Value {
    json!({
        "character_list":[],"equipment_list":[],"item_list":{},"user_info":{},
        "mail_arrived":false,"fund_receive_list":root.get("fund_receive_list"),
        "monthly_charge_bonus_info":root.get("monthly_charge_bonus_info"),
        "user_daily_challenge_point_list":root.get("user_daily_challenge_point_list"),
        "crazy_gacha_result_list":null,
    })
}

// //// 消耗付费星导石并发放自选券和十连券 [@x380kkm 2026-09-23] ////
pub(super) fn issue_ticket(
    root: &mut Map<String, Value>,
    campaign: &Campaign,
    response: &mut Value,
) -> Result<Delivery, PersonalServiceError> {
    let balance = root
        .get("user_info")
        .and_then(|info| info.get("vmoney"))
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new("missing CN paid stone balance"))?;
    let price = campaign.price;
    let remaining = balance
        .checked_sub(price)
        .filter(|value| *value >= 0)
        .ok_or_else(|| PersonalServiceError::new("insufficient CN exchange paid stones"))?;
    require_object(root, "user_info")?.insert("vmoney".to_owned(), json!(remaining));
    response["user_info"] = json!({"vmoney":remaining});
    let mut history = Vec::new();
    for id in [Some(campaign.ticket_id), campaign.gacha_ticket_id]
        .into_iter()
        .flatten()
    {
        let count = state::adjust_item(root, id, 1)?;
        response["item_list"][id.to_string()] = json!(count);
        history.push(ReceiveHistoryEntry::reward(1, Some(id), 1));
    }
    Ok(Delivery {
        history,
        costs: json!({"vmoney":price}),
    })
}
// //// /消耗付费星导石并发放自选券和十连券 ////

// //// 扣除兑换券并发放角色或装备及角色重复素材 [@x380kkm 2026-09-23] ////
pub(super) fn exchange(
    root: &mut Map<String, Value>,
    campaign: &Campaign,
    viewer: i64,
    target: i64,
    now: i64,
    response: &mut Value,
) -> Result<Delivery, PersonalServiceError> {
    let remaining = state::adjust_item(root, campaign.ticket_id, -1)?;
    response["item_list"][campaign.ticket_id.to_string()] = json!(remaining);
    let mut history = Vec::new();
    if campaign.kind == 0 {
        let reward = grant_character(root, viewer, target, now)?;
        response["character_list"] = json!([reward.character]);
        response["encyclopedia_info"] =
            Value::Object(record_character_encyclopedia_state(root, target)?);
        history.push(ReceiveHistoryEntry::reward(5, Some(target), 1));
        if let Some(duplicate) = reward.duplicate_item {
            response["item_list"][duplicate.id.to_string()] = json!(duplicate.total);
            history.push(ReceiveHistoryEntry::reward(
                1,
                Some(duplicate.id),
                duplicate.count,
            ));
        }
    } else {
        let reward = crate::cn_battle_rewards::apply_reward_at(
            root,
            &Reward {
                kind: 1,
                id: Some(target),
                count: Some(campaign.reward_count),
                rarity: None,
            },
            now,
        )?;
        response["equipment_list"] = json!(reward.equipment_list);
        history.push(ReceiveHistoryEntry::reward(
            6,
            Some(target),
            campaign.reward_count,
        ));
    }
    crate::cn_reward_overflow::apply_response(response, root)?;
    Ok(Delivery {
        history,
        costs: json!({"item_id":campaign.ticket_id,"number":1}),
    })
}
// //// /扣除兑换券并发放角色或装备及角色重复素材 ////
