// audience: internal
// # personal-service-cn-multi-room
//
// 该模块实现 CN 本地联机准备, COM 召唤, 恢复, 分享和解散.

use super::{
    authenticate, bad_request, error_response, missing_room_connection, msgpack_response,
    room_connection,
};
use crate::ai_teams::{get_or_create_multiplayer_ai_teams, MultiplayerAiTeam};
use crate::cn::decode_request;
use crate::database::{
    MultiplayerAiMate, MultiplayerAiMateInput, MultiplayerRoom, ServiceDatabase,
};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Deserialize)]
struct PrepareRequest {
    access_token: Option<String>,
    category: i64,
    quest_id: i64,
    room_number: Option<String>,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct RoomRequest {
    #[serde(default)]
    room_number: String,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct RestoreRoomRequest {
    room_number: String,
    room_sequence: i64,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct SummonRequest {
    category_id: i64,
    quest_id: i64,
    room_number: String,
    viewer_id: i64,
}

struct DefaultAiTemplate {
    com_id: i64,
    character_ids: [i64; 3],
    equipment_ids: [i64; 3],
    rank: i64,
    degree_id: i64,
}

const DEFAULT_AI_TEMPLATES: [DefaultAiTemplate; 2] = [
    DefaultAiTemplate {
        com_id: 1,
        character_ids: [131_012, 141_007, 151_001],
        equipment_ids: [300_101, 300_201, 300_301],
        rank: 80,
        degree_id: 1,
    },
    DefaultAiTemplate {
        com_id: 2,
        character_ids: [141_004, 121_002, 161_001],
        equipment_ids: [300_101, 300_201, 300_301],
        rank: 80,
        degree_id: 2_000,
    },
];

pub(super) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let response = match request.path() {
        "/api/index.php/multi_battle_quest/prepare" => prepare(request, database),
        "/api/index.php/multi_battle_quest/summon" => summon(request, database),
        "/api/index.php/multi_battle_quest/restore_room" => restore_room(request, database),
        "/api/index.php/multi_battle_quest/share_room" => share_room(request, database),
        "/api/index.php/multi_battle_quest/disband_room" => disband_room(request, database),
        _ => return None,
    };
    Some(response)
}

fn prepare(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<PrepareRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.category > 0 && body.quest_id > 0 => body,
        Ok(_) | Err(_) => return Ok(bad_request("Invalid request body.")),
    };
    let player = match authenticate(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    let room = resolve_room(
        database,
        body.room_number.as_deref(),
        body.access_token.as_deref(),
    )?;
    let Some(mut room) = room else {
        return msgpack_response(
            database,
            body.viewer_id,
            missing_room_connection(body.room_number.as_deref().unwrap_or_default()),
        );
    };
    if room.category_id != body.category || room.quest_id != body.quest_id {
        return Ok(error_response("409 Conflict", "room_mismatch"));
    }
    let member = database.multiplayer_member(&room.room_number, body.viewer_id)?;
    if member.as_ref().map(|member| member.account_id) != Some(player.account_id) {
        return Ok(error_response("403 Forbidden", "room_access_denied"));
    }
    database.trim_multiplayer_ai_mates_to_capacity(&room.room_number)?;
    room = database
        .multiplayer_room(&room.room_number)?
        .ok_or_else(|| PersonalServiceError::new("prepared multiplayer room disappeared"))?;
    if room.host_account_id == player.account_id {
        let host_entry_time = database.current_server_time_millis()? / 1_000;
        let expiry_anchor_ms = database.current_wall_time_millis()?;
        database.update_multiplayer_host_entry_time(
            &room.room_number,
            player.account_id,
            host_entry_time,
            expiry_anchor_ms,
        )?;
        room = database
            .multiplayer_room(&room.room_number)?
            .ok_or_else(|| PersonalServiceError::new("prepared multiplayer room disappeared"))?;
    }
    msgpack_response(
        database,
        body.viewer_id,
        room_connection(database, &room, body.viewer_id)?,
    )
}

// //// 冻结当前账号的两个 AI 队伍到房间 [@x380kkm 2026-08-22] ////
fn summon(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<SummonRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.category_id > 0
                && body.quest_id > 0
                && super::lobby::is_room_number(&body.room_number) =>
        {
            body
        }
        Ok(_) | Err(_) => return Ok(bad_request("Invalid request body.")),
    };
    let player = match authenticate(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    let room = match database.multiplayer_room(&body.room_number)? {
        Some(room) => room,
        None => return Ok(bad_request("Room doesn't exist.")),
    };
    if room.host_account_id != player.account_id || room.host_viewer_id != body.viewer_id {
        return Ok(error_response("403 Forbidden", "room_access_denied"));
    }
    if room.category_id != body.category_id || room.quest_id != body.quest_id {
        return Ok(error_response("409 Conflict", "room_mismatch"));
    }
    let mut mates: Vec<MultiplayerAiMate> =
        database.trim_multiplayer_ai_mates_to_capacity(&body.room_number)?;
    if mates.is_empty() {
        let prepared_members = database.list_multiplayer_members(&body.room_number)?.len() as i64;
        let available = usize::try_from((3 - prepared_members).clamp(0, 2)).unwrap_or_default();
        let teams = get_or_create_multiplayer_ai_teams(database, player.account_id)?;
        let entry_time = database.current_server_time_millis()?;
        let inputs = teams
            .iter()
            .take(available)
            .enumerate()
            .map(|(index, team)| create_ai_mate_input(team, &room, index + 1, entry_time))
            .collect::<Result<Vec<_>, _>>()?;
        mates = database
            .stage_multiplayer_ai_mates(&room.room_number, player.account_id, &inputs)?
            .ok_or_else(|| PersonalServiceError::new("multiplayer room ownership changed"))?;
    }
    let mut data = Map::from_iter([
        ("mate1".to_owned(), Value::Null),
        ("mate2".to_owned(), Value::Null),
    ]);
    for mate in &mates {
        if mate.room_number != body.room_number
            || mate.owner_account_id != player.account_id
            || mate.snapshot_id.is_empty()
        {
            return Err(PersonalServiceError::new(
                "frozen multiplayer AI mate ownership is invalid",
            ));
        }
        let value = serde_json::from_str::<Value>(&mate.client_mate_json).map_err(|error| {
            PersonalServiceError::new(format!(
                "failed to decode frozen multiplayer AI mate: {error}"
            ))
        })?;
        data.insert(format!("mate{}", mate.position), value);
    }
    msgpack_response(database, body.viewer_id, Value::Object(data))
}
// //// /冻结当前账号的两个 AI 队伍到房间 ////

fn restore_room(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<RestoreRoomRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.room_sequence > 0
                && super::lobby::is_room_number(&body.room_number) =>
        {
            body
        }
        Ok(_) | Err(_) => return Ok(bad_request("Invalid request body.")),
    };
    let player = match authenticate(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    let Some(room) = database.multiplayer_room(&body.room_number)? else {
        let mut missing = missing_room_connection(&body.room_number);
        missing
            .as_object_mut()
            .expect("missing room connection is an object")
            .insert("is_same_room".to_owned(), Value::Bool(true));
        return msgpack_response(database, body.viewer_id, missing);
    };
    if room.room_sequence != body.room_sequence {
        return msgpack_response(
            database,
            body.viewer_id,
            json!({
                "ip_address": super::SESSION_HOST,
                "port": database.multiplayer_session_port(),
                "raising_state": 10,
                "share_room_options": room.share_room_options,
                "is_same_room": false,
            }),
        );
    }
    let member = database.multiplayer_member(&room.room_number, body.viewer_id)?;
    if member.as_ref().map(|member| member.account_id) != Some(player.account_id) {
        return msgpack_response(
            database,
            body.viewer_id,
            json!({
                "ip_address": super::SESSION_HOST,
                "port": database.multiplayer_session_port(),
                "raising_state": 13,
                "share_room_options": room.share_room_options,
                "is_same_room": false,
            }),
        );
    }
    let mut data = room_connection(database, &room, body.viewer_id)?;
    data.as_object_mut()
        .expect("room connection is an object")
        .insert("is_same_room".to_owned(), Value::Bool(true));
    msgpack_response(database, body.viewer_id, data)
}

fn share_room(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<RoomRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(bad_request("Invalid request body.")),
    };
    msgpack_response(database, body.viewer_id, json!({}))
}

fn disband_room(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<RoomRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(bad_request("Invalid request body.")),
    };
    if super::lobby::is_room_number(&body.room_number) {
        if let Ok(player) = authenticate(database, body.viewer_id)? {
            database.disband_multiplayer_room(&body.room_number, player.account_id)?;
        }
    }
    msgpack_response(database, body.viewer_id, json!({}))
}

fn resolve_room(
    database: &ServiceDatabase,
    room_number: Option<&str>,
    access_token: Option<&str>,
) -> Result<Option<MultiplayerRoom>, PersonalServiceError> {
    if let Some(room_number) = room_number {
        return database.multiplayer_room(room_number);
    }
    match access_token {
        Some(access_token) => database.multiplayer_room_by_token(access_token),
        None => Ok(None),
    }
}

fn create_ai_mate_input(
    team: &MultiplayerAiTeam,
    room: &MultiplayerRoom,
    position: usize,
    entry_time: i64,
) -> Result<MultiplayerAiMateInput, PersonalServiceError> {
    let default_template = default_ai_template(team);
    let com_id = default_template
        .map(|template| template.com_id)
        .unwrap_or(900_000_000 + team.team_index);
    let rank = default_template.map(|template| template.rank).unwrap_or(1);
    let degree_id = default_template
        .map(|template| template.degree_id)
        .unwrap_or(1);
    let party = match default_template {
        Some(template) => create_default_client_party(template),
        None => create_client_party(&team.data)?,
    };
    let client_mate = json!({
        "com_id": com_id,
        "rank": rank,
        "party": party,
        "degree_id": degree_id,
    });
    let lobby_player = json!({
        "viewerId": 900_000_000 + position as i64,
        "comId": com_id,
        "name": format!("COM {position}"),
        "rank": rank,
        "degreeId": degree_id,
        "playerRoleKind": 99,
        "party": create_lobby_party(&client_mate["party"]),
        "connectionId": format!("{}-npc-{position}", room.room_number),
        "autoplayMode": false,
        "autoskillMode": 1,
        "autoSpeedLevel": 1,
        "autoStart": false,
        "skillAbilityBehaviorMode": 1,
        "dashBehaviorMode": 1,
        "allowHealFromOtherPlayers": true,
        "state": [0],
        "entryTime": entry_time,
        "isNewbie": false,
        "isHost": false,
        "snapshotId": team.snapshot_id,
        "sourceSlotId": team.slot_id,
        "sourcePartyId": team.party_id,
    });
    Ok(MultiplayerAiMateInput {
        snapshot_id: team.snapshot_id.clone(),
        client_mate_json: serde_json::to_string(&client_mate).map_err(|error| {
            PersonalServiceError::new(format!("failed to encode multiplayer AI mate: {error}"))
        })?,
        lobby_player_json: serde_json::to_string(&lobby_player).map_err(|error| {
            PersonalServiceError::new(format!(
                "failed to encode multiplayer AI lobby player: {error}"
            ))
        })?,
    })
}

// //// 为新手默认空编队提供参考 COM 队伍 [@x380kkm 2026-08-23] ////
fn default_ai_template(team: &MultiplayerAiTeam) -> Option<&'static DefaultAiTemplate> {
    let characters = team.data.get("character_ids").and_then(Value::as_array)?;
    let equipment = team.data.get("equipment_ids").and_then(Value::as_array)?;
    let unison = team
        .data
        .get("unison_character_ids")
        .and_then(Value::as_array)?;
    let is_new_player_party = characters.as_slice() == [json!(1), Value::Null, Value::Null]
        && equipment.iter().all(Value::is_null)
        && unison.iter().all(Value::is_null);
    is_new_player_party
        .then(|| usize::try_from(team.team_index).ok())
        .flatten()
        .and_then(|index| DEFAULT_AI_TEMPLATES.get(index))
}

fn create_default_client_party(template: &DefaultAiTemplate) -> Value {
    json!({
        "characters": template.character_ids.map(|id| json!({
            "id": id,
            "evolution_level": 5,
            "exp": 0,
            "over_limit_step": 0,
            "mana_node_ids": [],
            "ex_boost": null,
        })),
        "unison_characters": [null, null, null],
        "equipments": template.equipment_ids.map(|equipment_id| json!({
            "equipment_id": equipment_id,
            "level": 1,
            "enhancement_level": 0,
        })),
        "ability_soul_ids": [null, null, null],
    })
}
// //// /为新手默认空编队提供参考 COM 队伍 ////

fn create_client_party(snapshot: &Value) -> Result<Value, PersonalServiceError> {
    let character_ids = id_slots(snapshot, "character_ids")?;
    let unison_ids = id_slots(snapshot, "unison_character_ids")?;
    let equipment_ids = id_slots(snapshot, "equipment_ids")?;
    let ability_soul_ids = id_slots(snapshot, "ability_soul_ids")?;
    Ok(json!({
        "characters": character_ids.iter().map(|id| client_character(snapshot, id.as_i64())).collect::<Result<Vec<_>, _>>()?,
        "unison_characters": unison_ids.iter().map(|id| client_character(snapshot, id.as_i64())).collect::<Result<Vec<_>, _>>()?,
        "equipments": equipment_ids.iter().map(|id| client_equipment(snapshot, id.as_i64())).collect::<Result<Vec<_>, _>>()?,
        "ability_soul_ids": ability_soul_ids,
    }))
}

fn id_slots<'a>(snapshot: &'a Value, key: &str) -> Result<&'a Vec<Value>, PersonalServiceError> {
    snapshot
        .get(key)
        .and_then(Value::as_array)
        .filter(|slots| slots.len() == 3)
        .ok_or_else(|| {
            PersonalServiceError::new(format!("multiplayer AI snapshot {key} is invalid"))
        })
}

fn client_character(
    snapshot: &Value,
    character_id: Option<i64>,
) -> Result<Value, PersonalServiceError> {
    let Some(character_id) = character_id else {
        return Ok(Value::Null);
    };
    let character = snapshot
        .get("characters")
        .and_then(Value::as_object)
        .and_then(|characters| characters.get(&character_id.to_string()))
        .or_else(|| {
            snapshot
                .get("unison_characters")
                .and_then(Value::as_object)
                .and_then(|characters| characters.get(&character_id.to_string()))
        })
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("multiplayer AI character is not owned"))?;
    let mana_node_ids = snapshot
        .get("mana_nodes")
        .and_then(Value::as_object)
        .and_then(|nodes| nodes.get(&character_id.to_string()))
        .and_then(Value::as_array)
        .map(|nodes| {
            nodes
                .iter()
                .filter_map(|node| {
                    node.as_i64()
                        .or_else(|| node.get("mana_node_multiplied_id").and_then(Value::as_i64))
                        .or_else(|| node.get("multiplied_id").and_then(Value::as_i64))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut result = json!({
        "id": character_id,
        "mana_node_ids": mana_node_ids,
        "evolution_level": character.get("evolution_level").and_then(Value::as_i64).unwrap_or_default(),
        "exp": character.get("exp").and_then(Value::as_i64).unwrap_or_default(),
        "over_limit_step": character.get("over_limit_step").and_then(Value::as_i64).unwrap_or_default(),
        "ex_boost": null,
    });
    if let Some(ex_boost) = character.get("ex_boost").filter(|value| value.is_object()) {
        result
            .as_object_mut()
            .expect("client character is an object")
            .insert("ex_boost".to_owned(), ex_boost.clone());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::client_character;
    use serde_json::json;

    // //// 读取标准和旧版联机 Mana node 字段 [@x380kkm 2026-08-28] ////
    #[test]
    fn reads_canonical_mana_node_ids_for_client_party() {
        let snapshot = json!({
            "characters": {
                "1": {"evolution_level": 2, "exp": 10, "over_limit_step": 1}
            },
            "mana_nodes": {
                "1": [
                    {"mana_node_multiplied_id": 2201},
                    {"multiplied_id": 2202},
                    2203
                ]
            }
        });

        let character = client_character(&snapshot, Some(1)).expect("client character is built");
        assert_eq!(character["mana_node_ids"], json!([2201, 2202, 2203]));
    }
    // //// /读取标准和旧版联机 Mana node 字段 ////
}

fn client_equipment(
    snapshot: &Value,
    equipment_id: Option<i64>,
) -> Result<Value, PersonalServiceError> {
    let Some(equipment_id) = equipment_id else {
        return Ok(Value::Null);
    };
    let equipment = snapshot
        .get("equipment")
        .and_then(Value::as_object)
        .and_then(|equipment| equipment.get(&equipment_id.to_string()))
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("multiplayer AI equipment is not owned"))?;
    Ok(json!({
        "equipment_id": equipment_id,
        "level": equipment.get("level").and_then(Value::as_i64).unwrap_or(1),
        "enhancement_level": equipment.get("enhancement_level").and_then(Value::as_i64).unwrap_or_default(),
    }))
}

fn create_lobby_party(client_party: &Value) -> Value {
    json!({
        "characters": option_slots(&client_party["characters"], true),
        "unison_characters": option_slots(&client_party["unison_characters"], true),
        "equipments": lobby_equipment_slots(&client_party["equipments"]),
        "abilitySoulIds": option_slots(&client_party["ability_soul_ids"], false),
        "options": null,
    })
}

fn option_slots(value: &Value, character: bool) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|value| {
            if value.is_null() {
                json!([1])
            } else if character {
                let mut value = value.clone();
                let object = value.as_object_mut().expect("AI character is an object");
                let nodes = object
                    .remove("mana_node_ids")
                    .and_then(|nodes| nodes.as_array().cloned())
                    .unwrap_or_default();
                object.insert(
                    "mana_node_ids".to_owned(),
                    Value::Object(
                        nodes
                            .into_iter()
                            .filter_map(|node| node.as_i64())
                            .map(|node| (node.to_string(), Value::from(0)))
                            .collect(),
                    ),
                );
                object.insert("illustration_settings".to_owned(), json!([1]));
                let ex_boost = object
                    .remove("ex_boost")
                    .filter(|value| !value.is_null())
                    .map(|value| json!([0, value]))
                    .unwrap_or_else(|| json!([1]));
                object.insert("ex_boost".to_owned(), ex_boost);
                json!([0, value])
            } else {
                json!([0, value])
            }
        })
        .collect()
}

fn lobby_equipment_slots(value: &Value) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|value| {
            let Some(equipment) = value.as_object() else {
                return json!([1]);
            };
            json!([0, {
                "equipmentId": equipment
                    .get("equipment_id")
                    .and_then(Value::as_i64)
                    .unwrap_or_default(),
                "level": equipment
                    .get("level")
                    .and_then(Value::as_i64)
                    .unwrap_or(1),
                "enhancementLevel": equipment
                    .get("enhancement_level")
                    .and_then(Value::as_i64)
                    .unwrap_or_default(),
            }])
        })
        .collect()
}
