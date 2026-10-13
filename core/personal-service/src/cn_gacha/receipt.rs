// audience: internal
// # personal-service-cn-gacha-receipt
// 抽取和兑换的请求凭据使用 gacha 命名空间, 并随资产快照保存.

use crate::cn_request_receipt::RequestReceipt;
use crate::http::HttpResponse;
use crate::PersonalServiceError;
use serde_json::{Map, Value};

pub(crate) struct GachaReceipt(RequestReceipt);

impl GachaReceipt {
    // //// 在抽取命名空间内记录和重放请求结算 [@x380kkm 2026-09-23] ////
    pub(crate) fn new(action: &str, viewer_id: i64, api_count: i64, request: Value) -> Self {
        Self(RequestReceipt::new(
            "gacha", action, viewer_id, api_count, request,
        ))
    }

    pub(crate) fn replay(
        &self,
        root: &Map<String, Value>,
    ) -> Result<Option<HttpResponse>, PersonalServiceError> {
        self.0.replay(root)
    }

    pub(crate) fn record(
        self,
        root: &mut Map<String, Value>,
        response: &Value,
        response_time: i64,
    ) -> Result<String, PersonalServiceError> {
        self.0.record(root, response, response_time)
    }
    // //// /在抽取命名空间内记录和重放请求结算 ////
}
