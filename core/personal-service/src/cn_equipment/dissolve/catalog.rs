// audience: internal
// # personal-service-cn-equipment-dissolve-catalog
// 装备产出, 可出售性和物品上限由客户端主表生成.

use crate::PersonalServiceError;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Deserialize)]
pub(in crate::cn_equipment) struct EquipmentDefinition {
    pub(super) max_level: i64,
    pub(super) sellable: bool,
    pub(in crate::cn_equipment) craft_points: i64,
    pub(in crate::cn_equipment) star_grains: i64,
    pub(in crate::cn_equipment) generate_ability_soul: bool,
    pub(in crate::cn_equipment) ability_soul_id: i64,
    pub(super) ability_soul_max_count: i64,
}

#[derive(Deserialize)]
pub(super) struct ItemRule {
    pub(super) max_count: i64,
    pub(super) sale_price: i64,
}

// //// 读取装备和分解奖励的主表规则 [@x380kkm 2026-09-23] ////
pub(in crate::cn_equipment) fn equipment_definitions(
) -> Result<&'static BTreeMap<i64, EquipmentDefinition>, PersonalServiceError> {
    static DEFINITIONS: OnceLock<Result<BTreeMap<i64, EquipmentDefinition>, String>> =
        OnceLock::new();
    DEFINITIONS
        .get_or_init(|| {
            serde_json::from_str(include_str!(
                "../../../../../assets/equipment_dissolve.json"
            ))
            .map_err(|error| format!("failed to decode CN equipment dissolve data: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

pub(super) fn item_rules() -> Result<&'static BTreeMap<i64, ItemRule>, PersonalServiceError> {
    static RULES: OnceLock<Result<BTreeMap<i64, ItemRule>, String>> = OnceLock::new();
    RULES
        .get_or_init(|| {
            serde_json::from_str(include_str!(
                "../../../assets/cn-equipment-dissolve-rules.json"
            ))
            .map_err(|error| format!("failed to decode CN equipment dissolve rules: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

pub(in crate::cn_equipment) fn max_level(
    equipment_id: i64,
) -> Result<Option<i64>, PersonalServiceError> {
    Ok(equipment_definitions()?
        .get(&equipment_id)
        .map(|definition| definition.max_level))
}
// //// /读取装备和分解奖励的主表规则 ////
