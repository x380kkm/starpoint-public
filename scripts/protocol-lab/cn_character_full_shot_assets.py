# audience: internal
# # cn-character-full-shot-assets
#
# 此模块校验角色 full-shot 映射, PNG 资源和 CN archive 对应关系.
# 此模块只接受 full_shot_1440_1920 逻辑路径.

from __future__ import annotations

import hashlib
import io
import json
import re
from dataclasses import dataclass
from pathlib import Path, PurePosixPath

from PIL import Image, ImageChops, UnidentifiedImageError

CN_ASSET_HASH_SALT = "K6R9T9Hz22OpeIGEWB0ui6c6PYFQnJGy"
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"
PSEUDO_PNG_SIGNATURE = b"\x89png\r\n\x1a\n"
OUTPUT_MANIFEST_NAME = "starpoint-character-full-shots.json"
MAX_PNG_BYTES = 64 * 1024 * 1024
MAX_IMAGE_DIMENSION = 8192
MAX_COVER_CROP_FRACTION = 0.08
FULL_SHOT_PATTERN = re.compile(
    r"character/(?P<string_id>[a-z0-9][a-z0-9_-]*)/ui/"
    r"full_shot_1440_1920_(?P<evolution>[0-9]+)\.png\Z"
)
ASSET_DIRECTORIES = (
    "production/upload",
    "production/medium_upload",
    "production/small_upload",
    "production/android_upload",
    "production/ios_upload",
)


class PortraitStagingError(RuntimeError):
    def __init__(self, code: str, message: str, **details: object) -> None:
        super().__init__(message)
        self.code = code
        self.details = details

    def as_record(self) -> dict[str, object]:
        record: dict[str, object] = {"code": self.code, "message": str(self)}
        if self.details:
            record["details"] = dict(sorted(self.details.items()))
        return record


@dataclass(frozen=True)
class PortraitMapping:
    source: str
    target: str
    source_region: str = "manual"
    client_version: str = "unspecified"
    fallback_reason: str | None = None
    replacement_status: str = "selected"
    source_url: str | None = None
    source_archive_entry: str | None = None


@dataclass(frozen=True)
class PngInfo:
    width: int
    height: int
    mode: str
    resource_format: str


# //// 校验显式的一一映射和 full-shot 路径 [@x380kkm 2026-08-21] ////
def validate_full_shot_path(value: object, field: str) -> str:
    if not isinstance(value, str) or FULL_SHOT_PATTERN.fullmatch(value) is None:
        raise PortraitStagingError(
            "invalid_full_shot_path",
            f"{field} must match character/<string_id>/ui/full_shot_1440_1920_<evolution>.png",
            field=field,
            value=value,
        )
    if str(PurePosixPath(value)) != value:
        raise PortraitStagingError(
            "invalid_full_shot_path", f"{field} is not normalized", field=field
        )
    return value


def _resolved_mapping(entry: dict[str, object], index: int) -> PortraitMapping:
    required_fields = {
        "source",
        "target",
        "source_region",
        "client_version",
        "fallback_reason",
        "replacement_status",
        "source_url",
        "source_archive_entry",
    }
    if set(entry) != required_fields:
        raise PortraitStagingError(
            "invalid_mapping_entry",
            "resolved mapping entry fields do not match schema 2",
            index=index,
        )
    source_region = entry["source_region"]
    if source_region not in {"jp", "en", "cn", "manual"}:
        raise PortraitStagingError(
            "invalid_source_region", "source_region is unsupported", index=index
        )
    replacement_status = entry["replacement_status"]
    if replacement_status not in {"selected", "fallback_only"}:
        raise PortraitStagingError(
            "invalid_replacement_status",
            "replacement_status is unsupported",
            index=index,
        )
    if source_region == "cn" and replacement_status != "fallback_only":
        raise PortraitStagingError(
            "invalid_replacement_status",
            "CN source must be marked as fallback_only",
            index=index,
        )
    if source_region in {"jp", "en"} and replacement_status != "selected":
        raise PortraitStagingError(
            "invalid_replacement_status",
            "JP and EN sources must be marked as selected",
            index=index,
        )
    client_version = entry["client_version"]
    if not isinstance(client_version, str) or not client_version:
        raise PortraitStagingError(
            "invalid_client_version",
            "client_version must be a non-empty string",
            index=index,
        )
    fallback_reason = entry["fallback_reason"]
    if fallback_reason is not None and (
        not isinstance(fallback_reason, str) or not fallback_reason
    ):
        raise PortraitStagingError(
            "invalid_fallback_reason",
            "fallback_reason must be null or a non-empty string",
            index=index,
        )
    source_url = entry["source_url"]
    source_archive_entry = entry["source_archive_entry"]
    if not isinstance(source_url, str) or not source_url:
        raise PortraitStagingError(
            "invalid_source_url", "source_url must be a non-empty string", index=index
        )
    if not isinstance(source_archive_entry, str) or not source_archive_entry:
        raise PortraitStagingError(
            "invalid_source_archive_entry",
            "source_archive_entry must be a non-empty string",
            index=index,
        )
    return PortraitMapping(
        source=validate_full_shot_path(entry["source"], f"entries[{index}].source"),
        target=validate_full_shot_path(entry["target"], f"entries[{index}].target"),
        source_region=source_region,
        client_version=client_version,
        fallback_reason=fallback_reason,
        replacement_status=replacement_status,
        source_url=source_url,
        source_archive_entry=source_archive_entry,
    )


def load_mapping_spec(spec_path: Path) -> list[PortraitMapping]:
    try:
        payload = json.loads(spec_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise PortraitStagingError(
            "invalid_mapping_spec", "mapping spec cannot be read"
        ) from error
    if not isinstance(payload, dict) or payload.get("schema_version") not in {1, 2}:
        raise PortraitStagingError(
            "invalid_mapping_spec", "mapping spec schema_version must be 1 or 2"
        )
    if set(payload) != {"schema_version", "entries"}:
        raise PortraitStagingError(
            "invalid_mapping_spec", "mapping spec contains unsupported fields"
        )
    entries = payload.get("entries")
    if not isinstance(entries, list) or not entries:
        raise PortraitStagingError(
            "invalid_mapping_spec", "mapping spec entries must be a non-empty array"
        )

    mappings: list[PortraitMapping] = []
    for index, entry in enumerate(entries):
        if not isinstance(entry, dict):
            raise PortraitStagingError(
                "invalid_mapping_entry",
                "each mapping entry must be an object",
                index=index,
            )
        if payload["schema_version"] == 2:
            mappings.append(_resolved_mapping(entry, index))
            continue
        if set(entry) != {"source", "target"}:
            raise PortraitStagingError(
                "invalid_mapping_entry",
                "schema 1 mapping entry must contain only source and target",
                index=index,
            )
        mappings.append(
            PortraitMapping(
                source=validate_full_shot_path(
                    entry["source"], f"entries[{index}].source"
                ),
                target=validate_full_shot_path(
                    entry["target"], f"entries[{index}].target"
                ),
            )
        )

    sources = [mapping.source for mapping in mappings]
    targets = [mapping.target for mapping in mappings]
    if len(set(sources)) != len(sources):
        raise PortraitStagingError(
            "duplicate_source_mapping", "source full-shot paths must be unique"
        )
    if len(set(targets)) != len(targets):
        raise PortraitStagingError(
            "duplicate_target_mapping", "target full-shot paths must be unique"
        )
    return mappings


# //// /校验显式的一一映射和 full-shot 路径 ////


# //// 读取并验证标准 PNG 或客户端伪 PNG [@x380kkm 2026-08-21] ////
def _standard_png(data: bytes) -> tuple[bytes, str]:
    if data.startswith(PNG_SIGNATURE):
        return data, "png"
    if data.startswith(PSEUDO_PNG_SIGNATURE):
        return PNG_SIGNATURE + data[len(PNG_SIGNATURE) :], "pseudo_png"
    raise PortraitStagingError(
        "invalid_png_resource", "portrait is not PNG or pseudo PNG"
    )


def inspect_png(data: bytes, logical_path: str) -> PngInfo:
    if not data or len(data) > MAX_PNG_BYTES:
        raise PortraitStagingError(
            "invalid_png_size",
            "portrait byte size is outside the supported range",
            logical_path=logical_path,
            size=len(data),
        )
    standard, resource_format = _standard_png(data)
    try:
        with Image.open(io.BytesIO(standard)) as image:
            if image.format != "PNG" or getattr(image, "n_frames", 1) != 1:
                raise PortraitStagingError(
                    "invalid_png_resource", "portrait must be a single-frame PNG"
                )
            image.verify()
        with Image.open(io.BytesIO(standard)) as image:
            image.load()
            width, height = image.size
            mode = image.mode
    except PortraitStagingError:
        raise
    except (OSError, SyntaxError, UnidentifiedImageError) as error:
        raise PortraitStagingError(
            "invalid_png_resource", "portrait PNG cannot be decoded"
        ) from error
    if (
        width < 1
        or height < 1
        or width > MAX_IMAGE_DIMENSION
        or height > MAX_IMAGE_DIMENSION
    ):
        raise PortraitStagingError(
            "invalid_png_dimensions",
            "portrait dimensions are outside the supported range",
            logical_path=logical_path,
            width=width,
            height=height,
        )
    if mode not in {"RGB", "RGBA"}:
        raise PortraitStagingError(
            "invalid_png_mode",
            "portrait must use RGB or RGBA pixels",
            logical_path=logical_path,
            mode=mode,
        )
    return PngInfo(
        width=width, height=height, mode=mode, resource_format=resource_format
    )


def _alpha_aligned_crop(
    rendered: Image.Image,
    target: Image.Image,
    target_size: tuple[int, int],
) -> Image.Image:
    maximum_x = rendered.width - target_size[0]
    maximum_y = rendered.height - target_size[1]
    center_x = maximum_x // 2
    center_y = maximum_y // 2
    if "A" not in rendered.getbands() or "A" not in target.getbands():
        return rendered.crop(
            (center_x, center_y, center_x + target_size[0], center_y + target_size[1])
        )

    source_alpha = rendered.getchannel("A")
    target_alpha = target.getchannel("A")
    best_offset = (center_x, center_y)
    best_score: tuple[int, int, int, int] | None = None
    for offset_y in range(maximum_y + 1):
        for offset_x in range(maximum_x + 1):
            candidate = source_alpha.crop(
                (
                    offset_x,
                    offset_y,
                    offset_x + target_size[0],
                    offset_y + target_size[1],
                )
            )
            histogram = ImageChops.difference(candidate, target_alpha).histogram()
            pixel_difference = sum(value * count for value, count in enumerate(histogram))
            center_distance = abs(offset_x - center_x) + abs(offset_y - center_y)
            score = (pixel_difference, center_distance, offset_y, offset_x)
            if best_score is None or score < best_score:
                best_score = score
                best_offset = (offset_x, offset_y)
    offset_x, offset_y = best_offset
    return rendered.crop(
        (offset_x, offset_y, offset_x + target_size[0], offset_y + target_size[1])
    )


def _alpha_aligned_contain(
    rendered: Image.Image,
    target: Image.Image,
    target_size: tuple[int, int],
) -> Image.Image:
    if rendered.mode == "RGBA":
        canvas = Image.new("RGBA", target_size, (0, 0, 0, 0))
    else:
        canvas = Image.new(rendered.mode, target_size)
    offset_x = (target_size[0] - rendered.width) // 2
    offset_y = (target_size[1] - rendered.height) // 2
    if "A" in rendered.getbands() and "A" in target.getbands():
        source_bounds = rendered.getchannel("A").getbbox()
        target_bounds = target.getchannel("A").getbbox()
        if source_bounds is not None and target_bounds is not None:
            source_center_x = (source_bounds[0] + source_bounds[2]) // 2
            target_center_x = (target_bounds[0] + target_bounds[2]) // 2
            offset_x = target_center_x - source_center_x
            offset_y = target_bounds[3] - source_bounds[3]
    offset_x = min(max(offset_x, 0), target_size[0] - rendered.width)
    offset_y = min(max(offset_y, 0), target_size[1] - rendered.height)
    canvas.paste(rendered, (offset_x, offset_y))
    return canvas


def _render_aspect_safe(
    source: Image.Image,
    target: Image.Image,
    target_size: tuple[int, int],
) -> Image.Image:
    cover_scale = max(target_size[0] / source.width, target_size[1] / source.height)
    cover_size = (
        max(target_size[0], round(source.width * cover_scale)),
        max(target_size[1], round(source.height * cover_scale)),
    )
    crop_fraction = max(
        (cover_size[0] - target_size[0]) / cover_size[0],
        (cover_size[1] - target_size[1]) / cover_size[1],
    )
    if crop_fraction <= MAX_COVER_CROP_FRACTION:
        rendered = source.resize(cover_size, Image.Resampling.LANCZOS)
        return _alpha_aligned_crop(rendered, target, target_size)

    contain_scale = min(target_size[0] / source.width, target_size[1] / source.height)
    contain_size = (
        min(target_size[0], round(source.width * contain_scale)),
        min(target_size[1], round(source.height * contain_scale)),
    )
    rendered = source.resize(contain_size, Image.Resampling.LANCZOS)
    return _alpha_aligned_contain(rendered, target, target_size)


def render_replacement(
    source: bytes,
    target: bytes,
    source_info: PngInfo,
    target_info: PngInfo,
) -> bytes:
    if (source_info.width, source_info.height, source_info.mode) == (
        target_info.width,
        target_info.height,
        target_info.mode,
    ):
        standard, _ = _standard_png(source)
    else:
        standard_source, _ = _standard_png(source)
        standard_target, _ = _standard_png(target)
        with (
            Image.open(io.BytesIO(standard_source)) as source_image,
            Image.open(io.BytesIO(standard_target)) as target_image,
        ):
            source_image.load()
            target_image.load()
            rendered = source_image.convert(target_info.mode)
            target_render = target_image.convert(target_info.mode)
            if rendered.size != (target_info.width, target_info.height):
                rendered = _render_aspect_safe(
                    rendered,
                    target_render,
                    (target_info.width, target_info.height),
                )
            output = io.BytesIO()
            rendered.save(output, format="PNG", compress_level=9, optimize=False)
            standard = output.getvalue()
    if target_info.resource_format == "pseudo_png":
        return PSEUDO_PNG_SIGNATURE + standard[len(PNG_SIGNATURE) :]
    return standard


# //// /读取并验证标准 PNG 或客户端伪 PNG ////


# //// 计算 full-shot 的 CN 带盐资源路径 [@x380kkm 2026-08-21] ////
def hash_cn_asset_path(logical_path: str) -> str:
    normalized = logical_path.replace("\\", "/").lstrip("/")
    normalized = re.sub(r"/{2,}", "/", normalized)
    return hashlib.sha1((normalized + CN_ASSET_HASH_SALT).encode("utf-8")).hexdigest()


def _target_asset_paths(logical_path: str) -> set[str]:
    digest = hash_cn_asset_path(logical_path)
    suffix = f"{digest[:2]}/{digest[2:]}"
    return {f"{directory}/{suffix}" for directory in ASSET_DIRECTORIES}


# //// /计算 full-shot 的 CN 带盐资源路径 ////
