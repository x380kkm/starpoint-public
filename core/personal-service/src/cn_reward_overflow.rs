// audience: internal
// # personal-service-cn-reward-overflow
// 玛纳, EXP 和物品按持有容量入库, 余量与领取记录随玩家快照事务保存.

use crate::cn_reward_capacity::{capacities, held_count};
use crate::cn_tutorial::require_object;
use crate::database::MailReward;
use crate::PersonalServiceError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub(crate) const DELIVERIES_FIELD: &str = "reward_deliveries";

#[derive(Deserialize, Serialize)]
pub(crate) struct Conversion {
    pub(crate) created_at: i64,
    pub(crate) subject: String,
    pub(crate) description: String,
    pub(crate) notice: Option<Value>,
    pub(crate) reward: MailReward,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct Delivery {
    pub(crate) event_key: String,
    pub(crate) conversion: Conversion,
    pub(crate) mail: MailReward,
}

impl Delivery {
    // //// 判断转换余量是否需要邮件投递 [@x380kkm 2026-09-26] ////
    pub(crate) fn has_mail(&self) -> bool {
        self.mail.free_mana > 0 || self.mail.exp_pool > 0 || !self.mail.item_list.is_empty()
    }

    // //// 提取实际直接到账的转换资产 [@x380kkm 2026-09-26] ////
    pub(crate) fn credited_reward(&self) -> MailReward {
        let mut reward = MailReward {
            free_mana: self.conversion.reward.free_mana - self.mail.free_mana,
            exp_pool: self.conversion.reward.exp_pool - self.mail.exp_pool,
            ..MailReward::default()
        };
        for (id, amount) in &self.conversion.reward.item_list {
            let credited = amount - self.mail.item_list.get(id).copied().unwrap_or_default();
            if credited > 0 {
                reward.item_list.insert(id.clone(), credited);
            }
        }
        reward
    }
}

// //// 将转换奖励按当前容量分配到库存和邮件 [@x380kkm 2026-09-26] ////
pub(crate) fn grant(
    root: &mut Map<String, Value>,
    conversion: Conversion,
) -> Result<(), PersonalServiceError> {
    let mut mail = MailReward::default();
    if conversion.reward.free_mana > 0 {
        let before = held_count(root, "user_info", "free_mana");
        let paid = held_count(root, "user_info", "paid_mana");
        let credited = conversion.reward.free_mana.min(
            capacities()?.mana.saturating_sub(paid).saturating_sub(before).max(0),
        );
        require_object(root, "user_info")?.insert("free_mana".to_owned(), json!(before + credited));
        mail.free_mana = conversion.reward.free_mana - credited;
    }
    if conversion.reward.exp_pool > 0 {
        if conversion.created_at > 0 {
            crate::cn_exp_pool::settle(root, conversion.created_at)?;
        }
        let before = held_count(root, "user_info", "exp_pool");
        let credited = conversion
            .reward
            .exp_pool
            .min(capacities()?.exp_pool.saturating_sub(before).max(0));
        require_object(root, "user_info")?.insert("exp_pool".to_owned(), json!(before + credited));
        mail.exp_pool = conversion.reward.exp_pool - credited;
    }
    for (id, amount) in &conversion.reward.item_list {
        let before = held_count(root, "item_list", id);
        let credited = (*amount).min(item_capacity(id)?.saturating_sub(before).max(0));
        if credited > 0 {
            require_object(root, "item_list")?.insert(id.clone(), json!(before + credited));
        }
        if *amount > credited {
            mail.item_list.insert(id.clone(), amount - credited);
        }
    }
    let mut nonce = [0_u8; 16];
    getrandom::getrandom(&mut nonce).map_err(|error| {
        PersonalServiceError::new(format!("CN reward delivery id failed: {error}"))
    })?;
    let delivery = Delivery {
        event_key: format!("reward-conversion-{:032x}", u128::from_le_bytes(nonce)),
        conversion,
        mail,
    };
    root.entry(DELIVERIES_FIELD.to_owned())
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("CN reward deliveries are invalid"))?
        .push(
            serde_json::to_value(delivery)
                .map_err(|error| PersonalServiceError::new(error.to_string()))?,
        );
    Ok(())
}
// //// /将转换奖励按当前容量分配到库存和邮件 ////

// //// 将同批其他奖励占用的转换容量归入邮件余量 [@x380kkm 2026-09-26] ////
pub(crate) fn reconcile_capacity(
    root: &mut Map<String, Value>,
) -> Result<(), PersonalServiceError> {
    let Some(pending) = root.get(DELIVERIES_FIELD) else {
        return Ok(());
    };
    let mut deliveries: Vec<Delivery> = serde_json::from_value(pending.clone())
        .map_err(|error| PersonalServiceError::new(error.to_string()))?;
    let mut changed = false;
    for delivery in deliveries.iter_mut().rev() {
        let before = held_count(root, "user_info", "free_mana");
        let paid = held_count(root, "user_info", "paid_mana");
        let excess = before.saturating_add(paid).saturating_sub(capacities()?.mana).max(0);
        let mailed = excess.min(delivery.conversion.reward.free_mana - delivery.mail.free_mana);
        if mailed > 0 {
            delivery.mail.free_mana += mailed;
            require_object(root, "user_info")?
                .insert("free_mana".to_owned(), json!(before - mailed));
            changed = true;
        }
        let before = held_count(root, "user_info", "exp_pool");
        let excess = before.saturating_sub(capacities()?.exp_pool).max(0);
        let mailed = excess.min(delivery.conversion.reward.exp_pool - delivery.mail.exp_pool);
        if mailed > 0 {
            delivery.mail.exp_pool += mailed;
            require_object(root, "user_info")?
                .insert("exp_pool".to_owned(), json!(before - mailed));
            changed = true;
        }
        for (id, amount) in &delivery.conversion.reward.item_list {
            let before = held_count(root, "item_list", id);
            let excess = before.saturating_sub(item_capacity(id)?).max(0);
            let previous_mail = delivery.mail.item_list.get(id).copied().unwrap_or_default();
            let mailed = excess.min(amount - previous_mail);
            if mailed > 0 {
                delivery
                    .mail
                    .item_list
                    .insert(id.clone(), previous_mail + mailed);
                require_object(root, "item_list")?.insert(id.clone(), json!(before - mailed));
                changed = true;
            }
        }
    }
    if changed {
        root.insert(
            DELIVERIES_FIELD.to_owned(),
            serde_json::to_value(deliveries)
                .map_err(|error| PersonalServiceError::new(error.to_string()))?,
        );
    }
    Ok(())
}
// //// /将同批其他奖励占用的转换容量归入邮件余量 ////

// //// 将转换结果与邮件余量投影到原生客户端响应 [@x380kkm 2026-09-26] ////
pub(crate) fn apply_response(
    response: &mut Value,
    root: &mut Map<String, Value>,
) -> Result<(), PersonalServiceError> {
    reconcile_capacity(root)?;
    let Some(pending) = root.get(DELIVERIES_FIELD) else {
        return Ok(());
    };
    let deliveries: Vec<Delivery> = serde_json::from_value(pending.clone())
        .map_err(|error| PersonalServiceError::new(error.to_string()))?;
    if deliveries.is_empty() {
        return Ok(());
    }
    let response = response
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("CN reward response is invalid"))?;
    let mut over_max = Vec::new();
    for delivery in &deliveries {
        if let Some(notice) = &delivery.conversion.notice {
            over_max.push(notice.clone());
        }
        if delivery.mail.free_mana > 0 {
            over_max.push(json!({"process_type":1,"free_mana":delivery.mail.free_mana}));
        }
        if delivery.mail.exp_pool > 0 {
            over_max.push(json!({"process_type":1,"pooled_exp":delivery.mail.exp_pool}));
        }
        for (id, number) in &delivery.mail.item_list {
            let item_id: i64 = id
                .parse()
                .map_err(|_| PersonalServiceError::new("CN conversion item id is invalid"))?;
            over_max.push(json!({"process_type":1,"item":{"item_id":item_id,"number":number}}));
        }
        for id in delivery.conversion.reward.item_list.keys() {
            response_object(response, "item_list")?
                .insert(id.clone(), json!(held_count(root, "item_list", id)));
        }
    }
    response.insert("over_max".to_owned(), json!(over_max));
    if deliveries.iter().any(|delivery| delivery.conversion.reward.free_mana > 0) {
        response_object(response, "user_info")?
            .insert("free_mana".to_owned(), json!(held_count(root, "user_info", "free_mana")));
    }
    if deliveries
        .iter()
        .any(|delivery| delivery.conversion.reward.exp_pool > 0)
    {
        let user = response_object(response, "user_info")?;
        for field in ["exp_pool", "exp_pooled_time"] {
            if let Some(value) = root.get("user_info").and_then(|user| user.get(field)) {
                user.insert(field.to_owned(), value.clone());
            }
        }
    }
    if deliveries.iter().any(Delivery::has_mail) {
        response.insert("mail_arrived".to_owned(), json!(true));
    }
    Ok(())
}
// //// /将转换结果与邮件余量投影到原生客户端响应 ////

// //// 读取转换物品的主表持有上限 [@x380kkm 2026-09-26] ////
fn item_capacity(id: &str) -> Result<i64, PersonalServiceError> {
    let id: i64 = id
        .parse()
        .map_err(|_| PersonalServiceError::new("CN conversion item id is invalid"))?;
    capacities()?
        .items
        .get(&id)
        .copied()
        .ok_or_else(|| PersonalServiceError::new("CN conversion item capacity is missing"))
}

// //// 取得客户端响应中的资产对象 [@x380kkm 2026-09-26] ////
fn response_object<'a>(
    response: &'a mut Map<String, Value>,
    field: &str,
) -> Result<&'a mut Map<String, Value>, PersonalServiceError> {
    response
        .entry(field.to_owned())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("CN reward response object is invalid"))
}
