// audience: internal
// # personal-service-cn-player
//
// 该模块加载 Node 行为基准生成的 CN 默认玩家数据.
// 每个账号在 SQLite 中保存独立副本和状态时间字段.
// load 响应的未完成战斗列表从数据库状态派生.

use crate::cn_asset::{available_asset_version, ArchiveDigestCache, ClientPlatform};
use crate::cn_tutorial::create_stored_character;
use crate::database::UnfinishedQuest;
use crate::PersonalServiceError;
use serde_json::{json, Map, Number, Value};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::OnceLock;

const DEFAULT_PLAYER_DATA: &str = include_str!("../assets/cn-default-player.json");
const DEFAULT_ASSOCIATE_TOKEN: &str = "associate_token";
const DAILY_WEEK_QUEST_CATEGORY: i64 = 6;
const DAILY_WEEK_DRAWN_QUEST_COUNT: usize = 114;
const MAIN_QUEST_CATEGORY: i64 = 1;
const EXP_POOL_INCREMENT_LIMIT: i64 = 100_000;
const TRIGGERED_UI_TUTORIAL_ID: i64 = 12;
const DEFAULT_TRIGGERED_TUTORIAL_IDS: [i64; 30] = [
    TRIGGERED_UI_TUTORIAL_ID,
    55,
    57,
    6,
    8,
    22,
    52,
    5,
    4,
    18,
    19,
    17,
    9,
    13,
    14,
    58,
    51,
    101,
    700,
    53,
    60,
    500,
    2,
    10,
    20,
    11,
    31,
    54,
    29,
    70,
];
const DEFAULT_TUTORIAL_GACHA_CHARACTER_ID: i64 = 251001;
const DEFAULT_TUTORIAL_REWARD_CHARACTER_ID: i64 = 243001;
const DEFAULT_STAMINA: i64 = 10;
const NORMAL_PARTY_GROUP_COUNT: i64 = 12;
const PARTY_SLOTS_PER_GROUP: i64 = 10;
const DEFAULT_PARTY_GROUP_COLOR_ID: i64 = 15;
const DEFAULT_PARTY_NAMES: [&str; PARTY_SLOTS_PER_GROUP as usize] = [
    "Party A", "Party B", "Party C", "Party D", "Party E", "Party F", "Party G", "Party H",
    "Party I", "Party J",
];
const INTERNAL_PLAYER_STATE_FIELDS: [&str; 15] = [
    "cn_activity_state",
    "character_clear_counts",
    "character_leader_clear_counts",
    "character_multi_clear_counts",
    "character_leader_multi_clear_counts",
    "character_leader_power_flip_counts",
    "party_member_co_clear_counts",
    "party_race_clear_counts",
    "encyclopedia_list",
    "profile_settings",
    "shop_campaign_lineup_id",
    "shop_purchase_count_baselines",
    "shop_purchase_counts",
    "shop_purchase_windows",
    "pending_ex_boost",
];
const INTERNAL_QUEST_PROGRESS_FIELDS: [&str; 1] = ["leader_character_id"];
static DEFAULT_DAILY_WEEK_DRAWN_QUESTS: OnceLock<Result<Vec<Value>, String>> = OnceLock::new();

pub(crate) struct PreparedPlayerData {
    pub(crate) response: Value,
    pub(crate) snapshot: String,
}

// //// 创建带账号初始时间的默认玩家数据 [@x380kkm 2026-07-22] ////
pub(crate) fn create_default_player_data(
    server_time: i64,
    client_time: &str,
) -> Result<String, PersonalServiceError> {
    let mut player_data = decode_player_data(DEFAULT_PLAYER_DATA)?;
    set_initial_times(&mut player_data, server_time, client_time)?;
    if std::env::var_os("CN_REFERENCE_PLAYER_BASELINE").is_none() {
        set_completed_tutorial_state(&mut player_data, server_time)?;
    }
    ensure_normal_party_groups(response_root(&mut player_data)?)?;
    ensure_party_battle_power_fields(&mut player_data)?;
    rebuild_favorite_party_group_list(response_root(&mut player_data)?)?;
    ensure_client_config_fields(&mut player_data)?;
    encode_player_data(&player_data)
}
// //// /创建带账号初始时间的默认玩家数据 ////

// //// 更新持久化玩家状态并组装当前 viewer 响应 [@x380kkm 2026-08-23] ////
pub(crate) fn prepare_player_data(
    serialized: &str,
    viewer_id: i64,
    database: &crate::database::ServiceDatabase,
    asset_root: &Path,
    override_root: &Path,
    resource_version: Option<&str>,
    platform: ClientPlatform,
    digest_cache: &mut ArchiveDigestCache,
    server_time: i64,
    client_time: &str,
    unfinished_quest: Option<&UnfinishedQuest>,
) -> Result<PreparedPlayerData, PersonalServiceError> {
    let mut player_data = decode_player_data(serialized)?;
    ensure_associate_token(response_root(&mut player_data)?);
    update_login_state(&mut player_data, server_time, client_time)?;
    prepare_quest_progress(&mut player_data)?;
    ensure_normal_party_groups(response_root(&mut player_data)?)?;
    ensure_party_battle_power_fields(&mut player_data)?;
    rebuild_favorite_party_group_list(response_root(&mut player_data)?)?;
    ensure_client_config_fields(&mut player_data)?;
    let snapshot = encode_player_data(&player_data)?;

    let mut response = player_data;
    let available_asset_version = available_asset_version(
        database,
        asset_root,
        override_root,
        resource_version,
        platform,
        digest_cache,
    )?;
    let root = response_root(&mut response)?;
    for field in INTERNAL_PLAYER_STATE_FIELDS {
        root.remove(field);
    }
    remove_internal_quest_progress_fields(root)?;
    append_missing_daily_week_drawn_quests(root)?;
    root.insert(
        "available_asset_version".to_owned(),
        Value::String(available_asset_version),
    );
    let (unfinished_quest_list, unfinished_multi_quest_list) = match unfinished_quest {
        Some(quest) => {
            let entry = json!({
                "play_id": quest.play_id,
                "continue_count": quest.continue_count,
            });
            if quest.is_multi {
                (Vec::new(), vec![entry])
            } else {
                (vec![entry], Vec::new())
            }
        }
        None => (Vec::new(), Vec::new()),
    };
    root.insert(
        "unfinished_quest_list".to_owned(),
        Value::Array(unfinished_quest_list),
    );
    root.insert(
        "unfinished_multi_quest_list".to_owned(),
        Value::Array(unfinished_multi_quest_list),
    );
    let tutorial_completed = is_tutorial_completed(root);
    if tutorial_completed {
        root.insert("user_tutorial".to_owned(), Value::Null);
    } else {
        let tutorial = root
            .get_mut("user_tutorial")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| PersonalServiceError::new("stored CN tutorial data is missing"))?;
        tutorial.insert(
            "viewer_id".to_owned(),
            Value::Number(Number::from(viewer_id)),
        );
    }
    require_characters(root)?;
    Ok(PreparedPlayerData { response, snapshot })
}
// //// /更新持久化玩家状态并组装当前 viewer 响应 ////

// //// 校验并更新持久化的玩家 JSON [@x380kkm 2026-07-22] ////
fn decode_player_data(serialized: &str) -> Result<Value, PersonalServiceError> {
    let player_data = serde_json::from_str::<Value>(serialized).map_err(|error| {
        PersonalServiceError::new(format!("failed to decode stored CN player data: {error}"))
    })?;
    if !player_data.is_object() {
        return Err(PersonalServiceError::new(
            "stored CN player data is not an object",
        ));
    }
    Ok(player_data)
}

fn encode_player_data(player_data: &Value) -> Result<String, PersonalServiceError> {
    serde_json::to_string(player_data).map_err(|error| {
        PersonalServiceError::new(format!("failed to encode stored CN player data: {error}"))
    })
}

fn response_root(player_data: &mut Value) -> Result<&mut Map<String, Value>, PersonalServiceError> {
    player_data
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN player data is not an object"))
}

// //// 补齐载入存档的实例关联标识 [@x380kkm 2026-09-01] ////
fn ensure_associate_token(root: &mut Map<String, Value>) {
    if !root.get("associate_token").is_some_and(Value::is_string) {
        root.insert(
            "associate_token".to_owned(),
            Value::String(DEFAULT_ASSOCIATE_TOKEN.to_owned()),
        );
    }
}
// //// /补齐载入存档的实例关联标识 ////

// //// 补齐载入响应的每日周常随机表 [@x380kkm 2026-08-27] ////
fn append_missing_daily_week_drawn_quests(
    root: &mut Map<String, Value>,
) -> Result<(), PersonalServiceError> {
    let drawn_quests = root
        .get_mut("drawn_quest_list")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN drawn quest list is missing"))?;
    let mut existing_ids = drawn_quests
        .iter()
        .filter(|quest| {
            quest.get("category_id").and_then(Value::as_i64) == Some(DAILY_WEEK_QUEST_CATEGORY)
        })
        .filter_map(|quest| quest.get("quest_id").and_then(Value::as_i64))
        .collect::<BTreeSet<_>>();

    for quest in default_daily_week_drawn_quests()? {
        let quest_id = quest
            .get("quest_id")
            .and_then(Value::as_i64)
            .ok_or_else(|| PersonalServiceError::new("default CN drawn quest id is invalid"))?;
        if existing_ids.insert(quest_id) {
            drawn_quests.push(quest.clone());
        }
    }
    Ok(())
}

fn default_daily_week_drawn_quests() -> Result<&'static [Value], PersonalServiceError> {
    let quests = DEFAULT_DAILY_WEEK_DRAWN_QUESTS.get_or_init(|| {
        let player_data = serde_json::from_str::<Value>(DEFAULT_PLAYER_DATA)
            .map_err(|error| format!("failed to decode default CN player data: {error}"))?;
        let drawn_quests = player_data
            .get("drawn_quest_list")
            .and_then(Value::as_array)
            .ok_or_else(|| "default CN drawn quest list is missing".to_owned())?;
        let daily_week_quests = drawn_quests
            .iter()
            .filter(|quest| {
                quest.get("category_id").and_then(Value::as_i64) == Some(DAILY_WEEK_QUEST_CATEGORY)
            })
            .cloned()
            .collect::<Vec<_>>();
        if daily_week_quests.len() != DAILY_WEEK_DRAWN_QUEST_COUNT {
            return Err(format!(
                "default CN daily week drawn quest list contains {} entries",
                daily_week_quests.len()
            ));
        }
        Ok(daily_week_quests)
    });
    quests
        .as_ref()
        .map(Vec::as_slice)
        .map_err(|error| PersonalServiceError::new(error.clone()))
}
// //// /补齐载入响应的每日周常随机表 ////

fn remove_internal_quest_progress_fields(
    root: &mut Map<String, Value>,
) -> Result<(), PersonalServiceError> {
    let Some(quest_progress) = root.get_mut("quest_progress") else {
        return Ok(());
    };
    let quest_progress = quest_progress
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN quest progress is invalid"))?;
    for progress_list in quest_progress.values_mut() {
        let progress_list = progress_list
            .as_array_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN quest category is invalid"))?;
        for progress in progress_list {
            let progress = progress.as_object_mut().ok_or_else(|| {
                PersonalServiceError::new("stored CN quest progress entry is invalid")
            })?;
            for field in INTERNAL_QUEST_PROGRESS_FIELDS {
                progress.remove(field);
            }
        }
    }
    Ok(())
}

fn set_initial_times(
    player_data: &mut Value,
    server_time: i64,
    client_time: &str,
) -> Result<(), PersonalServiceError> {
    let root = response_root(player_data)?;
    let user_info = require_user_info(root)?;
    user_info.insert(
        "stamina".to_owned(),
        Value::Number(Number::from(DEFAULT_STAMINA)),
    );
    user_info.insert(
        "stamina_heal_time".to_owned(),
        Value::Number(Number::from(server_time)),
    );
    user_info.insert(
        "exp_pooled_time".to_owned(),
        Value::Number(Number::from(server_time)),
    );
    user_info.insert(
        "last_login_time".to_owned(),
        Value::String(client_time.to_owned()),
    );
    let characters = require_characters_mut(root)?;
    for character in characters.values_mut().filter_map(Value::as_object_mut) {
        character.insert(
            "join_time".to_owned(),
            Value::Number(Number::from(server_time)),
        );
        character.insert(
            "update_time".to_owned(),
            Value::Number(Number::from(server_time)),
        );
    }
    Ok(())
}

// //// 维护教程与主线进度 [@x380kkm 2026-08-23] ////
fn set_completed_tutorial_state(
    player_data: &mut Value,
    server_time: i64,
) -> Result<(), PersonalServiceError> {
    let root = response_root(player_data)?;
    let tutorial = root
        .get_mut("user_tutorial")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN tutorial data is missing"))?;
    tutorial.insert("tutorial_step".to_owned(), Value::Number(Number::from(6)));
    tutorial.insert("skip_flag".to_owned(), Value::Bool(true));

    root.insert(
        "user_triggered_tutorial".to_owned(),
        Value::Array(
            DEFAULT_TRIGGERED_TUTORIAL_IDS
                .into_iter()
                .map(Value::from)
                .collect(),
        ),
    );
    root.insert(
        "tutorial_gacha".to_owned(),
        Value::Object(Map::from_iter([(
            "character_id".to_owned(),
            Value::Number(Number::from(DEFAULT_TUTORIAL_GACHA_CHARACTER_ID)),
        )])),
    );
    ensure_completed_tutorial_quest_progress(root)?;
    let user_info = require_user_info(root)?;
    user_info.insert("free_vmoney".to_owned(), Value::Number(Number::from(1_500)));
    user_info.insert("vmoney".to_owned(), Value::Number(Number::from(0)));
    user_info.insert("free_mana".to_owned(), Value::Number(Number::from(1_000)));
    user_info.insert("paid_mana".to_owned(), Value::Number(Number::from(0)));

    let characters = require_characters_mut(root)?;
    characters.retain(|character_id, _| character_id == "1");
    for character_id in [
        DEFAULT_TUTORIAL_GACHA_CHARACTER_ID,
        DEFAULT_TUTORIAL_REWARD_CHARACTER_ID,
    ] {
        characters.insert(
            character_id.to_string(),
            create_stored_character(character_id, server_time)?,
        );
    }
    Ok(())
}

fn prepare_quest_progress(player_data: &mut Value) -> Result<(), PersonalServiceError> {
    let root = response_root(player_data)?;
    let ui_tutorial_triggered = root
        .get("user_triggered_tutorial")
        .and_then(Value::as_array)
        .is_some_and(|tutorials| {
            tutorials
                .iter()
                .any(|tutorial_id| tutorial_id.as_i64() == Some(TRIGGERED_UI_TUTORIAL_ID))
        });
    normalize_existing_quest_progress(root)?;
    if is_tutorial_completed(root) && ui_tutorial_triggered {
        ensure_completed_tutorial_quest_progress(root)?;
    }
    Ok(())
}

fn normalize_existing_quest_progress(
    root: &mut Map<String, Value>,
) -> Result<(), PersonalServiceError> {
    let Some(quest_progress) = root.get_mut("quest_progress") else {
        return Ok(());
    };
    let quest_progress = quest_progress
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN quest progress is invalid"))?;
    for progress_list in quest_progress.values_mut() {
        let progress_list = progress_list
            .as_array_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN quest category is invalid"))?;
        for progress in progress_list {
            normalize_quest_progress_entry(progress)?;
        }
    }
    Ok(())
}

pub(crate) fn normalize_quest_progress_entry(
    progress: &mut Value,
) -> Result<&mut Map<String, Value>, PersonalServiceError> {
    let progress = progress.as_object_mut().ok_or_else(|| {
        PersonalServiceError::new("stored CN main quest progress entry is invalid")
    })?;
    if progress.get("quest_id").and_then(Value::as_i64).is_none() {
        return Err(PersonalServiceError::new(
            "stored CN main quest id is invalid",
        ));
    }

    let finished = progress
        .get("finished")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !progress.get("finished").is_some_and(Value::is_boolean) {
        progress.insert("finished".to_owned(), Value::Bool(finished));
    }
    if !progress.get("unlocked").is_some_and(Value::is_boolean) {
        progress.insert("unlocked".to_owned(), Value::Bool(false));
    }
    if !progress
        .get("high_score")
        .is_some_and(|value| value.as_i64().is_some())
    {
        progress.insert("high_score".to_owned(), Value::Number(Number::from(0)));
    }
    if !progress
        .get("clear_rank")
        .is_some_and(|value| value.as_i64().is_some() || value.is_null())
    {
        progress.insert(
            "clear_rank".to_owned(),
            if finished {
                Value::Number(Number::from(5))
            } else {
                Value::Null
            },
        );
    }
    if !progress
        .get("best_elapsed_time_ms")
        .is_some_and(|value| value.as_i64().is_some() || value.is_null())
    {
        progress.insert("best_elapsed_time_ms".to_owned(), Value::Null);
    }
    Ok(progress)
}

fn is_tutorial_completed(root: &Map<String, Value>) -> bool {
    if root
        .get("user_triggered_tutorial")
        .and_then(Value::as_array)
        .is_some_and(|tutorials| {
            tutorials
                .iter()
                .any(|tutorial_id| tutorial_id.as_i64() == Some(TRIGGERED_UI_TUTORIAL_ID))
        })
    {
        return true;
    }
    matches!(root.get("user_tutorial"), Some(Value::Null))
}

fn ensure_completed_tutorial_quest_progress(
    root: &mut Map<String, Value>,
) -> Result<(), PersonalServiceError> {
    let quest_progress = root
        .entry("quest_progress".to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN quest progress is invalid"))?;
    let main_quest_progress = quest_progress
        .entry(MAIN_QUEST_CATEGORY.to_string())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN main quest progress is invalid"))?;
    finish_quest_progress_entry(main_quest_progress, 1_001_001, 0, None)?;
    finish_quest_progress_entry(main_quest_progress, 1_001_002, 57_076, Some(30_124))?;
    finish_quest_progress_entry(main_quest_progress, 1_001_003, 0, None)?;
    finish_quest_progress_entry(main_quest_progress, 1_002_001, 61_350, Some(35_700))?;
    Ok(())
}

pub(crate) fn finish_quest_progress(
    root: &mut Map<String, Value>,
    category: i64,
    quest_id: i64,
) -> Result<(), PersonalServiceError> {
    if category <= 0 {
        return Err(PersonalServiceError::new(
            "stored CN quest category is invalid",
        ));
    }
    let quest_progress = root
        .entry("quest_progress".to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN quest progress is invalid"))?;
    let category_progress = quest_progress
        .entry(category.to_string())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN quest category progress is invalid"))?;
    finish_quest_progress_entry(category_progress, quest_id, 0, None)
}

fn finish_quest_progress_entry(
    progress_list: &mut Vec<Value>,
    quest_id: i64,
    high_score: i64,
    best_elapsed_time_ms: Option<i64>,
) -> Result<(), PersonalServiceError> {
    if let Some(progress) = progress_list
        .iter_mut()
        .find(|progress| progress.get("quest_id").and_then(Value::as_i64) == Some(quest_id))
    {
        let progress = normalize_quest_progress_entry(progress)?;
        progress.insert("finished".to_owned(), Value::Bool(true));
        let preserved_high_score = progress
            .get("high_score")
            .and_then(Value::as_i64)
            .unwrap_or_default()
            .max(high_score);
        progress.insert(
            "high_score".to_owned(),
            Value::Number(Number::from(preserved_high_score)),
        );
        let preserved_clear_rank = progress
            .get("clear_rank")
            .and_then(Value::as_i64)
            .unwrap_or_default()
            .max(5);
        progress.insert(
            "clear_rank".to_owned(),
            Value::Number(Number::from(preserved_clear_rank)),
        );
        let preserved_clear_time = match (
            progress.get("best_elapsed_time_ms").and_then(Value::as_i64),
            best_elapsed_time_ms,
        ) {
            (Some(current), Some(default)) => Value::from(current.min(default)),
            (Some(current), None) => Value::from(current),
            (None, Some(default)) => Value::from(default),
            (None, None) => Value::Null,
        };
        progress.insert("best_elapsed_time_ms".to_owned(), preserved_clear_time);
    } else {
        progress_list.push(json!({
            "quest_id": quest_id,
            "finished": true,
            "unlocked": false,
            "high_score": high_score,
            "clear_rank": 5,
            "best_elapsed_time_ms": best_elapsed_time_ms,
        }));
    }
    Ok(())
}
// //// /维护教程与主线进度 ////

// //// 补齐普通编队分组和槽位 [@x380kkm 2026-08-28] ////
pub(crate) fn ensure_normal_party_groups(
    root: &mut Map<String, Value>,
) -> Result<(), PersonalServiceError> {
    let leader_character_id = default_party_leader_id(root)?;
    let party_groups = root
        .get_mut("user_party_group_list")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN party groups are missing"))?;
    for group_id in 1..=NORMAL_PARTY_GROUP_COUNT {
        let group = party_groups
            .entry(group_id.to_string())
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN party group is invalid"))?;
        group
            .entry("color_id".to_owned())
            .or_insert(Value::from(DEFAULT_PARTY_GROUP_COLOR_ID));
        let parties = group
            .entry("list".to_owned())
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN party group list is invalid"))?;
        for slot in 1..=PARTY_SLOTS_PER_GROUP {
            let party_id = (group_id - 1) * PARTY_SLOTS_PER_GROUP + slot;
            parties.entry(party_id.to_string()).or_insert_with(|| {
                json!({
                    "ability_soul_ids": [null, null, null],
                    "before_battle_power": 0,
                    "character_ids": [leader_character_id, null, null],
                    "current_battle_power": 0,
                    "edited": false,
                    "equipment_ids": [null, null, null],
                    "name": DEFAULT_PARTY_NAMES[(slot - 1) as usize],
                    "options": {
                        "allow_other_players_to_heal_me": true,
                    },
                    "unison_character_ids": [null, null, null],
                })
            });
        }
    }
    Ok(())
}

fn default_party_leader_id(root: &Map<String, Value>) -> Result<Option<i64>, PersonalServiceError> {
    let characters = root
        .get("user_character_list")
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("stored CN characters are missing"))?;
    let configured_leader = root
        .get("user_info")
        .and_then(Value::as_object)
        .and_then(|user_info| user_info.get("leader_character_id"))
        .and_then(Value::as_i64)
        .filter(|leader_id| characters.contains_key(&leader_id.to_string()));
    if configured_leader.is_some() {
        return Ok(configured_leader);
    }
    characters
        .keys()
        .map(|character_id| {
            character_id
                .parse::<i64>()
                .map_err(|_| PersonalServiceError::new("stored CN character id is invalid"))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|mut character_ids| {
            character_ids.sort_unstable();
            character_ids.into_iter().next()
        })
}
// //// /补齐普通编队分组和槽位 ////

// //// 写入编队战力字段 [@x380kkm 2026-08-23] ////
fn ensure_party_battle_power_fields(player_data: &mut Value) -> Result<(), PersonalServiceError> {
    let root = response_root(player_data)?;
    let party_groups = root
        .get_mut("user_party_group_list")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN party groups are missing"))?;
    for group in party_groups.values_mut() {
        let parties = group
            .get_mut("list")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| PersonalServiceError::new("stored CN party group is invalid"))?;
        for party in parties.values_mut() {
            let party = party
                .as_object_mut()
                .ok_or_else(|| PersonalServiceError::new("stored CN party is invalid"))?;
            party
                .entry("current_battle_power".to_owned())
                .or_insert(Value::Number(Number::from(0)));
            party
                .entry("before_battle_power".to_owned())
                .or_insert(Value::Number(Number::from(0)));
        }
    }
    Ok(())
}
// //// /写入编队战力字段 ////

// //// 派生收藏编队列表 [@x380kkm 2026-08-23] ////
pub(crate) fn rebuild_favorite_party_group_list(
    root: &mut Map<String, Value>,
) -> Result<(), PersonalServiceError> {
    let party_groups = root
        .get("user_party_group_list")
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("stored CN party groups are missing"))?;
    let mut party_group_entries = party_groups
        .iter()
        .map(|(group_id, group)| {
            group_id
                .parse::<i64>()
                .map(|group_id| (group_id, group))
                .map_err(|_| PersonalServiceError::new("stored CN party group id is invalid"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    party_group_entries.sort_by_key(|(group_id, _)| *group_id);

    let mut favorite_groups = Vec::with_capacity(party_group_entries.len());
    for (group_id, group) in party_group_entries {
        let group = group
            .as_object()
            .ok_or_else(|| PersonalServiceError::new("stored CN party group is invalid"))?;
        let color_id = group
            .get("color_id")
            .cloned()
            .ok_or_else(|| PersonalServiceError::new("stored CN party group color is missing"))?;
        let parties = group
            .get("list")
            .and_then(Value::as_object)
            .ok_or_else(|| PersonalServiceError::new("stored CN party group list is invalid"))?;
        let mut party_entries = parties
            .iter()
            .map(|(party_id, party)| {
                party_id
                    .parse::<i64>()
                    .map(|party_id| (party_id, party))
                    .map_err(|_| PersonalServiceError::new("stored CN party id is invalid"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        party_entries.sort_by_key(|(party_id, _)| *party_id);

        let mut favorite_parties = Vec::with_capacity(party_entries.len());
        for (party_id, party) in party_entries {
            let party = party
                .as_object()
                .ok_or_else(|| PersonalServiceError::new("stored CN party is invalid"))?;
            let field = |key: &str| {
                party.get(key).cloned().ok_or_else(|| {
                    PersonalServiceError::new(format!("stored CN party {key} is missing"))
                })
            };
            favorite_parties.push(json!({
                "party_id": party_id,
                "party_name": field("name")?,
                "character_ids": field("character_ids")?,
                "unison_character_ids": field("unison_character_ids")?,
                "equipment_ids": field("equipment_ids")?,
                "ability_soul_ids": field("ability_soul_ids")?,
                "options": field("options")?,
                "party_edited": field("edited")?,
                "current_battle_power": field("current_battle_power")?,
                "before_battle_power": field("before_battle_power")?,
            }));
        }
        favorite_groups.push(json!({
            "party_group_id": group_id,
            "party_group_color_id": color_id,
            "party_list": favorite_parties,
        }));
    }
    root.insert(
        "favorite_party_group_list".to_owned(),
        Value::Array(favorite_groups),
    );
    Ok(())
}
// //// /派生收藏编队列表 ////

// //// 写入客户端配置字段 [@x380kkm 2026-08-23] ////
fn ensure_client_config_fields(player_data: &mut Value) -> Result<(), PersonalServiceError> {
    let config = response_root(player_data)?
        .get_mut("config")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN client config is missing"))?;
    config
        .entry("summon_com_seconds".to_owned())
        .or_insert(Value::Number(Number::from(5)));
    Ok(())
}
// //// /写入客户端配置字段 ////

// //// 重置每日玩家状态 [@x380kkm 2026-08-23] ////
fn reset_daily_challenge_points(root: &mut Map<String, Value>) -> Result<(), PersonalServiceError> {
    let defaults = decode_player_data(DEFAULT_PLAYER_DATA)?;
    let defaults = defaults
        .get("user_daily_challenge_point_list")
        .and_then(Value::as_array)
        .ok_or_else(|| PersonalServiceError::new("default CN challenge points are missing"))?;
    let challenge_points = root
        .entry("user_daily_challenge_point_list".to_owned())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN challenge points are invalid"))?;
    for default in defaults {
        let challenge_id = default
            .get("id")
            .and_then(Value::as_i64)
            .ok_or_else(|| PersonalServiceError::new("default CN challenge point id is invalid"))?;
        let campaign_points = |campaigns: &Value| {
            campaigns
                .as_array()
                .ok_or_else(|| PersonalServiceError::new("CN challenge campaigns are invalid"))?
                .iter()
                .try_fold(0_i64, |total, campaign| {
                    let additional = campaign
                        .get("additional_point")
                        .and_then(Value::as_i64)
                        .ok_or_else(|| {
                            PersonalServiceError::new("CN challenge campaign point is invalid")
                        })?;
                    total.checked_add(additional).ok_or_else(|| {
                        PersonalServiceError::new("CN challenge campaign points overflow")
                    })
                })
        };
        let default_campaigns = default.get("campaign_list").ok_or_else(|| {
            PersonalServiceError::new("default CN challenge campaigns are missing")
        })?;
        let base_point = default
            .get("point")
            .and_then(Value::as_i64)
            .ok_or_else(|| PersonalServiceError::new("default CN challenge point is invalid"))?
            .checked_sub(campaign_points(default_campaigns)?)
            .ok_or_else(|| PersonalServiceError::new("default CN challenge point underflows"))?;
        if let Some(stored) = challenge_points
            .iter_mut()
            .find(|stored| stored.get("id").and_then(Value::as_i64) == Some(challenge_id))
        {
            let stored = stored
                .as_object_mut()
                .ok_or_else(|| PersonalServiceError::new("stored CN challenge point is invalid"))?;
            let campaigns = stored
                .entry("campaign_list".to_owned())
                .or_insert_with(|| default_campaigns.clone());
            let reset_point = base_point
                .checked_add(campaign_points(campaigns)?)
                .ok_or_else(|| PersonalServiceError::new("CN challenge point overflows"))?;
            stored.insert("point".to_owned(), Value::Number(Number::from(reset_point)));
        } else {
            challenge_points.push(default.clone());
        }
    }
    Ok(())
}
// //// /重置每日玩家状态 ////

fn update_login_state(
    player_data: &mut Value,
    server_time: i64,
    client_time: &str,
) -> Result<(), PersonalServiceError> {
    let root = response_root(player_data)?;
    let projected_stamina = crate::cn_stamina::current_stamina(root, server_time)?;
    let is_new_day = {
        let user_info = require_user_info(root)?;
        let stored_stamina = user_info
            .get("stamina")
            .and_then(Value::as_i64)
            .ok_or_else(|| PersonalServiceError::new("stored CN stamina is missing"))?;
        if projected_stamina != stored_stamina {
            user_info.insert(
                "stamina".to_owned(),
                Value::Number(Number::from(projected_stamina)),
            );
            user_info.insert(
                "stamina_heal_time".to_owned(),
                Value::Number(Number::from(server_time)),
            );
        }
        let is_new_day = user_info
            .get("last_login_time")
            .and_then(Value::as_str)
            .ok_or_else(|| PersonalServiceError::new("stored CN login time is missing"))?
            .get(..10)
            != client_time.get(..10);
        if is_new_day {
            user_info.insert("boost_point".to_owned(), Value::Number(Number::from(3)));
            user_info.insert(
                "boss_boost_point".to_owned(),
                Value::Number(Number::from(3)),
            );
        }
        let pooled_time = user_info
            .get("exp_pooled_time")
            .and_then(Value::as_i64)
            .ok_or_else(|| PersonalServiceError::new("stored CN pooled EXP time is missing"))?;
        let elapsed = server_time.saturating_sub(pooled_time).max(0);
        if elapsed >= 60 {
            let exp_pool = user_info
                .get("exp_pool")
                .and_then(Value::as_i64)
                .ok_or_else(|| PersonalServiceError::new("stored CN pooled EXP is missing"))?;
            let increment = (elapsed / 60).min(EXP_POOL_INCREMENT_LIMIT);
            let updated_exp_pool = exp_pool.checked_add(increment).ok_or_else(|| {
                PersonalServiceError::new("stored CN pooled EXP exceeds the supported range")
            })?;
            user_info.insert(
                "exp_pool".to_owned(),
                Value::Number(Number::from(updated_exp_pool)),
            );
            user_info.insert(
                "exp_pooled_time".to_owned(),
                Value::Number(Number::from(server_time)),
            );
        }
        user_info.insert(
            "last_login_time".to_owned(),
            Value::String(client_time.to_owned()),
        );
        is_new_day
    };
    if is_new_day {
        reset_daily_challenge_points(root)?;
        crate::cn_gacha::reset_daily_state(root)?;
    }
    require_characters(root)?;
    Ok(())
}

fn require_user_info(
    root: &mut Map<String, Value>,
) -> Result<&mut Map<String, Value>, PersonalServiceError> {
    root.get_mut("user_info")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN user data is missing"))
}

fn require_characters(
    root: &Map<String, Value>,
) -> Result<&Map<String, Value>, PersonalServiceError> {
    root.get("user_character_list")
        .and_then(Value::as_object)
        .filter(|characters| !characters.is_empty())
        .ok_or_else(|| PersonalServiceError::new("stored CN default character is missing"))
}

fn require_characters_mut(
    root: &mut Map<String, Value>,
) -> Result<&mut Map<String, Value>, PersonalServiceError> {
    root.get_mut("user_character_list")
        .and_then(Value::as_object_mut)
        .filter(|characters| !characters.is_empty())
        .ok_or_else(|| PersonalServiceError::new("stored CN default character is missing"))
}
// //// /校验并更新持久化的玩家 JSON ////
