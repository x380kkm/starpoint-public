# audience: internal
# # generate-cn-gacha-banners
#
# 此脚本为 CN 卡池生成客户端可读取的图标 banner, 并同步游戏资源和管理页图片.
# 同一 banner logical path 只生成一个确定性资源, 目录项保留各自 CN 卡池证据.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import tempfile
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from cn_gacha_banner_assets import (
    EntityCatalog,
    EntityRecord,
    GachaBannerError,
    decode_ordered_map,
    hash_cn_asset_path,
    inspect_png,
    normalize_logical_path,
    pseudo_png,
    read_logical_assets,
    render_icon_banner,
    standard_png,
)
from cn_gacha_banner_atlas import (
    ITEM_ATLAS_LOGICAL_PATH,
    ITEM_SPRITE_SHEET_LOGICAL_PATH,
    extract_item_atlas_icons,
)

GACHA_MASTER_PATH = "master/gacha/gacha.orderedmap"
CHARACTER_MASTER_PATH = "master/character/character.orderedmap"
EQUIPMENT_MASTER_PATH = "master/item/equipment.orderedmap"
CHARACTER_ODDS_FIELDS = (14, 15, 16)
EQUIPMENT_ODDS_FIELDS = (22, 23, 24)
CHARACTER_ICON_PATTERNS = (
    "character/{string_id}/ui/square_0.png",
    "character/{string_id}/ui/square_132_132_0.png",
    "character/{string_id}/ui/square_round_136_136_0.png",
    "character/{string_id}/ui/square_round_95_95_0.png",
)
BANNER_WIDTH = 510
BANNER_HEIGHT = 180
UNRESOLVED_TAG = "banner:unresolved"
GENERATED_TAG = "banner:generated"
GENERATED_BANNER_PATH_PREFIX = "dynamic/gacha_list_banner/starpoint_generated/"
ACTIVITY_MANIFEST_FIELDS = (
    "activity_id",
    "name",
    "kind",
    "tags",
    "description",
    "banner_key",
    "banner_width",
    "banner_height",
    "default_start_at_ms",
    "default_end_at_ms",
)
IMAGE_CANDIDATE_MANIFEST_FIELDS = (
    "key",
    "width",
    "height",
    "source_type",
    "evidence",
)
RICH_CATALOG_NAMES = ("activity-catalog-source.json", "activity-catalog-rich.json")


@dataclass(frozen=True)
class ItemCandidate:
    item_id: int
    rank: int
    is_rate_up: bool
    is_limited: bool


@dataclass(frozen=True)
class IconOption:
    candidate: ItemCandidate
    logical_path: str
    record: EntityRecord | None


@dataclass
class PoolPlan:
    activity: dict[str, Any]
    pool_id: int
    prize_type: str
    original_banner_logical_path: str
    banner_logical_path: str
    candidates: list[ItemCandidate]
    content_signature: tuple[str, tuple[tuple[str, ...], ...]]
    options: list[IconOption] = field(default_factory=list)
    selected: list[IconOption] = field(default_factory=list)
    gaps: list[dict[str, object]] = field(default_factory=list)


# //// 读取活动目录和 CN master [@x380kkm 2026-08-24] ////
def _read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8-sig"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise GachaBannerError("invalid_json", "JSON input cannot be read", path=str(path)) from error
    if not isinstance(value, dict):
        raise GachaBannerError("invalid_json", "JSON input root must be an object", path=str(path))
    return value


def _atomic_write(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_name = tempfile.mkstemp(prefix=f".{path.name}.", suffix=".tmp", dir=path.parent)
    temporary_path = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(data)
        os.replace(temporary_path, path)
    finally:
        temporary_path.unlink(missing_ok=True)


def _atomic_write_json(path: Path, value: object) -> None:
    data = (json.dumps(value, ensure_ascii=False, indent=2) + "\n").encode("utf-8")
    _atomic_write(path, data)


# //// 将富活动目录投影为个人服务清单 [@x380kkm 2026-08-24] ////
def _manifest_candidate_key(candidate: dict[str, Any]) -> str | None:
    key = candidate.get("key")
    if isinstance(key, str) and key:
        return key
    source_hash = candidate.get("source_hash")
    if (
        isinstance(source_hash, str)
        and len(source_hash) == 40
        and all(character in "0123456789abcdef" for character in source_hash)
    ):
        return f"{source_hash}.png"
    return None


def _require_rich_candidate_identity(
    activity_id: object, candidate: object
) -> tuple[str, str]:
    if not isinstance(candidate, dict):
        raise GachaBannerError(
            "invalid_catalog_candidate",
            "rich activity image candidate must be an object",
            activity_id=activity_id,
        )
    raw_key = candidate.get("key")
    if raw_key is not None and (
        not isinstance(raw_key, str) or not re.fullmatch(r"[0-9a-f]{40}\.png", raw_key)
    ):
        raise GachaBannerError(
            "invalid_catalog_candidate_key",
            "rich activity image candidate key is invalid",
            activity_id=activity_id,
        )
    source_hash = candidate.get("source_hash")
    if source_hash is not None and (
        not isinstance(source_hash, str)
        or not re.fullmatch(r"[0-9a-f]{40}", source_hash)
    ):
        raise GachaBannerError(
            "invalid_catalog_candidate_hash",
            "rich activity image candidate source_hash is invalid",
            activity_id=activity_id,
        )
    if raw_key is None and source_hash is None:
        raise GachaBannerError(
            "missing_catalog_candidate_key",
            "rich activity image candidate requires key or source_hash",
            activity_id=activity_id,
        )
    key = raw_key or f"{source_hash}.png"
    if source_hash is not None and key != f"{source_hash}.png":
        raise GachaBannerError(
            "catalog_candidate_hash_mismatch",
            "rich activity image candidate key does not match source_hash",
            activity_id=activity_id,
            key=key,
            source_hash=source_hash,
        )
    source_entry = candidate.get("source_entry")
    if not isinstance(source_entry, str) or not source_entry:
        raise GachaBannerError(
            "missing_catalog_entity_path",
            "rich activity image candidate requires source_entry",
            activity_id=activity_id,
            key=key,
        )
    entry_match = re.fullmatch(
        r"production/(?:(?:android|ios|medium)_)?upload/([0-9a-f]{2})/([0-9a-f]{38})",
        source_entry,
    )
    if entry_match is None:
        raise GachaBannerError(
            "invalid_catalog_entity_path",
            "rich activity image candidate source_entry is invalid",
            activity_id=activity_id,
            key=key,
            source_entry=source_entry,
        )
    if f"{entry_match.group(1)}{entry_match.group(2)}" != key.removesuffix(".png"):
        raise GachaBannerError(
            "catalog_entity_hash_mismatch",
            "rich activity image candidate source_entry does not match key",
            activity_id=activity_id,
            key=key,
            source_entry=source_entry,
        )
    return key, source_entry


# //// 从富活动目录恢复非卡池资源候选和时间字段 [@x380kkm 2026-08-28] ////
def _merge_rich_catalog_fields(
    catalog: dict[str, Any], rich_catalog: dict[str, Any] | None
) -> tuple[int, int]:
    if rich_catalog is None:
        return 0, 0
    activities = catalog.get("activities")
    rich_activities = rich_catalog.get("activities")
    if not isinstance(activities, list) or not isinstance(rich_activities, list):
        raise GachaBannerError(
            "invalid_catalog", "activity catalog activities must be an array"
        )
    rich_by_id = {
        activity.get("activity_id"): activity
        for activity in rich_activities
        if isinstance(activity, dict) and isinstance(activity.get("activity_id"), str)
    }
    for rich_activity in rich_activities:
        if not isinstance(rich_activity, dict) or rich_activity.get("kind") == "gacha":
            continue
        rich_candidates = rich_activity.get("image_candidates")
        if rich_candidates is None:
            continue
        if not isinstance(rich_candidates, list):
            raise GachaBannerError(
                "invalid_catalog_candidates",
                "rich activity image_candidates must be an array",
                activity_id=rich_activity.get("activity_id"),
            )
        for candidate in rich_candidates:
            _require_rich_candidate_identity(rich_activity.get("activity_id"), candidate)
    enriched_activities = 0
    enriched_candidates = 0
    for activity in activities:
        if not isinstance(activity, dict) or activity.get("kind") == "gacha":
            continue
        activity_id = activity.get("activity_id")
        rich_activity = rich_by_id.get(activity_id)
        if not isinstance(rich_activity, dict):
            continue

        current_candidates = activity.get("image_candidates")
        if not isinstance(current_candidates, list):
            current_candidates = []
        rich_candidates = rich_activity.get("image_candidates")
        if not isinstance(rich_candidates, list):
            rich_candidates = []
        merged_by_key: dict[str, dict[str, Any]] = {}
        candidate_order: list[str] = []
        for candidate in current_candidates:
            if not isinstance(candidate, dict):
                continue
            key = _manifest_candidate_key(candidate)
            if key is None:
                continue
            if key not in merged_by_key:
                candidate_order.append(key)
                merged_by_key[key] = dict(candidate)
        current_keys = set(merged_by_key)
        for candidate in rich_candidates:
            key, _ = _require_rich_candidate_identity(activity_id, candidate)
            existing = merged_by_key.get(key)
            if existing is None:
                candidate_order.append(key)
                merged_by_key[key] = dict(candidate)
                continue
            for identity_field in (
                "source_hash",
                "source_entry",
                "source_version",
                "source_byte_length",
                "source_digest",
            ):
                existing_value = existing.get(identity_field)
                candidate_value = candidate.get(identity_field)
                if (
                    existing_value is not None
                    and candidate_value is not None
                    and existing_value != candidate_value
                ):
                    raise GachaBannerError(
                        "conflicting_catalog_candidate",
                        "rich activity catalog has conflicting image metadata",
                        activity_id=activity_id,
                        key=key,
                        field=identity_field,
                    )
            merged = dict(candidate)
            merged.update(existing)
            for field_name, value in candidate.items():
                if merged.get(field_name) is None and value is not None:
                    merged[field_name] = value
            merged_by_key[key] = merged
        merged_candidates = [merged_by_key[key] for key in candidate_order]
        if merged_candidates and set(merged_by_key) != current_keys:
            activity["image_candidates"] = merged_candidates
            enriched_candidates += len(set(merged_by_key) - current_keys)
        elif merged_candidates:
            activity["image_candidates"] = merged_candidates

        fields_enriched = bool(merged_candidates) and not current_candidates
        for field_name in ("default_start_at_ms", "default_end_at_ms"):
            if activity.get(field_name) is None and rich_activity.get(field_name) is not None:
                activity[field_name] = rich_activity[field_name]
                fields_enriched = True
        if fields_enriched:
            enriched_activities += 1
    return enriched_activities, enriched_candidates


def _discover_rich_catalog_path(
    catalog_path: Path, explicit_path: Path | None
) -> Path | None:
    if explicit_path is not None:
        resolved = explicit_path.resolve(strict=True)
        if resolved == catalog_path.resolve():
            raise GachaBannerError(
                "invalid_catalog", "rich activity catalog must differ from catalog"
            )
        return resolved
    for name in RICH_CATALOG_NAMES:
        candidate = catalog_path.with_name(name)
        if candidate.resolve() == catalog_path.resolve():
            continue
        if candidate.is_file():
            return candidate.resolve()
    return None


def _to_activity_catalog_manifest(
    catalog: dict[str, Any], banner_directories: tuple[Path, ...]
) -> dict[str, Any]:
    activities = catalog.get("activities")
    if not isinstance(activities, list):
        raise GachaBannerError("invalid_catalog", "activity catalog activities must be an array")

    projected_activities: list[dict[str, Any]] = []
    for activity in activities:
        if not isinstance(activity, dict):
            raise GachaBannerError("invalid_catalog", "activity catalog entry must be an object")
        image_candidates = activity.get("image_candidates", [])
        if not isinstance(image_candidates, list) or not all(
            isinstance(candidate, dict) for candidate in image_candidates
        ):
            raise GachaBannerError(
                "invalid_catalog", "activity image candidates must be an array of objects"
            )
        projected_candidates: list[dict[str, Any]] = []
        candidate_keys: set[str] = set()
        for candidate in image_candidates:
            key = _manifest_candidate_key(candidate)
            if key is None or key in candidate_keys:
                continue
            banner_path = next(
                (
                    directory / key
                    for directory in banner_directories
                    if (directory / key).is_file()
                ),
                None,
            )
            if banner_path is None:
                continue
            dimensions = inspect_png(banner_path.read_bytes())
            if dimensions is None:
                raise GachaBannerError(
                    "invalid_activity_banner",
                    "activity banner is not a supported PNG",
                    activity_id=activity.get("activity_id"),
                    key=key,
                )
            projected_candidate = {
                field: candidate[field]
                for field in IMAGE_CANDIDATE_MANIFEST_FIELDS
                if field in candidate and candidate[field] is not None
            }
            projected_candidate["key"] = key
            projected_candidate["width"], projected_candidate["height"] = dimensions
            projected_candidates.append(projected_candidate)
            candidate_keys.add(key)

        projected_activity = {
            field: activity[field]
            for field in ACTIVITY_MANIFEST_FIELDS
            if field in activity
            and field not in {"banner_key", "banner_width", "banner_height"}
        }
        projected_activity["image_candidates"] = projected_candidates
        if projected_candidates:
            first_candidate = projected_candidates[0]
            projected_activity["banner_key"] = first_candidate["key"]
            projected_activity["banner_width"] = first_candidate["width"]
            projected_activity["banner_height"] = first_candidate["height"]
        projected_activities.append(projected_activity)

    return {
        "format_version": catalog.get("format_version", 1),
        "region": catalog.get("region", "cn"),
        "client_version": catalog.get("client_version"),
        "asset_version": catalog.get("asset_version"),
        "generated_at": catalog.get("generated_at"),
        "activities": projected_activities,
    }
# //// /将富活动目录投影为个人服务清单 ////


def _require_row(table: dict[str, Any], key: str, table_name: str) -> list[str]:
    row = table.get(key)
    if not isinstance(row, list) or not all(isinstance(value, str) for value in row):
        raise GachaBannerError(
            "missing_master_row",
            "CN master row is missing or invalid",
            table=table_name,
            key=key,
        )
    return row


def _to_int(value: object, field_name: str) -> int:
    try:
        return int(str(value))
    except (TypeError, ValueError) as error:
        raise GachaBannerError("invalid_master_value", f"{field_name} is not an integer") from error


def _to_bool(value: object, field_name: str) -> bool:
    normalized = str(value).lower()
    if normalized == "true":
        return True
    if normalized == "false":
        return False
    raise GachaBannerError("invalid_master_value", f"{field_name} is not a boolean")


def _activity_pool_id(activity: dict[str, Any]) -> int:
    activity_id = activity.get("activity_id")
    if not isinstance(activity_id, str) or not activity_id.startswith("gacha:"):
        raise GachaBannerError(
            "invalid_activity_id", "gacha activity ID is invalid", activity_id=activity_id
        )
    return _to_int(activity_id.removeprefix("gacha:"), "gacha activity ID")


def _load_banner_path_overrides(region_policy_path: Path | None) -> dict[int, str]:
    if region_policy_path is None:
        return {}
    policy = _read_json(region_policy_path)
    raw_overrides = policy.get("bannerPathOverrides")
    if not isinstance(raw_overrides, dict):
        raise GachaBannerError(
            "invalid_region_policy", "gacha region policy must contain bannerPathOverrides"
        )
    overrides: dict[int, str] = {}
    for raw_pool_id, raw_logical_path in raw_overrides.items():
        pool_id = _to_int(raw_pool_id, "banner override pool ID")
        expected_path = f"{GENERATED_BANNER_PATH_PREFIX}{pool_id}"
        if raw_logical_path != expected_path:
            raise GachaBannerError(
                "invalid_banner_override",
                "gacha banner override path is not deterministic",
                pool_id=pool_id,
                expected=expected_path,
                actual=raw_logical_path,
            )
        overrides[pool_id] = expected_path
    return overrides


def _load_master_assets(
    cdn_root: Path, entity_catalog: EntityCatalog
) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any]]:
    logical_paths = (GACHA_MASTER_PATH, CHARACTER_MASTER_PATH, EQUIPMENT_MASTER_PATH)
    records: dict[str, EntityRecord] = {}
    for logical_path in logical_paths:
        record = entity_catalog.find(logical_path)
        if record is None:
            raise GachaBannerError(
                "missing_master_asset", "CN EntityLists is missing a master asset", logical_path=logical_path
            )
        records[logical_path] = record
    assets, missing = read_logical_assets(cdn_root, records)
    if missing:
        raise GachaBannerError(
            "missing_master_archive_assets",
            "CN archives are missing current master assets",
            logical_paths=missing,
        )
    decoded = tuple(decode_ordered_map(assets[path]) for path in logical_paths)
    return decoded[0], decoded[1], decoded[2]


# //// 将富目录中的非卡池图片候选恢复到可读取目录 [@x380kkm 2026-08-28] ////
def _materialize_rich_catalog_assets(
    catalog: dict[str, Any],
    source_cdn_root: Path,
    output_cdn_root: Path,
    entity_catalog: EntityCatalog,
) -> tuple[int, int, list[dict[str, object]]]:
    activities = catalog.get("activities")
    if not isinstance(activities, list):
        raise GachaBannerError("invalid_catalog", "activity catalog activities must be an array")

    requested: dict[str, EntityRecord] = {}
    source_entries: dict[str, str] = {}
    gaps: list[dict[str, object]] = []
    for activity in activities:
        if not isinstance(activity, dict) or activity.get("kind") == "gacha":
            continue
        for candidate in activity.get("image_candidates", []):
            if not isinstance(candidate, dict):
                gaps.append(
                    {
                        "kind": "invalid_catalog_candidate",
                        "activity_id": activity.get("activity_id"),
                    }
                )
                continue
            key = _manifest_candidate_key(candidate)
            if key is None:
                gaps.append(
                    {
                        "kind": "missing_catalog_candidate_key",
                        "activity_id": activity.get("activity_id"),
                    }
                )
                continue
            source_entry = candidate.get("source_entry")
            if not isinstance(source_entry, str) or not source_entry:
                gaps.append(
                    {
                        "kind": "missing_catalog_entity_path",
                        "activity_id": activity.get("activity_id"),
                        "key": key,
                    }
                )
                continue
            entry_parts = source_entry.rsplit("/", 2)
            if len(entry_parts) != 3:
                gaps.append(
                    {
                        "kind": "invalid_catalog_entity_path",
                        "activity_id": activity.get("activity_id"),
                        "key": key,
                        "source_entry": source_entry,
                    }
                )
                continue
            entry_hash = "".join(entry_parts[-2:])
            if entry_hash != key.removesuffix(".png"):
                gaps.append(
                    {
                        "kind": "catalog_entity_hash_mismatch",
                        "activity_id": activity.get("activity_id"),
                        "key": key,
                        "source_entry": source_entry,
                    }
                )
                continue
            previous_entry = source_entries.get(key)
            if previous_entry is not None:
                if previous_entry != source_entry:
                    gaps.append(
                        {
                            "kind": "catalog_entity_conflict",
                            "activity_id": activity.get("activity_id"),
                            "key": key,
                            "source_entry": source_entry,
                            "previous_source_entry": previous_entry,
                        }
                    )
                continue
            record = entity_catalog.records.get(source_entry)
            if record is None:
                gaps.append(
                    {
                        "kind": "missing_catalog_entity",
                        "activity_id": activity.get("activity_id"),
                        "key": key,
                        "source_entry": source_entry,
                    }
                )
                continue
            source_entries[key] = source_entry
            requested[f"catalog-candidate/{key}"] = record

    if not requested:
        return 0, 0, gaps
    assets, missing = read_logical_assets(source_cdn_root, requested)
    for logical_path in missing:
        key = logical_path.removeprefix("catalog-candidate/")
        gaps.append(
            {
                "kind": "missing_catalog_archive",
                "key": key,
                "source_entry": source_entries.get(key),
            }
        )

    banner_root = output_cdn_root / "activity-banners"
    materialized = 0
    for logical_path, data in assets.items():
        key = logical_path.removeprefix("catalog-candidate/")
        normalized = standard_png(data)
        if inspect_png(normalized) is None:
            gaps.append(
                {
                    "kind": "invalid_catalog_png",
                    "key": key,
                    "source_entry": source_entries.get(key),
                }
            )
            continue
        output_path = banner_root / key
        if not output_path.is_file() or output_path.read_bytes() != normalized:
            _atomic_write(output_path, normalized)
        materialized += 1
    return len(source_entries), materialized, gaps


# //// /读取活动目录和 CN master ////


# //// 从 raw odds 选择每池图标候选 [@x380kkm 2026-08-24] ////
def _pool_metadata(pool_id: int, gacha_master: dict[str, Any]) -> tuple[list[str], str, tuple[str, ...]]:
    row = _require_row(gacha_master, str(pool_id), "gacha")
    if len(row) <= EQUIPMENT_ODDS_FIELDS[-1]:
        raise GachaBannerError(
            "invalid_gacha_row", "gacha master row is shorter than the required schema", pool_id=pool_id
        )
    prize_kind = _to_int(row[13], "gacha prize kind")
    if prize_kind == 0:
        prize_type = "character"
        odds_fields = CHARACTER_ODDS_FIELDS
    elif prize_kind == 1:
        prize_type = "equipment"
        odds_fields = EQUIPMENT_ODDS_FIELDS
    else:
        raise GachaBannerError(
            "unsupported_gacha_type", "gacha prize kind is unsupported", pool_id=pool_id, value=prize_kind
        )
    odds_ids = tuple(row[field] for field in odds_fields if row[field])
    if not odds_ids:
        raise GachaBannerError("missing_gacha_odds", "gacha master has no odds IDs", pool_id=pool_id)
    return row, prize_type, odds_ids


def _read_odds_maps(
    cdn_root: Path,
    entity_catalog: EntityCatalog,
    odds_ids: set[str],
) -> tuple[dict[str, dict[str, Any]], list[dict[str, object]]]:
    records: dict[str, EntityRecord] = {}
    gaps: list[dict[str, object]] = []
    for odds_id in sorted(odds_ids):
        logical_path = f"master/gacha_odds/{odds_id}.orderedmap"
        record = entity_catalog.find(logical_path)
        if record is None:
            gaps.append({"kind": "missing_odds_entity", "odds_id": odds_id, "logical_path": logical_path})
            continue
        records[logical_path] = record
    assets, missing_assets = read_logical_assets(cdn_root, records)
    for logical_path in missing_assets:
        gaps.append({"kind": "missing_odds_archive", "logical_path": logical_path})
    decoded: dict[str, dict[str, Any]] = {}
    for logical_path, data in assets.items():
        odds_id = Path(logical_path).stem
        decoded[odds_id] = decode_ordered_map(data)
    return decoded, gaps


def _candidate_list(
    odds_ids: tuple[str, ...], odds_maps: dict[str, dict[str, Any]]
) -> tuple[list[ItemCandidate], tuple[tuple[str, ...], ...]]:
    candidates: dict[int, ItemCandidate] = {}
    raw_rows: list[tuple[str, ...]] = []
    for odds_id in odds_ids:
        odds_map = odds_maps.get(odds_id)
        if odds_map is None:
            continue
        rows = odds_map.get(odds_id)
        if not isinstance(rows, dict):
            raise GachaBannerError(
                "invalid_odds_map",
                "gacha odds orderedmap is missing its named row group",
                odds_id=odds_id,
            )
        for row in rows.values():
            if not isinstance(row, list) or len(row) <= 4:
                raise GachaBannerError(
                    "invalid_odds_row", "gacha odds row is shorter than the required schema", odds_id=odds_id
                )
            if not all(isinstance(value, str) for value in row):
                raise GachaBannerError(
                    "invalid_odds_row", "gacha odds row contains a non-string value", odds_id=odds_id
                )
            raw_rows.append(tuple(row))
            candidate = ItemCandidate(
                item_id=_to_int(row[0], "gacha candidate ID"),
                rank=_to_int(row[1], "gacha candidate rank"),
                is_rate_up=_to_bool(row[3], "gacha rate-up flag"),
                is_limited=_to_bool(row[4], "gacha limited flag"),
            )
            current = candidates.get(candidate.item_id)
            if current is None:
                candidates[candidate.item_id] = candidate
            else:
                candidates[candidate.item_id] = ItemCandidate(
                    item_id=candidate.item_id,
                    rank=max(current.rank, candidate.rank),
                    is_rate_up=current.is_rate_up or candidate.is_rate_up,
                    is_limited=current.is_limited or candidate.is_limited,
                )
    selected = sorted(
        candidates.values(),
        key=lambda candidate: (
            candidate.is_rate_up,
            candidate.is_limited,
            candidate.rank,
            candidate.item_id,
        ),
        reverse=True,
    )
    return selected, tuple(sorted(raw_rows))


def _resolve_icon_options(
    plan: PoolPlan,
    entity_catalog: EntityCatalog,
    character_master: dict[str, Any],
    equipment_master: dict[str, Any],
) -> None:
    for candidate in plan.candidates:
        if len(plan.options) >= 8:
            break
        if plan.prize_type == "character":
            row = character_master.get(str(candidate.item_id))
            if not isinstance(row, list) or not row or not isinstance(row[0], str):
                plan.gaps.append({"kind": "missing_character_master", "item_id": candidate.item_id})
                continue
            logical_paths = tuple(
                pattern.format(string_id=row[0]) for pattern in CHARACTER_ICON_PATTERNS
            )
        else:
            row = equipment_master.get(str(candidate.item_id))
            if not isinstance(row, list) or len(row) <= 6 or not isinstance(row[6], str):
                plan.gaps.append({"kind": "missing_equipment_master", "item_id": candidate.item_id})
                continue
            plan.options.append(IconOption(candidate, row[6], None))
            continue

        selected = next(
            (
                (logical_path, record)
                for logical_path in logical_paths
                if (record := entity_catalog.find(logical_path)) is not None
            ),
            None,
        )
        if selected is None:
            plan.gaps.append(
                {
                    "kind": "missing_icon_entity",
                    "item_id": candidate.item_id,
                    "logical_paths": list(logical_paths),
                }
            )
            continue
        logical_path, record = selected
        plan.options.append(IconOption(candidate, logical_path, record))


def _read_equipment_icons(
    cdn_root: Path,
    entity_catalog: EntityCatalog,
    names: set[str],
) -> tuple[dict[str, bytes], list[dict[str, object]]]:
    if not names:
        return {}, []
    atlas_paths = (ITEM_ATLAS_LOGICAL_PATH, ITEM_SPRITE_SHEET_LOGICAL_PATH)
    records: dict[str, EntityRecord] = {}
    gaps: list[dict[str, object]] = []
    for logical_path in atlas_paths:
        record = entity_catalog.find(logical_path)
        if record is None:
            gaps.append({"kind": "missing_item_atlas_entity", "logical_path": logical_path})
        else:
            records[logical_path] = record
    if len(records) != len(atlas_paths):
        return {}, gaps
    assets, missing = read_logical_assets(cdn_root, records)
    for logical_path in missing:
        gaps.append({"kind": "missing_item_atlas_archive", "logical_path": logical_path})
    if len(assets) != len(atlas_paths):
        return {}, gaps
    icons, missing_names = extract_item_atlas_icons(
        assets[ITEM_ATLAS_LOGICAL_PATH],
        assets[ITEM_SPRITE_SHEET_LOGICAL_PATH],
        names,
    )
    gaps.extend(
        {"kind": "missing_equipment_atlas_entry", "logical_path": name}
        for name in missing_names
    )
    return icons, gaps


# //// /从 raw odds 选择每池图标候选 ////


# //// 生成共享 banner 并同步活动目录 [@x380kkm 2026-08-24] ////
def _banner_asset_logical_path(banner_logical_path: str) -> str:
    normalized = normalize_logical_path(banner_logical_path)
    return normalized if normalized.lower().endswith(".png") else f"{normalized}.png"


def _banner_asset_hash(banner_logical_path: str) -> str:
    return hash_cn_asset_path(_banner_asset_logical_path(banner_logical_path))


def _activity_has_banner(activity: dict[str, Any], expected_key: str) -> bool:
    if activity.get("banner_key") != expected_key:
        return False
    candidates = activity.get("image_candidates")
    return isinstance(candidates, list) and any(
        isinstance(candidate, dict) and candidate.get("key") == expected_key
        for candidate in candidates
    )


def _activity_requires_banner_overlay(
    activity: dict[str, Any],
    gacha_master: dict[str, Any],
    entity_catalog: EntityCatalog,
    banner_path_overrides: dict[int, str],
) -> bool:
    pool_id = _activity_pool_id(activity)
    row = _require_row(gacha_master, str(pool_id), "gacha")
    if len(row) <= 3 or not row[3]:
        raise GachaBannerError(
            "invalid_gacha_master",
            "gacha master row has no banner logical path",
            pool_id=pool_id,
        )
    logical_path = banner_path_overrides.get(pool_id, row[3])
    expected_key = f"{_banner_asset_hash(logical_path)}.png"
    if not _activity_has_banner(activity, expected_key):
        return True
    return entity_catalog.find(_banner_asset_logical_path(logical_path)) is None


def _resolved_banner_sources(
    catalog: dict[str, Any],
    gacha_master: dict[str, Any],
    source_cdn_root: Path,
) -> dict[str, dict[str, object]]:
    sources: dict[str, dict[str, object]] = {}
    activities = catalog.get("activities")
    if not isinstance(activities, list):
        raise GachaBannerError("invalid_catalog", "activity catalog activities must be an array")
    for activity in activities:
        if not isinstance(activity, dict) or activity.get("kind") != "gacha":
            continue
        tags = activity.get("tags")
        if isinstance(tags, list) and UNRESOLVED_TAG in tags:
            continue
        try:
            pool_id = _activity_pool_id(activity)
            row = _require_row(gacha_master, str(pool_id), "gacha")
        except GachaBannerError:
            continue
        logical_path = row[3] if len(row) > 3 else ""
        expected_key = f"{_banner_asset_hash(logical_path)}.png" if logical_path else ""
        if activity.get("banner_key") != expected_key:
            continue
        image_path = source_cdn_root / "activity-banners" / expected_key
        if not image_path.is_file():
            continue
        data = standard_png(image_path.read_bytes())
        if inspect_png(data) != (BANNER_WIDTH, BANNER_HEIGHT):
            continue
        current = sources.get(logical_path)
        if current is None or pool_id < int(current["pool_id"]):
            sources[logical_path] = {"pool_id": pool_id, "data": data, "key": expected_key}
    return sources


def _master_banner_sources(
    plans: list[PoolPlan],
    source_cdn_root: Path,
    entity_catalog: EntityCatalog,
) -> tuple[dict[str, dict[str, object]], list[dict[str, object]]]:
    records = {
        logical_path: record
        for logical_path in {plan.banner_logical_path for plan in plans}
        if (record := entity_catalog.find(_banner_asset_logical_path(logical_path)))
        is not None
    }
    assets, missing = read_logical_assets(source_cdn_root, records)
    sources: dict[str, dict[str, object]] = {}
    gaps: list[dict[str, object]] = [
        {"kind": "missing_banner_archive", "logical_path": logical_path}
        for logical_path in missing
    ]
    for logical_path, data in assets.items():
        standard = standard_png(data)
        if inspect_png(standard) != (BANNER_WIDTH, BANNER_HEIGHT):
            gaps.append(
                {"kind": "invalid_banner_dimensions", "logical_path": logical_path}
            )
            continue
        sources[logical_path] = {
            "pool_id": 0,
            "data": standard,
            "key": f"{_banner_asset_hash(logical_path)}.png",
        }
    return sources, gaps


def _candidate_evidence(plan: PoolPlan, representative_pool_id: int, method: str) -> str:
    item_ids = ",".join(str(option.candidate.item_id) for option in plan.selected) or "none"
    parts = [f"CN pool={plan.pool_id}", f"type={plan.prize_type}", f"selected_item_ids={item_ids}"]
    if plan.banner_logical_path != plan.original_banner_logical_path:
        parts.append(f"banner_path_override={plan.banner_logical_path}")
    if representative_pool_id != plan.pool_id:
        parts.append(f"representative_CN_pool={representative_pool_id}")
    if method == "shared_exact":
        parts.append("source=shared_exact_banner")
    return "; ".join(parts)


def _patch_activity(
    plan: PoolPlan,
    banner_key: str,
    representative_pool_id: int,
    method: str,
) -> None:
    tags = plan.activity.get("tags")
    if not isinstance(tags, list):
        tags = []
    plan.activity["tags"] = [tag for tag in tags if tag != UNRESOLVED_TAG]
    if GENERATED_TAG not in plan.activity["tags"]:
        plan.activity["tags"].append(GENERATED_TAG)
    description = plan.activity.get("description")
    if isinstance(description, str):
        description = description.replace(" 当前包内未解析到对应纹理.", "")
        if plan.banner_logical_path != plan.original_banner_logical_path:
            description = description.replace(
                f"Banner: {plan.original_banner_logical_path}",
                f"Banner: {plan.banner_logical_path}",
            )
        plan.activity["description"] = description
    candidate = {
        "key": banner_key,
        "width": BANNER_WIDTH,
        "height": BANNER_HEIGHT,
        "source_type": "generated_gacha_banner",
        "evidence": _candidate_evidence(plan, representative_pool_id, method),
    }
    existing_candidates = plan.activity.get("image_candidates")
    if not isinstance(existing_candidates, list):
        existing_candidates = []
    plan.activity["image_candidates"] = [candidate] + [
        item
        for item in existing_candidates
        if not isinstance(item, dict)
        or (
            item.get("key") != banner_key
            and item.get("source_type") != "generated_gacha_banner"
        )
    ]
    plan.activity["banner_key"] = banner_key
    plan.activity["banner_width"] = BANNER_WIDTH
    plan.activity["banner_height"] = BANNER_HEIGHT


def _write_banner_pair(
    standard: bytes,
    asset_hash: str,
    output_cdn_root: Path,
    app_asset_root: Path,
) -> tuple[Path, Path]:
    if inspect_png(standard) != (BANNER_WIDTH, BANNER_HEIGHT):
        raise GachaBannerError("invalid_banner", "banner output must be 510x180")
    banner_path = output_cdn_root / "activity-banners" / f"{asset_hash}.png"
    game_path = app_asset_root / "production" / "bundle" / asset_hash[:2] / asset_hash[2:]
    _atomic_write(banner_path, standard_png(standard))
    _atomic_write(game_path, pseudo_png(standard))
    return banner_path, game_path


def generate_banners(
    source_cdn_root: Path,
    catalog_path: Path,
    output_cdn_root: Path,
    app_asset_root: Path,
    manifest_path: Path | None,
    region_policy_path: Path | None,
    rich_catalog_path: Path | None = None,
) -> tuple[dict[str, Any], dict[str, Any]]:
    catalog = _read_json(catalog_path)
    resolved_rich_catalog_path = _discover_rich_catalog_path(
        catalog_path, rich_catalog_path
    )
    rich_catalog = (
        _read_json(resolved_rich_catalog_path)
        if resolved_rich_catalog_path is not None
        else None
    )
    enriched_activity_count, enriched_candidate_count = _merge_rich_catalog_fields(
        catalog, rich_catalog
    )
    banner_path_overrides = _load_banner_path_overrides(region_policy_path)
    entity_catalog = EntityCatalog.load(source_cdn_root, manifest_path)
    gacha_master, character_master, equipment_master = _load_master_assets(
        source_cdn_root, entity_catalog
    )
    (
        catalog_candidate_requested_count,
        catalog_candidate_materialized_count,
        catalog_candidate_gaps,
    ) = _materialize_rich_catalog_assets(
        catalog,
        source_cdn_root,
        output_cdn_root,
        entity_catalog,
    )
    activities = catalog.get("activities")
    if not isinstance(activities, list):
        raise GachaBannerError("invalid_catalog", "activity catalog activities must be an array")
    catalog_pool_ids = {
        _activity_pool_id(activity)
        for activity in activities
        if isinstance(activity, dict) and activity.get("kind") == "gacha"
    }
    missing_override_activities = sorted(
        set(banner_path_overrides) - catalog_pool_ids
    )
    if missing_override_activities:
        raise GachaBannerError(
            "missing_banner_override_activity",
            "gacha banner override has no activity catalog entry",
            pool_ids=missing_override_activities,
        )
    unresolved_activities = [
        activity
        for activity in activities
        if isinstance(activity, dict)
        and activity.get("kind") == "gacha"
        and isinstance(activity.get("tags"), list)
        and UNRESOLVED_TAG in activity["tags"]
    ]
    unresolved_pool_ids = {_activity_pool_id(activity) for activity in unresolved_activities}
    target_activities = [
        activity
        for activity in activities
        if isinstance(activity, dict)
        and activity.get("kind") == "gacha"
        and (
            (
                isinstance(activity.get("tags"), list)
                and (
                    UNRESOLVED_TAG in activity["tags"]
                    or GENERATED_TAG in activity["tags"]
                )
            )
            or _activity_pool_id(activity) in banner_path_overrides
            or _activity_requires_banner_overlay(
                activity,
                gacha_master,
                entity_catalog,
                banner_path_overrides,
            )
        )
    ]

    metadata: dict[int, tuple[list[str], str, tuple[str, ...]]] = {}
    all_odds_ids: set[str] = set()
    for activity in target_activities:
        pool_id = _activity_pool_id(activity)
        values = _pool_metadata(pool_id, gacha_master)
        metadata[pool_id] = values
        all_odds_ids.update(values[2])
    odds_maps, resource_gaps = _read_odds_maps(source_cdn_root, entity_catalog, all_odds_ids)
    resource_gaps = catalog_candidate_gaps + resource_gaps

    plans: list[PoolPlan] = []
    for activity in target_activities:
        pool_id = _activity_pool_id(activity)
        row, prize_type, odds_ids = metadata[pool_id]
        candidates, raw_rows = _candidate_list(odds_ids, odds_maps)
        original_banner_logical_path = row[3]
        plan = PoolPlan(
            activity=activity,
            pool_id=pool_id,
            prize_type=prize_type,
            original_banner_logical_path=original_banner_logical_path,
            banner_logical_path=banner_path_overrides.get(
                pool_id, original_banner_logical_path
            ),
            candidates=candidates,
            content_signature=(prize_type, raw_rows),
        )
        _resolve_icon_options(plan, entity_catalog, character_master, equipment_master)
        plans.append(plan)

    icon_records = {
        option.logical_path: option.record
        for plan in plans
        for option in plan.options
        if option.record is not None
    }
    icon_assets, missing_icon_assets = read_logical_assets(source_cdn_root, icon_records)
    resource_gaps.extend(
        {"kind": "missing_icon_archive", "logical_path": logical_path}
        for logical_path in missing_icon_assets
    )
    equipment_icons, equipment_gaps = _read_equipment_icons(
        source_cdn_root,
        entity_catalog,
        {
            option.logical_path
            for plan in plans
            if plan.prize_type == "equipment"
            for option in plan.options
        },
    )
    icon_assets.update(equipment_icons)
    resource_gaps.extend(equipment_gaps)
    for plan in plans:
        plan.selected = [
            option for option in plan.options if option.logical_path in icon_assets
        ][:3]

    exact_sources = _resolved_banner_sources(catalog, gacha_master, source_cdn_root)
    master_sources, master_source_gaps = _master_banner_sources(
        plans, source_cdn_root, entity_catalog
    )
    for logical_path, source in master_sources.items():
        exact_sources.setdefault(logical_path, source)
    resource_gaps.extend(master_source_gaps)
    groups: dict[str, list[PoolPlan]] = {}
    for plan in plans:
        groups.setdefault(plan.banner_logical_path, []).append(plan)

    conflicts = [
        {
            "logical_path": logical_path,
            "original_logical_paths": sorted(
                {plan.original_banner_logical_path for plan in group}
            ),
            "pools": [
                {
                    "pool_id": plan.pool_id,
                    "type": plan.prize_type,
                    "selected_item_ids": [
                        option.candidate.item_id for option in plan.selected
                    ],
                    "suggested_override": f"{GENERATED_BANNER_PATH_PREFIX}{plan.pool_id}",
                }
                for plan in sorted(group, key=lambda plan: plan.pool_id)
            ],
        }
        for logical_path, group in sorted(groups.items())
        if logical_path not in exact_sources
        if len({plan.content_signature for plan in group}) > 1
    ]
    if conflicts:
        raise GachaBannerError(
            "banner_content_conflict",
            "retained gacha pools with different content still share a banner path",
            conflicts=conflicts,
        )

    missing_groups: list[dict[str, object]] = []
    generated_groups: list[dict[str, object]] = []
    for logical_path in sorted(groups):
        group = sorted(groups[logical_path], key=lambda plan: plan.pool_id)
        selectable = [plan for plan in group if plan.selected]
        exact_source = exact_sources.get(logical_path)
        if exact_source is not None:
            representative = max(selectable or group, key=lambda plan: plan.pool_id)
            standard = exact_source["data"]
            method = "shared_exact"
        elif selectable:
            representative = max(selectable, key=lambda plan: plan.pool_id)
            standard = render_icon_banner(
                [icon_assets[option.logical_path] for option in representative.selected],
                representative.prize_type,
            )
            method = "generated_icons"
        else:
            missing_groups.append(
                {
                    "logical_path": logical_path,
                    "pool_ids": [plan.pool_id for plan in group],
                    "gaps": [gap for plan in group for gap in plan.gaps],
                }
            )
            continue

        asset_hash = _banner_asset_hash(logical_path)
        banner_path, game_path = _write_banner_pair(
            standard, asset_hash, output_cdn_root, app_asset_root
        )
        for plan in group:
            _patch_activity(plan, f"{asset_hash}.png", representative.pool_id, method)
        generated_groups.append(
            {
                "logical_path": logical_path,
                "original_logical_paths": sorted(
                    {plan.original_banner_logical_path for plan in group}
                ),
                "method": method,
                "activity_ids": [f"gacha:{plan.pool_id}" for plan in group],
                "representative_pool_id": representative.pool_id,
                "type": representative.prize_type,
                "selected_item_ids": [
                    option.candidate.item_id for option in representative.selected
                ],
                "management_path": str(banner_path),
                "game_path": str(game_path),
            }
        )

    written_by_logical_path = {
        str(group["logical_path"]): group for group in generated_groups
    }
    override_assets = []
    for plan in sorted(
        (plan for plan in plans if plan.pool_id in banner_path_overrides),
        key=lambda plan: plan.pool_id,
    ):
        generated = written_by_logical_path.get(plan.banner_logical_path)
        if generated is None:
            raise GachaBannerError(
                "missing_banner_override_asset",
                "gacha banner override did not produce both output resources",
                pool_id=plan.pool_id,
                logical_path=plan.banner_logical_path,
            )
        asset_hash = _banner_asset_hash(plan.banner_logical_path)
        override_assets.append(
            {
                "pool_id": plan.pool_id,
                "logical_path": plan.banner_logical_path,
                "asset_hash": asset_hash,
                "banner_key": f"{asset_hash}.png",
                "management_path": generated["management_path"],
                "game_path": generated["game_path"],
            }
        )

    output_catalog_path = output_cdn_root / "activity-catalog.json"
    manifest = _to_activity_catalog_manifest(
        catalog,
        (
            output_cdn_root / "activity-banners",
            source_cdn_root / "activity-banners",
        ),
    )
    _atomic_write_json(output_catalog_path, manifest)
    unresolved_after = [
        activity.get("activity_id")
        for activity in activities
        if isinstance(activity, dict)
        and isinstance(activity.get("tags"), list)
        and UNRESOLVED_TAG in activity["tags"]
    ]
    report = {
        "source_catalog": str(catalog_path),
        "rich_catalog": (
            str(resolved_rich_catalog_path)
            if resolved_rich_catalog_path is not None
            else None
        ),
        "output_catalog": str(output_catalog_path),
        "entity_manifest": str(entity_catalog.manifest_path),
        "region_policy": str(region_policy_path) if region_policy_path else None,
        "unresolved_before": len(unresolved_activities),
        "unresolved_after": len(unresolved_after),
        "unresolved_activity_ids": unresolved_after,
        "target_activity_count": len(target_activities),
        "enriched_activity_count": enriched_activity_count,
        "enriched_candidate_count": enriched_candidate_count,
        "catalog_candidate_requested_count": catalog_candidate_requested_count,
        "catalog_candidate_materialized_count": catalog_candidate_materialized_count,
        "override_activity_count": sum(
            plan.pool_id in banner_path_overrides for plan in plans
        ),
        "activity_types": {
            "character": sum(plan.prize_type == "character" for plan in plans),
            "equipment": sum(plan.prize_type == "equipment" for plan in plans),
        },
        "unresolved_activity_types": {
            "character": sum(
                plan.prize_type == "character"
                and plan.pool_id in unresolved_pool_ids
                for plan in plans
            ),
            "equipment": sum(
                plan.prize_type == "equipment"
                and plan.pool_id in unresolved_pool_ids
                for plan in plans
            ),
        },
        "logical_path_count": len(groups),
        "written_asset_count": len(generated_groups),
        "generated_icon_asset_count": sum(
            group["method"] == "generated_icons" for group in generated_groups
        ),
        "shared_exact_asset_count": sum(
            group["method"] == "shared_exact" for group in generated_groups
        ),
        "banner_path_override_count": len(banner_path_overrides),
        "banner_path_overrides": {
            str(pool_id): logical_path
            for pool_id, logical_path in sorted(banner_path_overrides.items())
        },
        "unused_banner_path_overrides": {
            str(pool_id): logical_path
            for pool_id, logical_path in sorted(banner_path_overrides.items())
            if pool_id not in metadata
        },
        "override_asset_count": len(override_assets),
        "override_assets": override_assets,
        "conflict_count": len(conflicts),
        "content_conflict_count": len(conflicts),
        "conflicts": conflicts,
        "resource_gaps": resource_gaps,
        "pool_gaps": [
            {"pool_id": plan.pool_id, "gaps": plan.gaps}
            for plan in plans
            if plan.gaps
        ],
        "missing_groups": missing_groups,
        "groups": generated_groups,
    }
    return manifest, report


# //// /生成共享 banner 并同步活动目录 ////


# //// 执行 CN 卡池 banner 补齐命令 [@x380kkm 2026-08-24] ////
def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source-cdn-root", required=True, type=Path)
    parser.add_argument("--catalog", required=True, type=Path)
    parser.add_argument("--output-cdn-root", required=True, type=Path)
    parser.add_argument("--app-asset-root", required=True, type=Path)
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--region-policy", type=Path)
    parser.add_argument("--rich-catalog", "--catalog-source", dest="rich_catalog", type=Path)
    parser.add_argument("--report", type=Path)
    arguments = parser.parse_args()

    source_cdn_root = arguments.source_cdn_root.resolve(strict=True)
    catalog_path = arguments.catalog.resolve(strict=True)
    output_cdn_root = arguments.output_cdn_root.resolve()
    app_asset_root = arguments.app_asset_root.resolve()
    output_cdn_root.mkdir(parents=True, exist_ok=True)
    app_asset_root.mkdir(parents=True, exist_ok=True)
    _, report = generate_banners(
        source_cdn_root,
        catalog_path,
        output_cdn_root,
        app_asset_root,
        arguments.manifest,
        arguments.region_policy.resolve(strict=True) if arguments.region_policy else None,
        arguments.rich_catalog.resolve(strict=True) if arguments.rich_catalog else None,
    )
    if arguments.report is not None:
        _atomic_write_json(arguments.report.resolve(), report)
    summary = {
        key: report[key]
        for key in (
            "unresolved_before",
            "unresolved_after",
            "enriched_activity_count",
            "enriched_candidate_count",
            "catalog_candidate_requested_count",
            "catalog_candidate_materialized_count",
            "activity_types",
            "logical_path_count",
            "written_asset_count",
            "generated_icon_asset_count",
            "shared_exact_asset_count",
            "banner_path_override_count",
            "override_asset_count",
            "conflict_count",
        )
    }
    summary["resource_gap_count"] = len(report["resource_gaps"])
    summary["missing_group_count"] = len(report["missing_groups"])
    summary["output_catalog"] = report["output_catalog"]
    if arguments.report is not None:
        summary["report"] = str(arguments.report.resolve())
    print(json.dumps(summary, ensure_ascii=False, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except GachaBannerError as error:
        failure = {"code": error.code, "message": str(error), "details": error.details}
        print(json.dumps(failure, ensure_ascii=False, separators=(",", ":")), file=sys.stderr)
        raise SystemExit(1) from None


# //// /执行 CN 缺图卡池生成命令 ////
