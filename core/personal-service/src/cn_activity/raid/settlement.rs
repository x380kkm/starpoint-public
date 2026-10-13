// audience: internal
// # personal-service-raid-settlement
// 全服讨伐奖励按当前期别的累计次数增量发放, 领取凭据保留统计值和发放来源.

use super::{rewards, FAMILY};
use crate::cn_activity::{competition, state};
use crate::cn_tutorial::encode_player_data;
use crate::database::{CreateMailInput, MailReward, ServiceDatabase};
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};

pub(in crate::cn_activity) struct Award {
    pub(in crate::cn_activity) response: Value,
    mail: MailReward,
    activity_id: String,
    event_key: String,
    response_time: i64,
}

// //// 根据共享讨伐统计生成尚未领取的奖励 [@x380kkm 2026-09-28] ////
pub(in crate::cn_activity) fn prepare(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    viewer_id: i64,
    event_id: i64,
    source: &str,
) -> Result<Award, PersonalServiceError> {
    let response_time = database.current_server_time_seconds()?;
    let activity_id = format!("raid:{event_id}");
    if source == "client" {
        state::select_event_period(root, database, FAMILY, event_id)?;
    }
    let period_start_ms = state::event_period_start(root, FAMILY, event_id);
    let boss = database.raid_boss_state_for_period(event_id, period_start_ms)?;
    let round = crate::activity_history::participation_round(root, &activity_id);
    let scope = state::reward_scope(root, FAMILY, event_id);
    let manual = source == "client" && competition::is_manual(root, &activity_id);
    let previous = state::event_state(root, FAMILY, event_id)
        .and_then(|event| event.get("kill_reward_received_up_to"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let current = if manual {
        previous
    } else {
        previous.max(boss.total_kill_count)
    };
    let (granted, tiers) = match rewards::catalog()?.raid.get(&event_id) {
        Some(definition) => rewards::kill_rewards(definition, previous, current)?,
        None => (Vec::new(), Vec::new()),
    };
    let (mail, rewards, degrees) = rewards::prepare(root, &granted, viewer_id)?;
    let degrees = competition::degree_list_for_viewer(&json!(degrees), viewer_id);
    let event = state::event_state_mut(root, FAMILY, event_id)?;
    event.insert("kill_reward_received_up_to".to_owned(), json!(current));
    if !granted.is_empty() {
        event.entry("kill_reward_receipts".to_owned()).or_insert_with(|| json!({})).as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN raid reward receipts are invalid"))?
            .insert(current.to_string(), json!({"received_at":response_time,"previous_kill_count":previous,
                "total_kill_count":current,"tiers":tiers,"rewards":rewards,"participation_round":round,
                "source":source,"period_start_ms":period_start_ms,
                "global_state":{"hp_percentage":boss.hp_percentage,"total_kill_count":boss.total_kill_count}}));
    }
    Ok(Award {
        response: json!({"status":if granted.is_empty() { if previous > 0 { 2 } else { 3 } } else { 1 },
            "received_up_to":current,"reward_list":rewards,"degree_list":degrees,"tiers":tiers,
            "global_state":{"hp_percentage":boss.hp_percentage,"total_kill_count":boss.total_kill_count}}),
        mail,
        activity_id,
        event_key: format!("{scope}:round:{round}:kill:{current}"),
        response_time,
    })
}

impl Award {
    // //// 将奖励与领取统计保存在同一事务内 [@x380kkm 2026-09-28] ////
    pub(in crate::cn_activity) fn deliver(
        &self,
        player: &state::ActivityPlayer,
        database: &mut ServiceDatabase,
    ) -> Result<(), PersonalServiceError> {
        if rewards::has_mail_rewards(&self.mail) {
            database.deliver_reward_mail_with_snapshot_once(
                &CreateMailInput {
                    account_id: player.account_id,
                    title: "团队战总讨伐奖励".to_owned(),
                    body: "团队战总讨伐奖励.".to_owned(),
                    sender: "Starpoint".to_owned(),
                    rewards: self.mail.clone(),
                    expires_at: None,
                    created_at: self.response_time,
                },
                &encode_player_data(&player.data)?,
                &self.event_key,
                Some(&self.activity_id),
            )?;
        } else {
            player.save(database)?;
        }
        Ok(())
    }
}

pub(in crate::cn_activity) fn settle(
    player: &mut state::ActivityPlayer,
    database: &mut ServiceDatabase,
    viewer_id: i64,
    event_id: i64,
) -> Result<Value, PersonalServiceError> {
    let award = prepare(
        player.root_mut()?,
        database,
        viewer_id,
        event_id,
        "management",
    )?;
    award.deliver(player, database)?;
    Ok(award.response)
}
