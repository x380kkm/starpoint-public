// audience: internal
// # personal-service-cn-mission-login
// 活动任务按游戏日和任务时窗保存登录日期. 日期集合使用任务进度的范围标识, 随活动进度共同重置.

use super::{mission_catalog, window, MissionKey};
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};

// //// 将游戏登录日计入当前开放的活动和修行任务 [@x380kkm 2026-09-28] ////
pub(crate) fn record_activity_login_days(
    root: &mut Map<String, Value>, database: &ServiceDatabase, now: i64,
) -> Result<(), PersonalServiceError> {
    window::record_pass_card_login_days(root, database, now)?;
    let day = window::game_day(now);
    let catalog = mission_catalog()?;
    for category in [3, 4] {
        for mission in catalog.categories.get(&category).into_iter().flatten()
            .filter(|mission| mission.pattern.as_deref() == Some("total_login_days")) {
            let key = MissionKey { category, mission_id: mission.id };
            let Some(scope) = window::mission_scope(key, mission, database)? else { continue; };
            if record_login_date(root, &scope.key, day)? {
                window::advance_progress(root, &scope, "total_login_days", 1, 1)?;
            }
        }
    }
    Ok(())
}
// //// /将游戏登录日计入当前开放的活动和修行任务 ////

// //// 保存任务范围内首次出现的游戏登录日 [@x380kkm 2026-09-28] ////
fn record_login_date(root: &mut Map<String, Value>, scope: &str, day: i64) -> Result<bool, PersonalServiceError> {
    let periods = root.entry("mission_activity_login_days".to_owned()).or_insert_with(|| json!({}))
        .as_object_mut().ok_or_else(|| PersonalServiceError::new("stored CN activity login periods are invalid"))?;
    let dates = periods.entry(scope.to_owned()).or_insert_with(|| json!([])).as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN activity login dates are invalid"))?;
    if dates.iter().any(|value| value.as_i64() == Some(day)) { return Ok(false); }
    dates.push(json!(day));
    Ok(true)
}
