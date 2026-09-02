# audience: internal
# # restore-cn-gacha-feature-images
#
# 此脚本补齐最终 gacha feature master 引用的图片, 并以客户端伪 PNG 格式写入 medium 差分归档和 iOS EntityLists.
# 精确资源优先来自给定 CDN 或 App bundle. 源图缺失时, 脚本使用同一卡池的列表 banner 生成 1440x624 图片.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

from __future__ import annotations

import argparse
import io
import json
import os
import re
import tempfile
import zipfile
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

from PIL import Image, ImageEnhance, ImageFilter, ImageOps

from cn_gacha_banner_assets import (
    EntityCatalog,
    GachaBannerError,
    PSEUDO_PNG_SIGNATURE,
    decode_ordered_map,
    encode_entity_digest,
    hash_cn_asset_path,
    inspect_png,
    normalize_logical_path,
    pseudo_png,
    read_logical_assets,
    standard_png,
)
from cn_gacha_banner_client_archive import (
    archive_relative_path,
    build_archive_location,
    current_target_version,
    entity_manifest_paths,
    increment_version,
    read_json_object,
    standard_archive_digest,
)


FEATURE_MASTER_PATH = "master/gacha/gacha_feature_content.orderedmap"
GACHA_MASTER_PATH = "master/gacha/gacha.orderedmap"
FEATURE_ARCHIVE_DIRECTORY = "archive-medium-diff"
FEATURE_ARCHIVE_NAME_PREFIX = "starpoint-feature-images-"
LEGACY_FEATURE_ARCHIVE_NAME_PREFIX = "starpoint-gacha-feature-images-"
PATH_MANIFEST_NAME = "path"
DEFAULT_FEATURE_IMAGE_SIZE = (1440, 1789)
LANDSCAPE_FEATURE_STEMS = frozenset(
    {
        "black_element_pickup_01",
        "black_element_pickup_02",
        "blue_element_pickup_01",
        "common_character_pickup_02",
        "common_character_pickup_03",
        "common_character_pickup_04",
        "common_character_pickup_05",
        "common_character_pickup_06",
        "common_character_pickup_07",
        "green_element_pickup_01",
        "green_element_pickup_02",
        "red_element_pickup_01",
        "thunder_element_pickup_01",
        "thunder_element_pickup_02",
        "white_element_pickup_01",
        "white_element_pickup_02",
    }
)
BUNDLE_DIRECTORIES = (
    "bundle",
    "ios_bundle",
    "medium_bundle",
    "ios_medium_bundle",
    "small_bundle",
    "ios_small_bundle",
)


class FeatureImageError(RuntimeError):
    pass


@dataclass(frozen=True)
class FeatureReference:
    logical_path: str
    pool_ids: tuple[int, ...]


@dataclass(frozen=True)
class AssetPayload:
    logical_path: str
    entry_path: str
    data: bytes
    source_kind: str
    source_path: str
    banner_logical_path: str | None = None
    dimension_evidence: str | None = None

    @property
    def digest(self) -> str:
        return encode_entity_digest(self.data)


@dataclass(frozen=True)
class ExistingArchive:
    original_version: str
    target_version: str
    relative_path: str
    location: str


# //// 读取和原子写入补齐结果 [@x380kkm 2026-08-28] ////
def atomic_write(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_name = tempfile.mkstemp(
        prefix=f".{path.name}.", suffix=".tmp", dir=path.parent
    )
    temporary_path = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(data)
        os.replace(temporary_path, path)
    finally:
        temporary_path.unlink(missing_ok=True)


def image_logical_path(value: str) -> str:
    normalized = normalize_logical_path(value)
    return normalized if normalized.lower().endswith(".png") else f"{normalized}.png"


def feature_entry_path(logical_path: str) -> str:
    asset_hash = hash_cn_asset_path(logical_path)
    return f"production/medium_upload/{asset_hash[:2]}/{asset_hash[2:]}"


def bundled_asset_path(root: Path, logical_path: str) -> Path | None:
    asset_hash = hash_cn_asset_path(logical_path)
    for directory in BUNDLE_DIRECTORIES:
        candidate = root / "production" / directory / asset_hash[:2] / asset_hash[2:]
        if candidate.is_file():
            return candidate
    return None
# //// /读取和原子写入补齐结果 ////


# //// 从最终 master 提取 feature image 引用 [@x380kkm 2026-08-28] ////
def read_master(cdn_root: Path, catalog: EntityCatalog, logical_path: str) -> dict[str, Any]:
    record = catalog.find(logical_path)
    if record is None:
        raise FeatureImageError(f"EntityLists 缺少 master: {logical_path}")
    assets, missing = read_logical_assets(cdn_root, {logical_path: record})
    if missing:
        raise FeatureImageError(f"归档缺少 master: {logical_path}")
    try:
        return decode_ordered_map(assets[logical_path])
    except GachaBannerError as error:
        raise FeatureImageError(f"master 无法解码: {logical_path}") from error


def collect_feature_references(feature_master: dict[str, Any]) -> list[FeatureReference]:
    pools_by_path: dict[str, set[int]] = {}
    for raw_pool_id, table in feature_master.items():
        if not isinstance(table, dict):
            raise FeatureImageError(f"gacha feature 池结构无效: {raw_pool_id}")
        try:
            pool_id = int(raw_pool_id)
        except ValueError as error:
            raise FeatureImageError(f"gacha feature 池 ID 无效: {raw_pool_id}") from error
        for raw_index, row in table.items():
            if not isinstance(row, list) or len(row) < 2:
                raise FeatureImageError(
                    f"gacha feature 行结构无效: pool={pool_id} index={raw_index}"
                )
            if row[0] != "1":
                continue
            if not isinstance(row[1], str) or not row[1]:
                raise FeatureImageError(
                    f"gacha feature 图片路径无效: pool={pool_id} index={raw_index}"
                )
            pools_by_path.setdefault(image_logical_path(row[1]), set()).add(pool_id)
    return [
        FeatureReference(logical_path, tuple(sorted(pool_ids)))
        for logical_path, pool_ids in sorted(pools_by_path.items())
    ]


def current_feature_assets(
    cdn_root: Path,
    app_asset_root: Path,
    catalog: EntityCatalog,
    references: Iterable[FeatureReference],
) -> dict[str, bytes]:
    records = {
        reference.logical_path: record
        for reference in references
        if (record := catalog.find(reference.logical_path)) is not None
    }
    archived, _ = read_logical_assets(cdn_root, records)
    reachable = dict(archived)
    for reference in references:
        for root in (cdn_root, app_asset_root):
            bundle_path = bundled_asset_path(root, reference.logical_path)
            if bundle_path is not None:
                reachable[reference.logical_path] = bundle_path.read_bytes()
                break
    for logical_path, data in reachable.items():
        try:
            inspect_png(data)
        except GachaBannerError as error:
            raise FeatureImageError(
                f"当前 gacha feature 图片无效: {logical_path}"
            ) from error
    return reachable
# //// /从最终 master 提取 feature image 引用 ////


# //// 在关联 CDN 和 App bundle 中查找精确源图 [@x380kkm 2026-08-28] ////
def exact_asset_from_root(
    root: Path,
    logical_path: str,
    catalog: EntityCatalog | None,
) -> tuple[bytes, str] | None:
    bundle_path = bundled_asset_path(root, logical_path)
    if bundle_path is not None:
        data = bundle_path.read_bytes()
        inspect_png(data)
        return standard_png(data), str(bundle_path)

    if catalog is None:
        return None
    try:
        record = catalog.find(logical_path)
        if record is None:
            return None
        assets, missing = read_logical_assets(root, {logical_path: record})
        if missing:
            return None
        data = assets[logical_path]
        inspect_png(data)
        return standard_png(data), str(catalog.manifest_path)
    except GachaBannerError as error:
        raise FeatureImageError(f"源 CDN 无法读取: {root}") from error


def find_exact_asset(
    logical_path: str,
    source_roots: Iterable[Path],
    source_catalogs: dict[Path, EntityCatalog | None],
) -> tuple[bytes, str] | None:
    for root in source_roots:
        result = exact_asset_from_root(root, logical_path, source_catalogs[root])
        if result is not None:
            return result
    return None
# //// /在关联 CDN 和 App bundle 中查找精确源图 ////


# //// 从同池列表 banner 生成 feature image [@x380kkm 2026-08-28] ////
def pool_banner_paths(
    reference: FeatureReference, gacha_master: dict[str, Any]
) -> list[str]:
    paths: list[str] = []
    for pool_id in reference.pool_ids:
        row = gacha_master.get(str(pool_id))
        if not isinstance(row, list) or len(row) <= 3 or not isinstance(row[3], str):
            continue
        if row[3]:
            paths.append(image_logical_path(row[3]))
    paths.append(
        reference.logical_path.replace(
            "/gacha_banner/", "/gacha_list_banner/", 1
        )
    )
    return list(dict.fromkeys(paths))


def find_banner_asset(
    cdn_root: Path,
    app_asset_root: Path,
    catalog: EntityCatalog,
    logical_paths: Iterable[str],
) -> tuple[bytes, str, str] | None:
    for logical_path in logical_paths:
        for root in (cdn_root, app_asset_root):
            bundle_path = bundled_asset_path(root, logical_path)
            if bundle_path is not None:
                data = bundle_path.read_bytes()
                inspect_png(data)
                return data, logical_path, str(bundle_path)
        record = catalog.find(logical_path)
        if record is None:
            continue
        assets, missing = read_logical_assets(cdn_root, {logical_path: record})
        if not missing:
            data = assets[logical_path]
            inspect_png(data)
            return data, logical_path, str(catalog.manifest_path)
    return None


def feature_family(logical_path: str) -> tuple[str, int | None]:
    stem = Path(logical_path).stem
    match = re.fullmatch(r"(?P<family>.+)_(?P<suffix>\d+)", stem)
    if match is None:
        return stem, None
    return match.group("family"), int(match.group("suffix"))


def infer_feature_image_size(
    logical_path: str, existing_assets: dict[str, bytes]
) -> tuple[tuple[int, int], str]:
    dimensions = {
        path: inspect_png(data) for path, data in existing_assets.items()
    }
    if not dimensions:
        return DEFAULT_FEATURE_IMAGE_SIZE, "default"
    family, suffix = feature_family(logical_path)
    candidates: list[tuple[int, str, tuple[int, int]]] = []
    for path, size in dimensions.items():
        candidate_family, candidate_suffix = feature_family(path)
        if candidate_family != family:
            continue
        distance = (
            abs(candidate_suffix - suffix)
            if suffix is not None and candidate_suffix is not None
            else 0
        )
        candidates.append((distance, path, size))
    if candidates:
        _, evidence_path, size = min(candidates)
        return size, evidence_path

    stem = Path(logical_path).stem
    if stem in LANDSCAPE_FEATURE_STEMS:
        return (1440, 624), "semantic:landscape-feature-family"

    size_counts = Counter(dimensions.values())
    size = min(
        size_counts,
        key=lambda item: (-size_counts[item], item[0], item[1]),
    )
    return size, "existing_feature_mode"


def render_feature_image(banner_data: bytes, output_size: tuple[int, int]) -> bytes:
    source = Image.open(io.BytesIO(standard_png(banner_data))).convert("RGB")
    background = ImageOps.fit(
        source,
        output_size,
        method=Image.Resampling.LANCZOS,
        centering=(0.5, 0.5),
    ).filter(ImageFilter.GaussianBlur(28))
    background = ImageEnhance.Brightness(background).enhance(0.58)

    foreground_width = 1320
    foreground_height = round(foreground_width * source.height / source.width)
    foreground = source.resize(
        (foreground_width, foreground_height), Image.Resampling.LANCZOS
    )
    shadow = Image.new("RGBA", output_size, (0, 0, 0, 0))
    shadow_box = Image.new(
        "RGBA", (foreground_width + 36, foreground_height + 36), (0, 0, 0, 150)
    ).filter(ImageFilter.GaussianBlur(18))
    shadow.alpha_composite(
        shadow_box,
        (
            (output_size[0] - shadow_box.width) // 2,
            min(96, max(24, (output_size[1] - shadow_box.height) // 2)) + 10,
        ),
    )

    canvas = background.convert("RGBA")
    canvas.alpha_composite(shadow)
    canvas.alpha_composite(
        foreground.convert("RGBA"),
        (
            (output_size[0] - foreground_width) // 2,
            min(86, max(14, (output_size[1] - foreground_height) // 2)),
        ),
    )
    output = io.BytesIO()
    canvas.convert("RGB").save(
        output, format="PNG", compress_level=9, optimize=False
    )
    rendered = output.getvalue()
    if inspect_png(rendered) != output_size:
        raise FeatureImageError("生成的 gacha feature 图片尺寸无效")
    return rendered
# //// /从同池列表 banner 生成 feature image ////


# //// 生成 medium 差分并同步 path 和 EntityLists [@x380kkm 2026-08-28] ////
def zip_entry(name: str) -> zipfile.ZipInfo:
    entry = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
    entry.create_system = 3
    entry.external_attr = 0o100644 << 16
    entry.compress_type = zipfile.ZIP_DEFLATED
    return entry


def build_archive(assets: list[AssetPayload]) -> bytes:
    output = io.BytesIO()
    with zipfile.ZipFile(
        output,
        "w",
        compression=zipfile.ZIP_DEFLATED,
        compresslevel=9,
        allowZip64=False,
    ) as archive:
        for asset in sorted(assets, key=lambda item: item.entry_path):
            archive.writestr(zip_entry(asset.entry_path), asset.data)
    data = output.getvalue()
    if b"PK\x06\x06" in data or b"PK\x06\x07" in data:
        raise FeatureImageError("gacha feature 差分使用了 ZIP64")
    return data


def read_existing_archive_payloads(
    cdn_root: Path,
    archive: ExistingArchive,
    references: Iterable[FeatureReference],
) -> list[AssetPayload]:
    archive_path = cdn_root.joinpath(*archive.relative_path.split("/"))
    if not archive_path.is_file():
        raise FeatureImageError(f"现有 gacha feature 差分不存在: {archive_path}")
    try:
        with zipfile.ZipFile(archive_path) as source_archive:
            references_by_entry = {
                feature_entry_path(reference.logical_path): reference
                for reference in references
            }
            payloads: list[AssetPayload] = []
            for entry_path in source_archive.namelist():
                reference = references_by_entry.get(entry_path)
                if reference is None:
                    continue
                data = source_archive.read(entry_path)
                inspect_png(data)
                payloads.append(
                    AssetPayload(
                        reference.logical_path,
                        entry_path,
                        pseudo_png(data),
                        "existing_archive",
                        str(archive_path),
                    )
                )
            if not payloads:
                raise FeatureImageError("现有 gacha feature 差分为空")
            return payloads
    except (OSError, RuntimeError, zipfile.BadZipFile) as error:
        raise FeatureImageError(f"现有 gacha feature 差分无法读取: {archive_path}") from error


def find_existing_archive(
    cdn_root: Path, manifest: dict[str, Any]
) -> ExistingArchive | None:
    current = current_target_version(manifest)
    diffs = manifest.get("diff")
    if not isinstance(diffs, list):
        raise FeatureImageError("path diff 无效")
    matches: list[ExistingArchive] = []
    for group in diffs:
        if not isinstance(group, dict) or group.get("version") != current:
            continue
        archives = group.get("archive")
        original = group.get("original_version")
        if not isinstance(archives, list) or not isinstance(original, str):
            raise FeatureImageError("path 当前差分组无效")
        for archive in archives:
            if not isinstance(archive, dict):
                raise FeatureImageError("path 当前差分归档无效")
            relative = archive_relative_path(archive.get("location"))
            directory, name = relative.split("/", 1)
            if directory != FEATURE_ARCHIVE_DIRECTORY or not (
                name.startswith(FEATURE_ARCHIVE_NAME_PREFIX)
                or name.startswith(LEGACY_FEATURE_ARCHIVE_NAME_PREFIX)
            ):
                continue
            location = archive.get("location")
            if not isinstance(location, str):
                raise FeatureImageError("path feature 归档地址无效")
            matches.append(ExistingArchive(original, current, relative, location))
    if len(matches) > 1:
        raise FeatureImageError("path 包含重复的 gacha feature 差分")
    return matches[0] if matches else None


def choose_versions(
    manifest: dict[str, Any],
    existing: ExistingArchive | None,
    requested_target: str | None,
) -> tuple[str, str]:
    current = current_target_version(manifest)
    if existing is not None:
        if requested_target is not None and requested_target != existing.target_version:
            raise FeatureImageError("现有 feature 差分与指定目标版本不一致")
        return existing.original_version, existing.target_version
    diffs = manifest.get("diff")
    if not isinstance(diffs, list):
        raise FeatureImageError("path diff 无效")
    current_groups = [
        group
        for group in diffs
        if isinstance(group, dict) and group.get("version") == current
    ]
    origins = {
        group.get("original_version")
        for group in current_groups
        if isinstance(group.get("original_version"), str)
    }
    if len(origins) > 1:
        raise FeatureImageError("path 当前目标版本包含多个 original_version")
    if requested_target is None or requested_target == current:
        if origins:
            return next(iter(origins)), current
        if requested_target == current:
            raise FeatureImageError("指定目标版本缺少可复用的差分组")
        return current, increment_version(current)
    return current, requested_target


def update_path_manifest(
    manifest: dict[str, Any],
    original_version: str,
    target_version: str,
    location: str,
    archive_data: bytes,
) -> bytes:
    info = manifest.get("info")
    diffs = manifest.get("diff")
    if not isinstance(info, dict) or not isinstance(diffs, list):
        raise FeatureImageError("path 版本信息无效")
    info["target_asset_version"] = target_version
    info["eventual_target_asset_version"] = target_version
    metadata = {
        "location": location,
        "size": len(archive_data),
        "sha256": standard_archive_digest(archive_data),
    }
    groups = [
        group
        for group in diffs
        if isinstance(group, dict)
        and group.get("version") == target_version
        and group.get("original_version") == original_version
    ]
    if len(groups) > 1:
        raise FeatureImageError("path 包含重复的目标差分组")
    if groups:
        group = groups[0]
        archives = group.get("archive")
        if not isinstance(archives, list):
            raise FeatureImageError("path 目标差分归档无效")
        group["archive"] = [
            archive
            for archive in archives
            if not (
                isinstance(archive, dict)
                and (
                    archive_relative_path(archive.get("location")).startswith(
                        f"{FEATURE_ARCHIVE_DIRECTORY}/{FEATURE_ARCHIVE_NAME_PREFIX}"
                    )
                    or archive_relative_path(archive.get("location")).startswith(
                        f"{FEATURE_ARCHIVE_DIRECTORY}/{LEGACY_FEATURE_ARCHIVE_NAME_PREFIX}"
                    )
                )
            )
        ] + [metadata]
    else:
        diffs.append(
            {
                "version": target_version,
                "original_version": original_version,
                "archive": [metadata],
            }
        )
    return json.dumps(manifest, ensure_ascii=False, separators=(",", ":")).encode(
        "utf-8"
    )


def render_entity_manifest(
    manifest_path: Path, target_version: str, assets: list[AssetPayload]
) -> bytes:
    text = manifest_path.read_text(encoding="utf-8-sig")
    newline = "\r\n" if "\r\n" in text else "\n"
    had_final_newline = text.endswith(("\r\n", "\n"))
    replacements = {
        asset.entry_path: (
            f"{asset.entry_path},{target_version},{len(asset.data)},"
            f"{asset.digest},medium"
        )
        for asset in assets
    }
    seen = {entry_path: 0 for entry_path in replacements}
    output: list[str] = []
    for line in text.splitlines():
        entry_path = line.split(",", 1)[0]
        replacement = replacements.get(entry_path)
        if replacement is None:
            output.append(line)
            continue
        seen[entry_path] += 1
        output.append(replacement)
    duplicates = [entry_path for entry_path, count in seen.items() if count > 1]
    if duplicates:
        raise FeatureImageError(
            f"EntityLists 包含重复目标行: {manifest_path} count={len(duplicates)}"
        )
    output.extend(
        replacements[entry_path]
        for entry_path in sorted(replacements)
        if seen[entry_path] == 0
    )
    rendered = newline.join(output)
    if had_final_newline or output:
        rendered += newline
    return rendered.encode("utf-8")
# //// /生成 medium 差分并同步 path 和 EntityLists ////


# //// 补齐全部不可达 feature image 并返回审计报告 [@x380kkm 2026-08-28] ////
def restore_feature_images(
    cdn_root: Path,
    app_asset_root: Path | None = None,
    source_roots: Iterable[Path] = (),
    requested_target: str | None = None,
    report_path: Path | None = None,
) -> dict[str, Any]:
    cdn_root = cdn_root.resolve(strict=True)
    app_asset_root = (app_asset_root or cdn_root).resolve(strict=True)
    resolved_sources = [root.resolve(strict=True) for root in source_roots]
    source_catalogs: dict[Path, EntityCatalog | None] = {}
    for root in resolved_sources:
        if not (root / "entities").is_dir():
            source_catalogs[root] = None
            continue
        try:
            source_catalogs[root] = EntityCatalog.load(root)
        except GachaBannerError as error:
            raise FeatureImageError(f"源 CDN EntityLists 无法读取: {root}") from error
    target_catalog = EntityCatalog.load(cdn_root)
    feature_master = read_master(cdn_root, target_catalog, FEATURE_MASTER_PATH)
    gacha_master = read_master(cdn_root, target_catalog, GACHA_MASTER_PATH)
    references = collect_feature_references(feature_master)
    reachable = current_feature_assets(
        cdn_root, app_asset_root, target_catalog, references
    )
    client_ready = {
        logical_path: data
        for logical_path, data in reachable.items()
        if data.startswith(PSEUDO_PNG_SIGNATURE)
    }
    pending = [
        reference
        for reference in references
        if reference.logical_path not in client_ready
    ]

    if not pending:
        path_manifest_path = cdn_root / PATH_MANIFEST_NAME
        path_manifest = read_json_object(path_manifest_path)
        current_version = current_target_version(path_manifest)
        existing = find_existing_archive(cdn_root, path_manifest)
        if existing is not None and Path(existing.relative_path).name.startswith(
            LEGACY_FEATURE_ARCHIVE_NAME_PREFIX
        ):
            payloads = read_existing_archive_payloads(cdn_root, existing, references)
            archive_data = build_archive(payloads)
            archive_name = Path(existing.relative_path).name.replace(
                LEGACY_FEATURE_ARCHIVE_NAME_PREFIX,
                FEATURE_ARCHIVE_NAME_PREFIX,
                1,
            )
            archive_relative = f"{FEATURE_ARCHIVE_DIRECTORY}/{archive_name}"
            archive_location = build_archive_location(path_manifest, archive_relative)
            archive_path = cdn_root.joinpath(*archive_relative.split("/"))
            atomic_write(archive_path, archive_data)
            updated_path = update_path_manifest(
                path_manifest,
                existing.original_version,
                existing.target_version,
                archive_location,
                archive_data,
            )
            atomic_write(path_manifest_path, updated_path)
            manifests = entity_manifest_paths(cdn_root)
            for manifest_path in manifests:
                rendered = render_entity_manifest(
                    manifest_path, existing.target_version, payloads
                )
                if manifest_path.read_bytes() != rendered:
                    atomic_write(manifest_path, rendered)
            legacy_path = cdn_root.joinpath(*existing.relative_path.split("/"))
            legacy_path.unlink(missing_ok=True)
            report = {
                "cdn_root": str(cdn_root),
                "app_asset_root": str(app_asset_root),
                "source_roots": [str(root) for root in resolved_sources],
                "feature_reference_count": len(references),
                "reachable_before": len(client_ready),
                "restored_count": 0,
                "exact_count": 0,
                "generated_count": 0,
                "migrated_count": len(payloads),
                "original_version": existing.original_version,
                "target_version": existing.target_version,
                "archive": {
                    "relative_path": archive_relative,
                    "location": archive_location,
                    "size": len(archive_data),
                    "sha256": standard_archive_digest(archive_data),
                    "zip64": False,
                    "entry_count": len(payloads),
                },
                "entity_manifests": [str(path) for path in manifests],
                "reused": False,
                "assets": [],
            }
            if report_path is not None:
                atomic_write(
                    report_path,
                    (json.dumps(report, ensure_ascii=False, indent=2) + "\n").encode(
                        "utf-8"
                    ),
                )
            return report
        report = {
            "cdn_root": str(cdn_root),
            "app_asset_root": str(app_asset_root),
            "source_roots": [str(root) for root in resolved_sources],
            "feature_reference_count": len(references),
            "reachable_before": len(client_ready),
            "restored_count": 0,
            "exact_count": 0,
            "generated_count": 0,
            "migrated_count": 0,
            "original_version": current_version,
            "target_version": current_version,
            "archive": None,
            "entity_manifests": [str(path) for path in entity_manifest_paths(cdn_root)],
            "reused": True,
            "assets": [],
        }
        if report_path is not None:
            atomic_write(
                report_path,
                (json.dumps(report, ensure_ascii=False, indent=2) + "\n").encode(
                    "utf-8"
                ),
            )
        return report

    payloads: list[AssetPayload] = []
    for reference in pending:
        current = reachable.get(reference.logical_path)
        if current is not None:
            payloads.append(
                AssetPayload(
                    reference.logical_path,
                    feature_entry_path(reference.logical_path),
                    pseudo_png(current),
                    "normalized_existing",
                    str(target_catalog.manifest_path),
                )
            )
            continue
        exact = find_exact_asset(
            reference.logical_path, resolved_sources, source_catalogs
        )
        if exact is not None:
            data, source_path = exact
            payloads.append(
                AssetPayload(
                    reference.logical_path,
                    feature_entry_path(reference.logical_path),
                    pseudo_png(data),
                    "exact",
                    source_path,
                )
            )
            continue

        banner = find_banner_asset(
            cdn_root,
            app_asset_root,
            target_catalog,
            pool_banner_paths(reference, gacha_master),
        )
        if banner is None:
            raise FeatureImageError(
                f"gacha feature 图片缺少精确源图和池内 banner: {reference.logical_path}"
            )
        banner_data, banner_logical_path, source_path = banner
        output_size, dimension_evidence = infer_feature_image_size(
            reference.logical_path, reachable
        )
        payloads.append(
            AssetPayload(
                reference.logical_path,
                feature_entry_path(reference.logical_path),
                pseudo_png(render_feature_image(banner_data, output_size)),
                "generated_from_pool_banner",
                source_path,
                banner_logical_path,
                dimension_evidence,
            )
        )

    path_manifest_path = cdn_root / PATH_MANIFEST_NAME
    path_manifest = read_json_object(path_manifest_path)
    existing = find_existing_archive(cdn_root, path_manifest)
    original_version, target_version = choose_versions(
        path_manifest, existing, requested_target
    )
    archive_data = build_archive(payloads)
    if existing is None:
        archive_name = (
            f"{FEATURE_ARCHIVE_NAME_PREFIX}{original_version}-{target_version}.zip"
        )
        archive_relative = f"{FEATURE_ARCHIVE_DIRECTORY}/{archive_name}"
        archive_location = build_archive_location(path_manifest, archive_relative)
    else:
        archive_name = Path(existing.relative_path).name
        if archive_name.startswith(LEGACY_FEATURE_ARCHIVE_NAME_PREFIX):
            archive_name = archive_name.replace(
                LEGACY_FEATURE_ARCHIVE_NAME_PREFIX,
                FEATURE_ARCHIVE_NAME_PREFIX,
                1,
            )
            archive_relative = f"{FEATURE_ARCHIVE_DIRECTORY}/{archive_name}"
            archive_location = build_archive_location(path_manifest, archive_relative)
        else:
            archive_relative = existing.relative_path
            archive_location = existing.location
    archive_path = cdn_root.joinpath(*archive_relative.split("/"))
    reused = archive_path.is_file() and archive_path.read_bytes() == archive_data
    if not reused:
        atomic_write(archive_path, archive_data)

    updated_path = update_path_manifest(
        path_manifest,
        original_version,
        target_version,
        archive_location,
        archive_data,
    )
    if path_manifest_path.read_bytes() != updated_path:
        atomic_write(path_manifest_path, updated_path)

    manifests = entity_manifest_paths(cdn_root)
    for manifest_path in manifests:
        rendered = render_entity_manifest(manifest_path, target_version, payloads)
        if manifest_path.read_bytes() != rendered:
            atomic_write(manifest_path, rendered)

    with zipfile.ZipFile(io.BytesIO(archive_data)) as archive:
        names = archive.namelist()
        expected_names = [
            asset.entry_path for asset in sorted(payloads, key=lambda item: item.entry_path)
        ]
        if names != expected_names:
            raise FeatureImageError("gacha feature 差分条目不完整")
        for asset in payloads:
            archived = archive.read(asset.entry_path)
            if encode_entity_digest(archived) != asset.digest:
                raise FeatureImageError(
                    f"gacha feature 差分摘要不一致: {asset.logical_path}"
                )

    report = {
        "cdn_root": str(cdn_root),
        "app_asset_root": str(app_asset_root),
        "source_roots": [str(root) for root in resolved_sources],
        "feature_reference_count": len(references),
        "reachable_before": len(client_ready),
        "restored_count": len(payloads),
        "exact_count": sum(asset.source_kind == "exact" for asset in payloads),
        "generated_count": sum(
            asset.source_kind == "generated_from_pool_banner" for asset in payloads
        ),
        "normalized_count": sum(
            asset.source_kind == "normalized_existing" for asset in payloads
        ),
        "migrated_count": 0,
        "original_version": original_version,
        "target_version": target_version,
        "archive": {
            "relative_path": archive_relative,
            "location": archive_location,
            "size": len(archive_data),
            "sha256": standard_archive_digest(archive_data),
            "zip64": False,
            "entry_count": len(payloads),
        },
        "entity_manifests": [str(path) for path in manifests],
        "reused": reused,
        "assets": [
            {
                "logical_path": asset.logical_path,
                "entry_path": asset.entry_path,
                "byte_length": len(asset.data),
                "digest": asset.digest,
                "dimensions": list(inspect_png(asset.data)),
                "source_kind": asset.source_kind,
                "source_path": asset.source_path,
                "banner_logical_path": asset.banner_logical_path,
                "dimension_evidence": asset.dimension_evidence,
            }
            for asset in payloads
        ],
    }
    if report_path is not None:
        atomic_write(
            report_path,
            (json.dumps(report, ensure_ascii=False, indent=2) + "\n").encode("utf-8"),
        )
    return report
# //// /补齐全部不可达 feature image 并返回审计报告 ////


# //// 执行 gacha feature 图片补齐命令 [@x380kkm 2026-08-28] ////
def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cdn-root", required=True, type=Path)
    parser.add_argument("--app-asset-root", type=Path)
    parser.add_argument("--source-root", action="append", default=[], type=Path)
    parser.add_argument("--target-version")
    parser.add_argument("--report", type=Path)
    arguments = parser.parse_args()
    report = restore_feature_images(
        arguments.cdn_root,
        arguments.app_asset_root,
        arguments.source_root,
        arguments.target_version,
        arguments.report.resolve() if arguments.report else None,
    )
    print(json.dumps(report, ensure_ascii=False, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (FeatureImageError, GachaBannerError) as error:
        print(json.dumps({"error": str(error)}, ensure_ascii=False), file=os.sys.stderr)
        raise SystemExit(1) from None
# //// /执行 gacha feature 图片补齐命令 ////
