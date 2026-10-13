// audience: internal
// # personal-service-cn-payment-subscriptions
//
// 订阅在中国时间 05:00 切换奖励日. 已观察时间与领取日期随存档保存.
// 日常奖励直接入账, 到期补领奖励通过调用方的同一事务投递邮件.

use super::catalog::{catalog, PaymentCatalog, PaymentReward, SubscriptionDefinition, SubscriptionKind};
use crate::cn_tutorial::require_object;
use crate::database::{MailReward, ReceiveHistoryEntry};
use crate::PersonalServiceError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const DAY_SECONDS: i64 = 86_400;
const DAY_OFFSET_SECONDS: i64 = 3 * 3600;
const CHALLENGE_POINT_MAP: &str = include_str!("../../../../assets/event_challenge_point_map.json");
const DEFAULT_PLAYER: &str = include_str!("../../assets/cn-default-player.json");

#[derive(Default)]
pub(crate) struct SubscriptionDelivery {
    pub(crate) entries: Vec<ReceiveHistoryEntry>,
    pub(crate) mails: Vec<SubscriptionMail>,
    pub(crate) key: String,
}

pub(crate) struct SubscriptionMail {
    pub(crate) event_key: String,
    pub(crate) title: String,
    pub(crate) rewards: MailReward,
    pub(crate) created_at: i64,
}

pub(crate) struct DailyResources {
    boost: i64,
    boss_boost: i64,
    challenges: Vec<Value>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct SubscriptionState {
    entitlements: BTreeMap<String, Entitlement>,
    payment_total: i64,
    observed_time: i64,
    resource_day: Option<i64>,
    resource_bonus: ResourceBonus,
    auto_3x_before_subscription: Option<bool>,
}

#[derive(Deserialize, Serialize)]
struct Entitlement {
    store_product_id: String,
    activated_at: i64,
    first_day: i64,
    end_day: i64,
    received_days: BTreeSet<i64>,
    mailed: bool,
}

#[derive(Default, Deserialize, Serialize)]
struct ResourceBonus {
    boost: i64,
    boss_boost: i64,
    challenge: i64,
}

// //// 捕获订阅日内已经消耗的资源余额 [@x380kkm 2026-09-22] ////
pub(crate) fn capture_daily_resources(root: &Map<String, Value>) -> Result<DailyResources, PersonalServiceError> {
    let user = root.get("user_info").and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("CN subscription user info is missing"))?;
    Ok(DailyResources {
        boost: user.get("boost_point").and_then(Value::as_i64).unwrap_or_default(),
        boss_boost: user.get("boss_boost_point").and_then(Value::as_i64).unwrap_or_default(),
        challenges: root.get("user_daily_challenge_point_list").and_then(Value::as_array).cloned().unwrap_or_default(),
    })
}
// //// /捕获订阅日内已经消耗的资源余额 ////

// //// 领取当日订阅奖励并投影有效权益 [@x380kkm 2026-09-22] ////
pub(crate) fn prepare_load(
    root: &mut Map<String, Value>, before_reset: &DailyResources, now: i64,
) -> Result<SubscriptionDelivery, PersonalServiceError> {
    let Some(mut state) = read_state(root)? else { return Ok(SubscriptionDelivery::default()); };
    let mut delivery = SubscriptionDelivery::default();
    settle_state(root, &mut state, before_reset, now, &mut delivery)?;
    save_state(root, &state)?;
    Ok(delivery)
}
// //// /领取当日订阅奖励并投影有效权益 ////

// //// 激活或延长订阅并结算购买当日奖励 [@x380kkm 2026-09-22] ////
pub(crate) fn activate(
    root: &mut Map<String, Value>, subscription_id: &str, store_product_id: &str, now: i64,
) -> Result<SubscriptionDelivery, PersonalServiceError> {
    let mut state = read_state(root)?.unwrap_or_default();
    let before = capture_daily_resources(root)?;
    let mut delivery = SubscriptionDelivery::default();
    settle_state(root, &mut state, &before, now, &mut delivery)?;
    let definition = definition(catalog()?, subscription_id)?;
    activate_entitlement(&mut state, subscription_id, store_product_id, definition)?;
    let before = capture_daily_resources(root)?;
    settle_state(root, &mut state, &before, now, &mut delivery)?;
    save_state(root, &state)?;
    Ok(delivery)
}
// //// /激活或延长订阅并结算购买当日奖励 ////

// //// 累计未激活期间的消费并自动开启小月卡 [@x380kkm 2026-09-22] ////
pub(crate) fn record_payment(
    root: &mut Map<String, Value>, price: i64, now: i64,
) -> Result<SubscriptionDelivery, PersonalServiceError> {
    if price < 0 { return Err(PersonalServiceError::new("CN subscription payment is invalid")); }
    let mut state = read_state(root)?.unwrap_or_default();
    let before = capture_daily_resources(root)?;
    let mut delivery = SubscriptionDelivery::default();
    settle_state(root, &mut state, &before, now, &mut delivery)?;
    let day = reward_day(state.observed_time)?;
    for (id, definition) in &catalog()?.subscriptions {
        let Some(target) = definition.activation_payment_total else { continue; };
        if state.entitlements.get(id).is_some_and(|entry| day < entry.end_day) { continue; }
        state.payment_total = add(state.payment_total, price)?;
        if state.payment_total >= target {
            activate_entitlement(&mut state, id, "worldflipper.daily_set_02", definition)?;
            state.payment_total = 0;
        }
    }
    let before = capture_daily_resources(root)?;
    settle_state(root, &mut state, &before, now, &mut delivery)?;
    save_state(root, &state)?;
    Ok(delivery)
}
// //// /累计未激活期间的消费并自动开启小月卡 ////

fn activate_entitlement(
    state: &mut SubscriptionState, id: &str, store_product_id: &str, definition: &SubscriptionDefinition,
) -> Result<(), PersonalServiceError> {
    let day = reward_day(state.observed_time)?;
    let remaining = state.entitlements.get(id).map(|entry| (entry.end_day - day).max(0)).unwrap_or_default();
    if let Some(entry) = state.entitlements.get_mut(id).filter(|_| remaining > 0) {
        entry.end_day = add(entry.end_day, definition.duration_days)?;
        return Ok(());
    }
    state.entitlements.insert(id.to_owned(), Entitlement {
        store_product_id: store_product_id.to_owned(), activated_at: state.observed_time,
        first_day: day, end_day: add(day, definition.duration_days)?, received_days: BTreeSet::new(), mailed: false,
    });
    Ok(())
}

// //// 将当日实发与到期补领写入订阅凭据 [@x380kkm 2026-09-22] ////
fn settle_state(
    root: &mut Map<String, Value>, state: &mut SubscriptionState, before: &DailyResources,
    now: i64, delivery: &mut SubscriptionDelivery,
) -> Result<(), PersonalServiceError> {
    if now < 0 { return Err(PersonalServiceError::new("CN subscription time is invalid")); }
    state.observed_time = state.observed_time.max(now);
    let day = reward_day(state.observed_time)?;
    let catalog = catalog()?;
    for (id, entitlement) in &mut state.entitlements {
        let definition = definition(catalog, id)?;
        if day < entitlement.end_day {
            if entitlement.received_days.insert(day) {
                let mut entries = super::grant::apply_rewards(root, &definition.daily_rewards, state.observed_time)?;
                for entry in &mut entries { entry.reason_id = definition.reason_id.unwrap_or_default(); }
                delivery.entries.extend(entries);
                add_delivery_key(delivery, &format!("daily:{id}:{}:{day}", entitlement.first_day));
            }
        } else if !entitlement.mailed {
            let missed = entitlement.end_day - entitlement.first_day - entitlement.received_days.len() as i64;
            if definition.catch_up_rewards && missed > 0 && !definition.daily_rewards.is_empty() {
                let event_key = format!("subscription:{id}:{}:expired", entitlement.first_day);
                delivery.mails.push(SubscriptionMail { event_key: event_key.clone(), title: "每日奖励补领".to_owned(),
                    rewards: accumulated_mail_rewards(&definition.daily_rewards, missed)?, created_at: state.observed_time });
                add_delivery_key(delivery, &event_key);
            }
            entitlement.mailed = true;
        }
    }
    let bonus = active_resource_bonus(state, catalog, day)?;
    update_daily_resources(root, before, state, &bonus, day)?;
    state.resource_day = Some(day);
    state.resource_bonus = bonus;
    project_entitlements(root, state, catalog, day)
}
// //// /将当日实发与到期补领写入订阅凭据 ////

fn active_resource_bonus(state: &SubscriptionState, catalog: &PaymentCatalog, day: i64) -> Result<ResourceBonus, PersonalServiceError> {
    let mut bonus = ResourceBonus::default();
    for (id, _) in state.entitlements.iter().filter(|(_, entry)| day < entry.end_day) {
        let definition = definition(catalog, id)?;
        bonus.boost = add(bonus.boost, definition.daily_quest_boost_amount)?;
        bonus.boss_boost = add(bonus.boss_boost, definition.boss_battle_boost_amount)?;
        bonus.challenge = add(bonus.challenge, definition.daily_challenge_amount)?;
    }
    Ok(bonus)
}

// //// 在订阅日切重置额外资源并保留日内消费 [@x380kkm 2026-09-22] ////
fn update_daily_resources(
    root: &mut Map<String, Value>, before: &DailyResources, state: &SubscriptionState, bonus: &ResourceBonus, day: i64,
) -> Result<(), PersonalServiceError> {
    let new_day = state.resource_day.is_some_and(|previous| day > previous);
    let defaults: Value = serde_json::from_str(DEFAULT_PLAYER).map_err(|error| PersonalServiceError::new(error.to_string()))?;
    let user = require_object(root, "user_info")?;
    for (field, current, old_bonus, new_bonus) in [
        ("boost_point", before.boost, state.resource_bonus.boost, bonus.boost),
        ("boss_boost_point", before.boss_boost, state.resource_bonus.boss_boost, bonus.boss_boost),
    ] {
        if old_bonus == 0 && new_bonus == 0 { continue; }
        let balance = if new_day {
            add(defaults["user_info"][field].as_i64().unwrap_or_default(), new_bonus)?
        } else { add(current, (new_bonus - old_bonus).max(0))? };
        user.insert(field.to_owned(), json!(balance));
    }
    if state.resource_bonus.challenge == 0 && bonus.challenge == 0 { return Ok(()); }
    let point_map: BTreeMap<String, i64> = serde_json::from_str(CHALLENGE_POINT_MAP).map_err(|error| PersonalServiceError::new(error.to_string()))?;
    let id = point_map.get("expert_1").ok_or_else(|| PersonalServiceError::new("CN subscription challenge point is missing"))?;
    let default = defaults["user_daily_challenge_point_list"].as_array().and_then(|list| list.iter().find(|entry| entry["id"] == *id))
        .ok_or_else(|| PersonalServiceError::new("CN default subscription challenge point is missing"))?;
    let mut entry = before.challenges.iter().find(|entry| entry["id"] == *id).cloned().unwrap_or_else(|| default.clone());
    let previous = entry["point"].as_i64().unwrap_or_default();
    let base = default["point"].as_i64().unwrap_or_default() - campaign_points(default)?;
    entry["point"] = json!(if new_day { add(add(base, campaign_points(&entry)?)?, bonus.challenge)? }
        else { add(previous, (bonus.challenge - state.resource_bonus.challenge).max(0))? });
    let challenges = root.entry("user_daily_challenge_point_list".to_owned()).or_insert_with(|| json!([])).as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("CN subscription challenge points are invalid"))?;
    if let Some(stored) = challenges.iter_mut().find(|entry| entry["id"] == *id) { *stored = entry; }
    else { challenges.push(entry); }
    Ok(())
}
// //// /在订阅日切重置额外资源并保留日内消费 ////

fn campaign_points(entry: &Value) -> Result<i64, PersonalServiceError> {
    entry["campaign_list"].as_array().into_iter().flatten().try_fold(0_i64, |total, campaign| {
        add(total, campaign["additional_point"].as_i64().unwrap_or_default())
    })
}

fn project_entitlements(root: &mut Map<String, Value>, state: &mut SubscriptionState, catalog: &PaymentCatalog, day: i64) -> Result<(), PersonalServiceError> {
    let managed_premium = state.entitlements.values().map(|entry| entry.store_product_id.as_str()).collect::<BTreeSet<_>>();
    let mut premium = root.get("premium_bonus_list").and_then(Value::as_array).into_iter().flatten()
        .filter(|entry| !entry["store_product_id"].as_str().is_some_and(|id| managed_premium.contains(id))).cloned().collect::<Vec<_>>();
    for (id, entry) in &state.entitlements {
        let definition = definition(catalog, id)?;
        let remaining = (entry.end_day - day).max(0);
        match definition.kind {
            SubscriptionKind::MonthCard => { require_object(root, "user_info")?.insert("month_card_remain_days".to_owned(), json!(remaining)); }
            SubscriptionKind::WeeklyBossBoost => { require_object(root, "user_info")?.insert("weekly_bonus_remain_days".to_owned(), json!(remaining)); }
            SubscriptionKind::WeeklyDailyChallenge => {
                let user = require_object(root, "user_info")?;
                let renewal_days = user.entry("renewal_gift_remain_days".to_owned()).or_insert_with(|| json!({}));
                if renewal_days.is_null() || *renewal_days == json!(0) { *renewal_days = json!({}); }
                renewal_days.as_object_mut()
                    .ok_or_else(|| PersonalServiceError::new("CN renewal gift days are invalid"))?.insert(entry.store_product_id.clone(), json!(remaining));
            }
            SubscriptionKind::MiniMonthCard => {
                let granted_days = if entry.mailed && definition.catch_up_rewards { entry.end_day - entry.first_day } else { entry.received_days.len() as i64 };
                root.insert("monthly_charge_bonus_info".to_owned(), json!({"bonus_days":granted_days,"init_time":entry.activated_at,"expired_time":day_start(entry.end_day)?}));
            }
            SubscriptionKind::PremiumLoginBonus => {
                if remaining > 0 { premium.push(json!({"remain_days":remaining,"store_product_id":entry.store_product_id})); }
            }
        }
    }
    root.insert("premium_bonus_list".to_owned(), json!(premium));
    let enable_auto_3x = state.entitlements.iter().any(|(id, entry)| day < entry.end_day
        && catalog.subscriptions.get(id).is_some_and(|definition| definition.enable_auto_3x));
    let user = require_object(root, "user_info")?;
    if enable_auto_3x {
        state.auto_3x_before_subscription.get_or_insert_with(|| user.get("enable_auto_3x").and_then(Value::as_bool).unwrap_or(false));
        user.insert("enable_auto_3x".to_owned(), json!(true));
    } else if let Some(previous) = state.auto_3x_before_subscription.take() {
        user.insert("enable_auto_3x".to_owned(), json!(previous));
    }
    Ok(())
}

fn accumulated_mail_rewards(rewards: &[PaymentReward], days: i64) -> Result<MailReward, PersonalServiceError> {
    let mut mail = MailReward::default();
    for reward in rewards {
        match *reward {
            PaymentReward::Item { id, amount } => {
                let count = mail.item_list.entry(id.to_string()).or_default();
                *count = add(*count, multiply(amount, days)?)?;
            }
            PaymentReward::FreeVmoney { amount } => mail.free_vmoney = add(mail.free_vmoney, multiply(amount, days)?)?,
            PaymentReward::Experience { amount } => mail.exp_pool = add(mail.exp_pool, multiply(amount, days)?)?,
            PaymentReward::Mana { amount } => mail.free_mana = add(mail.free_mana, multiply(amount, days)?)?,
            PaymentReward::StarCrumb { amount } => mail.star_crumb = add(mail.star_crumb, multiply(amount, days)?)?,
            PaymentReward::Character { id, amount } => {
                for _ in 0..multiply(amount, days)? { mail.character_list.push(id); }
            }
            PaymentReward::PassCardPoint { .. } => return Err(PersonalServiceError::new("CN subscription mail cannot contain pass card points")),
        }
    }
    Ok(mail)
}

fn read_state(root: &Map<String, Value>) -> Result<Option<SubscriptionState>, PersonalServiceError> {
    root.get("payment_subscription_state").map(|value| {
        serde_json::from_value(value.clone()).map_err(|error| PersonalServiceError::new(format!("invalid CN subscription state: {error}")))
    }).transpose()
}

fn save_state(root: &mut Map<String, Value>, state: &SubscriptionState) -> Result<(), PersonalServiceError> {
    root.insert("payment_subscription_state".to_owned(), serde_json::to_value(state).map_err(|error| PersonalServiceError::new(error.to_string()))?);
    Ok(())
}

fn definition<'a>(catalog: &'a PaymentCatalog, id: &str) -> Result<&'a SubscriptionDefinition, PersonalServiceError> {
    catalog.subscriptions.get(id).ok_or_else(|| PersonalServiceError::new("CN subscription definition is missing"))
}

fn reward_day(now: i64) -> Result<i64, PersonalServiceError> { Ok(add(now, DAY_OFFSET_SECONDS)?.div_euclid(DAY_SECONDS)) }
fn day_start(day: i64) -> Result<i64, PersonalServiceError> { Ok(multiply(day, DAY_SECONDS)? - DAY_OFFSET_SECONDS) }
fn add(left: i64, right: i64) -> Result<i64, PersonalServiceError> { left.checked_add(right).ok_or_else(|| PersonalServiceError::new("CN subscription amount exceeds range")) }
fn multiply(left: i64, right: i64) -> Result<i64, PersonalServiceError> { left.checked_mul(right).ok_or_else(|| PersonalServiceError::new("CN subscription amount exceeds range")) }

fn add_delivery_key(delivery: &mut SubscriptionDelivery, event: &str) {
    let digest = Sha256::digest(format!("{}|{event}", delivery.key).as_bytes());
    delivery.key = format!("subscription:{digest:x}");
}
