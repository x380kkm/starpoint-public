// audience: internal
// # personal-service-cn-battle-history
// 战斗历史保存当场队伍和统计. 历史记录与战斗结算共享快照提交事务.

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_battle_state::FinishBattleInput;
use crate::cn_tutorial::{format_client_time, player_snapshot};
use crate::database::{ActiveSingleQuest, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};

pub(crate) const SNAPSHOT_FIELD: &str = "last_battle_history_entry";
const PRACTICE_CATEGORY: i64 = 15;
const SCORE_ATTACK_CATEGORY: i64 = 27;

// //// 识别提供逐场历史列表的关卡分类 [@x380kkm 2026-09-23] ////
pub(crate) fn supports_history(category: i64) -> bool {
    matches!(category, PRACTICE_CATEGORY | SCORE_ATTACK_CATEGORY)
}

#[derive(Deserialize)]
struct HistoryRequest {
    viewer_id: i64,
    event_id: Option<i64>,
}

struct BattleResult {
    elapsed_time_ms: i64,
    score: Option<i64>,
    clear_rank: Option<i64>,
    finish_kind: i64,
    created_at: i64,
}

// //// 返回玩家的练习和无限演武战斗历史 [@x380kkm 2026-09-23] ////
pub(crate) fn history(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<HistoryRequest>(request) {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let category = if request.path() == "/api/index.php/history/practice_battle" {
        PRACTICE_CATEGORY
    } else {
        SCORE_ATTACK_CATEGORY
    };
    let entries = database.battle_history(snapshot.account_id, category, body.event_id)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({"history": entries}),
    )
}

// //// 保存正常结算的当场战斗记录 [@x380kkm 2026-09-23] ////
pub(crate) fn record_finish(
    root: &mut Map<String, Value>,
    active: &ActiveSingleQuest,
    input: &FinishBattleInput<'_>,
    clear_rank: Option<i64>,
    statistics: &Value,
    created_at: i64,
) -> Result<(), PersonalServiceError> {
    record(
        root,
        active,
        statistics,
        BattleResult {
            elapsed_time_ms: input.elapsed_time_ms,
            score: Some(input.score),
            clear_rank,
            finish_kind: 0,
            created_at,
        },
    )
}

// //// 保存主动结束的当场战斗记录 [@x380kkm 2026-09-23] ////
pub(crate) fn record_abort(
    root: &mut Map<String, Value>,
    active: &ActiveSingleQuest,
    statistics: &Value,
    finish_kind: i64,
    created_at: i64,
) -> Result<(), PersonalServiceError> {
    let elapsed_time_ms = statistics
        .get("zones")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|zone| zone.get("playthrough_ms").and_then(Value::as_i64))
        .try_fold(0_i64, |sum, duration| sum.checked_add(duration.max(0)))
        .ok_or_else(|| {
            PersonalServiceError::new("battle history duration exceeds supported range")
        })?;
    record(
        root,
        active,
        statistics,
        BattleResult {
            elapsed_time_ms,
            score: None,
            clear_rank: None,
            finish_kind,
            created_at,
        },
    )
}

// //// 将客户端统计转换为历史记录字段 [@x380kkm 2026-09-23] ////
fn record(
    root: &mut Map<String, Value>,
    active: &ActiveSingleQuest,
    statistics: &Value,
    result: BattleResult,
) -> Result<(), PersonalServiceError> {
    if !supports_history(active.category) {
        return Ok(());
    }
    let zones = statistics.get("zones").and_then(Value::as_array);
    let total_damage = sum_damage(
        zones
            .into_iter()
            .flatten()
            .filter_map(|zone| zone.get("damage_deal_total").and_then(Value::as_f64)),
    )?;
    let mut record = json!({
        "category_id": active.category,
        "quest_id": active.quest_id,
        "create_time": format_client_time(result.created_at),
        "finish_kind": result.finish_kind,
        "elapsed_time_ms": result.elapsed_time_ms.max(0),
        "score": result.score,
        "clear_rank": result.clear_rank,
        "total_damage": total_damage,
    });
    for slot in 0..3 {
        let index = slot + 1;
        let party = statistics.get("party");
        let character = party
            .and_then(|party| party.get("characters"))
            .and_then(|characters| characters.get(slot));
        let unison = party
            .and_then(|party| party.get("unison_characters"))
            .and_then(|characters| characters.get(slot));
        let equipment = party
            .and_then(|party| party.get("equipments"))
            .and_then(|equipments| equipments.get(slot));
        record[format!("character_id_{index}")] = entry_id(character);
        record[format!("unison_character_id_{index}")] = entry_id(unison);
        record[format!("equipment{index}_id")] = entry_id(equipment);
        record[format!("equipment_level_{index}")] = equipment
            .and_then(|equipment| equipment.get("level"))
            .cloned()
            .unwrap_or(Value::Null);
        record[format!("enhancement_level_{index}")] = equipment
            .and_then(|equipment| equipment.get("enhancement_level"))
            .cloned()
            .unwrap_or(Value::Null);
        record[format!("ability_soul_id_{index}")] = party
            .and_then(|party| party.get("ability_soul_ids"))
            .and_then(|souls| souls.get(slot))
            .cloned()
            .unwrap_or(Value::Null);
        record[format!("character_{index}_total_damage")] = if entry_id(character).is_null() {
            Value::Null
        } else {
            json!(sum_damage(zones.into_iter().flatten().filter_map(
                |zone| {
                    zone.get("members")
                        .and_then(|members| members.get(slot))
                        .and_then(|member| member.get("origin_damage"))
                        .and_then(Value::as_f64)
                }
            ))?)
        };
    }
    let event_id = crate::cn_battle_assets::load_battle_fixture()?
        .quests
        .get(&format!("{}:{}", active.category, active.quest_id))
        .and_then(|quest| quest.event_id);
    root.insert(
        SNAPSHOT_FIELD.to_owned(),
        json!({
            "play_id": active.play_id,
            "category": active.category,
            "event_id": event_id,
            "created_at": result.created_at,
            "record": record,
        }),
    );
    Ok(())
}

fn entry_id(entry: Option<&Value>) -> Value {
    entry
        .and_then(|entry| entry.get("id"))
        .and_then(Value::as_i64)
        .filter(|id| *id > 0)
        .map(Value::from)
        .unwrap_or(Value::Null)
}

fn sum_damage(values: impl Iterator<Item = f64>) -> Result<f64, PersonalServiceError> {
    let damage = values.map(|value| value.max(0.0)).sum::<f64>();
    if damage.is_finite() {
        Ok(damage.floor())
    } else {
        Err(PersonalServiceError::new(
            "battle history damage exceeds supported range",
        ))
    }
}
