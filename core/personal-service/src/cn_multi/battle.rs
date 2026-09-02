// audience: internal
// # personal-service-cn-multi-battle
//
// 该模块开始, 结算, 继续和终止 CN 本地联机战斗.

use super::{authenticate, bad_request, error_response};
use crate::cn::decode_request;
use crate::cn_battle_assets::load_battle_fixture;
use crate::cn_battle_state::{finish_battle, FinishBattleInput};
use crate::database::{
    MultiplayerBattleAbort, MultiplayerBattleContinue, MultiplayerBattleFinish,
    MultiplayerBattleIdentity, MultiplayerBattleReceipt, MultiplayerBattleStart, ServiceDatabase,
};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct StartRequest {
    api_count: Option<i64>,
    category: i64,
    is_auto_start_mode: bool,
    party_id: i64,
    play_id: String,
    quest_id: i64,
    room_number: String,
    use_boost_point: bool,
    use_boss_boost_point: bool,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct FinishRequest {
    add_mana: Option<i64>,
    api_count: Option<i64>,
    battle_time: Option<i64>,
    category: i64,
    contribution_score: Option<i64>,
    elapsed_time_ms: Option<i64>,
    is_accomplished: Option<bool>,
    mate_player_result: Option<Vec<Value>>,
    play_id: String,
    quest_id: i64,
    quest_statistics: Option<QuestStatistics>,
    score: Option<f64>,
    statistics: Option<QuestStatistics>,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct QuestStatistics {
    party: QuestParty,
    zones: Option<Vec<QuestZone>>,
    #[serde(default)]
    max_skill_chain_count: Option<i64>,
    #[serde(default)]
    max_combo_count: Option<i64>,
    #[serde(default)]
    is_host: Option<bool>,
    #[serde(default)]
    is_mvp: Option<bool>,
}

#[derive(Deserialize)]
struct QuestParty {
    characters: Option<Vec<Option<PartyCharacter>>>,
    unison_characters: Option<Vec<Option<PartyCharacter>>>,
    equipments: Option<Vec<Option<PartyCharacter>>>,
    ability_soul_ids: Option<Vec<Option<i64>>>,
}

#[derive(Deserialize)]
struct PartyCharacter {
    id: Option<i64>,
}

#[derive(Deserialize)]
struct QuestZone {
    #[serde(default)]
    use_power_flip_count: i64,
    #[serde(default)]
    use_dash_count: i64,
    #[serde(default)]
    use_skill_count: i64,
}

#[derive(Deserialize)]
struct ContinueRequest {
    api_count: Option<i64>,
    retry_count: Option<i64>,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct AbortRequest {
    api_count: Option<i64>,
    category: Option<i64>,
    play_id: String,
    quest_id: Option<i64>,
    viewer_id: i64,
}

// //// 编码并重放联机战斗动作收据 [@x380kkm 2026-08-23] ////
fn encode_battle_receipt(action: &str, response: &Value) -> Result<String, PersonalServiceError> {
    serde_json::to_string(response).map_err(|error| {
        PersonalServiceError::new(format!(
            "failed to encode multiplayer battle {action} receipt: {error}"
        ))
    })
}

fn replay_battle_receipt(
    viewer_id: i64,
    action: &str,
    receipt: &MultiplayerBattleReceipt,
) -> Result<HttpResponse, PersonalServiceError> {
    let response = serde_json::from_str::<Value>(&receipt.response_json).map_err(|error| {
        PersonalServiceError::new(format!(
            "failed to decode multiplayer battle {action} receipt: {error}"
        ))
    })?;
    crate::cn::msgpack_response_at(viewer_id, false, receipt.response_time, response)
}
// //// /编码并重放联机战斗动作收据 ////

pub(super) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let response = match request.path() {
        "/api/index.php/multi_battle_quest/start" => start(request, database),
        "/api/index.php/multi_battle_quest/finish" => finish(request, database),
        "/api/index.php/multi_battle_quest/abort" => abort(request, database),
        "/api/index.php/multi_battle_quest/play_continue" => play_continue(request, database),
        _ => return None,
    };
    Some(response)
}

// //// 原子开始房间内真人成员的联机战斗 [@x380kkm 2026-08-22] ////
fn start(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<StartRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.api_count.map_or(true, |api_count| api_count >= 0)
                && body.category > 0
                && body.quest_id > 0
                && body.party_id > 0
                && !body.play_id.is_empty()
                && super::lobby::is_room_number(&body.room_number) =>
        {
            body
        }
        Ok(_) | Err(_) => {
            return Ok(bad_request("Invalid request body."));
        }
    };
    let player = match authenticate(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    if super::party_leader_id(&player.data, body.party_id).is_none() {
        return Ok(error_response("400 Bad Request", "party_not_found"));
    }
    let room = match database.multiplayer_room(&body.room_number)? {
        Some(room) => room,
        None => return Ok(bad_request("Room doesn't exist.")),
    };
    let member = database.multiplayer_member(&room.room_number, body.viewer_id)?;
    if member.as_ref().map(|member| member.account_id) != Some(player.account_id) {
        return Ok(error_response("403 Forbidden", "room_access_denied"));
    }
    if member.as_ref().map(|member| member.party_id) != Some(body.party_id) {
        return Ok(error_response("409 Conflict", "party_mismatch"));
    }
    if room.category_id != body.category || room.quest_id != body.quest_id {
        return Ok(error_response("409 Conflict", "room_mismatch"));
    }
    let response_time = crate::cn::server_time(database)?;
    let response = json!({
        "user_info": { "last_main_quest_id": body.quest_id },
        "category_id": body.category,
        "is_multi": "multi",
        "start_time": response_time,
        "quest_name": "",
        "follow_bonus_info": null,
        "client_checks": null,
        "play_id": body.play_id,
    });
    let response_json = encode_battle_receipt("start", &response)?;
    let Some(receipt) = database.start_multiplayer_battle_member(MultiplayerBattleStart {
        identity: MultiplayerBattleIdentity {
            account_id: player.account_id,
            room_number: &room.room_number,
            play_id: &body.play_id,
            category_id: body.category,
            quest_id: body.quest_id,
            api_count: body.api_count,
        },
        use_boss_boost_point: body.use_boss_boost_point,
        use_boost_point: body.use_boost_point,
        is_auto_start_mode: body.is_auto_start_mode,
        response_time,
        response_json: &response_json,
    })?
    else {
        return Ok(error_response("409 Conflict", "battle_start_conflict"));
    };
    replay_battle_receipt(body.viewer_id, "start", &receipt)
}
// //// /原子开始房间内真人成员的联机战斗 ////

// //// 结算联机战斗并提交玩家快照 [@x380kkm 2026-08-22] ////
fn finish(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<FinishRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.api_count.map_or(true, |api_count| api_count >= 0)
                && body.category > 0
                && body.quest_id > 0
                && !body.play_id.is_empty() =>
        {
            body
        }
        Ok(_) | Err(_) => return Ok(bad_request("Invalid request body.")),
    };
    let player = match authenticate(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    if let Some(receipt) = database.multiplayer_battle_receipt(
        player.account_id,
        "finish",
        &body.play_id,
        body.api_count,
    )? {
        return replay_battle_receipt(body.viewer_id, "finish", &receipt);
    }
    let active_quest = match database.get_active_single_quest(player.account_id)? {
        Some(active_quest)
            if active_quest.play_id == body.play_id
                && active_quest.category == body.category
                && active_quest.quest_id == body.quest_id =>
        {
            active_quest
        }
        Some(_) | None => return Ok(bad_request("No active quest to finish.")),
    };
    let room_number = match database.multiplayer_battle_room(
        player.account_id,
        &body.play_id,
        body.category,
        body.quest_id,
    )? {
        Some(room_number) => room_number,
        None => return Ok(error_response("400 Bad Request", "room_not_found")),
    };
    let fixture = load_battle_fixture()?;
    let quest = match fixture
        .quests
        .get(&format!("{}:{}", body.category, body.quest_id))
    {
        Some(quest) => quest,
        None => return Ok(error_response("400 Bad Request", "quest_not_found")),
    };
    let statistics = body.quest_statistics.as_ref().or(body.statistics.as_ref());
    let main_character_ids = statistics
        .and_then(|statistics| statistics.party.characters.as_ref())
        .map(|characters| {
            characters
                .iter()
                .map(|character| character.as_ref().and_then(|character| character.id))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let unison_character_ids = statistics
        .and_then(|statistics| statistics.party.unison_characters.as_ref())
        .map(|characters| {
            characters
                .iter()
                .map(|character| character.as_ref().and_then(|character| character.id))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let equipment_ids = statistics
        .and_then(|statistics| statistics.party.equipments.as_ref())
        .map(|equipments| {
            equipments
                .iter()
                .map(|equipment| equipment.as_ref().and_then(|equipment| equipment.id))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let ability_soul_ids = statistics
        .and_then(|statistics| statistics.party.ability_soul_ids.clone())
        .unwrap_or_default();
    let mut character_ids = Vec::new();
    for character_id in main_character_ids
        .iter()
        .chain(unison_character_ids.iter())
        .flatten()
        .copied()
        .filter(|character_id| *character_id > 0)
    {
        if !character_ids.contains(&character_id) {
            character_ids.push(character_id);
        }
    }
    let action_counts = statistics
        .and_then(|statistics| statistics.zones.as_deref())
        .map(|zones| {
            zones
                .iter()
                .try_fold(
                    (0_i64, 0_i64, 0_i64),
                    |(power_flips, dashes, skills), zone| {
                        Some((
                            power_flips.checked_add(zone.use_power_flip_count.max(0))?,
                            dashes.checked_add(zone.use_dash_count.max(0))?,
                            skills.checked_add(zone.use_skill_count.max(0))?,
                        ))
                    },
                )
                .ok_or_else(|| {
                    PersonalServiceError::new(
                        "CN multiplayer statistics exceed the supported range",
                    )
                })
        })
        .transpose()?;
    let (power_flip_count, dash_count, skill_count) = match action_counts {
        Some((power_flips, dashes, skills)) => (Some(power_flips), Some(dashes), Some(skills)),
        None => (None, None, None),
    };
    let max_skill_chain_count = statistics.and_then(|statistics| statistics.max_skill_chain_count);
    let max_combo_count = statistics.and_then(|statistics| statistics.max_combo_count);
    let is_host = statistics.and_then(|statistics| statistics.is_host);
    let is_mvp = statistics.and_then(|statistics| statistics.is_mvp);
    let elapsed_time_ms = body
        .elapsed_time_ms
        .or_else(|| body.battle_time.and_then(|time| time.checked_mul(1_000)))
        .unwrap_or_default()
        .max(0);
    let response_time = crate::cn::server_time(database)?;
    let drop_multiplier = database.drop_multiplier()?;
    let mutation = finish_battle(
        database,
        player.account_id,
        &serde_json::to_string(&player.data).map_err(|error| {
            PersonalServiceError::new(format!("failed to encode multiplayer player data: {error}"))
        })?,
        &active_quest,
        quest,
        &fixture,
        &FinishBattleInput {
            elapsed_time_ms,
            score: body.score.unwrap_or_default() as i64,
            add_mana: body.add_mana.unwrap_or_default(),
            is_accomplished: body.is_accomplished.unwrap_or(true),
            character_ids: &character_ids,
            main_character_ids: &main_character_ids,
            unison_character_ids: &unison_character_ids,
            equipment_ids: &equipment_ids,
            ability_soul_ids: &ability_soul_ids,
            is_multi: true,
            power_flip_count,
            dash_count,
            skill_count,
            max_skill_chain_count,
            max_combo_count,
            is_host,
            is_mvp,
        },
        drop_multiplier,
        response_time,
    )?;
    let mut response = mutation
        .response
        .as_object()
        .cloned()
        .ok_or_else(|| PersonalServiceError::new("multiplayer finish response is invalid"))?;
    response.insert("is_multi".to_owned(), Value::String("multi".to_owned()));
    response.insert(
        "mate_player_result".to_owned(),
        Value::Array(body.mate_player_result.unwrap_or_default()),
    );
    response.insert(
        "contribution_score".to_owned(),
        Value::from(body.contribution_score.unwrap_or_default()),
    );
    for key in [
        "aborted_play_id",
        "drawn_quest",
        "follow_info",
        "party_info",
        "unfinished_play_id",
    ] {
        response.insert(key.to_owned(), Value::Null);
    }
    for key in [
        "carnival_event",
        "ranking_event",
        "score_attack_event",
        "solo_time_attack_event",
    ] {
        response.entry(key.to_owned()).or_insert(Value::Null);
    }
    for key in ["user_notice_list", "user_periodic_reward_point_list"] {
        response
            .entry(key.to_owned())
            .or_insert_with(|| Value::Array(Vec::new()));
    }
    response.insert(
        "presigned_quest_category".to_owned(),
        Value::Array(Vec::new()),
    );
    let response = Value::Object(response);
    let expiry_anchor_ms = database.current_wall_time_millis()?;
    let Some(receipt) = database.finish_multiplayer_battle_member(MultiplayerBattleFinish {
        identity: MultiplayerBattleIdentity {
            account_id: player.account_id,
            room_number: &room_number,
            play_id: &body.play_id,
            category_id: body.category,
            quest_id: body.quest_id,
            api_count: body.api_count,
        },
        snapshot: &mutation.snapshot,
        expiry_anchor_ms,
        response_time,
        response: &response,
    })?
    else {
        return Ok(error_response("409 Conflict", "battle_identity_mismatch"));
    };
    replay_battle_receipt(body.viewer_id, "finish", &receipt)
}
// //// /结算联机战斗并提交玩家快照 ////

fn abort(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<AbortRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.api_count.map_or(true, |api_count| api_count >= 0)
                && body.category.map_or(true, |category| category > 0)
                && body.quest_id.map_or(true, |quest_id| quest_id > 0)
                && !body.play_id.is_empty() =>
        {
            body
        }
        Ok(_) | Err(_) => return Ok(bad_request("Invalid request body.")),
    };
    let player = match authenticate(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    if let Some(receipt) = database.multiplayer_battle_receipt(
        player.account_id,
        "abort",
        &body.play_id,
        body.api_count,
    )? {
        return replay_battle_receipt(body.viewer_id, "abort", &receipt);
    }
    let active = match database.get_active_single_quest(player.account_id)? {
        Some(active)
            if active.play_id == body.play_id
                && body
                    .category
                    .map_or(true, |category| category == active.category)
                && body
                    .quest_id
                    .map_or(true, |quest_id| quest_id == active.quest_id) =>
        {
            active
        }
        Some(_) => return Ok(error_response("409 Conflict", "battle_identity_mismatch")),
        None => {
            let response_time = crate::cn::server_time(database)?;
            return crate::cn::msgpack_response_at(
                body.viewer_id,
                false,
                response_time,
                abort_response(body.category.unwrap_or_default(), response_time),
            );
        }
    };
    let room_number = match database.multiplayer_battle_room(
        player.account_id,
        &body.play_id,
        active.category,
        active.quest_id,
    )? {
        Some(room_number) => room_number,
        None => return Ok(error_response("400 Bad Request", "room_not_found")),
    };
    let response_time = crate::cn::server_time(database)?;
    let response = abort_response(active.category, response_time);
    let response_json = encode_battle_receipt("abort", &response)?;
    let expiry_anchor_ms = database.current_wall_time_millis()?;
    let Some(receipt) = database.abort_multiplayer_battle_member(MultiplayerBattleAbort {
        identity: MultiplayerBattleIdentity {
            account_id: player.account_id,
            room_number: &room_number,
            play_id: &body.play_id,
            category_id: active.category,
            quest_id: active.quest_id,
            api_count: body.api_count,
        },
        expiry_anchor_ms,
        response_time,
        response_json: &response_json,
    })?
    else {
        return Ok(error_response("409 Conflict", "battle_identity_mismatch"));
    };
    replay_battle_receipt(body.viewer_id, "abort", &receipt)
}

fn abort_response(category_id: i64, response_time: i64) -> Value {
    json!({
        "user_info": {},
        "category_id": category_id,
        "is_multi": "multi",
        "start_time": response_time,
        "quest_name": "",
        "aborted_play_id": null,
        "unfinished_play_id": null,
        "drawn_quest": null,
        "party_info": null,
        "presigned_url": null,
    })
}

fn play_continue(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ContinueRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.api_count.map_or(true, |api_count| api_count >= 0)
                && body.retry_count.unwrap_or_default() >= 0 =>
        {
            body
        }
        Ok(_) | Err(_) => {
            return Ok(bad_request("Invalid request body."));
        }
    };
    let player = match authenticate(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    let active = match database.get_active_single_quest(player.account_id)? {
        Some(active) => active,
        None => return Ok(bad_request("No active quest to continue.")),
    };
    if let Some(receipt) = database.multiplayer_battle_receipt(
        player.account_id,
        "continue",
        &active.play_id,
        body.api_count,
    )? {
        return replay_battle_receipt(body.viewer_id, "continue", &receipt);
    }
    let room_number = match database.multiplayer_battle_room(
        player.account_id,
        &active.play_id,
        active.category,
        active.quest_id,
    )? {
        Some(room_number) => room_number,
        None => return Ok(error_response("400 Bad Request", "room_not_found")),
    };
    let serialized_player = serde_json::to_string(&player.data).map_err(|error| {
        PersonalServiceError::new(format!("failed to encode multiplayer player data: {error}"))
    })?;
    let response = json!({});
    let response_time = crate::cn::server_time(database)?;
    let Some(receipt) = database.continue_multiplayer_battle_member(MultiplayerBattleContinue {
        identity: MultiplayerBattleIdentity {
            account_id: player.account_id,
            room_number: &room_number,
            play_id: &active.play_id,
            category_id: active.category,
            quest_id: active.quest_id,
            api_count: body.api_count,
        },
        snapshot: &serialized_player,
        response_time,
        response: &response,
    })?
    else {
        return Ok(error_response("409 Conflict", "battle_identity_mismatch"));
    };
    replay_battle_receipt(body.viewer_id, "continue", &receipt)
}
