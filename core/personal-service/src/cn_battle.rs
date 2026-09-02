// audience: internal
// # personal-service-cn-battle
//
// 该模块处理 CN 单机战斗的开始, 继续, 结算和终止请求. 活跃战斗和玩家快照保存在 SQLite 中.

use crate::cn::{decode_request, json_response_at, msgpack_response_at, server_time};
use crate::cn_battle_assets::load_battle_fixture;
use crate::cn_battle_state::{
    continue_battle, finish_battle, prepare_battle_start, FinishBattleInput, StartBattleFailure,
};
use crate::database::{ActiveSingleQuest, PlayerSnapshot, ServiceDatabase, ViewerSessionPlayer};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct StartRequest {
    viewer_id: i64,
    play_id: String,
    quest_id: i64,
    category: i64,
    party_id: i64,
    use_boss_boost_point: bool,
    use_boost_point: bool,
    is_auto_start_mode: bool,
}

#[derive(Deserialize)]
struct FinishRequest {
    viewer_id: i64,
    play_id: String,
    quest_id: i64,
    category: i64,
    elapsed_time_ms: i64,
    score: i64,
    add_mana: i64,
    is_accomplished: bool,
    statistics: QuestStatistics,
}

#[derive(Deserialize)]
struct QuestStatistics {
    party: QuestParty,
    #[serde(default)]
    zones: Vec<QuestZone>,
    #[serde(default)]
    max_skill_chain_count: Option<i64>,
    #[serde(default)]
    max_combo_count: Option<i64>,
}

#[derive(Deserialize)]
struct QuestParty {
    characters: Vec<Option<PartyCharacter>>,
    unison_characters: Vec<Option<PartyCharacter>>,
    #[serde(default)]
    equipments: Vec<Option<PartyCharacter>>,
    #[serde(default)]
    ability_soul_ids: Vec<Option<i64>>,
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
struct AbortRequest {
    viewer_id: i64,
    play_id: String,
    category: Option<i64>,
}

#[derive(Deserialize)]
struct ContinueRequest {
    viewer_id: i64,
    api_count: Option<i64>,
    quest_id: i64,
    category: i64,
    #[serde(alias = "paly_id")]
    play_id: String,
    retry_count: Option<i64>,
}

// //// 编码并重放单机战斗收据 [@x380kkm 2026-08-23] ////
fn encode_battle_receipt(action: &str, response: &Value) -> Result<String, PersonalServiceError> {
    serde_json::to_string(response).map_err(|error| {
        PersonalServiceError::new(format!(
            "failed to encode CN battle {action} receipt: {error}"
        ))
    })
}

fn replay_battle_receipt(
    viewer_id: i64,
    action: &str,
    receipt: &str,
    database: &ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let response = decode_battle_receipt(action, receipt)?;
    msgpack_response_at(viewer_id, false, server_time(database)?, response)
}

fn replay_abort_receipt(
    viewer_id: i64,
    receipt: &str,
    database: &ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let response = decode_battle_receipt("abort", receipt)?;
    json_response_at(viewer_id, false, server_time(database)?, response)
}

fn decode_battle_receipt(action: &str, receipt: &str) -> Result<Value, PersonalServiceError> {
    serde_json::from_str::<Value>(receipt).map_err(|error| {
        PersonalServiceError::new(format!(
            "failed to decode CN battle {action} receipt: {error}"
        ))
    })
}

// //// /编码并重放单机战斗收据 ////

// //// 开始并持久化 CN 单机战斗 [@x380kkm 2026-07-22] ////
fn route_start(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<StartRequest>(request) {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    let snapshot = match get_player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    if body.play_id.is_empty() {
        return Ok(battle_error(
            "400 Bad Request",
            "Bad Request",
            "Invalid play id.",
        ));
    }
    if let Some(active_quest) = database.get_active_single_quest(snapshot.account_id)? {
        if active_quest.play_id == body.play_id {
            if active_quest.quest_id != body.quest_id || active_quest.category != body.category {
                return Ok(battle_error(
                    "409 Conflict",
                    "Conflict",
                    "Active quest does not match start request.",
                ));
            }
            if let Some(receipt) =
                database.single_battle_start_receipt(snapshot.account_id, &body.play_id)?
            {
                return replay_battle_receipt(body.viewer_id, "start", &receipt, database);
            }
            let response_time = server_time(database)?;
            return msgpack_response_at(
                body.viewer_id,
                false,
                response_time,
                json!({
                    "user_info": { "last_main_quest_id": active_quest.quest_id },
                    "category_id": active_quest.category,
                    "is_multi": "single",
                    "start_time": response_time,
                    "quest_name": "",
                }),
            );
        }
    }
    let fixture = load_battle_fixture()?;
    let quest_key = format!("{}:{}", body.category, body.quest_id);
    let Some(quest) = fixture.quests.get(&quest_key) else {
        return Ok(battle_error(
            "400 Bad Request",
            "Bad Request",
            "Quest doesn't exist.",
        ));
    };
    let response_time = server_time(database)?;
    let start = match prepare_battle_start(&snapshot.data, quest, body.party_id, response_time)? {
        Ok(start) => start,
        Err(StartBattleFailure::InsufficientEntryItem) => {
            return Ok(battle_error(
                "400 Bad Request",
                "Bad Request",
                "Not enough entry items.",
            ));
        }
        Err(StartBattleFailure::InsufficientStamina) => {
            return Ok(battle_error(
                "400 Bad Request",
                "Bad Request",
                "Insufficient stamina.",
            ));
        }
    };
    let response = json!({
        "user_info": {
            "last_main_quest_id": body.quest_id,
            "stamina": start.stamina,
            "stamina_heal_time": start.stamina_heal_time,
        },
        "category_id": body.category,
        "is_multi": "single",
        "start_time": response_time,
        "quest_name": "",
    });
    let response_json = encode_battle_receipt("start", &response)?;
    database.start_active_single_quest_with_receipt(
        snapshot.account_id,
        &start.snapshot,
        &ActiveSingleQuest {
            play_id: body.play_id,
            quest_id: body.quest_id,
            category: body.category,
            use_boss_boost_point: body.use_boss_boost_point,
            use_boost_point: body.use_boost_point,
            is_auto_start_mode: body.is_auto_start_mode,
        },
        &response_json,
    )?;
    msgpack_response_at(body.viewer_id, false, response_time, response)
}
// //// /开始并持久化 CN 单机战斗 ////

// //// 结算并持久化 CN 单机战斗 [@x380kkm 2026-07-22] ////
fn route_finish(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<FinishRequest>(request) {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    let snapshot = match get_player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let Some(active_quest) = database.get_active_single_quest(snapshot.account_id)? else {
        if let Some(receipt) = database.single_battle_finish_receipt(
            snapshot.account_id,
            &body.play_id,
            body.category,
            body.quest_id,
        )? {
            return replay_battle_receipt(body.viewer_id, "finish", &receipt, database);
        }
        return Ok(battle_error(
            "400 Bad Request",
            "Bad Request",
            "No active quest to finish.",
        ));
    };
    if active_quest.play_id != body.play_id
        || active_quest.quest_id != body.quest_id
        || active_quest.category != body.category
    {
        return Ok(battle_error(
            "409 Conflict",
            "Conflict",
            "Active quest does not match finish request.",
        ));
    }
    let fixture = load_battle_fixture()?;
    let quest_key = format!("{}:{}", active_quest.category, active_quest.quest_id);
    let Some(quest) = fixture.quests.get(&quest_key) else {
        return Ok(battle_error(
            "400 Bad Request",
            "Bad Request",
            "Quest doesn't exist.",
        ));
    };
    let main_character_ids = body
        .statistics
        .party
        .characters
        .iter()
        .map(|character| character.as_ref().and_then(|character| character.id))
        .collect::<Vec<_>>();
    let unison_character_ids = body
        .statistics
        .party
        .unison_characters
        .iter()
        .map(|character| character.as_ref().and_then(|character| character.id))
        .collect::<Vec<_>>();
    let equipment_ids = body
        .statistics
        .party
        .equipments
        .iter()
        .map(|equipment| equipment.as_ref().and_then(|equipment| equipment.id))
        .collect::<Vec<_>>();
    let ability_soul_ids = body.statistics.party.ability_soul_ids.clone();
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
    let (power_flip_count, dash_count, skill_count) = body
        .statistics
        .zones
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
            PersonalServiceError::new("CN battle statistics exceed the supported range")
        })?;
    let response_time = server_time(database)?;
    let drop_multiplier = database.drop_multiplier()?;
    let mutation = finish_battle(
        database,
        snapshot.account_id,
        &snapshot.data,
        &active_quest,
        quest,
        &fixture,
        &FinishBattleInput {
            elapsed_time_ms: body.elapsed_time_ms,
            score: body.score,
            add_mana: body.add_mana,
            is_accomplished: body.is_accomplished,
            character_ids: &character_ids,
            main_character_ids: &main_character_ids,
            unison_character_ids: &unison_character_ids,
            equipment_ids: &equipment_ids,
            ability_soul_ids: &ability_soul_ids,
            is_multi: false,
            power_flip_count: Some(power_flip_count),
            dash_count: Some(dash_count),
            skill_count: Some(skill_count),
            max_skill_chain_count: body.statistics.max_skill_chain_count,
            max_combo_count: body.statistics.max_combo_count,
            is_host: None,
            is_mvp: None,
        },
        drop_multiplier,
        response_time,
    )?;
    let response_json = encode_battle_receipt("finish", &mutation.response)?;
    database.finish_active_single_quest_with_receipt(
        snapshot.account_id,
        &body.play_id,
        active_quest.category,
        active_quest.quest_id,
        &mutation.snapshot,
        &response_json,
    )?;
    msgpack_response_at(body.viewer_id, false, response_time, mutation.response)
}
// //// /结算并持久化 CN 单机战斗 ////

// //// 扣费并继续 CN 单机战斗 [@x380kkm 2026-07-22] ////
fn route_continue(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ContinueRequest>(request) {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    let snapshot = match get_player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let Some(active_quest) = database.get_active_single_quest(snapshot.account_id)? else {
        return Ok(battle_error(
            "400 Bad Request",
            "Bad Request",
            "No active quest to continue.",
        ));
    };
    if active_quest.quest_id != body.quest_id
        || active_quest.category != body.category
        || active_quest.play_id != body.play_id
    {
        return Ok(battle_error(
            "409 Conflict",
            "Conflict",
            "Active quest does not match continue request.",
        ));
    }
    if body.retry_count.unwrap_or_default() > 0 {
        if let Some(receipt) = database.single_battle_continue_receipt(
            snapshot.account_id,
            &body.play_id,
            body.api_count,
        )? {
            return replay_battle_receipt(body.viewer_id, "continue", &receipt, database);
        }
    }
    let Some(mutation) = continue_battle(&snapshot.data)? else {
        return Ok(battle_error(
            "400 Bad Request",
            "Bad Request",
            "Not enough vmoney to continue",
        ));
    };
    let response_json = encode_battle_receipt("continue", &mutation.response)?;
    if database
        .continue_active_single_quest_with_receipt(
            snapshot.account_id,
            &body.play_id,
            body.api_count,
            &mutation.snapshot,
            &response_json,
        )?
        .is_none()
    {
        return Ok(battle_error(
            "400 Bad Request",
            "Bad Request",
            "No active quest to continue.",
        ));
    }
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        mutation.response,
    )
}
// //// /扣费并继续 CN 单机战斗 ////

// //// 终止并删除 CN 单机战斗 [@x380kkm 2026-07-22] ////
fn route_abort(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<AbortRequest>(request) {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    let snapshot = match get_player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let Some(active_quest) = database.get_active_single_quest(snapshot.account_id)? else {
        if let Some(receipt) =
            database.single_battle_abort_receipt(snapshot.account_id, &body.play_id)?
        {
            return replay_abort_receipt(body.viewer_id, &receipt, database);
        }
        let response_time = server_time(database)?;
        return json_response_at(
            body.viewer_id,
            false,
            response_time,
            abort_response(body.category.unwrap_or_default(), response_time),
        );
    };
    let response_time = server_time(database)?;
    let response = abort_response(
        body.category.unwrap_or(active_quest.category),
        response_time,
    );
    let response_json = encode_battle_receipt("abort", &response)?;
    if !database.abort_active_single_quest_with_receipt(
        snapshot.account_id,
        &body.play_id,
        &response_json,
    )? {
        if let Some(receipt) =
            database.single_battle_abort_receipt(snapshot.account_id, &body.play_id)?
        {
            return replay_abort_receipt(body.viewer_id, &receipt, database);
        }
        return json_response_at(body.viewer_id, false, response_time, response);
    }
    json_response_at(body.viewer_id, false, response_time, response)
}
// //// /终止并删除 CN 单机战斗 ////

fn abort_response(category: i64, response_time: i64) -> Value {
    json!({
        "user_info": {},
        "category_id": category,
        "is_multi": "single",
        "start_time": response_time,
        "quest_name": "",
    })
}

// //// 校验 CN viewer 会话并读取玩家快照 [@x380kkm 2026-07-22] ////
fn get_player_snapshot(
    database: &ServiceDatabase,
    viewer_id: i64,
) -> Result<Result<PlayerSnapshot, HttpResponse>, PersonalServiceError> {
    match database.lookup_viewer_session_player(viewer_id)? {
        ViewerSessionPlayer::Present(snapshot) => Ok(Ok(snapshot)),
        ViewerSessionPlayer::InvalidSession => Ok(Err(battle_error(
            "400 Bad Request",
            "Bad Request",
            "Invalid viewer id.",
        ))),
        ViewerSessionPlayer::MissingPlayer => Ok(Err(battle_error(
            "500 Internal Server Error",
            "Internal Server Error",
            "No player bound to account.",
        ))),
        ViewerSessionPlayer::MissingPlayerData(_) => Ok(Err(battle_error(
            "500 Internal Server Error",
            "Internal Server Error",
            "No player data.",
        ))),
    }
}

fn battle_error(status: &'static str, error: &str, message: &str) -> HttpResponse {
    HttpResponse::json(
        status,
        json!({ "error": error, "message": message }).to_string(),
    )
}
// //// /校验 CN viewer 会话并读取玩家快照 ////

// //// 分派 CN 单机战斗请求 [@x380kkm 2026-07-22] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let response = match request.path() {
        "/api/index.php/single_battle_quest/start" => route_start(request, database),
        "/api/index.php/single_battle_quest/finish" => route_finish(request, database),
        "/api/index.php/single_battle_quest/play_continue" => route_continue(request, database),
        "/api/index.php/single_battle_quest/abort" => route_abort(request, database),
        _ => return None,
    };
    Some(response)
}
// //// /分派 CN 单机战斗请求 ////
