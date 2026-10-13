// audience: internal
// # personal-service-cn-multi-attention
//
// 本地救援房由后台现实时间维护, 活动关卡使用游戏时间筛选.
// 房间发出一次铃铛投递, 入房资格固定到该房间的后续战斗.

use super::{authenticate, bad_request, msgpack_response};
use crate::cn::decode_request;
use crate::database::{MultiplayerRoom, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct CheckRequest {
    viewer_id: i64,
    #[serde(default)]
    holding_number: usize,
    #[serde(default = "request_number")]
    request_number: usize,
}

fn request_number() -> usize {
    3
}

// //// 按现实时间补充和回收 BOT 救援房 [@x380kkm 2026-09-23] ////
pub(crate) fn maintain_attention_rooms(
    database: &mut ServiceDatabase,
) -> Result<(), PersonalServiceError> {
    let policy = crate::rescue_settings::policy(database)?;
    let now = database.current_wall_time_millis()?;
    let owner = database.attention_owner_account()?;
    let rooms = database.attention_rooms()?;
    let mut retained = rooms.iter().filter(|room| room.battle_started).count();
    for room in rooms {
        if room.battle_started {
            continue;
        }
        let available = policy.enabled
            && owner == Some(room.host_account_id)
            && policy.quests.iter().any(|quest| {
                quest.category_id == room.category_id && quest.quest_id == room.quest_id
            });
        let occupied = database
            .list_multiplayer_members(&room.room_number)?
            .iter()
            .any(|member| member.entered);
        let expired = !occupied
            && now
                >= room
                    .expiry_anchor_ms
                    .saturating_add(policy.room_lifetime_seconds.saturating_mul(1000));
        if !available || expired || (!occupied && retained >= policy.max_rooms) {
            database.disband_multiplayer_room(&room.room_number, room.host_account_id)?;
            if owner != Some(room.host_account_id) {
                database.set_attention_next_spawn_ms(now)?;
            }
        } else {
            retained += 1;
        }
    }
    if !policy.enabled
        || policy.quests.is_empty()
        || retained >= policy.max_rooms
        || now < database.attention_next_spawn_ms()?
    {
        return Ok(());
    }
    let Some(account_id) = owner else {
        return Ok(());
    };
    let teams = crate::ai_teams::get_or_create_multiplayer_ai_teams(database, account_id)?;
    let Some(host) = teams.first() else {
        return Ok(());
    };
    let mut random = [0_u8; 8];
    getrandom::getrandom(&mut random).map_err(|error| {
        PersonalServiceError::new(format!("failed to select local rescue quest: {error}"))
    })?;
    let quest = &policy.quests[(u64::from_le_bytes(random) % policy.quests.len() as u64) as usize];
    let server_time = database.current_server_time_millis()?;
    let is_newbie = database.attention_owner_is_newbie(account_id)?;
    database.create_attention_room(
        account_id,
        (quest.category_id, quest.quest_id),
        super::room::ai_team_leader_id(host),
        server_time,
        now,
        now.saturating_add(policy.spawn_interval_seconds.saturating_mul(1000)),
        |room| {
            teams
                .iter()
                .enumerate()
                .map(|(index, team)| {
                    super::room::create_attention_mate_input(
                        team,
                        room,
                        index + 1,
                        server_time,
                        is_newbie,
                    )
                })
                .collect()
        },
    )?;
    Ok(())
}

// //// 根据活动策略核对救援房的下一局资格 [@x380kkm 2026-09-23] ////
pub(crate) fn can_start_room(
    database: &ServiceDatabase,
    room: &MultiplayerRoom,
) -> Result<bool, PersonalServiceError> {
    if !room.is_attention_room {
        return Ok(true);
    }
    let policy = crate::rescue_settings::policy(database)?;
    Ok(policy.enabled
        && policy
            .quests
            .iter()
            .any(|quest| quest.category_id == room.category_id && quest.quest_id == room.quest_id))
}

// //// 返回救援轮询配置和可信 BOT 房间投递 [@x380kkm 2026-09-23] ////
pub(crate) fn check(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<CheckRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        _ => return Ok(bad_request("Invalid request body.")),
    };
    let player = match authenticate(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    maintain_attention_rooms(database)?;
    let now = database.current_wall_time_millis()?;
    let mut offers = Vec::new();
    let interval_ms = database
        .rescue_settings()?
        .spawn_interval_seconds
        .saturating_mul(1000);
    let can_deliver = database
        .attention_last_delivery_ms(body.viewer_id)?
        .map_or(true, |last| now >= last.saturating_add(interval_ms));
    if body.request_number > 0 && body.holding_number < 3 && can_deliver {
        let rooms = database.attention_rooms()?;
        for room in rooms.into_iter().rev().filter(|room| {
            room.host_account_id == player.account_id && !room.battle_started && !room.lobby_started
        }) {
            if database
                .list_multiplayer_members(&room.room_number)?
                .iter()
                .any(|member| member.entered)
            {
                continue;
            }
            let Some(key) =
                database.deliver_attention_room(&room.room_number, body.viewer_id, now)?
            else {
                continue;
            };
            let host = database
                .list_multiplayer_ai_mates(&room.room_number)?
                .into_iter()
                .next();
            let Some(host) = host else {
                continue;
            };
            let host: Value = serde_json::from_str(&host.lobby_player_json).map_err(|error| {
                PersonalServiceError::new(format!("invalid rescue host: {error}"))
            })?;
            offers.push(json!({"attention_key": key, "quest_info": {
                "category_id":room.category_id, "quest_id":room.quest_id, "room_number":room.room_number,
                "host_entry_time":room.host_entry_time, "establisher_character":room.host_main_character_id,
                "establisher_character_evolution_img_level":host["party"]["characters"][0][1]["evolution_image_level"].as_i64().unwrap_or_default(),
                "establisher_follow":0, "establisher_rank":host["rank"].as_i64().unwrap_or(1),
                "is_newbie":host["isNewbie"].as_bool().unwrap_or(false)
            }}));
            break;
        }
    }
    msgpack_response(
        database,
        body.viewer_id,
        json!({"multi":offers, "config": {
            "attention_recruitment_interval_seconds":15, "attention_recruitment_redeliver_limit":20,
            "attention_polling_interval_seconds_normal":10, "attention_polling_interval_seconds_battle":15,
            "multi_attention_lifetime_seconds":30, "contribution_score_rate_to_parasite":0.25,
            "attention_log_interval_seconds":600, "disable_finish_duration_seconds":5,
            "disable_decline_count_seconds":60, "disable_decline_count_limit":14, "disable_decline_duration_seconds":30,
            "disable_intent_disconnect_duration_seconds":300, "disable_unintent_disconnect_duration_seconds":5,
            "disable_remote_error_duration_seconds":300, "attention_animation_time_seconds":6,
            "disable_expire_count_limit":4, "disable_expire_duration_seconds":180,
            "polling_delay_normal_seconds_range_min":1, "polling_delay_normal_seconds_range_max":10,
            "polling_delay_battle_seconds_range_min":1, "polling_delay_battle_seconds_range_max":15,
            "return_attention_max_num":3
        }}),
    )
}
