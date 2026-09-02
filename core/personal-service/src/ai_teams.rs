// audience: internal
// # personal-service-ai-teams
//
// 该模块从本地 CN 玩家存档提取队伍候选和两个队伍的白名单数据.
// GET 为未配置槽位选择前两个有效队伍. PUT 追加不可变快照并移动当前 head.
// DELETE 清除当前选择并关闭该槽位的自动选择. 源存档变化不会改写已有快照.

use crate::database::ServiceDatabase;
use crate::database::{AiTeamSnapshot, AiTeamSnapshotInput, AiTeamStoreError, LocalSaveStoreError};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

const LOCAL_SAVES_PREFIX: &str = "/v1/local-saves/";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplaceRequest {
    party_ids: Vec<i64>,
}

enum PartyValidationError {
    NotFound,
    Incomplete,
}

struct PartyCandidate {
    party_id: i64,
    data: Value,
}

#[derive(Clone)]
pub(crate) struct MultiplayerAiTeam {
    pub(crate) snapshot_id: String,
    pub(crate) slot_id: i64,
    pub(crate) team_index: i64,
    pub(crate) party_id: i64,
    pub(crate) data: Value,
}

enum PlayerDataError {
    Response(HttpResponse),
    Storage(PersonalServiceError),
}

// //// 读取或建立当前账号的两个联机 AI 队伍 [@x380kkm 2026-08-22] ////
pub(crate) fn get_or_create_multiplayer_ai_teams(
    database: &mut ServiceDatabase,
    account_id: i64,
) -> Result<Vec<MultiplayerAiTeam>, PersonalServiceError> {
    let slot_id = database
        .multiplayer_current_slot_id(account_id)?
        .ok_or_else(|| PersonalServiceError::new("current account has no local save slot"))?;
    let mut snapshots = database
        .list_ai_team_snapshots(slot_id)?
        .ok_or_else(|| PersonalServiceError::new("current local save slot does not exist"))?;
    if snapshots.is_empty() {
        let (revision_id, player_data) =
            load_current_player_data(database, slot_id).map_err(|error| match error {
                PlayerDataError::Response(_) => {
                    PersonalServiceError::new("current local save cannot provide AI teams")
                }
                PlayerDataError::Storage(error) => error,
            })?;
        let candidates = collect_valid_party_candidates(&player_data);
        if candidates.len() < 2 {
            return Err(PersonalServiceError::new(
                "current local save has fewer than two valid non-empty AI parties",
            ));
        }
        let inputs = snapshot_inputs(candidates.iter().take(2))?;
        snapshots = database
            .replace_ai_team_snapshots(slot_id, &revision_id, &inputs)
            .map_err(map_storage_error)?;
    }
    if snapshots.len() != 2
        || snapshots
            .iter()
            .map(|snapshot| snapshot.team_index)
            .collect::<BTreeSet<_>>()
            != [0_i64, 1_i64].into_iter().collect::<BTreeSet<_>>()
        || snapshots.iter().any(|snapshot| snapshot.slot_id != slot_id)
    {
        return Err(PersonalServiceError::new(
            "current local save AI team heads are invalid",
        ));
    }
    snapshots
        .into_iter()
        .map(|snapshot| {
            let data = serde_json::from_str::<Value>(&snapshot.data_json).map_err(|error| {
                PersonalServiceError::new(format!(
                    "failed to decode multiplayer AI team snapshot: {error}"
                ))
            })?;
            if data.get("party_id").and_then(Value::as_i64) != Some(snapshot.party_id) {
                return Err(PersonalServiceError::new(
                    "multiplayer AI team snapshot party does not match its source",
                ));
            }
            Ok(MultiplayerAiTeam {
                snapshot_id: snapshot.id,
                slot_id: snapshot.slot_id,
                team_index: snapshot.team_index,
                party_id: snapshot.party_id,
                data,
            })
        })
        .collect()
}
// //// /读取或建立当前账号的两个联机 AI 队伍 ////

// //// 分派本地槽位 AI 队伍快照管理 [@x380kkm 2026-08-18] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let Some((slot_id, action)) = parse_path(request.path()) else {
        return None;
    };
    if action != "ai-teams" {
        return None;
    }
    Some(match request.method() {
        "GET" => list(database, slot_id),
        "PUT" => replace(request, database, slot_id),
        "DELETE" => clear(database, slot_id),
        _ => Ok(error_response(
            "405 Method Not Allowed",
            "method_not_allowed",
        )),
    })
}
// //// /分派本地槽位 AI 队伍快照管理 ////

fn parse_path(path: &str) -> Option<(i64, &str)> {
    let suffix = path.strip_prefix(LOCAL_SAVES_PREFIX)?;
    let mut segments = suffix.split('/');
    let slot_id = segments.next()?.parse::<i64>().ok()?;
    if slot_id <= 0 {
        return None;
    }
    let action = segments.next()?;
    if segments.next().is_some() {
        return None;
    }
    Some((slot_id, action))
}

fn list(
    database: &mut ServiceDatabase,
    slot_id: i64,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(mut snapshots) = database.list_ai_team_snapshots(slot_id)? else {
        return Ok(error_response("404 Not Found", "local_save_not_found"));
    };
    let (revision_id, player_data) = match load_current_player_data(database, slot_id) {
        Ok(source) => source,
        Err(PlayerDataError::Response(response)) => return Ok(response),
        Err(PlayerDataError::Storage(error)) => return Err(error),
    };
    let candidates = collect_valid_party_candidates(&player_data);
    let automatic_selection_enabled = database
        .is_ai_team_automatic_selection_enabled(slot_id)?
        .unwrap_or(false);
    if snapshots.is_empty() && automatic_selection_enabled && candidates.len() >= 2 {
        let inputs = snapshot_inputs(candidates.iter().take(2))?;
        snapshots = match database.replace_ai_team_snapshots(slot_id, &revision_id, &inputs) {
            Ok(saved) => saved,
            Err(error) => return map_replace_error(error),
        };
    }
    serialize_json(
        "200 OK",
        response_body(slot_id, snapshots, &candidates, automatic_selection_enabled)?,
    )
}

fn replace(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    slot_id: i64,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(body) = parse_json::<ReplaceRequest>(request) else {
        return Ok(error_response("400 Bad Request", "invalid_ai_team_request"));
    };
    if body.party_ids.len() != 2
        || body.party_ids.iter().any(|party_id| *party_id <= 0)
        || body.party_ids[0] == body.party_ids[1]
    {
        return Ok(error_response(
            "400 Bad Request",
            "invalid_ai_team_party_ids",
        ));
    }

    let (revision_id, player_data) = match load_current_player_data(database, slot_id) {
        Ok(source) => source,
        Err(PlayerDataError::Response(response)) => return Ok(response),
        Err(PlayerDataError::Storage(error)) => return Err(error),
    };
    let candidates = collect_valid_party_candidates(&player_data);
    let mut snapshots = Vec::with_capacity(2);
    for (team_index, party_id) in body.party_ids.iter().enumerate() {
        let data = match extract_party_snapshot(&player_data, *party_id) {
            Ok(data) => data,
            Err(PartyValidationError::NotFound) => {
                return Ok(error_response(
                    "422 Unprocessable Entity",
                    "ai_team_party_not_found",
                ));
            }
            Err(PartyValidationError::Incomplete) => {
                return Ok(error_response(
                    "422 Unprocessable Entity",
                    "ai_team_party_incomplete",
                ));
            }
        };
        let data_json = serde_json::to_string(&data).map_err(|error| {
            PersonalServiceError::new(format!("failed to encode AI team snapshot: {error}"))
        })?;
        snapshots.push(AiTeamSnapshotInput {
            team_index: team_index as i64,
            party_id: *party_id,
            data_json,
        });
    }
    let saved = match database.replace_ai_team_snapshots(slot_id, &revision_id, &snapshots) {
        Ok(snapshots) => snapshots,
        Err(error) => return map_replace_error(error),
    };
    serialize_json("200 OK", response_body(slot_id, saved, &candidates, true)?)
}

fn clear(
    database: &mut ServiceDatabase,
    slot_id: i64,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(_) = database
        .clear_ai_team_snapshots(slot_id)
        .map_err(map_storage_error)?
    else {
        return Ok(error_response("404 Not Found", "local_save_not_found"));
    };
    serialize_json("200 OK", json!({ "slot_id": slot_id, "deleted": true }))
}

// //// 读取槽位当前玩家 revision [@x380kkm 2026-08-20] ////
fn load_current_player_data(
    database: &mut ServiceDatabase,
    slot_id: i64,
) -> Result<(String, Value), PlayerDataError> {
    let revision = database
        .ensure_local_save_revision(slot_id, "AI team selection")
        .map_err(|error| match error {
            LocalSaveStoreError::NotFound => {
                PlayerDataError::Response(error_response("404 Not Found", "local_save_not_found"))
            }
            LocalSaveStoreError::Busy => {
                PlayerDataError::Response(error_response("409 Conflict", "local_save_busy"))
            }
            LocalSaveStoreError::InvalidState => PlayerDataError::Response(error_response(
                "409 Conflict",
                "local_save_invalid_state",
            )),
            LocalSaveStoreError::Storage(error) => PlayerDataError::Storage(error),
        })?;
    let serialized = database
        .local_save_revision_data(slot_id, &revision.id)
        .map_err(PlayerDataError::Storage)?
        .ok_or_else(|| {
            PlayerDataError::Response(error_response(
                "409 Conflict",
                "local_save_revision_missing",
            ))
        })?;
    let player_data = serde_json::from_str::<Value>(&serialized).map_err(|error| {
        PlayerDataError::Storage(PersonalServiceError::new(format!(
            "failed to decode AI team source revision: {error}"
        )))
    })?;
    Ok((revision.id, player_data))
}
// //// /读取槽位当前玩家 revision ////

// //// 提取可用于 AI 编队的队伍候选 [@x380kkm 2026-08-20] ////
fn collect_valid_party_candidates(player_data: &Value) -> Vec<PartyCandidate> {
    let party_ids = player_data
        .get("user_party_group_list")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|groups| groups.values())
        .filter_map(|group| group.get("list").and_then(Value::as_object))
        .flat_map(|parties| parties.keys())
        .filter_map(|party_id| party_id.parse::<i64>().ok())
        .filter(|party_id| *party_id > 0)
        .collect::<BTreeSet<_>>();
    party_ids
        .into_iter()
        .filter_map(|party_id| {
            extract_party_snapshot(player_data, party_id)
                .ok()
                .map(|data| PartyCandidate { party_id, data })
        })
        .collect()
}
// //// /提取可用于 AI 编队的队伍候选 ////

// //// 编码两个队伍候选为快照输入 [@x380kkm 2026-08-20] ////
fn snapshot_inputs<'a>(
    candidates: impl Iterator<Item = &'a PartyCandidate>,
) -> Result<Vec<AiTeamSnapshotInput>, PersonalServiceError> {
    candidates
        .enumerate()
        .map(|(team_index, candidate)| {
            serde_json::to_string(&candidate.data)
                .map(|data_json| AiTeamSnapshotInput {
                    team_index: team_index as i64,
                    party_id: candidate.party_id,
                    data_json,
                })
                .map_err(|error| {
                    PersonalServiceError::new(format!("failed to encode AI team snapshot: {error}"))
                })
        })
        .collect()
}
// //// /编码两个队伍候选为快照输入 ////

fn extract_party_snapshot(
    player_data: &Value,
    party_id: i64,
) -> Result<Value, PartyValidationError> {
    let root = player_data
        .as_object()
        .ok_or(PartyValidationError::Incomplete)?;
    let party_groups = root
        .get("user_party_group_list")
        .and_then(Value::as_object)
        .ok_or(PartyValidationError::Incomplete)?;
    let key = party_id.to_string();
    let mut found = None;
    for group in party_groups.values() {
        let Some(list) = group.get("list").and_then(Value::as_object) else {
            continue;
        };
        if let Some(party) = list.get(&key) {
            if found.is_some() {
                return Err(PartyValidationError::Incomplete);
            }
            found = Some(party);
        }
    }
    let party = found.ok_or(PartyValidationError::NotFound)?;
    let party_object = party.as_object().ok_or(PartyValidationError::Incomplete)?;
    let character_ids = copy_id_array(party_object, "character_ids")?;
    if character_ids.first().map_or(true, Value::is_null) {
        return Err(PartyValidationError::Incomplete);
    }
    let unison_character_ids = copy_id_array(party_object, "unison_character_ids")?;
    let equipment_ids = copy_id_array(party_object, "equipment_ids")?;
    let ability_soul_ids = copy_id_array(party_object, "ability_soul_ids")?;
    let options = party_object
        .get("options")
        .filter(|value| value.is_object())
        .cloned()
        .ok_or(PartyValidationError::Incomplete)?;
    let characters = copy_referenced_entries(root, "user_character_list", &character_ids)?;
    let unison_characters =
        copy_referenced_entries(root, "user_character_list", &unison_character_ids)?;
    let mana_nodes = copy_referenced_mana_nodes(root, &character_ids, &unison_character_ids)?;
    let equipment = copy_referenced_entries(root, "user_equipment_list", &equipment_ids)?;
    let mut snapshot = Map::new();
    snapshot.insert("party_id".to_owned(), Value::from(party_id));
    for key in ["name", "edited", "category"] {
        if let Some(value) = party_object.get(key) {
            snapshot.insert(key.to_owned(), value.clone());
        }
    }
    snapshot.insert("character_ids".to_owned(), Value::Array(character_ids));
    snapshot.insert(
        "unison_character_ids".to_owned(),
        Value::Array(unison_character_ids),
    );
    snapshot.insert("equipment_ids".to_owned(), Value::Array(equipment_ids));
    snapshot.insert(
        "ability_soul_ids".to_owned(),
        Value::Array(ability_soul_ids),
    );
    snapshot.insert("options".to_owned(), options);
    snapshot.insert("characters".to_owned(), Value::Object(characters));
    snapshot.insert(
        "unison_characters".to_owned(),
        Value::Object(unison_characters),
    );
    snapshot.insert("mana_nodes".to_owned(), Value::Object(mana_nodes));
    snapshot.insert("equipment".to_owned(), Value::Object(equipment));
    let snapshot = Value::Object(snapshot);
    (!contains_sensitive_key(&snapshot))
        .then_some(snapshot)
        .ok_or(PartyValidationError::Incomplete)
}

fn copy_id_array(
    party: &Map<String, Value>,
    key: &str,
) -> Result<Vec<Value>, PartyValidationError> {
    let values = party
        .get(key)
        .and_then(Value::as_array)
        .filter(|values| values.len() == 3)
        .ok_or(PartyValidationError::Incomplete)?;
    let mut copied = Vec::with_capacity(values.len());
    for value in values {
        match value {
            Value::Null => copied.push(Value::Null),
            Value::Number(number) if number.as_i64().is_some_and(|id| id > 0) => {
                copied.push(value.clone())
            }
            _ => return Err(PartyValidationError::Incomplete),
        }
    }
    Ok(copied)
}

fn copy_referenced_entries(
    root: &Map<String, Value>,
    list_key: &str,
    ids: &[Value],
) -> Result<Map<String, Value>, PartyValidationError> {
    let list = root
        .get(list_key)
        .and_then(Value::as_object)
        .ok_or(PartyValidationError::Incomplete)?;
    let mut copied = BTreeMap::new();
    for id in ids.iter().filter_map(Value::as_i64) {
        let key = id.to_string();
        let value = list
            .get(&key)
            .filter(|value| value.is_object())
            .cloned()
            .ok_or(PartyValidationError::Incomplete)?;
        copied.insert(key, value);
    }
    Ok(copied.into_iter().collect())
}

fn copy_referenced_mana_nodes(
    root: &Map<String, Value>,
    character_ids: &[Value],
    unison_character_ids: &[Value],
) -> Result<Map<String, Value>, PartyValidationError> {
    let list = root
        .get("user_character_mana_node_list")
        .and_then(Value::as_object)
        .ok_or(PartyValidationError::Incomplete)?;
    let mut copied = BTreeMap::new();
    for id in character_ids
        .iter()
        .chain(unison_character_ids.iter())
        .filter_map(Value::as_i64)
    {
        let key = id.to_string();
        if copied.contains_key(&key) {
            continue;
        }
        let value = match list.get(&key) {
            Some(value) if value.is_array() => value.clone(),
            Some(_) => return Err(PartyValidationError::Incomplete),
            None => Value::Array(Vec::new()),
        };
        copied.insert(key, value);
    }
    Ok(copied.into_iter().collect())
}

fn contains_sensitive_key(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, value)| {
            let key = key.to_ascii_lowercase();
            [
                "token",
                "session",
                "session_token",
                "access_token",
                "management_token",
                "credential",
                "credentials",
                "viewer_id",
                "password",
                "secret",
                "associate_token",
            ]
            .contains(&key.as_str())
                || contains_sensitive_key(value)
        }),
        Value::Array(values) => values.iter().any(contains_sensitive_key),
        _ => false,
    }
}

// //// 返回候选队伍和当前选择状态 [@x380kkm 2026-08-20] ////
fn response_body(
    slot_id: i64,
    snapshots: Vec<AiTeamSnapshot>,
    candidates: &[PartyCandidate],
    automatic_selection_enabled: bool,
) -> Result<Value, PersonalServiceError> {
    let teams = snapshots
        .into_iter()
        .map(snapshot_response)
        .collect::<Result<Vec<_>, _>>()?;
    let selected_party_ids = teams
        .iter()
        .filter_map(|team| team.get("party_id").and_then(Value::as_i64))
        .collect::<Vec<_>>();
    let selection_status = if teams.len() == 2 {
        "ready"
    } else if candidates.len() < 2 {
        "default_template_required"
    } else if !automatic_selection_enabled {
        "manual_selection_required"
    } else {
        "selection_unavailable"
    };
    let candidates = candidates
        .iter()
        .map(|candidate| {
            json!({
                "party_id": candidate.party_id,
                "name": candidate.data.get("name").cloned(),
                "category": candidate.data.get("category").cloned(),
                "character_ids": candidate.data.get("character_ids").cloned(),
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "slot_id": slot_id,
        "teams": teams,
        "candidates": candidates,
        "selected_party_ids": selected_party_ids,
        "selection_status": selection_status,
    }))
}
// //// /返回候选队伍和当前选择状态 ////

fn snapshot_response(snapshot: AiTeamSnapshot) -> Result<Value, PersonalServiceError> {
    let data = serde_json::from_str::<Value>(&snapshot.data_json).map_err(|error| {
        PersonalServiceError::new(format!("failed to decode stored AI team snapshot: {error}"))
    })?;
    Ok(json!({
        "snapshot_id": snapshot.id,
        "slot_id": snapshot.slot_id,
        "team_index": snapshot.team_index,
        "party_id": snapshot.party_id,
        "source_revision_id": snapshot.source_revision_id,
        "created_at": snapshot.created_at,
        "data": data,
    }))
}

fn parse_json<T: serde::de::DeserializeOwned>(request: &HttpRequest) -> Option<T> {
    if !request
        .header("content-type")
        .is_some_and(|value| value.starts_with("application/json"))
    {
        return None;
    }
    serde_json::from_slice(request.body()).ok()
}

fn serialize_json<T: serde::Serialize>(
    status: &'static str,
    value: T,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = serde_json::to_string(&value).map_err(|error| {
        PersonalServiceError::new(format!("failed to encode AI team response: {error}"))
    })?;
    Ok(HttpResponse::json(status, body))
}

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}

fn map_storage_error(error: AiTeamStoreError) -> PersonalServiceError {
    match error {
        AiTeamStoreError::Storage(error) => error,
        AiTeamStoreError::NotFound => PersonalServiceError::new("local save was not found"),
        AiTeamStoreError::InvalidState => PersonalServiceError::new("AI team state is invalid"),
    }
}

fn map_replace_error(error: AiTeamStoreError) -> Result<HttpResponse, PersonalServiceError> {
    match error {
        AiTeamStoreError::NotFound => Ok(error_response("404 Not Found", "local_save_not_found")),
        AiTeamStoreError::InvalidState => Ok(error_response(
            "409 Conflict",
            "ai_team_snapshot_invalid_state",
        )),
        AiTeamStoreError::Storage(error) => Err(error),
    }
}
