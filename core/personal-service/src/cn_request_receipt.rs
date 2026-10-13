// audience: internal
// # personal-service-cn-request-receipt
// 请求参数, 响应和原响应时间随玩家资产一同持久化. 操作和 viewer 会话共同限定客户端请求计数.

use crate::cn::msgpack_response_at;
use crate::http::HttpResponse;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};

pub(crate) struct RequestReceipt {
    namespace: &'static str,
    key: String,
    viewer_id: i64,
    request: Value,
}

impl RequestReceipt {
    // //// 以操作和会话内请求计数标识一次结算 [@x380kkm 2026-09-23] ////
    pub(crate) fn new(
        namespace: &'static str,
        action: &str,
        viewer_id: i64,
        api_count: i64,
        request: Value,
    ) -> Self {
        Self {
            namespace,
            key: format!("{namespace}:{action}:{viewer_id}:{api_count}"),
            viewer_id,
            request,
        }
    }
    // //// /以操作和会话内请求计数标识一次结算 ////

    // //// 返回相同参数已提交的响应及其原时间 [@x380kkm 2026-09-23] ////
    pub(crate) fn replay(
        &self,
        root: &Map<String, Value>,
    ) -> Result<Option<HttpResponse>, PersonalServiceError> {
        let field = format!("{}_request_receipts", self.namespace);
        let Some(receipt) = root
            .get(&field)
            .and_then(Value::as_object)
            .and_then(|receipts| receipts.get(&self.key))
        else {
            return Ok(None);
        };
        if receipt.get("request") != Some(&self.request) {
            return Ok(Some(HttpResponse::json(
                "400 Bad Request",
                json!({"error":format!("{}_request_conflict", self.namespace)}).to_string(),
            )));
        }
        let response_time = receipt
            .get("response_time")
            .and_then(Value::as_i64)
            .ok_or_else(|| {
                PersonalServiceError::new(format!(
                    "stored CN {} response time is invalid",
                    self.namespace
                ))
            })?;
        let mut response = receipt.get("response").cloned().ok_or_else(|| {
            PersonalServiceError::new(format!("stored CN {} response is missing", self.namespace))
        })?;
        crate::cn_character::complete_character_response_times(&mut response, root, response_time);
        Ok(Some(msgpack_response_at(
            self.viewer_id,
            false,
            response_time,
            response,
        )?))
    }
    // //// /返回相同参数已提交的响应及其原时间 ////

    // //// 保存响应凭据并返回领取历史的结算标识 [@x380kkm 2026-09-23] ////
    pub(crate) fn record(
        self,
        root: &mut Map<String, Value>,
        response: &Value,
        response_time: i64,
    ) -> Result<String, PersonalServiceError> {
        let field = format!("{}_request_receipts", self.namespace);
        let receipts = root
            .entry(field)
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| {
                PersonalServiceError::new(format!(
                    "stored CN {} receipts are invalid",
                    self.namespace
                ))
            })?;
        receipts.insert(
            self.key.clone(),
            json!({
                "request":self.request,
                "response":response,
                "response_time":response_time,
            }),
        );
        Ok(self.key)
    }
    // //// /保存响应凭据并返回领取历史的结算标识 ////
}
