// audience: internal
// # personal-service-event-currency-display
// 活动主表的显示币沿用本期商店和奖励的道具身份, 其余主表字段保持原值.

use super::OrderedValue;
use crate::PersonalServiceError;
use std::collections::BTreeMap;
use std::sync::OnceLock;

pub(super) const CATALOG: &str = include_str!("../../assets/cn-event-currency-display.json");
type Catalog = BTreeMap<String, BTreeMap<String, BTreeMap<usize, i64>>>;

// //// 读取活动显示币字段映射 [@x380kkm 2026-10-02] ////
fn catalog() -> Result<&'static Catalog, PersonalServiceError> {
    static VALUE: OnceLock<Result<Catalog, String>> = OnceLock::new();
    VALUE
        .get_or_init(|| serde_json::from_str(CATALOG).map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| {
            PersonalServiceError::new(format!("invalid CN event currency display: {error}"))
        })
}

// //// 返回显示币投影所需的主表 [@x380kkm 2026-10-02] ////
pub(super) fn master_names() -> Result<impl Iterator<Item = &'static String>, PersonalServiceError>
{
    Ok(catalog()?.keys())
}

// //// 将本期兑换币写入活动显示字段 [@x380kkm 2026-10-02] ////
pub(super) fn project(value: &mut OrderedValue, name: &str) -> Result<(), PersonalServiceError> {
    let Some(events) = catalog()?.get(name) else {
        return Ok(());
    };
    let OrderedValue::Map(rows) = value else {
        return Err(PersonalServiceError::new(
            "CN currency display master is not an event map",
        ));
    };
    for (event_id, fields) in events {
        let Some((_, OrderedValue::Row(row))) = rows.iter_mut().find(|(id, _)| id == event_id)
        else {
            return Err(PersonalServiceError::new(format!(
                "CN currency display event is missing: {name}/{event_id}"
            )));
        };
        for (index, item_id) in fields {
            let field = row.get_mut(*index).ok_or_else(|| {
                PersonalServiceError::new(format!(
                    "CN currency display field is missing: {name}/{event_id}/{index}"
                ))
            })?;
            *field = item_id.to_string();
        }
    }
    Ok(())
}
// //// /将本期兑换币写入活动显示字段 ////

#[cfg(test)]
mod tests;
