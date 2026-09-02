# audience: internal
# # cn-gacha-banner-assets
#
# 此模块读取 CN 客户端 orderedmap 和归档资源, 并生成游戏与管理页共用的卡池图像.
# 资源身份使用客户端带盐路径和 iOS EntityLists 当前摘要.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

from __future__ import annotations

import base64
import csv
import hashlib
import io
import math
import struct
import zlib
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageOps, UnidentifiedImageError


CN_ASSET_HASH_SALT = "K6R9T9Hz22OpeIGEWB0ui6c6PYFQnJGy"
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"
PSEUDO_PNG_SIGNATURE = b"\x89png\r\n\x1a\n"
ASSET_DIRECTORIES = (
    "production/upload",
    "production/medium_upload",
    "production/android_upload",
    "production/ios_upload",
)
ARCHIVE_SUFFIXES = ("full", "diff")
MAX_ASSET_BYTES = 64 * 1024 * 1024
MAX_ORDERED_MAP_DEPTH = 64
MAX_ORDERED_MAP_ENTRIES = 500_000
MAX_INFLATED_BYTES = 128 * 1024 * 1024


class GachaBannerError(RuntimeError):
    def __init__(self, code: str, message: str, **details: object) -> None:
        super().__init__(message)
        self.code = code
        self.details = details


@dataclass(frozen=True)
class EntityRecord:
    entry_path: str
    byte_length: int
    digest: str
    asset_kind: str


@dataclass
class OrderedMapDecodeState:
    remaining_entries: int = MAX_ORDERED_MAP_ENTRIES
    remaining_inflated_bytes: int = MAX_INFLATED_BYTES


# //// 计算 CN 客户端资源身份 [@x380kkm 2026-08-24] ////
def normalize_logical_path(logical_path: str) -> str:
    normalized = logical_path.replace("\\", "/").lstrip("/")
    while "//" in normalized:
        normalized = normalized.replace("//", "/")
    if not normalized:
        raise GachaBannerError("invalid_logical_path", "CN logical path is empty")
    return normalized


def hash_cn_asset_path(logical_path: str) -> str:
    normalized = normalize_logical_path(logical_path)
    return hashlib.sha1((normalized + CN_ASSET_HASH_SALT).encode("utf-8")).hexdigest()


def asset_entry_paths(logical_path: str) -> tuple[str, ...]:
    digest = hash_cn_asset_path(logical_path)
    suffix = f"{digest[:2]}/{digest[2:]}"
    return tuple(f"{directory}/{suffix}" for directory in ASSET_DIRECTORIES)


def encode_entity_digest(data: bytes) -> str:
    encoded = base64.b64encode(hashlib.sha256(data).digest()).decode("ascii")
    return encoded.rstrip("=").replace("+", "_").replace("/", "-")


# //// /计算 CN 客户端资源身份 ////


# //// 解码 CN orderedmap 容器 [@x380kkm 2026-08-24] ////
def _read_u32(data: bytes, offset: int) -> int:
    if offset < 0 or offset + 4 > len(data):
        raise GachaBannerError("invalid_orderedmap", "orderedmap integer is out of bounds")
    return struct.unpack_from("<I", data, offset)[0]


def _inflate(data: bytes, state: OrderedMapDecodeState, section: str) -> bytes:
    try:
        inflated = zlib.decompress(data)
    except zlib.error as error:
        raise GachaBannerError(
            "invalid_orderedmap", f"orderedmap {section} cannot be inflated"
        ) from error
    state.remaining_inflated_bytes -= len(inflated)
    if state.remaining_inflated_bytes < 0:
        raise GachaBannerError(
            "invalid_orderedmap", "orderedmap inflated byte budget is exhausted"
        )
    return inflated


def _parse_csv_row(data: bytes) -> list[str]:
    try:
        text = data.decode("utf-8")
        rows = list(csv.reader(io.StringIO(text, newline=""), strict=True))
    except (UnicodeDecodeError, csv.Error) as error:
        raise GachaBannerError(
            "invalid_orderedmap", "orderedmap CSV row is invalid"
        ) from error
    if len(rows) != 1:
        raise GachaBannerError(
            "invalid_orderedmap", "orderedmap CSV value contains multiple rows"
        )
    return rows[0]


def _is_nested_container(data: bytes) -> bool:
    if len(data) < 6:
        return False
    index_length = _read_u32(data, 0)
    return index_length > 0 and 4 + index_length <= len(data)


def _decode_value(
    data: bytes, state: OrderedMapDecodeState, depth: int
) -> list[str] | dict[str, Any]:
    if _is_nested_container(data):
        return _decode_container(data, state, depth + 1)
    return _parse_csv_row(_inflate(data, state, "row"))


def _decode_container(
    data: bytes, state: OrderedMapDecodeState, depth: int
) -> dict[str, Any]:
    if depth > MAX_ORDERED_MAP_DEPTH:
        raise GachaBannerError(
            "invalid_orderedmap", "orderedmap nesting depth exceeds the configured limit"
        )
    index_length = _read_u32(data, 0)
    index_end = 4 + index_length
    if index_length == 0 or index_end > len(data):
        raise GachaBannerError("invalid_orderedmap", "orderedmap index is out of bounds")

    index = _inflate(data[4:index_end], state, "index")
    count = _read_u32(index, 0)
    state.remaining_entries -= count
    if state.remaining_entries < 0:
        raise GachaBannerError(
            "invalid_orderedmap", "orderedmap entry count exceeds the configured limit"
        )
    index_table_end = 4 + count * 8
    if index_table_end > len(index):
        raise GachaBannerError("invalid_orderedmap", "orderedmap index table is truncated")

    key_bytes = index[index_table_end:]
    key_offset = 0
    data_offset = 0
    entries: dict[str, Any] = {}
    for entry_index in range(count):
        table_offset = 4 + entry_index * 8
        key_end = _read_u32(index, table_offset)
        data_end = _read_u32(index, table_offset + 4)
        if key_end < key_offset or key_end > len(key_bytes):
            raise GachaBannerError(
                "invalid_orderedmap", "orderedmap key table is invalid"
            )
        data_start = index_end + data_offset
        data_stop = index_end + data_end
        if data_end < data_offset or data_stop > len(data):
            raise GachaBannerError(
                "invalid_orderedmap", "orderedmap data table is invalid"
            )
        try:
            key = key_bytes[key_offset:key_end].decode("utf-8")
        except UnicodeDecodeError as error:
            raise GachaBannerError(
                "invalid_orderedmap", "orderedmap key is not UTF-8"
            ) from error
        if key in entries:
            raise GachaBannerError(
                "invalid_orderedmap", f"orderedmap contains a duplicate key: {key}"
            )
        entries[key] = _decode_value(data[data_start:data_stop], state, depth)
        key_offset = key_end
        data_offset = data_end

    if key_offset != len(key_bytes) or index_end + data_offset != len(data):
        raise GachaBannerError(
            "invalid_orderedmap", "orderedmap container has trailing bytes"
        )
    return entries


def decode_ordered_map(data: bytes) -> dict[str, Any]:
    if not data or len(data) > MAX_ASSET_BYTES:
        raise GachaBannerError(
            "invalid_orderedmap", "orderedmap input size is outside the supported range"
        )
    return _decode_container(data, OrderedMapDecodeState(), 1)


# //// /解码 CN orderedmap 容器 ////


# //// 定位 EntityLists 当前资源和归档字节 [@x380kkm 2026-08-24] ////
class EntityCatalog:
    def __init__(self, manifest_path: Path, records: dict[str, EntityRecord]) -> None:
        self.manifest_path = manifest_path
        self.records = records

    @classmethod
    def load(cls, cdn_root: Path, manifest_path: Path | None = None) -> EntityCatalog:
        if manifest_path is None:
            candidates = sorted((cdn_root / "entities").glob("*-ios_medium.csv"))
            if len(candidates) != 1:
                raise GachaBannerError(
                    "ambiguous_entity_manifest",
                    "CN CDN must contain exactly one iOS EntityLists manifest",
                    count=len(candidates),
                )
            manifest_path = candidates[0]
        manifest_path = manifest_path.resolve(strict=True)
        records: dict[str, EntityRecord] = {}
        try:
            with manifest_path.open("r", encoding="utf-8-sig", newline="") as stream:
                for fields in csv.reader(stream, strict=True):
                    if not fields:
                        continue
                    if len(fields) != 5:
                        raise GachaBannerError(
                            "invalid_entity_manifest",
                            "EntityLists row field count is invalid",
                            path=str(manifest_path),
                        )
                    entry_path, _, byte_length, digest, asset_kind = fields
                    record = EntityRecord(
                        entry_path=entry_path,
                        byte_length=int(byte_length),
                        digest=digest,
                        asset_kind=asset_kind,
                    )
                    if entry_path in records and records[entry_path] != record:
                        raise GachaBannerError(
                            "duplicate_entity_record",
                            "EntityLists contains conflicting resource records",
                            entry_path=entry_path,
                        )
                    records[entry_path] = record
        except (OSError, UnicodeError, csv.Error, ValueError) as error:
            raise GachaBannerError(
                "invalid_entity_manifest", "EntityLists cannot be read"
            ) from error
        return cls(manifest_path, records)

    def find(self, logical_path: str) -> EntityRecord | None:
        matches = [
            self.records[entry_path]
            for entry_path in asset_entry_paths(logical_path)
            if entry_path in self.records
        ]
        if len(matches) > 1:
            raise GachaBannerError(
                "ambiguous_logical_asset",
                "CN logical path maps to multiple current resources",
                logical_path=logical_path,
                entries=[record.entry_path for record in matches],
            )
        return matches[0] if matches else None


def read_logical_assets(
    cdn_root: Path, assets: dict[str, EntityRecord]
) -> tuple[dict[str, bytes], list[str]]:
    logical_by_entry: dict[str, str] = {}
    for logical_path, record in assets.items():
        existing = logical_by_entry.get(record.entry_path)
        if existing is not None and existing != logical_path:
            raise GachaBannerError(
                "logical_asset_collision",
                "CN logical paths share one archive entry",
                first=existing,
                second=logical_path,
            )
        logical_by_entry[record.entry_path] = logical_path

    pending = set(logical_by_entry)
    resolved: dict[str, bytes] = {}
    asset_kinds = sorted({record.asset_kind for record in assets.values()})
    for asset_kind in asset_kinds:
        for suffix in ARCHIVE_SUFFIXES:
            archive_root = cdn_root / f"archive-{asset_kind}-{suffix}"
            if not archive_root.is_dir():
                continue
            for archive_path in sorted(archive_root.glob("*.zip")):
                if not pending:
                    break
                try:
                    with zipfile.ZipFile(archive_path, "r") as archive:
                        for info in archive.infolist():
                            if info.filename not in pending:
                                continue
                            logical_path = logical_by_entry[info.filename]
                            record = assets[logical_path]
                            if info.file_size != record.byte_length:
                                continue
                            if info.file_size > MAX_ASSET_BYTES:
                                raise GachaBannerError(
                                    "asset_too_large",
                                    "CN archive resource exceeds the configured limit",
                                    logical_path=logical_path,
                                )
                            data = archive.read(info)
                            if encode_entity_digest(data) != record.digest:
                                continue
                            resolved[logical_path] = data
                            pending.remove(info.filename)
                except (OSError, RuntimeError, zipfile.BadZipFile) as error:
                    raise GachaBannerError(
                        "invalid_archive",
                        "CN archive cannot be read",
                        archive=str(archive_path),
                    ) from error
            if not pending:
                break
        if not pending:
            break
    missing = sorted(logical_by_entry[entry_path] for entry_path in pending)
    return resolved, missing


# //// /定位 EntityLists 当前资源和归档字节 ////


# //// 合成无文字的 510x180 图标 banner [@x380kkm 2026-08-24] ////
def standard_png(data: bytes) -> bytes:
    if data.startswith(PNG_SIGNATURE):
        return data
    if data.startswith(PSEUDO_PNG_SIGNATURE):
        return PNG_SIGNATURE + data[len(PNG_SIGNATURE) :]
    raise GachaBannerError("invalid_png", "resource is not PNG or pseudo PNG")


def pseudo_png(data: bytes) -> bytes:
    normalized = standard_png(data)
    return PSEUDO_PNG_SIGNATURE + normalized[len(PNG_SIGNATURE) :]


def inspect_png(data: bytes) -> tuple[int, int]:
    normalized = standard_png(data)
    try:
        with Image.open(io.BytesIO(normalized)) as image:
            image.load()
            if image.format != "PNG" or getattr(image, "n_frames", 1) != 1:
                raise GachaBannerError("invalid_png", "resource must be one PNG frame")
            return image.size
    except GachaBannerError:
        raise
    except (OSError, SyntaxError, UnidentifiedImageError) as error:
        raise GachaBannerError("invalid_png", "PNG resource cannot be decoded") from error


def _open_icon(data: bytes) -> Image.Image:
    try:
        with Image.open(io.BytesIO(standard_png(data))) as image:
            image.load()
            return image.convert("RGBA")
    except (OSError, SyntaxError, UnidentifiedImageError) as error:
        raise GachaBannerError("invalid_icon", "gacha icon cannot be decoded") from error


def _average_color(image: Image.Image) -> tuple[int, int, int]:
    sample = image.copy()
    sample.thumbnail((32, 32), Image.Resampling.BILINEAR)
    pixels = [pixel[:3] for pixel in sample.getdata() if pixel[3] >= 24]
    if not pixels:
        return (87, 111, 151)
    return tuple(sum(pixel[channel] for pixel in pixels) // len(pixels) for channel in range(3))


def _mix(
    left: tuple[int, int, int], right: tuple[int, int, int], amount: float
) -> tuple[int, int, int]:
    return tuple(
        max(0, min(255, round(left[channel] * (1 - amount) + right[channel] * amount)))
        for channel in range(3)
    )


def _background(colors: list[tuple[int, int, int]]) -> Image.Image:
    first = _mix(colors[0], (27, 35, 61), 0.64)
    last = _mix(colors[-1], (49, 29, 67), 0.58)
    canvas = Image.new("RGB", (510, 180))
    pixels = canvas.load()
    for y in range(canvas.height):
        shade = 0.86 + 0.14 * math.sin((y / max(canvas.height - 1, 1)) * math.pi)
        for x in range(canvas.width):
            color = _mix(first, last, x / max(canvas.width - 1, 1))
            pixels[x, y] = tuple(round(channel * shade) for channel in color)

    glow = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
    draw = ImageDraw.Draw(glow)
    for index, color in enumerate(colors):
        center_x = round((index + 1) * canvas.width / (len(colors) + 1))
        draw.ellipse(
            (center_x - 95, -50, center_x + 95, 230),
            fill=(*_mix(color, (255, 255, 255), 0.18), 110),
        )
    glow = glow.filter(ImageFilter.GaussianBlur(46))
    return Image.alpha_composite(canvas.convert("RGBA"), glow)


def _render_icon_panel(icon: Image.Image, prize_type: str) -> Image.Image:
    panel = Image.new("RGBA", (120, 120), (0, 0, 0, 0))
    shadow = Image.new("RGBA", panel.size, (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle((8, 10, 116, 118), radius=20, fill=(0, 0, 0, 105))
    shadow = shadow.filter(ImageFilter.GaussianBlur(7))
    panel.alpha_composite(shadow)

    frame = Image.new("RGBA", panel.size, (0, 0, 0, 0))
    frame_draw = ImageDraw.Draw(frame)
    frame_draw.rounded_rectangle((4, 4, 115, 115), radius=19, fill=(244, 247, 255, 238))
    frame_draw.rounded_rectangle((8, 8, 111, 111), radius=16, fill=(35, 42, 62, 255))

    if prize_type == "equipment":
        rendered = Image.new("RGBA", (99, 99), (0, 0, 0, 0))
        bounds = icon.getchannel("A").getbbox()
        contained = icon.crop(bounds) if bounds is not None else icon.copy()
        scale = min(82 / max(contained.width, 1), 82 / max(contained.height, 1))
        contained = contained.resize(
            (
                max(1, round(contained.width * scale)),
                max(1, round(contained.height * scale)),
            ),
            Image.Resampling.NEAREST,
        )
        rendered.alpha_composite(
            contained, ((rendered.width - contained.width) // 2, (rendered.height - contained.height) // 2)
        )
    else:
        rendered = ImageOps.fit(
            icon,
            (99, 99),
            method=Image.Resampling.LANCZOS,
            centering=(0.5, 0.45),
        )
    mask = Image.new("L", rendered.size, 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, 98, 98), radius=13, fill=255)
    frame.paste(rendered, (10, 10), ImageChops.multiply(rendered.getchannel("A"), mask))
    panel.alpha_composite(frame)
    return panel


def render_icon_banner(icon_data: list[bytes], prize_type: str) -> bytes:
    if prize_type not in {"character", "equipment"}:
        raise GachaBannerError("invalid_prize_type", "gacha prize type is unsupported")
    if not 1 <= len(icon_data) <= 3:
        raise GachaBannerError("invalid_icon_count", "gacha banner requires one to three icons")
    icons = [_open_icon(data) for data in icon_data]
    canvas = _background([_average_color(icon) for icon in icons])
    positions = {
        1: (195,),
        2: (133, 257),
        3: (72, 195, 318),
    }[len(icons)]
    for index, (icon, x) in enumerate(zip(icons, positions, strict=True)):
        panel = _render_icon_panel(icon, prize_type)
        y = 29 + (2 if len(icons) == 3 and index != 1 else 0)
        canvas.alpha_composite(panel, (x, y))

    output = io.BytesIO()
    canvas.convert("RGB").save(output, format="PNG", compress_level=9, optimize=False)
    rendered = output.getvalue()
    if inspect_png(rendered) != (510, 180):
        raise GachaBannerError("invalid_banner", "generated gacha banner dimensions are invalid")
    return rendered


# //// /合成无文字的 510x180 图标 banner ////
