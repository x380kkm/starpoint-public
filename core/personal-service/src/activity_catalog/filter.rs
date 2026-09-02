// audience: internal
// # personal-service-activity-catalog-filter
//
// 该模块解析活动目录查询参数, 并按搜索词、类型、状态、收藏和 UTC 时间范围筛选条目.

use super::ActivityCatalogEntry;
use crate::database::{activity_schedule_overlaps_range, parse_iso_timestamp};
use crate::http::HttpRequest;

#[derive(Default)]
pub(super) struct CatalogFilter {
    query: Option<String>,
    kind: Option<String>,
    status: Option<String>,
    favorite: Option<bool>,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
}

// //// 解析活动目录查询参数 [@x380kkm 2026-08-19] ////
pub(super) fn parse(request: &HttpRequest) -> Option<CatalogFilter> {
    let Some((_, query)) = request.target().split_once('?') else {
        return Some(CatalogFilter::default());
    };
    let mut filter = CatalogFilter::default();
    for (name, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match name.as_ref() {
            "q" => {
                let value = value.trim().to_lowercase();
                if !value.is_empty() {
                    filter.query = Some(value);
                }
            }
            "kind" => {
                let value = value.trim().to_owned();
                if !value.is_empty() {
                    filter.kind = Some(value);
                }
            }
            "status" if is_known_status(&value) => filter.status = Some(value.into_owned()),
            "favorite" => {
                filter.favorite = Some(match value.as_ref() {
                    "true" | "1" => true,
                    "false" | "0" => false,
                    _ => return None,
                });
            }
            "from" => filter.from_ms = parse_timestamp(&value)?,
            "to" => filter.to_ms = parse_timestamp(&value)?,
            _ => return None,
        }
    }
    if filter
        .from_ms
        .zip(filter.to_ms)
        .is_some_and(|(from, to)| from > to)
    {
        return None;
    }
    Some(filter)
}

pub(super) fn matches(entry: &ActivityCatalogEntry, filter: &CatalogFilter) -> bool {
    if filter
        .favorite
        .is_some_and(|favorite| favorite != entry.favorite)
        || filter
            .kind
            .as_ref()
            .is_some_and(|kind| !entry.kind.eq_ignore_ascii_case(kind))
        || filter
            .status
            .as_ref()
            .is_some_and(|status| entry.status != status)
    {
        return false;
    }
    if (filter.from_ms.is_some() || filter.to_ms.is_some())
        && !entry_overlaps_range(entry, filter.from_ms, filter.to_ms)
    {
        return false;
    }
    let Some(query) = &filter.query else {
        return true;
    };
    entry.activity_id.to_lowercase().contains(query)
        || entry.name.to_lowercase().contains(query)
        || entry.kind.to_lowercase().contains(query)
        || entry.description.to_lowercase().contains(query)
        || entry
            .tags
            .iter()
            .any(|tag| tag.to_lowercase().contains(query))
}
// //// /解析活动目录查询参数 ////

fn entry_overlaps_range(
    entry: &ActivityCatalogEntry,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
) -> bool {
    if crate::activity_projection::is_permanent_activity_id(&entry.activity_id)
        && entry.rule.is_none()
    {
        return true;
    }
    if let Some(rule) = &entry.rule {
        return activity_schedule_overlaps_range(rule, from_ms, to_ms);
    }
    let Some((start_at_ms, end_at_ms)) = entry.default_start_at_ms.zip(entry.default_end_at_ms)
    else {
        return false;
    };
    let range_start = from_ms.unwrap_or(0);
    let range_end = to_ms.unwrap_or(i64::MAX);
    start_at_ms <= range_end && end_at_ms > range_start
}

fn parse_timestamp(value: &str) -> Option<Option<i64>> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Ok(milliseconds) = value.parse::<i64>() {
        return (milliseconds >= 0).then_some(Some(milliseconds));
    }
    let timestamp = if value.len() == 10 {
        format!("{value}T00:00:00.000Z")
    } else {
        value.to_owned()
    };
    parse_iso_timestamp(&timestamp).map(Some)
}

fn is_known_status(value: &str) -> bool {
    matches!(
        value,
        "unscheduled" | "disabled" | "not_started" | "open" | "ended"
    )
}
