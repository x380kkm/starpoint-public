// audience: internal
// # personal-service-cn-mission-statistics
//
// 统计编码取自客户端任务主表. 缺少整项统计时保留未知状态.

use crate::PersonalServiceError;
use serde_json::Value;

const COUNT_FIELDS: &[(i64, &str)] = &[
    (0, "weak_point_attack_count"), (1, "use_power_flip_count"), (2, "use_dash_count"),
    (4, "use_skill_count"), (5, "fever_count"), (7, "enemy_kill_count"),
    (8, "use_emotion_count"), (9, "use_buff_to_all_party_members"), (10, "use_heal_to_all_party_members"),
    (11, "coffin_count_reduced_count"), (12, "clear_debuff_of_self_count"), (13, "use_debuff_to_enemy_count"),
    (14, "clear_buff_of_enemy_count"), (15, "fever_ms"), (16, "use_power_flip_lv3_count"), (17, "encoffinment_count"),
];

fn values(statistics: &Value, field: &str) -> Result<Vec<i64>, PersonalServiceError> {
    statistics.get("zones").and_then(Value::as_array).into_iter().flatten()
        .filter_map(|zone| zone.get(field).and_then(Value::as_f64)).map(|value| {
            if !value.is_finite() || value < 0.0 || value >= i64::MAX as f64 {
                return Err(PersonalServiceError::new("CN battle statistic exceeds supported range"));
            }
            Ok(value.floor() as i64)
        }).collect()
}

// //// 汇总有效区域统计并保留最大值 [@x380kkm 2026-09-22] ////
pub(super) fn total(statistics: &Value, kind: Option<i64>) -> Result<Option<i64>, PersonalServiceError> {
    let Some((_, field)) = COUNT_FIELDS.iter().find(|(code, _)| Some(*code) == kind) else { return Ok(None); };
    total_field(statistics, field)
}

pub(super) fn sent_emotions(statistics: &Value) -> Result<Option<i64>, PersonalServiceError> {
    total_field(statistics, "send_emotion_count")
}

fn total_field(statistics: &Value, field: &str) -> Result<Option<i64>, PersonalServiceError> {
    let values = values(statistics, field)?;
    if values.is_empty() { return Ok(None); }
    values.into_iter().try_fold(0_i64, |sum, value| sum.checked_add(value)
        .ok_or_else(|| PersonalServiceError::new("CN battle statistics exceed supported range"))).map(Some)
}

pub(super) fn maximum(statistics: &Value, kind: Option<i64>) -> Result<Option<i64>, PersonalServiceError> {
    let field = match kind { Some(0) => "damage_deal_max", Some(1) => "max_coffin_count_by_revival", _ => return Ok(None) };
    Ok(values(statistics, field)?.into_iter().max())
}

pub(super) fn is_zero(statistics: &Value, kind: Option<i64>) -> bool {
    let Some((_, field)) = COUNT_FIELDS.iter().find(|(code, _)| Some(*code) == kind) else { return false; };
    statistics.get("zones").and_then(Value::as_array).filter(|zones| !zones.is_empty()).is_some_and(|zones| {
        zones.iter().all(|zone| zone.get(*field).and_then(Value::as_f64) == Some(0.0))
    })
}

pub(super) fn has_no_resistance_debuff(statistics: &Value) -> bool {
    let Some(zones) = statistics.get("zones").and_then(Value::as_array).filter(|zones| !zones.is_empty()) else { return false; };
    zones.iter().all(|zone| {
        zone.get("members").and_then(Value::as_array).filter(|members| !members.is_empty()).is_some_and(|members| {
            let present = members.iter().filter(|member| !member.is_null()).collect::<Vec<_>>();
            !present.is_empty() && present.iter().all(|member| member.get("debuff_r").and_then(Value::as_i64) == Some(0))
        })
    })
}
// //// /汇总有效区域统计并保留最大值 ////
