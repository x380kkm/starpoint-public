// audience: internal
// # personal-service-cn-reference-state-common
//
// 该模块提供参考 CN 状态路由共享的 JSON 资产和玩家数值操作.

use crate::cn_tutorial::require_object;
use crate::http::HttpResponse;
use crate::PersonalServiceError;
use serde_json::{Map, Value};
use std::sync::OnceLock;

pub(super) fn json_document<'a>(
    storage: &'a OnceLock<Result<Value, String>>,
    source: &str,
    label: &str,
) -> Result<&'a Value, PersonalServiceError> {
    storage
        .get_or_init(|| {
            serde_json::from_str(source)
                .map_err(|error| format!("failed to decode CN {label}: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

pub(super) fn add_item(
    root: &mut Map<String, Value>,
    item_id: i64,
    amount: i64,
) -> Result<i64, PersonalServiceError> {
    let items = require_object(root, "item_list")?;
    let key = item_id.to_string();
    let current = items.get(&key).and_then(Value::as_i64).unwrap_or_default();
    let updated = current
        .checked_add(amount)
        .ok_or_else(|| PersonalServiceError::new("CN item count exceeds the supported range"))?;
    items.insert(key, Value::from(updated));
    Ok(updated)
}

pub(super) fn add_user_info(
    root: &mut Map<String, Value>,
    key: &str,
    amount: i64,
) -> Result<(), PersonalServiceError> {
    let user_info = require_object(root, "user_info")?;
    let current = required_i64(user_info, key)?;
    let updated = current.checked_add(amount).ok_or_else(|| {
        PersonalServiceError::new(format!("CN user info {key} exceeds the supported range"))
    })?;
    user_info.insert(key.to_owned(), Value::from(updated));
    Ok(())
}

pub(super) fn required_i64(
    object: &Map<String, Value>,
    key: &str,
) -> Result<i64, PersonalServiceError> {
    object
        .get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new(format!("stored CN {key} is missing")))
}

pub(super) fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}
