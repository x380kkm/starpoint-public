# audience: internal
# # audit-cn-gacha-candidate-assets
#
# 此脚本从最终客户端 master 和服务端卡池展开全部候选, 并核对结果页资源与动画 seed 契约.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

from __future__ import annotations

import argparse
import base64
import csv
import hashlib
import io
import json
import subprocess
import sys
import zlib
import zipfile
from collections.abc import Iterable
from pathlib import Path
from typing import Any

import cn_gacha_banner_assets as banner_assets
from cn_gacha_banner_assets import (
    EntityCatalog,
    EntityRecord,
    GachaBannerError,
    hash_cn_asset_path,
    read_logical_assets,
)
from cn_gacha_banner_atlas import (
    ITEM_ATLAS_LOGICAL_PATH,
    ITEM_SPRITE_SHEET_LOGICAL_PATH,
    _Amf3Decoder,
    _atlas_index,
    _decode_atlas,
)


GACHA_MASTER_PATH = "master/gacha/gacha.orderedmap"
CHARACTER_MASTER_PATH = "master/character/character.orderedmap"
EQUIPMENT_MASTER_PATH = "master/item/equipment.orderedmap"
CHARACTER_GACHA_SOUND_MASTER_PATH = "master/character/character_gacha_sound.orderedmap"
CHARACTER_ODDS_FIELDS = (14, 15, 16)
EQUIPMENT_ODDS_FIELDS = (22, 23, 24)
MOVIE_IDS = ("normal", "fes", "normal_guarantee", "fes_guarantee", "rarity_5_guarantee")
MOVIE_SEED_FILES = {
    "normal": "gacha_movie_seeds_normal.json",
    "fes": "gacha_movie_seeds_fes.json",
    "normal_guarantee": "gacha_movie_seeds_normal_guarantee.json",
    "fes_guarantee": "gacha_movie_seeds_fes_guarantee.json",
}
EXPECTED_SIMULATOR_RARITY = {"1": 2, "2": 1, "3": 0}
CLIENT_SEED_MIN = -(2**31)
CLIENT_SEED_MAX = 2**31 - 1
GENERATED_SEED_MIN = 10_000_000
GENERATED_SEED_MAX = 10_100_000


# //// 读取命令参数和 JSON 输入 [@x380kkm 2026-08-25] ////
def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cdn-root", type=Path, required=True)
    parser.add_argument("--app-asset-root", type=Path, required=True)
    parser.add_argument("--reference-cdn-root", type=Path, required=True)
    parser.add_argument("--service-assets-root", type=Path, required=True)
    parser.add_argument("--reference-assets-root", type=Path, required=True)
    parser.add_argument("--physics-root", type=Path, required=True)
    return parser.parse_args()


def read_json(path: Path) -> Any:
    with path.open("r", encoding="utf-8") as stream:
        return json.load(stream)


def required_row(master: dict[str, Any], key: str, table_name: str) -> list[str]:
    row = master.get(key)
    if not isinstance(row, list) or not all(isinstance(value, str) for value in row):
        raise GachaBannerError(
            "invalid_master_row",
            "CN master row is missing or invalid",
            table=table_name,
            key=key,
        )
    return row
# //// /读取命令参数和 JSON 输入 ////


# //// 解码含多行数组的 orderedmap [@x380kkm 2026-08-25] ////
def parse_csv_rows(data: bytes) -> list[str] | list[list[str]]:
    try:
        rows = list(csv.reader(io.StringIO(data.decode("utf-8"), newline=""), strict=True))
    except (UnicodeDecodeError, csv.Error) as error:
        raise GachaBannerError("invalid_orderedmap", "orderedmap CSV rows are invalid") from error
    if not rows:
        raise GachaBannerError("invalid_orderedmap", "orderedmap CSV rows are empty")
    return rows[0] if len(rows) == 1 else rows


def decode_multirow_ordered_map(data: bytes) -> dict[str, Any]:
    original_parser = banner_assets._parse_csv_row
    banner_assets._parse_csv_row = parse_csv_rows
    try:
        return banner_assets.decode_ordered_map(data)
    finally:
        banner_assets._parse_csv_row = original_parser
# //// /解码含多行数组的 orderedmap ////


# //// 读取最终 master 和 raw odds [@x380kkm 2026-08-25] ////
def resolve_records(
    catalog: EntityCatalog, logical_paths: Iterable[str]
) -> tuple[dict[str, EntityRecord], list[dict[str, object]]]:
    records: dict[str, EntityRecord] = {}
    gaps: list[dict[str, object]] = []
    for logical_path in sorted(set(logical_paths)):
        record = catalog.find(logical_path)
        if record is None:
            gaps.append({"kind": "missing_entity", "logicalPath": logical_path})
        else:
            records[logical_path] = record
    return records, gaps


def load_master_assets(
    cdn_root: Path, catalog: EntityCatalog
) -> tuple[dict[str, dict[str, Any]], list[dict[str, object]]]:
    logical_paths = (
        GACHA_MASTER_PATH,
        CHARACTER_MASTER_PATH,
        EQUIPMENT_MASTER_PATH,
        CHARACTER_GACHA_SOUND_MASTER_PATH,
    )
    records, gaps = resolve_records(catalog, logical_paths)
    assets, missing = read_logical_assets(cdn_root, records)
    gaps.extend({"kind": "missing_archive_entity", "logicalPath": path} for path in missing)
    decoded: dict[str, dict[str, Any]] = {}
    for logical_path, data in assets.items():
        if logical_path == CHARACTER_GACHA_SOUND_MASTER_PATH:
            decoded[logical_path] = decode_multirow_ordered_map(data)
        else:
            decoded[logical_path] = banner_assets.decode_ordered_map(data)
    return decoded, gaps


def gacha_odds_ids(gacha_master: dict[str, Any]) -> set[str]:
    odds_ids: set[str] = set()
    for pool_id in gacha_master:
        row = required_row(gacha_master, pool_id, "gacha")
        if len(row) <= EQUIPMENT_ODDS_FIELDS[-1]:
            raise GachaBannerError("invalid_gacha_row", "gacha row is shorter than the client schema", poolId=pool_id)
        prize_kind = int(row[13])
        fields = CHARACTER_ODDS_FIELDS if prize_kind == 0 else EQUIPMENT_ODDS_FIELDS
        if prize_kind not in {0, 1}:
            raise GachaBannerError("invalid_gacha_kind", "gacha prize kind is invalid", poolId=pool_id)
        odds_ids.update(row[index] for index in fields if row[index])
    return odds_ids


def load_odds_maps(
    cdn_root: Path, catalog: EntityCatalog, odds_ids: set[str]
) -> tuple[dict[str, dict[str, Any]], list[dict[str, object]]]:
    logical_by_id = {odds_id: f"master/gacha_odds/{odds_id}.orderedmap" for odds_id in odds_ids}
    records, gaps = resolve_records(catalog, logical_by_id.values())
    assets, missing = read_logical_assets(cdn_root, records)
    gaps.extend({"kind": "missing_archive_entity", "logicalPath": path} for path in missing)
    decoded = {
        odds_id: banner_assets.decode_ordered_map(assets[logical_path])
        for odds_id, logical_path in logical_by_id.items()
        if logical_path in assets
    }
    return decoded, gaps
# //// /读取最终 master 和 raw odds ////


# //// 展开客户端和服务端的全部候选 [@x380kkm 2026-08-25] ////
def client_candidates(
    gacha_master: dict[str, Any], odds_maps: dict[str, dict[str, Any]]
) -> tuple[set[int], set[int], list[dict[str, object]]]:
    characters: set[int] = set()
    equipment: set[int] = set()
    gaps: list[dict[str, object]] = []
    for pool_id in gacha_master:
        row = required_row(gacha_master, pool_id, "gacha")
        prize_kind = int(row[13])
        fields = CHARACTER_ODDS_FIELDS if prize_kind == 0 else EQUIPMENT_ODDS_FIELDS
        destination = characters if prize_kind == 0 else equipment
        for odds_id in (row[index] for index in fields if row[index]):
            odds_map = odds_maps.get(odds_id)
            rows = odds_map.get(odds_id) if odds_map else None
            if not isinstance(rows, dict):
                gaps.append({"kind": "missing_odds_group", "poolId": pool_id, "oddsId": odds_id})
                continue
            for odds_row in rows.values():
                if not isinstance(odds_row, list) or len(odds_row) < 2:
                    gaps.append({"kind": "invalid_odds_row", "poolId": pool_id, "oddsId": odds_id})
                    continue
                destination.add(int(odds_row[0]))
    return characters, equipment, gaps


def service_candidates(gacha_document: dict[str, Any]) -> tuple[set[int], set[int]]:
    characters: set[int] = set()
    equipment: set[int] = set()
    for pool in gacha_document.values():
        if not isinstance(pool, dict) or pool.get("type") not in {0, 1}:
            raise GachaBannerError("invalid_service_gacha", "service gacha definition is invalid")
        destination = characters if pool["type"] == 0 else equipment
        rank_pools = pool.get("pool")
        if not isinstance(rank_pools, dict):
            raise GachaBannerError("invalid_service_gacha", "service gacha candidate pool is invalid")
        for entries in rank_pools.values():
            if not isinstance(entries, list):
                raise GachaBannerError("invalid_service_gacha", "service gacha rank pool is invalid")
            for entry in entries:
                if not isinstance(entry, dict) or not isinstance(entry.get("id"), int):
                    raise GachaBannerError("invalid_service_gacha", "service gacha candidate is invalid")
                destination.add(entry["id"])
    return characters, equipment
# //// /展开客户端和服务端的全部候选 ////


# //// 推导角色结果页资源和装备图标键 [@x380kkm 2026-08-25] ////
def character_requirements(character_id: int, row: list[str]) -> list[tuple[str, tuple[str, ...]]]:
    if len(row) <= 2 or not row[0]:
        raise GachaBannerError("invalid_character_master", "character master row is invalid", characterId=character_id)
    string_id = row[0]
    rarity = int(row[2])
    base = f"character/{string_id}"
    requirements = [
        ("pixelart_frame", (f"{base}/pixelart/pixelart.frame.amf3.deflate",)),
        ("pixelart_atlas", (f"{base}/pixelart/sprite_sheet.atlas.amf3.deflate",)),
        ("pixelart_texture", (f"{base}/pixelart/sprite_sheet.png", f"{base}/pixelart/sprite_sheet.atf.deflate")),
        ("full_shot", (f"{base}/ui/full_shot_1440_1920_0.png",)),
    ]
    if rarity >= 4:
        requirements.extend(
            [
                ("special_frame", (f"{base}/pixelart/special.frame.amf3.deflate",)),
                ("special_atlas", (f"{base}/pixelart/special_sprite_sheet.atlas.amf3.deflate",)),
                (
                    "special_texture",
                    (f"{base}/pixelart/special_sprite_sheet.png", f"{base}/pixelart/special_sprite_sheet.atf.deflate"),
                ),
            ]
        )
    return requirements


def sound_paths(value: Any) -> set[str]:
    paths: set[str] = set()
    if isinstance(value, dict):
        for child in value.values():
            paths.update(sound_paths(child))
    elif isinstance(value, list):
        for child in value:
            paths.update(sound_paths(child))
    elif isinstance(value, str) and value.startswith("sound_effect/"):
        paths.add(f"{value}.mp3")
    return paths


def app_bundle_path(app_asset_root: Path, logical_path: str) -> Path:
    digest = hash_cn_asset_path(logical_path)
    return app_asset_root / "production" / "ios_bundle" / digest[:2] / digest[2:]


def app_bundle_location(app_asset_root: Path, logical_paths: Iterable[str]) -> tuple[str, str] | None:
    for logical_path in logical_paths:
        candidate = app_bundle_path(app_asset_root, logical_path)
        if candidate.is_file() and candidate.stat().st_size > 0:
            return logical_path, str(candidate.relative_to(app_asset_root)).replace("\\", "/")
    return None


def resolve_candidate_resources(
    app_asset_root: Path,
    catalog: EntityCatalog,
    character_master: dict[str, Any],
    sound_master: dict[str, Any],
    character_ids: set[int],
) -> tuple[dict[str, EntityRecord], dict[str, str], list[dict[str, object]], int]:
    records: dict[str, EntityRecord] = {}
    app_locations: dict[str, str] = {}
    gaps: list[dict[str, object]] = []
    sound_count = 0
    for character_id in sorted(character_ids):
        row = character_master.get(str(character_id))
        if not isinstance(row, list):
            gaps.append({"kind": "missing_character_master", "characterId": character_id})
            continue
        for name, options in character_requirements(character_id, row):
            bundled = app_bundle_location(app_asset_root, options)
            if bundled is not None:
                app_locations[bundled[0]] = bundled[1]
                continue
            matches = [(path, catalog.find(path)) for path in options]
            selected = next(((path, record) for path, record in matches if record is not None), None)
            if selected is None:
                gaps.append(
                    {"kind": "missing_character_resource_entity", "characterId": character_id, "name": name, "paths": options}
                )
            else:
                records[selected[0]] = selected[1]
        for logical_path in sorted(sound_paths(sound_master.get(str(character_id)))):
            sound_count += 1
            bundled = app_bundle_location(app_asset_root, (logical_path,))
            if bundled is not None:
                app_locations[bundled[0]] = bundled[1]
                continue
            record = catalog.find(logical_path)
            if record is None:
                gaps.append(
                    {"kind": "missing_character_sound_entity", "characterId": character_id, "logicalPath": logical_path}
                )
            else:
                records[logical_path] = record
    return records, app_locations, gaps, sound_count
# //// /推导角色结果页资源和装备图标键 ////


# //// 流式核对包内覆盖层和归档实体 [@x380kkm 2026-08-25] ////
def stream_digest(stream: Any) -> str:
    digest = hashlib.sha256()
    while chunk := stream.read(1024 * 1024):
        digest.update(chunk)
    encoded = base64.b64encode(digest.digest()).decode("ascii")
    return encoded.rstrip("=").replace("+", "_").replace("/", "-")


def bundle_path(cdn_root: Path, record: EntityRecord) -> Path:
    parts = record.entry_path.split("/")
    return cdn_root / "production" / "bundle" / parts[-2] / parts[-1]


def verify_storage(
    cdn_root: Path, records: dict[str, EntityRecord]
) -> tuple[dict[str, str], list[str]]:
    logical_by_entry = {record.entry_path: path for path, record in records.items()}
    pending = set(logical_by_entry)
    locations: dict[str, str] = {}
    for logical_path, record in records.items():
        direct_path = bundle_path(cdn_root, record)
        if not direct_path.is_file() or direct_path.stat().st_size != record.byte_length:
            continue
        with direct_path.open("rb") as stream:
            if stream_digest(stream) != record.digest:
                continue
        pending.discard(record.entry_path)
        locations[logical_path] = str(direct_path.relative_to(cdn_root)).replace("\\", "/")

    for asset_kind in sorted({records[logical_by_entry[entry]].asset_kind for entry in pending}):
        for suffix in ("full", "diff"):
            archive_root = cdn_root / f"archive-{asset_kind}-{suffix}"
            if not archive_root.is_dir():
                continue
            for archive_path in sorted(archive_root.glob("*.zip")):
                relevant = {entry for entry in pending if records[logical_by_entry[entry]].asset_kind == asset_kind}
                if not relevant:
                    break
                with zipfile.ZipFile(archive_path, "r") as archive:
                    for info in archive.infolist():
                        if info.filename not in relevant:
                            continue
                        logical_path = logical_by_entry[info.filename]
                        record = records[logical_path]
                        if info.file_size != record.byte_length:
                            continue
                        with archive.open(info, "r") as stream:
                            if stream_digest(stream) != record.digest:
                                continue
                        pending.remove(info.filename)
                        locations[logical_path] = f"{archive_root.name}/{archive_path.name}"
    return locations, sorted(logical_by_entry[entry] for entry in pending)
# //// /流式核对包内覆盖层和归档实体 ////


# //// 核对装备候选的 master 和 item atlas [@x380kkm 2026-08-25] ////
def audit_equipment(
    cdn_root: Path,
    catalog: EntityCatalog,
    equipment_master: dict[str, Any],
    equipment_ids: set[int],
) -> tuple[dict[str, object], list[dict[str, object]]]:
    gaps: list[dict[str, object]] = []
    pixelart_names: set[str] = set()
    for equipment_id in sorted(equipment_ids):
        row = equipment_master.get(str(equipment_id))
        if not isinstance(row, list) or len(row) <= 6 or not row[6]:
            gaps.append({"kind": "missing_equipment_master", "equipmentId": equipment_id})
            continue
        pixelart_names.add(row[6])
    atlas_paths = (ITEM_ATLAS_LOGICAL_PATH, ITEM_SPRITE_SHEET_LOGICAL_PATH)
    records, entity_gaps = resolve_records(catalog, atlas_paths)
    gaps.extend(entity_gaps)
    assets, missing = read_logical_assets(cdn_root, records)
    gaps.extend({"kind": "missing_archive_entity", "logicalPath": path} for path in missing)
    missing_names: list[str] = []
    if ITEM_ATLAS_LOGICAL_PATH in assets:
        atlas_names = set(_atlas_index(_decode_atlas(assets[ITEM_ATLAS_LOGICAL_PATH])))
        missing_names = sorted(pixelart_names - atlas_names)
        gaps.extend({"kind": "missing_equipment_atlas_entry", "pixelart": name} for name in missing_names)
    return {
        "candidateCount": len(equipment_ids),
        "masterCount": len(equipment_master),
        "pixelartKeyCount": len(pixelart_names),
        "missingPixelartKeys": missing_names,
    }, gaps
# //// /核对装备候选的 master 和 item atlas ////


# //// 核对动画配置和 seed 重放 [@x380kkm 2026-08-25] ////
def movie_combinations(gacha_document: dict[str, Any]) -> dict[tuple[str, str], set[int]]:
    combinations: dict[tuple[str, str], set[int]] = {}
    for pool in gacha_document.values():
        if pool["type"] != 0:
            continue
        for rarity_key in ("1", "2", "3"):
            entries = [entry for entry in pool["pool"].get(rarity_key, []) if float(entry.get("rarity", 0)) > 0]
            if not entries:
                continue
            normal_rates = pool["rankRates"]["normal"]
            normal_possible = len(normal_rates) >= int(rarity_key) and float(normal_rates[int(rarity_key) - 1]) > 0
            guarantee_rates = pool["rankRates"]["multiGuarantee"]
            guarantee_index = int(rarity_key) - 1
            guarantee_possible = guarantee_index < len(guarantee_rates) and float(guarantee_rates[guarantee_index]) > 0
            if not (normal_possible or guarantee_possible):
                continue
            movie_ids = {pool["movieName"]}
            if rarity_key in {"1", "2"}:
                movie_ids.add(pool["guaranteeMovieName"])
            for movie_id in movie_ids - {""}:
                combinations.setdefault((movie_id, rarity_key), set()).update(int(entry["id"]) for entry in entries)
    return combinations


def physics_module_path(physics_root: Path) -> Path:
    source_path = physics_root / "src" / "lib" / "gacha-physics.ts"
    module_path = physics_root / "out" / "lib" / "gacha-physics.js"
    if not source_path.is_file() or not module_path.is_file():
        raise GachaBannerError(
            "invalid_physics_root",
            "reference gacha physics source and compiled module are required",
            root=str(physics_root),
        )
    if module_path.stat().st_mtime_ns < source_path.stat().st_mtime_ns:
        raise GachaBannerError(
            "stale_physics_module",
            "reference gacha physics compiled module is older than its source",
            source=str(source_path),
            module=str(module_path),
        )
    return module_path


def run_simulator(physics_root: Path, cases: list[dict[str, object]]) -> dict[str, Any]:
    module_path = physics_module_path(physics_root)
    javascript = r"""
const fs = require("fs");
const payload = JSON.parse(fs.readFileSync(0, "utf8"));
const physics = require(payload.modulePath);
if (typeof physics.generateSeedPools !== "function") {
  throw new Error("reference gacha physics module does not expose generateSeedPools");
}
const mismatches = [];
let replayed = 0;
let generatedExtensionsChecked = 0;
for (const test of payload.cases) {
  const config = physics.MOVIE_CONFIGS[test.movieId];
  if (!config) {
    mismatches.push({ kind: "missing_movie_config", movieId: test.movieId });
    continue;
  }
  for (const seed of test.generatedExtensionSeeds ?? []) {
    const generated = physics.generateSeedPools(config, seed, seed)[test.expectedRarity] ?? [];
    generatedExtensionsChecked += 1;
    if (!generated.includes(seed)) {
      mismatches.push({
        kind: "movie_seed_extension_not_generated",
        movieId: test.movieId,
        rarityKey: test.rarityKey,
        seed,
      });
    }
  }
  for (const seed of test.seeds) {
    const simulator = new physics.GachaSimulator(seed, config);
    const rarity = simulator.simulate();
    replayed += 1;
    if (rarity !== test.expectedRarity || (test.expectSkipped && simulator.moviePlayable)) {
      mismatches.push({
        kind: "movie_seed_mismatch",
        movieId: test.movieId,
        rarityKey: test.rarityKey,
        seed,
        expectedRarity: test.expectedRarity,
        actualRarity: rarity,
        moviePlayable: simulator.moviePlayable,
      });
    }
  }
}
process.stdout.write(JSON.stringify({ generatedExtensionsChecked, mismatches, replayed }));
"""
    completed = subprocess.run(
        ["node", "-e", javascript],
        input=json.dumps({"modulePath": str(module_path), "cases": cases}),
        text=True,
        capture_output=True,
        check=False,
    )
    if completed.returncode != 0:
        raise GachaBannerError(
            "simulator_failed",
            "reference gacha simulator failed",
            stderr=completed.stderr.strip(),
        )
    return json.loads(completed.stdout)


def audit_movies(
    cdn_root: Path,
    catalog: EntityCatalog,
    service_assets_root: Path,
    reference_assets_root: Path,
    physics_root: Path,
    gacha_document: dict[str, Any],
) -> tuple[dict[str, object], list[dict[str, object]]]:
    gaps: list[dict[str, object]] = []
    combinations = movie_combinations(gacha_document)
    unknown_movies = sorted({movie_id for movie_id, _ in combinations} - set(MOVIE_IDS))
    gaps.extend({"kind": "unknown_movie_id", "movieId": movie_id} for movie_id in unknown_movies)

    config_paths = {movie_id: f"gacha/{movie_id}.gacha.amf3.deflate" for movie_id in MOVIE_IDS}
    records, entity_gaps = resolve_records(catalog, config_paths.values())
    gaps.extend(entity_gaps)
    assets, missing = read_logical_assets(cdn_root, records)
    gaps.extend({"kind": "missing_archive_entity", "logicalPath": path} for path in missing)
    config_rows: list[dict[str, object]] = []
    for movie_id, logical_path in config_paths.items():
        data = assets.get(logical_path)
        if data is None:
            continue
        try:
            inflated = zlib.decompress(data, -zlib.MAX_WBITS)
        except zlib.error as error:
            raise GachaBannerError("invalid_movie_config", "movie config raw DEFLATE is invalid", movieId=movie_id) from error
        source_path = reference_assets_root / "gacha_movie_configs" / f"{movie_id}.amf3"
        source = source_path.read_bytes()
        if inflated != source:
            gaps.append({"kind": "movie_config_source_mismatch", "movieId": movie_id})
        decoded = _Amf3Decoder(inflated).decode()
        threshold = decoded.get("threshold") if isinstance(decoded, dict) else None
        if movie_id == "rarity_5_guarantee" and (not isinstance(threshold, dict) or threshold.get("isRarity5") is not True):
            gaps.append({"kind": "rarity_5_config_is_not_forced", "movieId": movie_id})
        config_rows.append(
            {
                "movieId": movie_id,
                "logicalPath": logical_path,
                "compressedBytes": len(data),
                "inflatedBytes": len(inflated),
                "isRarity5": threshold.get("isRarity5") if isinstance(threshold, dict) else None,
            }
        )

    cases: list[dict[str, object]] = []
    seed_rows: list[dict[str, object]] = []
    extension_seeds_by_pool: dict[tuple[str, str], set[int]] = {}
    for movie_id, file_name in MOVIE_SEED_FILES.items():
        current = read_json(service_assets_root / file_name)
        reference = read_json(reference_assets_root / file_name)
        for rarity_key in ("1", "2", "3"):
            if (movie_id, rarity_key) not in combinations:
                continue
            seeds = current.get(rarity_key, {}).get("0", [])
            if not seeds:
                gaps.append({"kind": "empty_seed_pool", "movieId": movie_id, "rarityKey": rarity_key})
                continue
            if not isinstance(seeds, list) or any(
                not isinstance(seed, int) or isinstance(seed, bool) for seed in seeds
            ):
                gaps.append({"kind": "invalid_movie_seed_pool", "movieId": movie_id, "rarityKey": rarity_key})
                continue
            invalid_seeds = sorted(seed for seed in seeds if seed < CLIENT_SEED_MIN or seed > CLIENT_SEED_MAX)
            if invalid_seeds:
                gaps.append(
                    {
                        "kind": "movie_seed_out_of_range",
                        "movieId": movie_id,
                        "rarityKey": rarity_key,
                        "seeds": invalid_seeds,
                    }
                )
                continue
            if len(seeds) != len(set(seeds)):
                gaps.append({"kind": "duplicate_movie_seed", "movieId": movie_id, "rarityKey": rarity_key})

            reference_values = reference.get(rarity_key, {}).get("0", [])
            if not isinstance(reference_values, list) or any(
                not isinstance(seed, int) or isinstance(seed, bool) for seed in reference_values
            ):
                raise GachaBannerError(
                    "invalid_reference_seed_pool",
                    "reference movie seed pool is invalid",
                    movieId=movie_id,
                    rarityKey=rarity_key,
                )
            reference_seeds = set(reference_values)
            extension_seeds = sorted(set(seeds) - reference_seeds)
            unproven_extensions = [
                seed for seed in extension_seeds if seed < GENERATED_SEED_MIN or seed > GENERATED_SEED_MAX
            ]
            if unproven_extensions:
                gaps.append(
                    {
                        "kind": "movie_seed_unproven_extension",
                        "movieId": movie_id,
                        "rarityKey": rarity_key,
                        "seeds": unproven_extensions,
                    }
                )
            generated_extensions = sorted(set(extension_seeds) - set(unproven_extensions))
            extension_seeds_by_pool[(movie_id, rarity_key)] = set(generated_extensions)
            cases.append(
                {
                    "movieId": movie_id,
                    "rarityKey": rarity_key,
                    "expectedRarity": EXPECTED_SIMULATOR_RARITY[rarity_key],
                    "expectSkipped": False,
                    "generatedExtensionSeeds": generated_extensions,
                    "seeds": seeds,
                }
            )
            seed_rows.append(
                {
                    "movieId": movie_id,
                    "rarityKey": rarity_key,
                    "seedCount": len(seeds),
                    "referenceSeedCount": len(set(seeds) & reference_seeds),
                    "generatedExtensionCount": len(generated_extensions),
                    "unprovenExtensionCount": len(unproven_extensions),
                    "replayCount": len(seeds),
                }
            )

    rarity_5_ids = sorted(
        set().union(
            *(character_ids for (movie_id, _), character_ids in combinations.items() if movie_id == "rarity_5_guarantee")
        )
    )
    rarity_5_seeds = [character_id * 1000 for character_id in rarity_5_ids]
    invalid_rarity_5_seeds = [
        seed for seed in rarity_5_seeds if seed < CLIENT_SEED_MIN or seed > CLIENT_SEED_MAX
    ]
    if invalid_rarity_5_seeds:
        gaps.append({"kind": "rarity_5_seed_out_of_range", "seeds": invalid_rarity_5_seeds})
    if rarity_5_seeds:
        cases.append(
            {
                "movieId": "rarity_5_guarantee",
                "rarityKey": "forced",
                "expectedRarity": 2,
                "expectSkipped": True,
                "generatedExtensionSeeds": [],
                "seeds": rarity_5_seeds,
            }
        )
    simulation = run_simulator(physics_root, cases)
    mismatch_counts: dict[tuple[str, str], dict[int, int]] = {}
    mismatch_seeds: dict[tuple[str, str], set[int]] = {}
    generation_mismatch_seeds: dict[tuple[str, str], set[int]] = {}
    for mismatch in simulation["mismatches"]:
        kind = mismatch.get("kind")
        if kind not in {"movie_seed_mismatch", "movie_seed_extension_not_generated"}:
            continue
        key = (str(mismatch["movieId"]), str(mismatch["rarityKey"]))
        if kind == "movie_seed_extension_not_generated":
            generation_mismatch_seeds.setdefault(key, set()).add(int(mismatch["seed"]))
            continue
        actual_rarity = int(mismatch["actualRarity"])
        counts = mismatch_counts.setdefault(key, {})
        counts[actual_rarity] = counts.get(actual_rarity, 0) + 1
        mismatch_seeds.setdefault(key, set()).add(int(mismatch["seed"]))
    extension_rows: list[dict[str, object]] = []
    for row in seed_rows:
        key = (str(row["movieId"]), str(row["rarityKey"]))
        actual_counts = mismatch_counts.get(key, {})
        mismatch_count = sum(actual_counts.values())
        extension_seeds = extension_seeds_by_pool.get(key, set())
        invalid_extensions = extension_seeds & (
            mismatch_seeds.get(key, set()) | generation_mismatch_seeds.get(key, set())
        )
        row["validReplayCount"] = int(row["replayCount"]) - mismatch_count
        row["mismatchCount"] = mismatch_count
        row["mismatchActualRarityCounts"] = {
            str(rarity): count for rarity, count in sorted(actual_counts.items())
        }
        row["validGeneratedExtensionCount"] = len(extension_seeds) - len(invalid_extensions)
        row["invalidGeneratedExtensionCount"] = len(invalid_extensions)
        if extension_seeds:
            extension_rows.append(
                {
                    "movieId": key[0],
                    "rarityKey": key[1],
                    "source": "referencePhysicsGeneratedRange",
                    "seedCount": len(extension_seeds),
                    "seedMinimum": min(extension_seeds),
                    "seedMaximum": max(extension_seeds),
                    "generationRangeMinimum": GENERATED_SEED_MIN,
                    "generationRangeMaximum": GENERATED_SEED_MAX,
                    "validReplayCount": len(extension_seeds) - len(invalid_extensions),
                    "mismatchCount": len(invalid_extensions),
                }
            )
    gaps.extend(simulation["mismatches"])
    return {
        "combinations": [
            {"movieId": movie_id, "rarityKey": rarity_key, "candidateCount": len(character_ids)}
            for (movie_id, rarity_key), character_ids in sorted(combinations.items())
        ],
        "config": config_rows,
        "seedPools": seed_rows,
        "seedExtensions": extension_rows,
        "generatedExtensionSeedCount": sum(int(row["seedCount"]) for row in extension_rows),
        "generatedExtensionCheckCount": simulation["generatedExtensionsChecked"],
        "rarity5DeterministicSeedCount": len(rarity_5_seeds),
        "replayedSeedCount": simulation["replayed"],
    }, gaps
# //// /核对动画配置和 seed 重放 ////


# //// 为缺口定位 CN 参考实体 [@x380kkm 2026-08-25] ////
def fallback_mappings(
    reference_root: Path, logical_paths: Iterable[str]
) -> list[dict[str, object]]:
    catalog = EntityCatalog.load(reference_root)
    mappings: list[dict[str, object]] = []
    for logical_path in sorted(set(logical_paths)):
        record = catalog.find(logical_path)
        if record is None:
            continue
        _, missing = verify_storage(reference_root, {logical_path: record})
        mappings.append(
            {
                "logicalPath": logical_path,
                "sourceRegion": "cn",
                "entryPath": record.entry_path,
                "available": not missing,
            }
        )
    return mappings
# //// /为缺口定位 CN 参考实体 ////


# //// 执行全量候选与结果资源审计 [@x380kkm 2026-08-25] ////
def audit(args: argparse.Namespace) -> dict[str, object]:
    cdn_root = args.cdn_root.resolve(strict=True)
    app_asset_root = args.app_asset_root.resolve(strict=True)
    reference_cdn_root = args.reference_cdn_root.resolve(strict=True)
    service_assets_root = args.service_assets_root.resolve(strict=True)
    reference_assets_root = args.reference_assets_root.resolve(strict=True)
    physics_root = args.physics_root.resolve(strict=True)
    catalog = EntityCatalog.load(cdn_root)
    decoded, gaps = load_master_assets(cdn_root, catalog)
    required_masters = {
        GACHA_MASTER_PATH,
        CHARACTER_MASTER_PATH,
        EQUIPMENT_MASTER_PATH,
        CHARACTER_GACHA_SOUND_MASTER_PATH,
    }
    if set(decoded) != required_masters:
        missing = sorted(required_masters - set(decoded))
        raise GachaBannerError("missing_master_data", "candidate audit master data is incomplete", logicalPaths=missing)

    gacha_master = decoded[GACHA_MASTER_PATH]
    character_master = decoded[CHARACTER_MASTER_PATH]
    equipment_master = decoded[EQUIPMENT_MASTER_PATH]
    sound_master = decoded[CHARACTER_GACHA_SOUND_MASTER_PATH]
    odds_ids = gacha_odds_ids(gacha_master)
    odds_maps, odds_gaps = load_odds_maps(cdn_root, catalog, odds_ids)
    gaps.extend(odds_gaps)
    client_character_ids, client_equipment_ids, candidate_gaps = client_candidates(gacha_master, odds_maps)
    gaps.extend(candidate_gaps)

    gacha_document = read_json(service_assets_root / "gacha.json")
    service_character_ids, service_equipment_ids = service_candidates(gacha_document)
    character_ids = client_character_ids | service_character_ids
    equipment_ids = client_equipment_ids | service_equipment_ids

    character_records, app_locations, character_gaps, sound_count = resolve_candidate_resources(
        app_asset_root, catalog, character_master, sound_master, character_ids
    )
    gaps.extend(character_gaps)
    cdn_locations, missing_storage = verify_storage(cdn_root, character_records)
    locations = app_locations | cdn_locations
    gaps.extend({"kind": "missing_character_resource_storage", "logicalPath": path} for path in missing_storage)

    equipment_report, equipment_gaps = audit_equipment(cdn_root, catalog, equipment_master, equipment_ids)
    gaps.extend(equipment_gaps)
    movie_report, movie_gaps = audit_movies(
        cdn_root,
        catalog,
        service_assets_root,
        reference_assets_root,
        physics_root,
        gacha_document,
    )
    gaps.extend(movie_gaps)

    missing_paths = [
        gap["logicalPath"]
        for gap in gaps
        if isinstance(gap.get("logicalPath"), str)
        and gap["kind"] in {"missing_entity", "missing_character_resource_entity", "missing_character_sound_entity"}
    ]
    return {
        "status": "ok" if not gaps else "gaps",
        "clientGachaPoolCount": len(gacha_master),
        "clientOddsMapCount": len(odds_ids),
        "serviceGachaPoolCount": len(gacha_document),
        "characters": {
            "clientCandidateCount": len(client_character_ids),
            "serviceCandidateCount": len(service_character_ids),
            "candidateUnionCount": len(character_ids),
            "masterCount": len(character_master),
            "soundPathCount": sound_count,
            "appBundleResourceCount": len(app_locations),
            "cdnResourceCount": len(cdn_locations),
            "resourceEntityCount": len(character_records),
            "verifiedStorageCount": len(locations),
        },
        "equipment": equipment_report,
        "movie": movie_report,
        "gaps": gaps,
        "fallbackMappings": fallback_mappings(reference_cdn_root, missing_paths),
    }


def main() -> int:
    try:
        report = audit(parse_args())
        sys.stdout.write(json.dumps(report, ensure_ascii=False, separators=(",", ":")) + "\n")
        return 0
    except GachaBannerError as error:
        sys.stderr.write(
            json.dumps(
                {"code": error.code, "message": str(error), "details": error.details},
                ensure_ascii=False,
                separators=(",", ":"),
            )
            + "\n"
        )
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
# //// /执行全量候选与结果资源审计 ////
