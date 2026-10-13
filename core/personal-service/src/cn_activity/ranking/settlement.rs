// audience: internal
// # personal-service-ranking-settlement
// 客户端领取和管理员结算共用期别凭据, 原始成绩与选定奖励档位分别记录.

use super::{catalog, recorded_summary, records, rewards, summary, RECEIPTS};
use crate::cn::server_time;
use crate::cn_activity::state::{self, ActivityPlayer};
use crate::cn_tutorial::encode_player_data;
use crate::database::{CreateMailInput, ServiceDatabase};
use crate::PersonalServiceError;
use serde_json::{json, Value};

// //// 按真实成绩或选定目录档位原子发放一次奖励 [@x380kkm 2026-09-28] ////
pub(in crate::cn_activity) fn settle(
    player: &mut ActivityPlayer,
    database: &mut ServiceDatabase,
    viewer_id: i64,
    event_id: i64,
    selected_tier: Option<i64>,
    source: &str,
) -> Result<Value, PersonalServiceError> {
    let event = catalog()?
        .get(&event_id)
        .ok_or_else(|| PersonalServiceError::new("ranking event is missing"))?;
    let response_time = server_time(database)?;
    let account_id = player.account_id;
    let root = player.root_mut()?;
    if source == "client" {
        state::select_event_period(root, database, "ranking", event_id)?;
    }
    if let Some(mut response) = recorded_summary(root, event_id) {
        let scope = state::reward_scope(root, "ranking", event_id);
        let receipt = &root[RECEIPTS][scope.strip_prefix("ranking:").unwrap()];
        response["status"] = json!(2);
        response["tier_id"] = receipt["tier_id"].clone();
        response["rewards"] = receipt.get("rewards").cloned().unwrap_or(json!([]));
        response["degree_list"] = crate::cn_activity::competition::degree_list_for_viewer(
            &receipt["degree_list"],
            viewer_id,
        );
        response["mail_arrived"] = json!(database.has_unreceived_mail(account_id, response_time)?);
        return Ok(response);
    }
    if records::best_record(root, event_id, event).is_none() {
        return Ok(json!({"status":3}));
    }
    if source == "client"
        && crate::cn_activity::competition::is_manual(root, &format!("ranking:{event_id}"))
    {
        return Ok(json!({"status":3}));
    }
    let mut response = summary(root, event_id, event);
    let percentage = response["rank_percentage"].as_f64().unwrap_or(100.0);
    let tier = event
        .tiers
        .iter()
        .find(|tier| selected_tier.map_or(percentage <= tier.border_percentage, |id| tier.id == id))
        .ok_or_else(|| PersonalServiceError::new("CN ranking reward tier is missing"))?;
    let (mail, degrees) = rewards::prepare(root, &tier.rewards, viewer_id)?;
    let degrees =
        crate::cn_activity::competition::degree_list_for_viewer(&json!(degrees), viewer_id);
    let degree_receipt = crate::cn_activity::competition::degree_list_for_viewer(&degrees, 0);
    let activity_id = format!("ranking:{event_id}");
    let round = crate::activity_history::participation_round(root, &activity_id);
    let scope = state::reward_scope(root, "ranking", event_id);
    let receipt_key = scope.strip_prefix("ranking:").unwrap().to_owned();
    root.entry(RECEIPTS.to_owned()).or_insert_with(|| json!({})).as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN ranking receipts are invalid"))?
        .insert(receipt_key, json!({"tier_id":tier.id,"summary":response,"received_at":response_time,
            "degree_list":degree_receipt,"participation_round":round,"source":source,"rewards":tier.rewards}));
    let delivered = database.deliver_reward_mail_with_snapshot_once(
        &CreateMailInput {
            account_id,
            title: format!("{}排名奖励", event.name),
            body: format!("{}排名奖励.", event.name),
            sender: "Starpoint".to_owned(),
            rewards: mail,
            expires_at: None,
            created_at: response_time,
        },
        &encode_player_data(&player.data)?,
        &if round == 0 {
            scope
        } else {
            format!("{scope}:round:{round}")
        },
        Some(&activity_id),
    )?;
    response["status"] = json!(if delivered { 1 } else { 2 });
    response["tier_id"] = json!(tier.id);
    response["degree_list"] = json!(degrees);
    response["rewards"] = json!(tier.rewards);
    response["mail_arrived"] = json!(database.has_unreceived_mail(account_id, response_time)?);
    Ok(response)
}
