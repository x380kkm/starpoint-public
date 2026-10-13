// audience: internal
// # personal-service-cn-helper
// 小助手按客户端请求汇总任务, 商店, 箱池和嘉年华的实际状态, 展示项与跳转由客户端主表决定.
// 浏览汇总保留玩家资产, browse_treasure_flag 仅确认玛纳商店通知的已读状态.

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_mission::MissionCategory;
use crate::cn_shop::SalesQuery;
use crate::cn_tutorial::{decode_player_data, encode_player_data, player_snapshot, require_root};
use crate::database::{ActivityWindowStatus, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::path::Path;

#[derive(Deserialize)]
struct HelperRequest {
    viewer_id: i64,
    #[serde(flatten)]
    sales: SalesQuery,
    #[serde(default)]
    box_gacha_ids: Vec<i64>,
    #[serde(default)]
    category_list: Vec<MissionCategory>,
    #[serde(default)]
    carnival_event_id: i64,
    #[serde(default)]
    browse_treasure_flag: bool,
}

// //// 返回客户端选择的小助手汇总 [@x380kkm 2026-09-23] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" || request.path() != "/api/index.php/assistant/get_assistant_list"
    {
        return None;
    }
    Some(get_list(request, database, asset_root))
}

fn get_list(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<HelperRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.carnival_event_id >= 0
                && body.category_list.iter().all(MissionCategory::is_valid) =>
        {
            body
        }
        _ => {
            return Ok(HttpResponse::json(
                "400 Bad Request",
                r#"{"error":"invalid_assistant_request"}"#.to_owned(),
            ))
        }
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let response_time = server_time(database)?;
    let mut player = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player)?;
    let all_box_info =
        crate::cn_box_gacha::available_box_info(root, database, asset_root, &body.box_gacha_ids)?;
    let mission_progress_list = crate::cn_mission::project_progress(
        root,
        database,
        snapshot.account_id,
        &body.category_list,
    )?;
    let (sales_list, _) =
        crate::cn_shop::prepare_sales_list(root, database, asset_root, &body.sales, response_time)?;
    let records = carnival_records(root, database, body.carnival_event_id)?;
    let changed = body.browse_treasure_flag && mark_treasure_browsed(root)?;
    let user_notice_list = root
        .get("user_notice_list")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let response = json!({
        "all_box_info": all_box_info,
        "mission_progress_list": mission_progress_list,
        "records": records,
        "sales_list": sales_list,
        "user_notice_list": user_notice_list,
    });
    if changed {
        database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player)?)?;
    }
    msgpack_response_at(body.viewer_id, false, response_time, response)
}
// //// /返回客户端选择的小助手汇总 ////

// //// 返回当前可访问嘉年华的已保存成绩 [@x380kkm 2026-09-23] ////
fn carnival_records(
    root: &Map<String, Value>,
    database: &ServiceDatabase,
    event_id: i64,
) -> Result<Value, PersonalServiceError> {
    if event_id <= 0 {
        return Ok(json!([]));
    }
    let activity_id = format!("carnival:{event_id}");
    let status =
        database.activity_window_status(&activity_id, database.current_server_time_millis()?)?;
    Ok(
        if matches!(
            status,
            ActivityWindowStatus::Open | ActivityWindowStatus::Unscheduled
        ) {
            crate::cn_activity::carnival_records(root, event_id)
        } else {
            json!([])
        },
    )
}
// //// /返回当前可访问嘉年华的已保存成绩 ////

// //// 确认玛纳商店通知并保留其他通知 [@x380kkm 2026-09-23] ////
fn mark_treasure_browsed(root: &mut Map<String, Value>) -> Result<bool, PersonalServiceError> {
    let notices = root
        .entry("user_notice_list".to_owned())
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN user notices are invalid"))?;
    if let Some(notice) = notices.iter_mut().find(|notice| notice["kind"] == 1) {
        if notice["status"] == 0 {
            return Ok(false);
        }
        notice["status"] = json!(0);
    } else {
        notices.push(json!({"kind":1, "status":0}));
    }
    Ok(true)
}
// //// /确认玛纳商店通知并保留其他通知 ////
