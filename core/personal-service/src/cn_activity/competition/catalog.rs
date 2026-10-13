// audience: internal
// # personal-service-competition-catalog
// 管理目录仅包含拥有排名奖励或全服讨伐奖励的活动, 统计按玩家当前期别投影.

use super::super::{raid, ranking, rush, state};
use crate::database::{evaluate_activity_schedule, ActivityWindowStatus, ServiceDatabase};
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};

// //// 汇总竞速排名, 狂热排名和团队全服奖励 [@x380kkm 2026-09-28] ////
pub(super) fn entries(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
) -> Result<Vec<Value>, PersonalServiceError> {
    let mut entries = ranking::management_entries(root, database)?;
    let catalog = raid::rewards::catalog()?;
    for (&event_id, definition) in &catalog.rush {
        if definition.ranking_rewards.is_empty() {
            continue;
        }
        state::select_event_period(root, database, "rush", event_id)?;
        let summary = rush::player_ranking(root, "rush", event_id, 1);
        let received = state::event_state(root, "rush", event_id)
            .and_then(|event| event.get("ranking_reward_receipt"))
            .is_some();
        let date = crate::cn_tutorial::format_client_time(definition.period.start_at_ms / 1_000);
        entries.push(json!({"activity_id":format!("rush:{event_id}"),"family":"rush","event_id":event_id,
            "name":format!("狂热激战 ({})", &date[..10]),"has_record":summary.is_some(),"received":received,
            "suggested_rank_number":summary.as_ref().and_then(|entry| entry["rank_number"].as_i64()),"summary":summary,
            "start_at_ms":definition.period.start_at_ms,"end_at_ms":definition.period.play_end_at_ms,
            "tiers":definition.ranking_rewards.iter().enumerate().map(|(index,tier)| json!({"id":index+1,
                "label":format!("第 {}-{} 名",tier.rank_start,tier.rank_end),"rank_from":tier.rank_start,
                "rank_to":tier.rank_end,"rewards":tier.rewards})).collect::<Vec<_>>() }));
    }
    for (&event_id, definition) in &catalog.raid {
        state::select_event_period(root, database, "raid", event_id)?;
        let period = state::event_period_start(root, "raid", event_id);
        let boss = database.raid_boss_state_for_period(event_id, period)?;
        let event = state::event_state(root, "raid", event_id);
        let previous = event
            .and_then(|event| event.get("kill_reward_received_up_to"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let (available, pending) =
            raid::rewards::kill_rewards(definition, previous, previous.max(boss.total_kill_count))?;
        let tiers = definition
            .overall_rewards
            .iter()
            .map(|tier| {
                let mut value = serde_json::to_value(tier).expect("raid reward is serializable");
                let first = value["first_kill"]
                    .as_i64()
                    .or(value["interval"].as_i64())
                    .unwrap_or(1);
                value["label"] = json!(format!("累计讨伐 {first} 次"));
                value["required_kill_count"] = json!(first);
                value["received"] = json!(previous >= first);
                value["pending_count"] = pending
                    .iter()
                    .find(|pending| pending["reward_id"] == value["id"])
                    .map(|pending| pending["count"].clone())
                    .unwrap_or(json!(0));
                value
            })
            .collect::<Vec<_>>();
        entries.push(json!({"activity_id":format!("raid:{event_id}"),"family":"raid","event_id":event_id,
            "name":format!("团队战 {event_id}"),"has_record":true,"received":available.is_empty() && previous>0,
            "summary":{"received_up_to":previous,"quest_list":event.and_then(|event|event.get("quest_list"))},
            "global_state":{"hp_percentage":boss.hp_percentage,"total_kill_count":boss.total_kill_count,"period_start_ms":period},
            "start_at_ms":definition.period.start_at_ms,"end_at_ms":definition.period.play_end_at_ms,"tiers":tiers}));
    }
    let fixture = crate::cn_battle_assets::load_battle_fixture()?;
    for entry in &mut entries {
        decorate(entry, root, database, &fixture)?;
    }
    Ok(entries)
}

fn decorate(
    entry: &mut Value,
    root: &Map<String, Value>,
    database: &ServiceDatabase,
    fixture: &crate::cn_battle_assets::BattleFixture,
) -> Result<(), PersonalServiceError> {
    let activity_id = entry["activity_id"].as_str().unwrap().to_owned();
    let family = entry["family"].as_str().unwrap().to_owned();
    let event_id = entry["event_id"].as_i64().unwrap();
    entry["participation_round"] = json!(crate::activity_history::participation_round(
        root,
        &activity_id
    ));
    entry["period_start_ms"] = json!(state::event_period_start(root, &family, event_id));
    entry["reward_mode"] = json!(if super::is_manual(root, &activity_id) {
        "manual"
    } else {
        "automatic"
    });
    entry["suggested_tier_id"] = entry.get("default_tier_id").cloned().unwrap_or(Value::Null);
    let start = entry["start_at_ms"].as_i64();
    let end = entry["end_at_ms"].as_i64();
    entry["availability"] = availability(database, &activity_id, start, end)?;
    if let Some(name) = fixture
        .quests
        .values()
        .find(|quest| quest.activity_id.as_deref() == Some(activity_id.as_str()))
        .map(|quest| quest.name.replace("::quest_rank::", "").trim().to_owned())
    {
        if !name.is_empty() {
            if family == "raid" {
                entry["name"] = json!(name);
            }
            entry["quest_name"] = json!(name);
        }
    }
    for tier in entry["tiers"].as_array_mut().into_iter().flatten() {
        for reward in tier["rewards"].as_array_mut().into_iter().flatten() {
            let label = match reward["kind"].as_i64().unwrap_or(-1) {
                0 => format!("道具 {}", reward["id"]),
                1 => format!("装备 {}", reward["id"]),
                2 => "星导石".to_owned(),
                3 => "玛纳".to_owned(),
                4 => "经验".to_owned(),
                6 => format!("角色 {}", reward["id"]),
                7 => format!("称号 {}", reward["id"]),
                _ => "奖励".to_owned(),
            };
            reward["label"] = json!(label);
        }
    }
    Ok(())
}

fn availability(
    database: &ServiceDatabase,
    activity_id: &str,
    start: Option<i64>,
    end: Option<i64>,
) -> Result<Value, PersonalServiceError> {
    let now = database.current_server_time_millis()?;
    let (mode, status, start, end) =
        if let Some(until) = database.activity_temporary_open_until(activity_id)? {
            (
                "temporary",
                ActivityWindowStatus::Open,
                Some(now),
                Some(until),
            )
        } else if let Some(schedule) = database.get_activity_schedule(activity_id)? {
            let window = evaluate_activity_schedule(&schedule, now);
            (
                schedule.mode.as_str(),
                window.status,
                window.active_start_ms.or(window.next_start_ms),
                window.active_end_ms.or(window.next_end_ms),
            )
        } else {
            let window = crate::activity_projection::regional_activity_window(activity_id, now)?
                .or(start.zip(end));
            let (start, end) = window.map_or((start, end), |(start, end)| (Some(start), Some(end)));
            let status = if start.is_some_and(|start| now < start) {
                ActivityWindowStatus::NotStarted
            } else if end.is_some_and(|end| now >= end) {
                ActivityWindowStatus::Ended
            } else {
                ActivityWindowStatus::Open
            };
            ("default", status, start, end)
        };
    let status = match status {
        ActivityWindowStatus::Unscheduled => "unscheduled",
        ActivityWindowStatus::Disabled => "disabled",
        ActivityWindowStatus::NotStarted => "not_started",
        ActivityWindowStatus::Open => "open",
        ActivityWindowStatus::Ended => "ended",
    };
    Ok(json!({"mode":mode,"status":status,"start_at_ms":start,"end_at_ms":end}))
}
