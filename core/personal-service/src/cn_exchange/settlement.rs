// audience: internal
// # personal-service-cn-exchange-settlement
// 兑换请求凭据保留原响应和时间, 资产, 库存, 任务与领取记录在同一玩家事务内提交.

use super::{catalog::Offer, error_response};
use crate::cn::{
    decode_request, msgpack_response_at, msgpack_result_code_response_at, server_time,
};
use crate::cn_battle_assets::Reward;
use crate::cn_request_receipt::RequestReceipt;
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, player_snapshot, require_root, set_user_info_value,
    user_info_value,
};
use crate::database::{ReceiveHistoryEntry, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Value};

pub(super) type OfferResolver = fn(i64) -> Result<Option<&'static Offer>, PersonalServiceError>;

#[derive(Deserialize)]
struct ExchangeRequest {
    viewer_id: i64,
    api_count: i64,
    #[serde(alias = "equipment_id")]
    exchange_id: i64,
}

// //// 校验兑换资格并提交费用和实发奖励 [@x380kkm 2026-09-23] ////
pub(super) fn exchange(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    resolve: OfferResolver,
) -> Result<HttpResponse, PersonalServiceError> {
    let now = server_time(database)?;
    let body = match decode_request::<ExchangeRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.exchange_id > 0 && body.api_count >= 0 => body,
        _ => return Ok(error_response("invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let Some(offer) = resolve(body.exchange_id)? else {
        return Ok(error_response("exchange_not_found"));
    };
    let mut data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut data)?;
    let receipt = RequestReceipt::new(
        "exchange",
        offer.currency,
        body.viewer_id,
        body.api_count,
        json!({"exchange_id":offer.id}),
    );
    if let Some(response) = receipt.replay(root)? {
        return Ok(response);
    }
    if !offer.is_available(database, now)? {
        return msgpack_result_code_response_at(body.viewer_id, now, 2053);
    }
    if let Some(code) = offer.eligibility_error(root) {
        return Ok(error_response(code));
    }
    let Some(remaining) = user_info_value(root, offer.currency)?
        .checked_sub(offer.cost)
        .filter(|remaining| *remaining >= 0)
    else {
        return Ok(error_response(&format!("not_enough_{}", offer.currency)));
    };
    let previous_items = crate::cn_mission::capture_collect_item_counts(root)?;
    set_user_info_value(root, offer.currency, remaining)?;
    let reward = crate::cn_battle_rewards::apply_reward_at(
        root,
        &Reward {
            kind: offer.reward.kind,
            id: Some(offer.reward.target_id),
            count: Some(1),
            rarity: None,
        },
        now,
    )?;
    offer.record_purchase(root)?;
    let mut response = json!({
        "user_info":{offer.currency:remaining},
        "character_list":reward.character_list,
        "equipment_list":reward.equipment_list,
        "item_list":reward.items,
        "fund_receive_list":root.get("fund_receive_list"),
        "monthly_charge_bonus_info":root.get("monthly_charge_bonus_info"),
        "user_daily_challenge_point_list":root.get("user_daily_challenge_point_list"),
    });
    if offer.reward.kind == 2 {
        response["encyclopedia_info"] = Value::Object(
            crate::cn_character_reward::record_character_encyclopedia_state(
                root,
                offer.reward.target_id,
            )?,
        );
    }
    let completed = crate::cn_mission::record_collected_items(
        root,
        database,
        snapshot.account_id,
        &previous_items,
    )?;
    response["mission_info"] = json!(completed);
    crate::cn_mission::apply_mission_reward_response(
        &mut response,
        root,
        &completed,
        body.viewer_id,
        now,
    )?;
    response["active_mission_list"] =
        json!(crate::cn_mission::refresh_active_missions(root, database)?);
    response["mail_arrived"] = json!(database.has_unreceived_mail(snapshot.account_id, now)?);
    let event_key = receipt.record(root, &response, now)?;
    database.save_player_snapshot_with_receive_history(
        snapshot.account_id,
        &encode_player_data(&data)?,
        &event_key,
        now,
        &[ReceiveHistoryEntry::reward(
            offer.reward.history_kind,
            Some(offer.reward.target_id),
            1,
        )],
    )?;
    msgpack_response_at(body.viewer_id, false, now, response)
}
// //// /校验兑换资格并提交费用和实发奖励 ////
