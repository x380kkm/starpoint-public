// audience: internal
// # personal-service-cn-payment-grant
// 商品奖励沿用客户端的资产分类. 基金, 通行证和订阅资格与即时资产写入同一玩家快照.

use super::catalog::{PaymentEntitlement, PaymentProduct, PaymentReward};
use super::subscriptions::{self, SubscriptionDelivery};
use crate::cn_battle_assets::Reward;
use crate::cn_battle_rewards::apply_reward_at;
use crate::cn_tutorial::require_object;
use crate::database::{ReceiveHistoryEntry, ServiceDatabase};
use crate::PersonalServiceError;
use serde_json::{Map, Value};

// //// 发放商品资产并激活对应资格 [@x380kkm 2026-09-22] ////
pub(super) fn grant_product(root: &mut Map<String, Value>, product: &PaymentProduct, database: &ServiceDatabase, now: i64) -> Result<SubscriptionDelivery, PersonalServiceError> {
    let mut delivery = SubscriptionDelivery::default();
    for (field, amount, kind) in [("vmoney", product.charge_vmoney_num, 3), ("free_vmoney", product.free_vmoney_num, 4)] {
        if amount > 0 {
            let amount = add_balance(root, field, amount)?;
            if amount > 0 { delivery.entries.push(ReceiveHistoryEntry::reward(kind, None, amount)); }
        }
    }
    let ordinary = product.rewards.iter().filter(|reward| !matches!(reward, PaymentReward::PassCardPoint { .. })).cloned().collect::<Vec<_>>();
    delivery.entries.extend(apply_rewards(root, &ordinary, now)?);
    for entitlement in &product.entitlements {
        match entitlement {
            PaymentEntitlement::GrowthFund { kind } => { require_object(root, "user_info")?.insert(kind.user_flag().to_owned(), Value::Bool(true)); }
            PaymentEntitlement::PassCard => { crate::cn_pass_card::grant_purchase(root, database, now)?; }
            PaymentEntitlement::Subscription { id } => { merge_delivery(&mut delivery, subscriptions::activate(root, id, &product.store_product_id, now)?); }
        }
    }
    for reward in &product.rewards {
        if let PaymentReward::PassCardPoint { amount } = reward { crate::cn_pass_card::add_points(root, database, *amount, now)?; }
    }
    merge_delivery(&mut delivery, subscriptions::record_payment(root, product.price, now)?);
    for entry in &mut delivery.entries {
        if entry.reason_id == 0 { entry.reason_id = product.reason_id.unwrap_or_default(); }
    }
    Ok(delivery)
}
// //// /发放商品资产并激活对应资格 ////

fn merge_delivery(target: &mut SubscriptionDelivery, source: SubscriptionDelivery) {
    target.entries.extend(source.entries);
    target.mails.extend(source.mails);
}

// //// 将商品奖励转换为存档资产和领取记录 [@x380kkm 2026-09-22] ////
pub(super) fn apply_rewards(root: &mut Map<String, Value>, rewards: &[PaymentReward], now: i64) -> Result<Vec<ReceiveHistoryEntry>, PersonalServiceError> {
    let mut entries = Vec::new();
    for reward in rewards {
        let (kind, id, amount, history_kind) = match *reward {
            PaymentReward::Item { id, amount } => (0, Some(id), amount, 1),
            PaymentReward::FreeVmoney { amount } => {
                let granted = add_balance(root, "free_vmoney", amount)?;
                if granted > 0 { entries.push(ReceiveHistoryEntry::reward(4, None, granted)); }
                continue;
            }
            PaymentReward::Experience { amount } => (5, None, amount, 9),
            PaymentReward::Mana { amount } => (4, None, amount, 8),
            PaymentReward::StarCrumb { amount } => {
                let granted = add_balance(root, "star_crumb", amount)?;
                if granted > 0 { entries.push(ReceiveHistoryEntry::reward(7, None, granted)); }
                continue;
            }
            PaymentReward::Character { id, amount } => (2, Some(id), amount, 5),
            PaymentReward::PassCardPoint { .. } => return Err(PersonalServiceError::new("CN pass card points require an event")),
        };
        if amount <= 0 { return Err(PersonalServiceError::new("CN payment reward amount is invalid")); }
        let asset = Reward { kind, id, count: Some(amount), rarity: None };
        for _ in 0..if kind == 2 { amount } else { 1 } {
            let result = apply_reward_at(root, &asset, now)?;
            for character_id in result.joined_character_ids {
                crate::cn_character_reward::record_character_encyclopedia_state(root, character_id)?;
            }
        }
        entries.push(ReceiveHistoryEntry::reward(history_kind, id, amount));
    }
    Ok(entries)
}
// //// /将商品奖励转换为存档资产和领取记录 ////

fn add_balance(root: &mut Map<String, Value>, field: &str, amount: i64) -> Result<i64, PersonalServiceError> {
    if amount < 0 { return Err(PersonalServiceError::new("CN payment reward amount is invalid")); }
    let user = require_object(root, "user_info")?;
    let previous = user.get(field).and_then(Value::as_i64).unwrap_or_default();
    let maximum = if field == "star_crumb" { i32::MAX as i64 } else { 99_999_999 };
    let available = maximum.saturating_sub(previous).max(0);
    let granted = amount.min(available);
    user.insert(field.to_owned(), Value::from(previous.checked_add(granted)
        .ok_or_else(|| PersonalServiceError::new("CN payment balance exceeds range"))?));
    Ok(granted)
}
