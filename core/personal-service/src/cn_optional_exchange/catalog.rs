// audience: internal
// # personal-service-cn-optional-exchange-catalog
// 客户端主表决定自选候选, 赠送方式和兑换数量; 个人新手期限沿用初始角色的游戏内入队时间.

use super::{Map, PersonalServiceError, ServiceDatabase, Value};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Deserialize)]
pub(super) struct Catalog {
    pub(super) special: BTreeMap<i64, Campaign>,
    pub(super) treasure: BTreeMap<i64, Campaign>,
    pub(super) start_dash: BTreeMap<i64, StartDash>,
    pub(super) gachas: BTreeMap<i64, Gacha>,
}

#[derive(Clone, Deserialize)]
pub(super) struct Campaign {
    pub(super) kind: i64,
    pub(super) start_at: i64,
    pub(super) end_at: i64,
    pub(super) exchange_start_at: i64,
    pub(super) exchange_end_at: i64,
    pub(super) ticket_id: i64,
    pub(super) candidates: Vec<i64>,
    pub(super) product_source: i64,
    pub(super) price: i64,
    pub(super) gacha_ticket_id: Option<i64>,
    pub(super) reward_count: i64,
}

#[derive(Deserialize)]
pub(super) struct StartDash {
    pub(super) start_at: i64,
    pub(super) end_at: Option<i64>,
    pub(super) period_days: i64,
    pub(super) ticket_id: i64,
    pub(super) price: i64,
    pub(super) gacha_ticket_id: i64,
    pub(super) terms: BTreeMap<i64, i64>,
    pub(super) excluded: BTreeMap<i64, Vec<i64>>,
}

#[derive(Deserialize)]
pub(super) struct Gacha {
    pub(super) start_at: i64,
    pub(super) end_at: i64,
    pub(super) list_order: i64,
    pub(super) candidates: Vec<i64>,
}

static CATALOG: OnceLock<Result<Catalog, String>> = OnceLock::new();

pub(super) fn definitions() -> Result<&'static Catalog, PersonalServiceError> {
    CATALOG
        .get_or_init(|| {
            serde_json::from_str(include_str!("../../assets/cn-optional-exchange.json"))
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| {
            PersonalServiceError::new(format!("invalid CN optional exchange catalog: {error}"))
        })
}

// //// 解析固定卡池和档期对应的新手兑换资格 [@x380kkm 2026-09-23] ////
pub(super) fn resolve(
    root: &Map<String, Value>,
    family: &str,
    id: i64,
) -> Result<Option<Campaign>, PersonalServiceError> {
    let catalog = definitions()?;
    if family == "special" {
        return Ok(catalog.special.get(&id).cloned());
    }
    if family == "treasure" {
        return Ok(catalog.treasure.get(&id).cloned());
    }
    let Some(definition) = catalog.start_dash.get(&id) else {
        return Ok(None);
    };
    let Some(state) = super::state::campaign_state(root, family, id) else {
        return Ok(None);
    };
    let Some(gacha) = state
        .get("gacha_id")
        .and_then(Value::as_i64)
        .and_then(|id| catalog.gachas.get(&id))
    else {
        return Ok(None);
    };
    let term = state
        .get("term_index")
        .and_then(Value::as_i64)
        .unwrap_or(-1);
    if !definition.terms.contains_key(&term) {
        return Ok(None);
    }
    let excluded = definition.excluded.get(&term).cloned().unwrap_or_default();
    let Some(start_at) = state.get("period_start_time").and_then(Value::as_i64) else {
        return Ok(None);
    };
    let Some(end_at) = state.get("period_end_time").and_then(Value::as_i64) else {
        return Ok(None);
    };
    Ok(Some(Campaign {
        kind: 0,
        start_at,
        end_at,
        exchange_start_at: start_at,
        exchange_end_at: end_at,
        ticket_id: definition.ticket_id,
        candidates: gacha
            .candidates
            .iter()
            .copied()
            .filter(|id| !excluded.contains(id))
            .collect(),
        product_source: 0,
        price: definition.price,
        gacha_ticket_id: Some(definition.gacha_ticket_id),
        reward_count: 1,
    }))
}
// //// /解析固定卡池和档期对应的新手兑换资格 ////

// //// 将管理窗口的起止位移应用到购买和兑换期 [@x380kkm 2026-09-23] ////
pub(super) fn is_open(
    database: &ServiceDatabase,
    family: &str,
    id: i64,
    start: i64,
    end: i64,
    campaign_end: i64,
) -> Result<bool, PersonalServiceError> {
    use crate::database::{evaluate_activity_schedule, ActivityMode, ActivityWindowStatus};
    let activity_id = super::management::activity_id(family, id);
    let reward_delay_ms = end.saturating_sub(campaign_end).max(0).saturating_mul(1000);
    if database
        .activity_temporary_reward_window(&activity_id, reward_delay_ms)?
        .is_some()
    {
        return Ok(true);
    }
    if let Some(mut schedule) = database.get_activity_schedule(&activity_id)? {
        if !schedule.enabled {
            return Ok(false);
        }
        if matches!(schedule.mode, ActivityMode::Manual | ActivityMode::Always) {
            return Ok(true);
        }
        schedule.end_at_ms = schedule
            .end_at_ms
            .saturating_add(reward_delay_ms);
        return Ok(
            evaluate_activity_schedule(&schedule, database.current_server_time_millis()?).status
                == ActivityWindowStatus::Open,
        );
    }
    let now = database.current_server_time_seconds()?;
    Ok(start <= now && now <= end)
}
// //// /将管理窗口的起止位移应用到购买和兑换期 ////
