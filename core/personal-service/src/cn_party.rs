// audience: internal
// # personal-service-cn-party
//
// 该模块实现 CN 编队编辑协议. 服务端只保留玩家拥有的角色和装备, 并保存主编队选择.

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_player::{ensure_normal_party_groups, rebuild_favorite_party_group_list};
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, player_snapshot, require_object, require_root,
};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};

const PUBLISHED_PARTY_CODE: &str = "https://www.howLongCanThisBe?=+-.comhttps://www.howLongCanThisBe?=+-.comhttps://www.howLongCanThisBe?=+-.com";
const MAX_PARTY_ID: i64 = 120;
const DEFAULT_PARTY_GROUP_COLOR_ID: i64 = 15;

#[derive(Deserialize)]
struct PartyEditRequest {
    viewer_id: i64,
    main_party_id: i64,
    #[serde(rename = "use_party_group_edit")]
    _use_party_group_edit: bool,
    #[serde(rename = "ignore_ngword")]
    _ignore_ngword: bool,
    api_count: i64,
    party_info_list: Vec<PartyInfo>,
}

#[derive(Deserialize)]
struct PartyInfo {
    party_edited: bool,
    party_category: i64,
    party_name: String,
    party_id: i64,
    #[serde(default)]
    current_battle_power: i64,
    #[serde(default)]
    before_battle_power: i64,
    unison_character_ids: Vec<Option<i64>>,
    equipment_ids: Vec<Option<i64>>,
    character_ids: Vec<Option<i64>>,
    ability_soul_ids: Vec<Option<i64>>,
    options: PartyOptions,
}

#[derive(Deserialize)]
struct PartyOptions {
    allow_other_players_to_heal_me: bool,
}

#[derive(Deserialize)]
struct PublishRequest {
    viewer_id: i64,
}

#[derive(Deserialize)]
struct ReferRequest {
    viewer_id: i64,
    party_code: String,
}

#[derive(Deserialize)]
struct CheckWordRequest {
    viewer_id: i64,
    word: String,
}

// //// 分派 CN 编队编辑请求 [@x380kkm 2026-07-24] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    let response = match request.path() {
        "/api/index.php/party/edit" => edit(request, database),
        "/api/index.php/party/publish" => publish(request, database),
        "/api/index.php/party/refer" => refer(request, database),
        "/api/index.php/party/check_word" => check_word(request, database),
        _ => return None,
    };
    Some(response)
}
// //// /分派 CN 编队编辑请求 ////

// //// 返回当前编队的本地分享代码 [@x380kkm 2026-08-22] ////
fn publish(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<PublishRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    match player_snapshot(database, body.viewer_id)? {
        Ok(_) => {}
        Err(response) => return Ok(response),
    }
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({"party_code": PUBLISHED_PARTY_CODE}),
    )
}
// //// /返回当前编队的本地分享代码 ////

// //// 按本地分享代码返回当前编队 [@x380kkm 2026-08-24] ////
fn refer(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ReferRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.party_code == PUBLISHED_PARTY_CODE => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "party_code_not_found")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let player_data = decode_player_data(&snapshot.data)?;
    let root = player_data
        .as_object()
        .ok_or_else(|| PersonalServiceError::new("stored CN player data is not an object"))?;
    let user_info = require_object_ref(root, "user_info")?;
    let party_id = user_info
        .get("party_slot")
        .and_then(Value::as_i64)
        .unwrap_or(1)
        .max(1);
    let party = find_party(
        require_object_ref(root, "user_party_group_list")?,
        PartyLocation::from_global_id(party_id),
    )
    .and_then(Value::as_object)
    .ok_or_else(|| PersonalServiceError::new("stored CN active party is missing"))?;
    let characters = require_object_ref(root, "user_character_list")?;
    let equipment = require_object_ref(root, "user_equipment_list")?;
    let character_ids = party_slots(party, "character_ids")?;
    let unison_character_ids = party_slots(party, "unison_character_ids")?;
    let equipment_ids = party_slots(party, "equipment_ids")?;
    let response_time = server_time(database)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        json!({
            "battle_party": {
                "ability_soul_ids": required_array(party, "ability_soul_ids")?,
                "characters": character_ids
                    .into_iter()
                    .map(|id| refer_character(characters, id))
                    .collect::<Result<Vec<_>, _>>()?,
                "equipments": equipment_ids
                    .into_iter()
                    .map(|id| refer_equipment(equipment, id))
                    .collect::<Result<Vec<_>, _>>()?,
                "unison_characters": unison_character_ids
                    .into_iter()
                    .map(|id| refer_character(characters, id))
                    .collect::<Result<Vec<_>, _>>()?,
            },
            "party_name": party.get("name").and_then(Value::as_str).unwrap_or_default(),
        }),
    )
}
// //// /按本地分享代码返回当前编队 ////

// //// 校验编队名称文本 [@x380kkm 2026-08-22] ////
fn check_word(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<CheckWordRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.word.chars().count() <= 100 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    match player_snapshot(database, body.viewer_id)? {
        Ok(_) => {}
        Err(response) => return Ok(response),
    }
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({"check_passed": true}),
    )
}
// //// /校验编队名称文本 ////

// //// 保存 CN 编队编辑 [@x380kkm 2026-08-28] ////
fn edit(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<PartyEditRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && (1..=MAX_PARTY_ID).contains(&body.main_party_id)
                && body.api_count >= 0 =>
        {
            body
        }
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    ensure_normal_party_groups(root)?;
    let character_ids = owned_ids(root, "user_character_list")?;
    let equipment_ids = owned_ids(root, "user_equipment_list")?;
    let party_groups = require_object(root, "user_party_group_list")?;
    for party in &body.party_info_list {
        if !(1..=MAX_PARTY_ID).contains(&party.party_id)
            || party.party_category < 0
            || party.unison_character_ids.len() > 3
            || party.equipment_ids.len() > 3
            || party.character_ids.len() > 3
            || party.ability_soul_ids.len() > 3
        {
            return Ok(error_response("400 Bad Request", "invalid_party_info"));
        }
        let location = PartyLocation::from_global_id(party.party_id);
        let party_object = get_or_create_party_mut(party_groups, location)?;
        party_object.insert("edited".to_owned(), Value::from(party.party_edited));
        party_object.insert("name".to_owned(), Value::String(party.party_name.clone()));
        party_object.insert(
            "current_battle_power".to_owned(),
            Value::from(party.current_battle_power),
        );
        party_object.insert(
            "before_battle_power".to_owned(),
            Value::from(party.before_battle_power),
        );
        party_object.insert(
            "character_ids".to_owned(),
            Value::Array(filter_owned(&party.character_ids, &character_ids)),
        );
        party_object.insert(
            "unison_character_ids".to_owned(),
            Value::Array(filter_owned(&party.unison_character_ids, &character_ids)),
        );
        party_object.insert(
            "equipment_ids".to_owned(),
            Value::Array(filter_owned(&party.equipment_ids, &equipment_ids)),
        );
        party_object.insert(
            "ability_soul_ids".to_owned(),
            Value::Array(
                party
                    .ability_soul_ids
                    .iter()
                    .copied()
                    .map(|value| value.map(Value::from).unwrap_or(Value::Null))
                    .collect(),
            ),
        );
        let options = party_object
            .entry("options".to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
        let options = options
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN party options are invalid"))?;
        options.insert(
            "allow_other_players_to_heal_me".to_owned(),
            Value::from(party.options.allow_other_players_to_heal_me),
        );
    }
    let user_info = require_object(root, "user_info")?;
    user_info.insert("party_slot".to_owned(), Value::from(body.main_party_id));
    rebuild_favorite_party_group_list(root)?;
    let response_time = server_time(database)?;
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        json!({"mail_arrived": false}),
    )
}
// //// /保存 CN 编队编辑 ////

fn owned_ids(
    root: &Map<String, Value>,
    key: &str,
) -> Result<std::collections::BTreeSet<i64>, PersonalServiceError> {
    let values = require_object_ref(root, key)?;
    values
        .keys()
        .map(|key| {
            key.parse::<i64>()
                .map_err(|_| PersonalServiceError::new(format!("stored CN {key} id is invalid")))
        })
        .collect()
}

fn require_object_ref<'a>(
    root: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a Map<String, Value>, PersonalServiceError> {
    root.get(key)
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new(format!("stored CN {key} is missing")))
}

fn required_array(
    object: &Map<String, Value>,
    key: &str,
) -> Result<Vec<Value>, PersonalServiceError> {
    object
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| PersonalServiceError::new(format!("stored CN {key} is missing")))
}

fn party_slots(
    party: &Map<String, Value>,
    key: &str,
) -> Result<Vec<Option<i64>>, PersonalServiceError> {
    Ok(required_array(party, key)?
        .into_iter()
        .map(|value| value.as_i64())
        .collect())
}

fn refer_character(
    characters: &Map<String, Value>,
    character_id: Option<i64>,
) -> Result<Value, PersonalServiceError> {
    let Some(character_id) = character_id else {
        return Ok(Value::Null);
    };
    let character = characters
        .get(&character_id.to_string())
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("stored CN party character is missing"))?;
    Ok(json!({
        "evolution_level": character
            .get("evolution_level")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        "ex_boost": character.get("ex_boost").cloned().unwrap_or(Value::Null),
        "exp": character.get("exp").and_then(Value::as_i64).unwrap_or_default(),
        "id": character_id,
        "illustration_settings": character
            .get("illustration_settings")
            .cloned()
            .unwrap_or(Value::Null),
        "mana_node_ids": Value::Null,
        "over_limit_step": character
            .get("over_limit_step")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
    }))
}

fn refer_equipment(
    equipment: &Map<String, Value>,
    equipment_id: Option<i64>,
) -> Result<Value, PersonalServiceError> {
    let Some(equipment_id) = equipment_id else {
        return Ok(Value::Null);
    };
    let stored = equipment
        .get(&equipment_id.to_string())
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("stored CN party equipment is missing"))?;
    Ok(json!({
        "equipment_id": equipment_id,
        "level": stored.get("level").and_then(Value::as_i64).unwrap_or(1),
    }))
}

fn filter_owned(values: &[Option<i64>], owned: &std::collections::BTreeSet<i64>) -> Vec<Value> {
    values
        .iter()
        .map(|value| match value {
            Some(id) if owned.contains(id) => Value::from(*id),
            _ => Value::Null,
        })
        .collect()
}

#[derive(Clone, Copy)]
struct PartyLocation {
    group_id: i64,
    slot: i64,
}

impl PartyLocation {
    fn from_global_id(party_id: i64) -> Self {
        Self {
            group_id: (party_id - 1) / 10 + 1,
            slot: (party_id - 1) % 10 + 1,
        }
    }

    fn global_id(self) -> i64 {
        (self.group_id - 1) * 10 + self.slot
    }
}

// //// 读取或创建全局编队槽 [@x380kkm 2026-08-28] ////
fn get_or_create_party_mut(
    groups: &mut Map<String, Value>,
    location: PartyLocation,
) -> Result<&mut Map<String, Value>, PersonalServiceError> {
    let group = groups
        .entry(location.group_id.to_string())
        .or_insert_with(|| {
            json!({
                "color_id": DEFAULT_PARTY_GROUP_COLOR_ID,
                "list": {},
            })
        })
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN party group is invalid"))?;
    let parties = group
        .entry("list".to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN party list is invalid"))?;
    parties
        .entry(location.global_id().to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN party data is invalid"))
}
// //// /读取或创建全局编队槽 ////

fn find_party(groups: &Map<String, Value>, location: PartyLocation) -> Option<&Value> {
    groups
        .get(&location.group_id.to_string())?
        .get("list")?
        .as_object()?
        .get(&location.global_id().to_string())
}

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}
