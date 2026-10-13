// audience: internal
// # personal-service-cn-rank-reward
// 段位由累计段位点和主表派生. 跨段奖励先结算原段位自然恢复, 再增加新段位体力上限.

use crate::cn_battle_assets::rank_degree_and_stamina;
use crate::cn_stamina::current_stamina;
use crate::cn_tutorial::{require_object, user_info_value};
use crate::PersonalServiceError;
use serde_json::{Map, Value};

// //// 增加段位点并结算跨段体力 [@x380kkm 2026-09-23] ////
pub(crate) fn grant_rank_points(
    root: &mut Map<String, Value>,
    amount: i64,
    now: i64,
) -> Result<bool, PersonalServiceError> {
    if amount < 0 {
        return Err(PersonalServiceError::new(
            "CN rank reward amount is invalid",
        ));
    }
    if amount == 0 {
        return Ok(false);
    }
    let before = user_info_value(root, "rank_point")?;
    let after = before
        .checked_add(amount)
        .ok_or_else(|| PersonalServiceError::new("CN rank points exceed the supported range"))?;
    let (previous_degree, _, _) = rank_degree_and_stamina(before)?;
    let (new_degree, new_max_stamina, _) = rank_degree_and_stamina(after)?;
    let stamina = if new_degree > previous_degree {
        Some(
            current_stamina(root, now)?
                .checked_add(new_max_stamina)
                .ok_or_else(|| {
                    PersonalServiceError::new("CN rank-up stamina exceeds the supported range")
                })?,
        )
    } else {
        None
    };
    let user = require_object(root, "user_info")?;
    user.insert("rank_point".to_owned(), Value::from(after));
    if let Some(stamina) = stamina {
        user.insert("stamina".to_owned(), Value::from(stamina));
        user.insert("stamina_heal_time".to_owned(), Value::from(now));
    }
    Ok(stamina.is_some())
}
// //// /增加段位点并结算跨段体力 ////
