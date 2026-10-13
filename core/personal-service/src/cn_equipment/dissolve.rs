// audience: internal
// # personal-service-cn-equipment-dissolve
// 整件出售消耗最后一件装备, 副本分解保留本体. 请求凭据随奖励和装备库存一起保存.

pub(super) mod catalog;
mod settlement;
mod validation;

use super::error_response;
use crate::cn::decode_request;
use crate::cn_request_receipt::RequestReceipt;
use crate::cn_tutorial::{decode_player_data, player_snapshot, require_root};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub(super) use catalog::max_level;
use catalog::EquipmentDefinition;

struct Sale {
    equipment_id: i64,
    remaining_stack: Option<i64>,
    count: i64,
}

#[derive(Deserialize)]
struct SellRequest {
    viewer_id: i64,
    api_count: Option<i64>,
    equipment_list: Vec<SellItem>,
}

#[derive(Deserialize, Serialize)]
struct SellItem {
    equipment_id: i64,
    number: Option<i64>,
}

#[derive(Deserialize)]
struct BulkSellRequest {
    viewer_id: i64,
    api_count: Option<i64>,
    equipment_ids: Vec<i64>,
}

// //// 按整件与副本规则分派装备出售 [@x380kkm 2026-09-23] ////
pub(super) fn sell_equipment(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    sell(request, database, "sell-equipment", validation::whole)
}

pub(super) fn sell_stack(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    sell(request, database, "sell-stack", validation::stack)
}

type SalePlanner = fn(
    &Map<String, Value>,
    &BTreeMap<i64, EquipmentDefinition>,
    &[SellItem],
) -> Result<Vec<Sale>, &'static str>;

fn sell(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    action: &str,
    plan: SalePlanner,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<SellRequest>(request) {
        Ok(body) if valid_request(body.viewer_id, body.api_count, body.equipment_list.len()) => {
            body
        }
        _ => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let parameters = json!({"equipment_list": body.equipment_list});
    execute(
        database,
        body.viewer_id,
        body.api_count,
        action,
        parameters,
        |root, definitions| plan(root, definitions, &body.equipment_list),
    )
}
// //// /按整件与副本规则分派装备出售 ////

// //// 批量分解可出售装备的全部副本 [@x380kkm 2026-09-23] ////
pub(super) fn bulk_sell_stack(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<BulkSellRequest>(request) {
        Ok(body) if valid_request(body.viewer_id, body.api_count, body.equipment_ids.len()) => body,
        _ => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let parameters = json!({"equipment_ids": body.equipment_ids});
    execute(
        database,
        body.viewer_id,
        body.api_count,
        "bulk-sell-stack",
        parameters,
        |root, definitions| validation::bulk(root, definitions, &body.equipment_ids),
    )
}
// //// /批量分解可出售装备的全部副本 ////

fn valid_request(viewer_id: i64, api_count: Option<i64>, count: usize) -> bool {
    viewer_id > 0 && api_count.map_or(true, |count| count >= 0) && (1..=100).contains(&count)
}

// //// 先重放已提交请求再按当前库存规划分解 [@x380kkm 2026-09-23] ////
fn execute(
    database: &mut ServiceDatabase,
    viewer_id: i64,
    api_count: Option<i64>,
    action: &str,
    parameters: Value,
    plan: impl FnOnce(
        &Map<String, Value>,
        &BTreeMap<i64, EquipmentDefinition>,
    ) -> Result<Vec<Sale>, &'static str>,
) -> Result<HttpResponse, PersonalServiceError> {
    let snapshot = match player_snapshot(database, viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let receipt = api_count
        .map(|count| RequestReceipt::new("equipment", action, viewer_id, count, parameters));
    if let Some(response) = receipt
        .as_ref()
        .map(|receipt| receipt.replay(root))
        .transpose()?
        .flatten()
    {
        return Ok(response);
    }
    let definitions = catalog::equipment_definitions()?;
    let sales = match plan(root, definitions) {
        Ok(sales) => sales,
        Err(code) => return Ok(error_response("400 Bad Request", code)),
    };
    settlement::settle(
        database,
        snapshot.account_id,
        viewer_id,
        &mut player_data,
        &sales,
        definitions,
        receipt,
    )
}
// //// /先重放已提交请求再按当前库存规划分解 ////

// //// 返回装备觉醒产生的能力魂珠 [@x380kkm 2026-09-23] ////
pub(super) fn ability_soul_rewards(
    equipment_id: i64,
    count: i64,
) -> Result<BTreeMap<i64, i64>, PersonalServiceError> {
    let definition = catalog::equipment_definitions()?.get(&equipment_id);
    Ok(definition
        .filter(|definition| definition.generate_ability_soul && count > 0)
        .map(|definition| BTreeMap::from([(definition.ability_soul_id, count)]))
        .unwrap_or_default())
}
// //// /返回装备觉醒产生的能力魂珠 ////
