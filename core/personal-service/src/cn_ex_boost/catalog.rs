// audience: internal
// # personal-service-cn-ex-boost-catalog
// 组合决定能力槽数量, 各状态和能力 ID 使用独立主表权重.

use super::DrawResult;
use crate::PersonalServiceError;
use getrandom::getrandom;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

const CATALOG_ASSET: &str = include_str!("../../assets/cn-ex-boost.json");
static CATALOG: OnceLock<Result<Catalog, String>> = OnceLock::new();

#[derive(Deserialize)]
struct Catalog {
    materials: BTreeMap<i64, Material>,
    combinations: BTreeMap<String, Vec<Combination>>,
    status_pools: BTreeMap<String, Vec<WeightedId>>,
    ability_pools: BTreeMap<String, Vec<WeightedId>>,
}

#[derive(Deserialize)]
pub(super) struct Material {
    pub(super) cost_item_count: i64,
    pub(super) elements: Vec<i64>,
    combination_pool: String,
}

#[derive(Deserialize)]
struct Combination {
    weight: u64,
    status_pool: String,
    ability_pools: Vec<String>,
}

#[derive(Deserialize)]
struct WeightedId {
    id: i64,
    weight: u64,
}

fn catalog() -> Result<&'static Catalog, PersonalServiceError> {
    CATALOG
        .get_or_init(|| {
            serde_json::from_str(CATALOG_ASSET)
                .map_err(|error| format!("failed to decode CN EX catalog: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

pub(super) fn material(id: i64) -> Result<Option<&'static Material>, PersonalServiceError> {
    Ok(catalog()?.materials.get(&id))
}

// //// 按组合和各槽位的独立权重抽取 EX 能力 [@x380kkm 2026-09-23] ////
pub(super) fn draw(
    character_id: i64,
    material: &Material,
) -> Result<DrawResult, PersonalServiceError> {
    draw_with_random(character_id, material, &mut random_index)
}

fn draw_with_random(
    character_id: i64,
    material: &Material,
    random: &mut impl FnMut(u64) -> Result<u64, PersonalServiceError>,
) -> Result<DrawResult, PersonalServiceError> {
    let catalog = catalog()?;
    let combinations = catalog
        .combinations
        .get(&material.combination_pool)
        .ok_or_else(|| PersonalServiceError::new("CN EX combination pool is missing"))?;
    let combination = choose_weighted(combinations, |entry| entry.weight, random)?;
    let statuses = catalog
        .status_pools
        .get(&combination.status_pool)
        .ok_or_else(|| PersonalServiceError::new("CN EX status pool is missing"))?;
    let status_id = choose_weighted(statuses, |entry| entry.weight, random)?.id;
    let mut ability_ids = Vec::new();
    for name in &combination.ability_pools {
        let abilities = catalog
            .ability_pools
            .get(name)
            .ok_or_else(|| PersonalServiceError::new("CN EX ability pool is missing"))?;
        ability_ids.push(choose_weighted(abilities, |entry| entry.weight, random)?.id);
    }
    Ok(DrawResult {
        character_id,
        status_id,
        ability_ids,
    })
}
// //// /按组合和各槽位的独立权重抽取 EX 能力 ////

// //// 将有界随机值映射到累积权重区间 [@x380kkm 2026-09-23] ////
fn choose_weighted<'a, T>(
    entries: &'a [T],
    weight: impl Fn(&T) -> u64,
    random: &mut impl FnMut(u64) -> Result<u64, PersonalServiceError>,
) -> Result<&'a T, PersonalServiceError> {
    let total = entries.iter().map(&weight).sum::<u64>();
    if total == 0 {
        return Err(PersonalServiceError::new("CN EX draw pool is empty"));
    }
    let mut roll = random(total)?;
    for entry in entries {
        let entry_weight = weight(entry);
        if roll < entry_weight {
            return Ok(entry);
        }
        roll -= entry_weight;
    }
    Err(PersonalServiceError::new(
        "CN EX draw exceeds its weight range",
    ))
}
// //// /将有界随机值映射到累积权重区间 ////

fn random_index(length: u64) -> Result<u64, PersonalServiceError> {
    let mut bytes = [0_u8; 8];
    getrandom(&mut bytes).map_err(|error| {
        PersonalServiceError::new(format!("failed to draw CN EX boost: {error}"))
    })?;
    Ok(u64::from_le_bytes(bytes) % length)
}

#[cfg(test)]
mod tests {
    use super::*;

    // //// 验证组合槽数与状态和能力 ID 的权重边界 [@x380kkm 2026-09-23] ////
    #[test]
    fn follows_combination_and_individual_pool_weights() {
        let material = material(10001).unwrap().unwrap();
        for (rolls, status, abilities) in [
            (vec![80_000, 0], 1, vec![]),
            (vec![70_000, 20_001, 99_999], 4, vec![30]),
            (vec![75_000, 99_999, 0], 9, vec![31]),
        ] {
            let mut rolls = rolls.into_iter();
            let result = draw_with_random(1, material, &mut |total| {
                let roll = rolls.next().unwrap();
                assert!(roll < total);
                Ok(roll)
            })
            .unwrap();
            assert_eq!(result.status_id, status);
            assert_eq!(result.ability_ids, abilities);
            assert_eq!(rolls.next(), None);
        }
    }
    // //// /验证组合槽数与状态和能力 ID 的权重边界 ////
}
