// audience: internal
// # personal-service-cn-shop-receipt
// 商店请求收据与资产在同一快照提交, viewer 会话和活动参与轮次共同隔离请求计数.

use super::catalog::{
    event_activity_id, event_shop_activity_alias, event_shop_identity, SHOP_TYPE_EVENT,
};
use super::purchase::ShopRewards;
use crate::cn::msgpack_response_at;
use crate::database::ServiceDatabase;
use crate::http::HttpResponse;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

const RECEIPTS_FIELD: &str = "shop_request_receipts";

// //// 以保留的累计购买数量标识资产提交 [@x380kkm 2026-09-21] ////
pub(super) fn total_purchased(root: &Map<String, Value>) -> Result<i64, PersonalServiceError> {
    root.get("shop_purchase_counts")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|counts| counts.values())
        .filter_map(Value::as_i64)
        .try_fold(0_i64, |total, count| {
            total.checked_add(count.max(0)).ok_or_else(|| {
                PersonalServiceError::new("CN total shop purchase count exceeds range")
            })
        })
}
// //// /以保留的累计购买数量标识资产提交 ////

pub(super) struct PurchaseReceipt {
    key: Option<String>,
    action: &'static str,
    viewer_id: i64,
    request: Value,
    activity_rounds: BTreeMap<String, i64>,
}

impl PurchaseReceipt {
    // //// 按会话、请求计数和活动轮次标识商店操作 [@x380kkm 2026-09-21] ////
    pub(super) fn new(
        root: &Map<String, Value>,
        database: &ServiceDatabase,
        action: &'static str,
        viewer_id: i64,
        api_count: Option<i64>,
        shop_type: i64,
        items: &BTreeMap<i64, i64>,
    ) -> Result<Self, PersonalServiceError> {
        let mut activity_rounds = BTreeMap::new();
        if shop_type == SHOP_TYPE_EVENT {
            for item_id in items.keys() {
                if let Some(activity_id) =
                    super::selection::product_activity_id(shop_type, *item_id)?
                {
                    activity_rounds.insert(
                        activity_id.to_owned(),
                        crate::activity_history::participation_round(root, activity_id),
                    );
                }
                if let Some((kind, id)) = event_shop_identity(*item_id)? {
                    if let Some(activity_id) = event_activity_id(kind, id) {
                        let activity_id =
                            super::activity::event_shop_activity_state(database, &activity_id)?
                                .activity_id;
                        let round =
                            crate::activity_history::participation_round(root, &activity_id);
                        activity_rounds.insert(activity_id, round);
                    }
                }
            }
        }
        let mut request_rounds = activity_rounds.clone();
        for activity_id in activity_rounds.keys() {
            if let Some(alias) = event_shop_activity_alias(activity_id)? {
                request_rounds.insert(
                    alias.clone(),
                    crate::activity_history::participation_round(root, &alias),
                );
            }
        }
        Ok(Self {
            key: api_count.map(|count| format!("{action}:{viewer_id}:{count}:{request_rounds:?}")),
            action,
            viewer_id,
            request: json!({"shop_type": shop_type, "items": items}),
            activity_rounds,
        })
    }
    // //// /按会话、请求计数和活动轮次标识商店操作 ////

    // //// 返回同一商店请求的已提交响应 [@x380kkm 2026-09-21] ////
    pub(super) fn replay(
        &self,
        root: &Map<String, Value>,
    ) -> Result<Option<HttpResponse>, PersonalServiceError> {
        let Some(receipt) = self.key.as_ref().and_then(|key| {
            root.get(RECEIPTS_FIELD)
                .and_then(Value::as_object)
                .and_then(|receipts| receipts.get(key))
        }) else {
            return Ok(None);
        };
        if receipt.get("request") != Some(&self.request) {
            return Ok(Some(super::error_response(
                "400 Bad Request",
                "shop_request_conflict",
            )));
        }
        let response_time = receipt
            .get("response_time")
            .and_then(Value::as_i64)
            .ok_or_else(|| PersonalServiceError::new("stored CN shop response time is invalid"))?;
        let mut response = receipt
            .get("response")
            .cloned()
            .ok_or_else(|| PersonalServiceError::new("stored CN shop response is missing"))?;
        crate::cn_character::complete_character_response_times(&mut response, root, response_time);
        Ok(Some(msgpack_response_at(
            self.viewer_id,
            false,
            response_time,
            response,
        )?))
    }
    // //// /返回同一商店请求的已提交响应 ////

    // //// 记录商品、费用、实发奖励和客户端响应 [@x380kkm 2026-09-21] ////
    pub(super) fn record(
        self,
        root: &mut Map<String, Value>,
        response: &Value,
        rewards: &ShopRewards,
        response_time: i64,
    ) -> Result<(), PersonalServiceError> {
        let key = match self.key {
            Some(key) => key,
            None => format!(
                "{}:{}:total:{}",
                self.action,
                self.viewer_id,
                total_purchased(root)?
            ),
        };
        let reward_entries = rewards.history_entries.iter().map(|reward| json!({"kind": reward.kind, "type_id": reward.type_id, "number": reward.number})).collect::<Vec<_>>();
        let receipts = root
            .entry(RECEIPTS_FIELD.to_owned())
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN shop receipts are invalid"))?;
        receipts.insert(
            key,
            json!({
                "request": self.request,
                "activity_ids": self.activity_rounds.keys().collect::<Vec<_>>(),
                "participation_rounds": self.activity_rounds,
                "costs": rewards.cost_entries,
                "rewards": reward_entries,
                "pass_card_points": rewards.pass_card_points,
                "equipment_list": rewards.equipment_list,
                "response_time": response_time,
                "response": response,
            }),
        );
        Ok(())
    }
    // //// /记录商品、费用、实发奖励和客户端响应 ////
}
