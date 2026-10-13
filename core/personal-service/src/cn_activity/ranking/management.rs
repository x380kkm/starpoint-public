// audience: internal
// # personal-service-ranking-management
// 竞速管理预览读取当前期别的成绩, 奖励目录和领取凭据.

use super::{catalog, recorded_summary, summary};
use crate::cn_activity::state;
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};

// //// 汇总全部竞速活动的可选奖励档位 [@x380kkm 2026-09-28] ////
pub(in crate::cn_activity) fn entries(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
) -> Result<Vec<Value>, PersonalServiceError> {
    let mut entries = Vec::new();
    for (&event_id, event) in catalog()? {
        state::select_event_period(root, database, "ranking", event_id)?;
        let saved = recorded_summary(root, event_id);
        let scope = state::reward_scope(root, "ranking", event_id);
        let receipt = root
            .get(super::RECEIPTS)
            .and_then(|receipts| receipts.get(scope.strip_prefix("ranking:").unwrap()));
        let current = saved
            .clone()
            .unwrap_or_else(|| summary(root, event_id, event));
        let percentage = current["rank_percentage"].as_f64();
        let selected = percentage
            .and_then(|value| {
                event
                    .tiers
                    .iter()
                    .find(|tier| value <= tier.border_percentage)
            })
            .map(|tier| tier.id);
        entries.push(json!({"activity_id":format!("ranking:{event_id}"),"family":"ranking","event_id":event_id,
            "name":event.name,"summary":current,"has_record":!current["best_record"].is_null(),"received":saved.is_some(),
            "default_tier_id":selected,"receipt":receipt,"start_at_ms":event.start_at_ms,"end_at_ms":event.play_end_at_ms,
            "tiers":event.tiers.iter().map(|tier| json!({"id":tier.id,"label":format!("前 {}%",tier.border_percentage),
                "border_percentage":tier.border_percentage,"rewards":tier.rewards})).collect::<Vec<_>>() }));
    }
    Ok(entries)
}
