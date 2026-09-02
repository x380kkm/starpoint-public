// audience: internal
// # personal-service-cn-gacha-movie
//
// 该模块按动画类型和角色稀有度选择客户端可播放的完整 seed 池.

use crate::cn_character::character_asset_data;
use crate::PersonalServiceError;
use getrandom::getrandom;
use serde_json::Value;
use std::sync::OnceLock;

const FALLBACK_SEEDS_ASSET: &str = include_str!("../../../../assets/gacha_movie_seeds.json");
const NORMAL_SEEDS_ASSET: &str = include_str!("../../../../assets/gacha_movie_seeds_normal.json");
const FES_SEEDS_ASSET: &str = include_str!("../../../../assets/gacha_movie_seeds_fes.json");
const NORMAL_GUARANTEE_SEEDS_ASSET: &str =
    include_str!("../../../../assets/gacha_movie_seeds_normal_guarantee.json");
const FES_GUARANTEE_SEEDS_ASSET: &str =
    include_str!("../../../../assets/gacha_movie_seeds_fes_guarantee.json");

static FALLBACK_SEEDS: OnceLock<Result<Value, String>> = OnceLock::new();
static NORMAL_SEEDS: OnceLock<Result<Value, String>> = OnceLock::new();
static FES_SEEDS: OnceLock<Result<Value, String>> = OnceLock::new();
static NORMAL_GUARANTEE_SEEDS: OnceLock<Result<Value, String>> = OnceLock::new();
static FES_GUARANTEE_SEEDS: OnceLock<Result<Value, String>> = OnceLock::new();

pub(super) fn draw_movie_id(
    gacha: &Value,
    character_id: i64,
) -> Result<&str, PersonalServiceError> {
    let rarity = character_asset_data(character_id)?.rarity;
    let use_guarantee =
        matches!(rarity, 4 | 5) && super::select_weighted_index(&[80.0, 20.0])? == 1;
    let key = if use_guarantee {
        "guaranteeMovieName"
    } else {
        "movieName"
    };
    Ok(gacha
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or(if use_guarantee {
            "normal_guarantee"
        } else {
            "normal"
        }))
}

// //// 按动画和稀有度选择可播放 seed [@x380kkm 2026-08-24] ////
pub(super) fn movie_seed(character_id: i64, movie_id: &str) -> Result<i64, PersonalServiceError> {
    let rarity = character_asset_data(character_id)?.rarity;
    if movie_id == "rarity_5_guarantee" {
        return Ok(character_id * 1_000);
    }
    let (seeds, source) = match movie_id {
        "normal" => (&NORMAL_SEEDS, NORMAL_SEEDS_ASSET),
        "fes" => (&FES_SEEDS, FES_SEEDS_ASSET),
        "normal_guarantee" => (&NORMAL_GUARANTEE_SEEDS, NORMAL_GUARANTEE_SEEDS_ASSET),
        "fes_guarantee" => (&FES_GUARANTEE_SEEDS, FES_GUARANTEE_SEEDS_ASSET),
        _ => (&FALLBACK_SEEDS, FALLBACK_SEEDS_ASSET),
    };
    let document = seeds.get_or_init(|| {
        serde_json::from_str::<Value>(source)
            .map_err(|error| format!("failed to decode CN gacha movie seeds: {error}"))
    });
    let document = document
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    let rarity_key = (6 - rarity).to_string();
    let seeds = document
        .get(&rarity_key)
        .and_then(|value| value.get("0"))
        .and_then(Value::as_array);
    let Some(seeds) = seeds.filter(|seeds| !seeds.is_empty()) else {
        return Ok(character_id * 1_000);
    };
    let mut bytes = [0_u8; 8];
    getrandom(&mut bytes).map_err(|error| {
        PersonalServiceError::new(format!("failed to select CN gacha movie seed: {error}"))
    })?;
    let index = (u64::from_le_bytes(bytes) % seeds.len() as u64) as usize;
    seeds[index]
        .as_i64()
        .ok_or_else(|| PersonalServiceError::new("CN gacha movie seed is invalid"))
}
