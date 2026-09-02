// audience: internal
// # personal-service-cn-multiplayer-transport
//
// 该模块读写 NUL 分隔 JSON 帧并序列化大厅成员.

use super::SessionClient;
use crate::database::{MultiplayerMember, MultiplayerRoom, ServiceDatabase};
use crate::PersonalServiceError;
use getrandom::getrandom;
use serde_json::{json, Value};
use std::io::{ErrorKind, Read, Write};

const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;
const READ_CHUNK_BYTES: usize = 64 * 1024;

pub(super) fn read_client_frames(
    client_index: usize,
    client: &mut SessionClient,
    frames: &mut Vec<(usize, Value)>,
) -> Result<bool, PersonalServiceError> {
    let mut did_work = false;
    let mut chunk = [0_u8; READ_CHUNK_BYTES];
    loop {
        match client.stream.read(&mut chunk) {
            Ok(0) => {
                client.peer_closed = true;
                break;
            }
            Ok(read) => {
                did_work = true;
                client.buffer.extend_from_slice(&chunk[..read]);
                if client.buffer.len() > MAX_FRAME_BYTES {
                    client.peer_closed = true;
                    break;
                }
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => break,
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::ConnectionReset | ErrorKind::BrokenPipe
                ) =>
            {
                client.peer_closed = true;
                break;
            }
            Err(error) => {
                return Err(PersonalServiceError::new(format!(
                    "failed to read multiplayer session: {error}"
                )));
            }
        }
    }
    while let Some(separator) = client.buffer.iter().position(|byte| *byte == 0) {
        let raw = client.buffer.drain(..separator).collect::<Vec<_>>();
        client.buffer.drain(..1);
        if raw.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let frame = match serde_json::from_slice::<Value>(&raw) {
            Ok(frame) => frame,
            Err(_) => {
                client.close_after_write = true;
                continue;
            }
        };
        frames.push((client_index, frame));
    }
    Ok(did_work)
}

pub(super) fn flush_client(client: &mut SessionClient) -> Result<bool, PersonalServiceError> {
    let mut wrote = false;
    while !client.pending_write.is_empty() {
        match client.stream.write(&client.pending_write) {
            Ok(0) => {
                client.peer_closed = true;
                break;
            }
            Ok(count) => {
                client.pending_write.drain(..count);
                client.flushed_write_bytes = client
                    .flushed_write_bytes
                    .checked_add(u64::try_from(count).map_err(|_| {
                        PersonalServiceError::new("multiplayer flushed byte count overflow")
                    })?)
                    .ok_or_else(|| {
                        PersonalServiceError::new("multiplayer flushed byte count overflow")
                    })?;
                wrote = true;
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => break,
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::ConnectionReset | ErrorKind::BrokenPipe
                ) =>
            {
                client.peer_closed = true;
                break;
            }
            Err(error) => {
                return Err(PersonalServiceError::new(format!(
                    "failed to write multiplayer session: {error}"
                )));
            }
        }
    }
    if client.peer_closed && !client.pending_write.is_empty() {
        discard_pending_writes(client);
    }
    if client.close_after_write && client.pending_write.is_empty() {
        let _ = client.stream.shutdown(std::net::Shutdown::Both);
        client.peer_closed = true;
    }
    Ok(wrote)
}

pub(super) fn queue_frame(
    client: &mut SessionClient,
    frame: &Value,
) -> Result<(), PersonalServiceError> {
    let mut encoded = Vec::new();
    serde_json::to_writer(&mut encoded, frame).map_err(|error| {
        PersonalServiceError::new(format!(
            "failed to encode multiplayer session frame: {error}"
        ))
    })?;
    encoded.push(0);
    let encoded_bytes = u64::try_from(encoded.len())
        .map_err(|_| PersonalServiceError::new("multiplayer queued byte count overflow"))?;
    client.queued_write_bytes = client
        .queued_write_bytes
        .checked_add(encoded_bytes)
        .ok_or_else(|| PersonalServiceError::new("multiplayer queued byte count overflow"))?;
    client.pending_write.extend(encoded);
    Ok(())
}

pub(super) fn discard_pending_writes(client: &mut SessionClient) {
    client.pending_write.clear();
    client
        .pending_room_notice_deliveries
        .retain(|delivery| delivery.flush_after_bytes <= client.flushed_write_bytes);
    client
        .pending_battle_start_deliveries
        .retain(|delivery| delivery.flush_after_bytes <= client.flushed_write_bytes);
    client.queued_write_bytes = client.flushed_write_bytes;
}

pub(super) fn meeting_command(frame: &Value) -> Option<&Vec<Value>> {
    let frame = frame.as_array()?;
    (frame.len() == 2 && frame[0].as_i64() == Some(0))
        .then(|| frame[1].as_array())
        .flatten()
}

pub(super) fn value_i64(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_i64))
        .filter(|value| *value > 0)
}

pub(super) fn value_string(
    object: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<String> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

pub(super) fn random_connection_id() -> Result<String, PersonalServiceError> {
    let mut bytes = [0_u8; 16];
    getrandom(&mut bytes).map_err(|error| {
        PersonalServiceError::new(format!(
            "failed to create multiplayer connection id: {error}"
        ))
    })?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub(super) fn lobby_roster(
    database: &ServiceDatabase,
    room_number: &str,
    include_ai: bool,
    ai_ready: bool,
) -> Result<Vec<Value>, PersonalServiceError> {
    let mut roster = database
        .list_multiplayer_members(room_number)?
        .into_iter()
        .filter(|member| member.entered)
        .filter_map(|member| {
            member
                .lobby_player_json
                .map(|serialized| (serialized, member.ready))
        })
        .map(|(serialized, ready)| {
            let mut player = serde_json::from_str::<Value>(&serialized).map_err(|error| {
                PersonalServiceError::new(format!(
                    "failed to decode multiplayer lobby player: {error}"
                ))
            })?;
            set_lobby_state(&mut player, ready)?;
            Ok(player)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if include_ai {
        roster.extend(
            database
                .list_multiplayer_ai_mates(room_number)?
                .into_iter()
                .map(|mate| {
                    let mut player = serde_json::from_str::<Value>(&mate.lobby_player_json)
                        .map_err(|error| {
                            PersonalServiceError::new(format!(
                                "failed to decode multiplayer AI lobby player: {error}"
                            ))
                        })?;
                    set_lobby_state(&mut player, ai_ready)?;
                    Ok(player)
                })
                .collect::<Result<Vec<_>, PersonalServiceError>>()?,
        );
    }
    Ok(roster)
}

fn set_lobby_state(player: &mut Value, ready: bool) -> Result<(), PersonalServiceError> {
    player
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("multiplayer lobby player is not an object"))?
        .insert("state".to_owned(), json!([i64::from(ready)]));
    Ok(())
}

pub(super) fn lobby_room(
    room: &MultiplayerRoom,
    fallback_connection_id: &str,
    roster: &[Value],
) -> Value {
    let host = roster
        .iter()
        .find(|player| player.get("isHost").and_then(Value::as_bool) == Some(true));
    json!({
        "roomNumber": room.room_number,
        "establisherConnectionId": host
            .and_then(|host| host.get("connectionId"))
            .and_then(Value::as_str)
            .unwrap_or(fallback_connection_id),
        "establisherName": host
            .and_then(|host| host.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "establisherCharacter": host
            .and_then(|host| host.get("mainCharacterId"))
            .and_then(Value::as_i64)
            .unwrap_or(room.host_main_character_id),
        "questCategory": room.category_id,
        "questId": room.quest_id,
        "status": 2,
    })
}

pub(super) fn all_human_members_ready(
    database: &ServiceDatabase,
    room_number: &str,
) -> Result<bool, PersonalServiceError> {
    let members: Vec<MultiplayerMember> = database.list_multiplayer_members(room_number)?;
    let entered = members
        .iter()
        .filter(|member| member.entered)
        .collect::<Vec<_>>();
    Ok(!entered.is_empty() && entered.iter().all(|member| member.ready))
}
