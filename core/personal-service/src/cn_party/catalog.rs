// audience: internal
// # personal-service-cn-party-catalog
// 角色和魂珠使用客户端主表的 0 到 5 属性编号.

use crate::PersonalServiceError;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

#[derive(Deserialize)]
pub(super) struct PartyRules {
    pub(super) character_elements: BTreeMap<i64, i64>,
    pub(super) ability_soul_elements: BTreeMap<i64, BTreeSet<i64>>,
    pub(super) group_colors: BTreeSet<i64>,
    pub(super) initial_group_color: i64,
}

static RULES: OnceLock<Result<PartyRules, String>> = OnceLock::new();

// //// 读取客户端编队规则 [@x380kkm 2026-09-23] ////
pub(super) fn rules() -> Result<&'static PartyRules, PersonalServiceError> {
    RULES
        .get_or_init(|| {
            serde_json::from_str(include_str!("../../assets/cn-party-rules.json"))
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

// //// 将分组颜色归入客户端目录 [@x380kkm 2026-09-23] ////
pub(crate) fn normalize_group_color(color: i64) -> Result<i64, PersonalServiceError> {
    let rules = rules()?;
    Ok(if rules.group_colors.contains(&color) {
        color
    } else {
        rules.initial_group_color
    })
}
