// audience: internal
// # personal-service-cn-shop-selection
// 自选商店按账号, 商店类型和活动保存选择, 商品目录与购买使用同一选择条件.

mod catalog;

use self::catalog::{catalog, Campaign};
use crate::cn::{
    decode_request, msgpack_response_at, msgpack_result_code_response_at, server_time,
};
use crate::cn_tutorial::{decode_player_data, encode_player_data, player_snapshot, require_root};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};

const SELECTIONS: &str = "shop_campaign_lineups";
const LEGACY_SELECTION: &str = "shop_campaign_lineup_id";
pub(super) const UNAVAILABLE_ERROR: &str = "shop_lineup_unavailable";

#[derive(Deserialize)]
struct SelectionRequest {
    viewer_id: i64,
    shop_type: i64,
    campaign_id: i64,
    lineup_id: Option<i64>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Availability {
    Ordinary,
    Available,
    Unselected,
    Unavailable,
}

fn selected_lineup(root: &Map<String, Value>, key: &str, campaign: &Campaign) -> Option<i64> {
    root.get(SELECTIONS)
        .and_then(|selections| selections.get(key))
        .and_then(Value::as_i64)
        .or_else(|| root.get(LEGACY_SELECTION).and_then(Value::as_i64))
        .filter(|id| campaign.lineups.contains(id))
}

// //// 返回具体活动已保存的有效选项 [@x380kkm 2026-09-23] ////
pub(super) fn get(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let now = server_time(database)?;
    let body = match decode_request::<SelectionRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.campaign_id > 0 => body,
        _ => return msgpack_result_code_response_at(0, now, 20001),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let key = format!("{}:{}", body.shop_type, body.campaign_id);
    let Some(campaign) = catalog()?.campaigns.get(&key) else {
        return msgpack_result_code_response_at(body.viewer_id, now, 1652);
    };
    if !campaign.is_available(database, now)? {
        return msgpack_result_code_response_at(body.viewer_id, now, 1652);
    }
    let mut data = decode_player_data(&snapshot.data)?;
    let lineup_id = selected_lineup(require_root(&mut data)?, &key, campaign);
    msgpack_response_at(body.viewer_id, false, now, json!({"lineup_id":lineup_id}))
}

// //// 保存一次活动选择并重放相同选择 [@x380kkm 2026-09-23] ////
pub(super) fn set(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let now = server_time(database)?;
    let body = match decode_request::<SelectionRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.campaign_id > 0
                && body.lineup_id.is_some_and(|id| id > 0) =>
        {
            body
        }
        _ => return msgpack_result_code_response_at(0, now, 20001),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let key = format!("{}:{}", body.shop_type, body.campaign_id);
    let Some(campaign) = catalog()?.campaigns.get(&key) else {
        return msgpack_result_code_response_at(body.viewer_id, now, 1652);
    };
    if !campaign.is_available(database, now)? {
        return msgpack_result_code_response_at(body.viewer_id, now, 1652);
    }
    let lineup_id = body.lineup_id.expect("selection request contains a lineup");
    if !campaign.lineups.contains(&lineup_id) {
        return msgpack_result_code_response_at(body.viewer_id, now, 20001);
    }
    let mut data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut data)?;
    if let Some(selected) = selected_lineup(root, &key, campaign) {
        return if selected == lineup_id {
            msgpack_response_at(body.viewer_id, false, now, json!({}))
        } else {
            msgpack_result_code_response_at(body.viewer_id, now, 20001)
        };
    }
    root.entry(SELECTIONS.to_owned())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN shop selections are invalid"))?
        .insert(key, json!(lineup_id));
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&data)?)?;
    msgpack_response_at(body.viewer_id, false, now, json!({}))
}

// //// 将商品映射到开放活动和已选栏位 [@x380kkm 2026-09-23] ////
pub(super) fn availability(
    root: &Map<String, Value>,
    database: &ServiceDatabase,
    shop_type: i64,
    item_id: i64,
    now: i64,
) -> Result<Availability, PersonalServiceError> {
    let definitions = catalog()?;
    let Some(product) = definitions.products.get(&format!("{shop_type}:{item_id}")) else {
        return Ok(Availability::Ordinary);
    };
    let campaign = definitions
        .campaigns
        .get(&product.campaign)
        .ok_or_else(|| PersonalServiceError::new("CN selection shop campaign is missing"))?;
    if !campaign.is_available(database, now)? {
        return Ok(Availability::Unavailable);
    }
    let Some(required) = product.lineup_id else {
        return Ok(Availability::Available);
    };
    Ok(match selected_lineup(root, &product.campaign, campaign) {
        Some(selected) if selected == required => Availability::Available,
        Some(_) => Availability::Unavailable,
        None => Availability::Unselected,
    })
}

pub(crate) fn product_activity_id(
    shop_type: i64,
    item_id: i64,
) -> Result<Option<&'static str>, PersonalServiceError> {
    let definitions = catalog()?;
    Ok(definitions
        .products
        .get(&format!("{shop_type}:{item_id}"))
        .and_then(|product| definitions.campaigns.get(&product.campaign))
        .map(|campaign| campaign.activity_id.as_str()))
}

pub(super) fn activity_item_ids(activity_id: &str) -> Result<Vec<i64>, PersonalServiceError> {
    let definitions = catalog()?;
    Ok(definitions
        .products
        .iter()
        .filter(|(_, product)| {
            definitions
                .campaigns
                .get(&product.campaign)
                .is_some_and(|campaign| campaign.activity_id == activity_id)
        })
        .filter_map(|(key, _)| key.strip_prefix("4:")?.parse().ok())
        .collect())
}

// //// 清除显式重开活动的选择并保留领取历史 [@x380kkm 2026-09-23] ////
pub(super) fn reset(
    root: &mut Map<String, Value>,
    activity_id: &str,
) -> Result<(), PersonalServiceError> {
    for (key, campaign) in &catalog()?.campaigns {
        if campaign.activity_id != activity_id && campaign.event_activity_id()? != activity_id {
            continue;
        }
        if let Some(selections) = root.get_mut(SELECTIONS).and_then(Value::as_object_mut) {
            selections.remove(key);
        }
        if root
            .get(LEGACY_SELECTION)
            .and_then(Value::as_i64)
            .is_some_and(|id| campaign.lineups.contains(&id))
        {
            root.remove(LEGACY_SELECTION);
        }
    }
    Ok(())
}
