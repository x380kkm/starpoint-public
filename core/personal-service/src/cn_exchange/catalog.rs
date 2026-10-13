// audience: internal
// # personal-service-cn-exchange-catalog
// 兑换费用和窗口来自客户端主表. 星屑角色与装备按持有资格兑换, 羁绊证装备按独立次数限购.

use crate::cn_tutorial::require_object;
use crate::database::{parse_iso_timestamp, ServiceDatabase};
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

const CN_UTC_OFFSET_SECONDS: i64 = 8 * 60 * 60;
static CATALOG: OnceLock<Result<Catalog, String>> = OnceLock::new();

pub(super) struct RewardDefinition {
    pub(super) kind: i64,
    pub(super) target_id: i64,
    pub(super) history_kind: i64,
    owned_field: Option<&'static str>,
    max_count: Option<i64>,
}

pub(super) struct Offer {
    pub(super) id: i64,
    pub(super) currency: &'static str,
    pub(super) cost: i64,
    pub(super) reward: RewardDefinition,
    start_time: i64,
    end_time: i64,
    gacha_id: Option<i64>,
    stock: Option<(String, i64)>,
}

struct Catalog {
    star_crumb: BTreeMap<i64, Offer>,
    bond_token: BTreeMap<i64, Offer>,
}

impl Offer {
    pub(super) fn is_available(
        &self,
        database: &ServiceDatabase,
        now: i64,
    ) -> Result<bool, PersonalServiceError> {
        if let Some(gacha_id) = self.gacha_id {
            return crate::cn_gacha::exchange_campaign_is_available(database, gacha_id, now);
        }
        Ok(self.start_time <= now && now <= self.end_time)
    }

    pub(super) fn purchase_count(&self, root: &Map<String, Value>) -> i64 {
        self.stock
            .as_ref()
            .and_then(|(key, _)| {
                root.get("shop_purchase_counts")
                    .and_then(|counts| counts.get(key))
                    .and_then(Value::as_i64)
            })
            .unwrap_or_default()
            .max(0)
    }

    pub(super) fn eligibility_error(&self, root: &Map<String, Value>) -> Option<&'static str> {
        if self
            .stock
            .as_ref()
            .is_some_and(|(_, limit)| self.purchase_count(root) >= *limit)
        {
            return Some("shop_out_of_stock");
        }
        let key = self.reward.target_id.to_string();
        if self.reward.owned_field.is_some_and(|field| {
            root.get(field)
                .and_then(Value::as_object)
                .is_some_and(|owned| owned.contains_key(&key))
        }) {
            return Some("exchange_reward_owned");
        }
        if let Some(limit) = self.reward.max_count {
            let held = root
                .get("item_list")
                .and_then(|items| items.get(&key))
                .and_then(Value::as_i64)
                .unwrap_or_default();
            if held.checked_add(1).map_or(true, |count| count > limit) {
                return Some("exchange_item_limit");
            }
        }
        None
    }

    pub(super) fn record_purchase(
        &self,
        root: &mut Map<String, Value>,
    ) -> Result<(), PersonalServiceError> {
        if let Some((key, _)) = &self.stock {
            let next = self
                .purchase_count(root)
                .checked_add(1)
                .ok_or_else(|| PersonalServiceError::new("CN exchange count exceeds range"))?;
            root.entry("shop_purchase_counts")
                .or_insert_with(|| json!({}));
            require_object(root, "shop_purchase_counts")?.insert(key.clone(), json!(next));
        }
        Ok(())
    }
}

fn catalog() -> Result<&'static Catalog, PersonalServiceError> {
    CATALOG
        .get_or_init(|| load_catalog().map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

pub(super) fn star_crumb_offer(id: i64) -> Result<Option<&'static Offer>, PersonalServiceError> {
    Ok(catalog()?.star_crumb.get(&id))
}

pub(super) fn bond_token_offer(id: i64) -> Result<Option<&'static Offer>, PersonalServiceError> {
    Ok(catalog()?.bond_token.get(&id))
}

pub(super) fn bond_token_offers(
) -> Result<impl Iterator<Item = &'static Offer>, PersonalServiceError> {
    Ok(catalog()?.bond_token.values())
}

// //// 将两类兑换主表转换为统一费用和奖励规则 [@x380kkm 2026-09-23] ////
fn load_catalog() -> Result<Catalog, PersonalServiceError> {
    let stars: BTreeMap<i64, Vec<Vec<String>>> =
        decode(include_str!("../../../../assets/star_crumb_exchange.json"))?;
    let costs: BTreeMap<i64, Vec<Vec<String>>> = decode(include_str!(
        "../../../../assets/star_crumb_exchange_cost.json"
    ))?;
    let bonds: BTreeMap<i64, Vec<String>> =
        decode(include_str!("../../../../assets/bond_token_exchange.json"))?;
    let limits: BTreeMap<i64, i64> =
        decode(include_str!("../../assets/cn-exchange-item-limits.json"))?;
    let mut star_crumb = BTreeMap::new();
    for (id, rows) in stars {
        let row = rows
            .first()
            .ok_or_else(|| PersonalServiceError::new("CN exchange row is missing"))?;
        let kind = integer(row, 0)?;
        let target = integer(row, 1)?;
        let rarity = integer(row, 8)?;
        let cost_index = match rarity {
            4 => 0,
            5 => 1,
            _ => return Err(PersonalServiceError::new("CN exchange rarity is invalid")),
        };
        let cost_row = costs
            .get(&kind)
            .and_then(|rows| rows.first())
            .ok_or_else(|| PersonalServiceError::new("CN exchange costs are missing"))?;
        star_crumb.insert(
            id,
            Offer {
                id,
                currency: "star_crumb",
                cost: integer(cost_row, cost_index)?,
                reward: reward(kind, target, &limits)?,
                start_time: timestamp(field(row, 3)?)?,
                end_time: timestamp(field(row, 4)?)?,
                gacha_id: optional_integer(row, 6)?.or(optional_integer(row, 7)?),
                stock: None,
            },
        );
    }
    let mut bond_token = BTreeMap::new();
    for (id, row) in bonds {
        let mut reward = reward(2, id, &limits)?;
        reward.owned_field = None;
        bond_token.insert(
            id,
            Offer {
                id,
                currency: "bond_token",
                cost: integer(&row, 0)?,
                reward,
                start_time: timestamp(field(&row, 2)?)?,
                end_time: timestamp(field(&row, 3)?)?,
                gacha_id: None,
                stock: Some((format!("bond_token_exchange:{id}"), integer(&row, 1)?)),
            },
        );
    }
    Ok(Catalog {
        star_crumb,
        bond_token,
    })
}
// //// /将两类兑换主表转换为统一费用和奖励规则 ////

fn reward(
    kind: i64,
    target_id: i64,
    limits: &BTreeMap<i64, i64>,
) -> Result<RewardDefinition, PersonalServiceError> {
    let (kind, history_kind, owned_field, max_count) =
        match kind {
            0 => (2, 5, Some("user_character_list"), None),
            1 => (
                0,
                1,
                None,
                Some(*limits.get(&target_id).ok_or_else(|| {
                    PersonalServiceError::new("CN exchange item limit is missing")
                })?),
            ),
            2 => (1, 6, Some("user_equipment_list"), None),
            _ => {
                return Err(PersonalServiceError::new(
                    "CN exchange reward kind is invalid",
                ))
            }
        };
    Ok(RewardDefinition {
        kind,
        target_id,
        history_kind,
        owned_field,
        max_count,
    })
}

fn decode<T: serde::de::DeserializeOwned>(source: &str) -> Result<T, PersonalServiceError> {
    serde_json::from_str(source).map_err(|error| PersonalServiceError::new(error.to_string()))
}

fn field(row: &[String], index: usize) -> Result<&str, PersonalServiceError> {
    row.get(index)
        .map(String::as_str)
        .ok_or_else(|| PersonalServiceError::new("CN exchange field is missing"))
}

fn integer(row: &[String], index: usize) -> Result<i64, PersonalServiceError> {
    field(row, index)?
        .parse::<i64>()
        .ok()
        .filter(|value| *value >= 0)
        .ok_or_else(|| PersonalServiceError::new("CN exchange integer is invalid"))
}

fn optional_integer(row: &[String], index: usize) -> Result<Option<i64>, PersonalServiceError> {
    if field(row, index)? == "(None)" {
        Ok(None)
    } else {
        integer(row, index).map(Some)
    }
}

fn timestamp(value: &str) -> Result<i64, PersonalServiceError> {
    parse_iso_timestamp(&format!("{}.000Z", value.replace(' ', "T")))
        .and_then(|millis| millis.checked_div(1000))
        .and_then(|seconds| seconds.checked_sub(CN_UTC_OFFSET_SECONDS))
        .ok_or_else(|| PersonalServiceError::new("CN exchange time is invalid"))
}
