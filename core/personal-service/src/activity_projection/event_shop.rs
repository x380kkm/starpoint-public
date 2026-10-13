// audience: internal
// # activity-projection-event-shop
// 活动商品复用 CN 商品身份与奖励, 区域主表校准日期和商店归属, 客户端与兑换共用有效窗口.

use super::{
    active_windows, decode_ordered_map, master_seed, parse_master_time, projection_manifest,
    quest_windows::QuestTimes, OrderedValue, ProjectionMaster, ProjectionWindow,
    MASTER_TIMEZONE_OFFSET_MS,
};
use crate::cn_shop::{event_activity_id, event_shop_activity_alias, selection_activity_id};
use crate::cn_tutorial::format_client_time;
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

pub(crate) type ItemPeriods = BTreeMap<i64, (i64, i64)>;

struct SourcePeriod {
    start: i64,
    end: Option<i64>,
}

struct ShopItem {
    activity_id: String,
    campaign_id: Option<String>,
    period: SourcePeriod,
}

struct Catalog {
    items: BTreeMap<i64, ShopItem>,
    events: BTreeMap<String, SourcePeriod>,
    canonical_periods: BTreeMap<i64, [i64; 2]>,
}

// //// 读取商品及父活动并校准本地商品日期 [@x380kkm 2026-09-29] ////
fn catalog() -> Result<&'static Catalog, PersonalServiceError> {
    static VALUE: OnceLock<Result<Catalog, String>> = OnceLock::new();
    VALUE
        .get_or_init(|| read_catalog().map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

fn read_catalog() -> Result<Catalog, PersonalServiceError> {
    let mut items = BTreeMap::new();
    let canonical_periods = super::regional::canonical_item_periods()?;
    let value = decode_ordered_map(master_seed("event_item_shop").expect("event shop seed exists"))
        .map_err(PersonalServiceError::new)?;
    if let OrderedValue::Map(entries) = value {
        for (id, value) in entries {
            let OrderedValue::Row(row) = value else {
                continue;
            };
            let identity = row
                .get(2)
                .and_then(|kind| kind.parse().ok())
                .zip(row.get(1).and_then(|id| id.parse().ok()));
            let Some((activity_id, mut period)) = identity
                .and_then(|(kind, id)| event_activity_id(kind, id))
                .zip(source_period(&row, 26, 27))
            else {
                continue;
            };
            let id = id
                .parse()
                .map_err(|_| PersonalServiceError::new("invalid event shop item id"))?;
            if let Some([start, end]) = canonical_periods.get(&id) {
                period = SourcePeriod {
                    start: *start,
                    end: Some(*end),
                };
            }
            let campaign_id = selection_activity_id(4, id)?.map(str::to_owned);
            items.insert(
                id,
                ShopItem {
                    activity_id,
                    campaign_id,
                    period,
                },
            );
        }
    }
    let activity_ids = items
        .values()
        .flat_map(|item| [Some(item.activity_id.as_str()), item.campaign_id.as_deref()])
        .flatten()
        .collect::<BTreeSet<_>>();
    let mut events = BTreeMap::new();
    for master in &projection_manifest()?.masters {
        if master.quest_category.is_some() || master.activity_id_index.is_some() {
            continue;
        }
        let Some((start, end)) = master.start_index.zip(master.end_index) else {
            continue;
        };
        if !activity_ids
            .iter()
            .any(|id| id.starts_with(&master.activity_id_prefix))
        {
            continue;
        }
        let value = decode_ordered_map(master_seed(&master.name).expect("projection seed exists"))
            .map_err(PersonalServiceError::new)?;
        let OrderedValue::Map(entries) = value else {
            continue;
        };
        for (id, value) in entries {
            let activity_id = format!("{}{id}", master.activity_id_prefix);
            let OrderedValue::Row(row) = value else {
                continue;
            };
            if let Some(period) = source_period(&row, start, end) {
                if activity_ids.contains(activity_id.as_str()) {
                    events.insert(activity_id, period);
                }
            }
        }
    }
    Ok(Catalog {
        items,
        events,
        canonical_periods,
    })
}

fn source_period(row: &[String], start: usize, end: usize) -> Option<SourcePeriod> {
    Some(SourcePeriod {
        start: parse_master_time(row.get(start)?)?,
        end: row.get(end).and_then(|value| parse_master_time(value)),
    })
}

// //// /读取商品及父活动并校准本地商品日期 ////

pub(super) fn matches_master(master: &ProjectionMaster, activity_id: &str) -> bool {
    master.name == "event_item_shop"
        && activity_id.split_once(':').is_some_and(|(kind, _)| {
            matches!(
                kind,
                "advent"
                    | "story"
                    | "challenge-dungeon"
                    | "world-story"
                    | "collect-item"
                    | "carnival"
                    | "rush"
                    | "score-attack"
                    | "event-shop-select-item-campaign"
            )
        })
}

// //// 将商品相对日期限制在管理活动的兑换期内 [@x380kkm 2026-09-26] ////
fn item_period(
    item_id: i64,
    item: &ShopItem,
    source: &SourcePeriod,
    window: &ProjectionWindow,
) -> (i64, i64) {
    if let Some(period) = window.regional_period {
        if let Some([start, end]) = period.items.get(&item_id) {
            return (*start, *end);
        }
    }
    let reward_end = window.end_at_ms.saturating_add(window.reward_delay_ms);
    if matches!(window.quest_times, QuestTimes::FirstRelease) {
        let start = if item.period.start == source.start {
            window.start_at_ms
        } else {
            item.period.start
        };
        return (start, item.period.end.unwrap_or(reward_end));
    }
    if !matches!(window.quest_times, QuestTimes::Relative) {
        return (window.start_at_ms, reward_end);
    }
    let start = window
        .start_at_ms
        .saturating_add(item.period.start.saturating_sub(source.start).max(0));
    let end = match item.period.end {
        Some(end) if source.end.is_some_and(|event_end| end >= event_end) => window
            .end_at_ms
            .saturating_add(end.saturating_sub(source.end.unwrap()).max(0)),
        Some(end) => window
            .start_at_ms
            .saturating_add(end.saturating_sub(source.start).max(0)),
        None => reward_end,
    };
    (start.max(window.start_at_ms), end.min(reward_end))
}

fn managed_window<'a>(
    windows: &'a BTreeMap<String, ProjectionWindow>,
    activity_id: &str,
) -> Result<Option<&'a ProjectionWindow>, PersonalServiceError> {
    let alias = event_shop_activity_alias(activity_id)?;
    Ok(windows
        .get(activity_id)
        .or_else(|| alias.and_then(|alias| windows.get(&alias))))
}
// //// /将商品相对日期限制在管理活动的兑换期内 ////

// //// 为商品合并父活动与独立自选活动的时间约束 [@x380kkm 2026-09-29] ////
fn projected_item_period(
    id: i64,
    item: &ShopItem,
    catalog: &Catalog,
    windows: &BTreeMap<String, ProjectionWindow>,
) -> Result<Option<(i64, i64)>, PersonalServiceError> {
    let parent = managed_window(windows, &item.activity_id)?;
    let campaign = item
        .campaign_id
        .as_ref()
        .and_then(|id| windows.get(id).zip(catalog.events.get(id)));
    let activity = if item.campaign_id.is_some() {
        campaign
    } else {
        parent.zip(catalog.events.get(&item.activity_id))
    };
    let (mut start, mut end) = match activity {
        Some((window, source)) => item_period(id, item, source, window),
        None if parent.is_some() || catalog.canonical_periods.contains_key(&id) => {
            (item.period.start, item.period.end.unwrap_or(i64::MAX))
        }
        None => return Ok(None),
    };
    if let Some(parent) = parent {
        start = start.max(parent.start_at_ms);
        end = end.min(parent.end_at_ms.saturating_add(parent.reward_delay_ms));
    }
    Ok(Some((start, end)))
}
// //// /为商品合并父活动与独立自选活动的时间约束 ////

// //// 返回管理活动对应的商品窗口 [@x380kkm 2026-09-26] ////
pub(crate) fn managed_item_periods(
    database: &ServiceDatabase,
    activity_id: &str,
) -> Result<Option<ItemPeriods>, PersonalServiceError> {
    let windows = active_windows(database)?;
    let catalog = catalog()?;
    let alias = event_shop_activity_alias(activity_id)?;
    let mut periods = BTreeMap::new();
    let mut projected = false;
    for (id, item) in &catalog.items {
        if item.activity_id == activity_id || Some(&item.activity_id) == alias.as_ref() {
            let period = projected_item_period(*id, item, catalog, &windows)?;
            projected |= period.is_some();
            periods.insert(
                *id,
                period.unwrap_or((item.period.start, item.period.end.unwrap_or(i64::MAX))),
            );
        }
    }
    Ok(projected.then_some(periods))
}
// //// /返回管理活动对应的商品窗口 ////

// //// 投影关联商品并保留其成本和奖励字段 [@x380kkm 2026-09-26] ////
pub(super) fn project(
    value: &mut OrderedValue,
    windows: &BTreeMap<String, ProjectionWindow>,
) -> Result<(), PersonalServiceError> {
    let catalog = catalog()?;
    let OrderedValue::Map(entries) = value else {
        return Ok(());
    };
    for (id, value) in entries {
        let Some((id, item)) = id
            .parse()
            .ok()
            .and_then(|id| catalog.items.get(&id).map(|item| (id, item)))
        else {
            continue;
        };
        let Some((start, end)) = projected_item_period(id, item, catalog, windows)? else {
            continue;
        };
        let OrderedValue::Row(row) = value else {
            continue;
        };
        if let Some(alias) = super::regional::regional_shop_activity_alias(&item.activity_id)? {
            if windows.contains_key(&alias) && !windows.contains_key(&item.activity_id) {
                if let Some((_, alias_id)) = alias.split_once(':') {
                    row[1] = alias_id.to_owned();
                }
            }
        }
        row[26] = format_client_time((start + MASTER_TIMEZONE_OFFSET_MS) / 1_000);
        row[27] = format_client_time((end + MASTER_TIMEZONE_OFFSET_MS) / 1_000);
    }
    Ok(())
}
// //// /投影关联商品并保留其成本和奖励字段 ////

#[cfg(test)]
mod tests;
