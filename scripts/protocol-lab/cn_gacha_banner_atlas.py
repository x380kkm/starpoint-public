# audience: internal
# # cn-gacha-banner-atlas
#
# 此模块解码 CN item atlas 的 AMF3 子集, 并按 Equipment master 的 pixelart 名称还原图标.

from __future__ import annotations

import io
import math
import struct
import zlib
from dataclasses import dataclass
from typing import Any

from PIL import Image, UnidentifiedImageError

from cn_gacha_banner_assets import GachaBannerError, standard_png


ITEM_ATLAS_LOGICAL_PATH = "item/sprite_sheet.atlas.amf3.deflate"
ITEM_SPRITE_SHEET_LOGICAL_PATH = "item/sprite_sheet.png"
MAX_COMPRESSED_ATLAS_BYTES = 32 * 1024 * 1024
MAX_ATLAS_BYTES = 128 * 1024 * 1024
MAX_SPRITE_SHEET_BYTES = 512 * 1024 * 1024
MAX_STRING_BYTES = 1024 * 1024
MAX_DENSE_ITEMS = 200_000
MAX_REFERENCES = 400_000
MAX_TRAITS = 20_000
MAX_VALUES = 2_000_000
MAX_DEPTH = 64
MAX_IMAGE_PIXELS = 64_000_000


@dataclass(frozen=True)
class _Traits:
    members: tuple[str, ...]
    is_dynamic: bool


@dataclass(frozen=True)
class AtlasEntry:
    name: str
    x: int
    y: int
    width: int
    height: int
    frame_x: int
    frame_y: int
    frame_width: int
    frame_height: int
    is_rotated: bool


# //// 解码图集使用的有界 AMF3 子集 [@x380kkm 2026-08-24] ////
class _Amf3Decoder:
    def __init__(self, data: bytes) -> None:
        if len(data) > MAX_ATLAS_BYTES:
            raise GachaBannerError("atlas_too_large", "AMF3 atlas exceeds the configured limit")
        self.data = memoryview(data)
        self.position = 0
        self.strings: list[str] = []
        self.objects: list[Any] = []
        self.traits: list[_Traits] = []
        self.value_count = 0

    def decode(self) -> Any:
        value = self._read_value(0)
        if self.position != len(self.data):
            raise GachaBannerError("invalid_atlas", "AMF3 atlas contains trailing bytes")
        return value

    def _read_value(self, depth: int) -> Any:
        if depth > MAX_DEPTH:
            raise GachaBannerError("invalid_atlas", "AMF3 atlas nesting exceeds the configured limit")
        self.value_count += 1
        if self.value_count > MAX_VALUES:
            raise GachaBannerError("invalid_atlas", "AMF3 atlas value count exceeds the configured limit")
        marker = self._read_byte()
        if marker in {0x00, 0x01}:
            return None
        if marker == 0x02:
            return False
        if marker == 0x03:
            return True
        if marker == 0x04:
            value = self._read_u29()
            return value - 0x20000000 if value & 0x10000000 else value
        if marker == 0x05:
            return struct.unpack(">d", self._read_exact(8))[0]
        if marker == 0x06:
            return self._read_string()
        if marker == 0x09:
            return self._read_array(depth)
        if marker == 0x0A:
            return self._read_object(depth)
        raise GachaBannerError("invalid_atlas", f"unsupported AMF3 marker: 0x{marker:02x}")

    def _read_array(self, depth: int) -> Any:
        header = self._read_u29()
        if header & 1 == 0:
            return self._object_reference(header >> 1)
        item_count = header >> 1
        if item_count > MAX_DENSE_ITEMS:
            raise GachaBannerError("invalid_atlas", "AMF3 dense array exceeds the configured limit")
        values: list[Any] = []
        self._append_object(values)
        if self._read_string() != "":
            raise GachaBannerError("invalid_atlas", "AMF3 associative arrays are unsupported")
        for _ in range(item_count):
            values.append(self._read_value(depth + 1))
        return values

    def _read_object(self, depth: int) -> Any:
        header = self._read_u29()
        if header & 1 == 0:
            return self._object_reference(header >> 1)
        if header & 2 == 0:
            trait_index = header >> 2
            if trait_index >= len(self.traits):
                raise GachaBannerError("invalid_atlas", "AMF3 trait reference is out of range")
            traits = self.traits[trait_index]
        else:
            if header & 4:
                raise GachaBannerError("invalid_atlas", "AMF3 externalizable objects are unsupported")
            member_count = header >> 4
            if member_count > MAX_DENSE_ITEMS:
                raise GachaBannerError("invalid_atlas", "AMF3 member count exceeds the configured limit")
            self._read_string()
            members = tuple(self._read_string() for _ in range(member_count))
            if len(set(members)) != len(members):
                raise GachaBannerError("invalid_atlas", "AMF3 object has duplicate sealed members")
            traits = _Traits(members, bool(header & 8))
            if len(self.traits) >= MAX_TRAITS:
                raise GachaBannerError("invalid_atlas", "AMF3 trait count exceeds the configured limit")
            self.traits.append(traits)
        result: dict[str, Any] = {}
        self._append_object(result)
        for member in traits.members:
            result[member] = self._read_value(depth + 1)
        if traits.is_dynamic:
            while True:
                member = self._read_string()
                if member == "":
                    break
                if member in result:
                    raise GachaBannerError("invalid_atlas", "AMF3 object has duplicate dynamic members")
                result[member] = self._read_value(depth + 1)
        return result

    def _read_string(self) -> str:
        header = self._read_u29()
        if header & 1 == 0:
            index = header >> 1
            if index >= len(self.strings):
                raise GachaBannerError("invalid_atlas", "AMF3 string reference is out of range")
            return self.strings[index]
        byte_count = header >> 1
        if byte_count == 0:
            return ""
        if byte_count > MAX_STRING_BYTES:
            raise GachaBannerError("invalid_atlas", "AMF3 string exceeds the configured limit")
        try:
            value = self._read_exact(byte_count).decode("utf-8")
        except UnicodeDecodeError as error:
            raise GachaBannerError("invalid_atlas", "AMF3 string is not UTF-8") from error
        if len(self.strings) >= MAX_REFERENCES:
            raise GachaBannerError("invalid_atlas", "AMF3 string count exceeds the configured limit")
        self.strings.append(value)
        return value

    def _read_u29(self) -> int:
        value = 0
        for _ in range(3):
            current = self._read_byte()
            if current & 0x80 == 0:
                return (value << 7) | current
            value = (value << 7) | (current & 0x7F)
        return (value << 8) | self._read_byte()

    def _read_byte(self) -> int:
        if self.position >= len(self.data):
            raise GachaBannerError("invalid_atlas", "AMF3 atlas ended unexpectedly")
        value = self.data[self.position]
        self.position += 1
        return value

    def _read_exact(self, size: int) -> bytes:
        end = self.position + size
        if end > len(self.data):
            raise GachaBannerError("invalid_atlas", "AMF3 atlas ended unexpectedly")
        value = bytes(self.data[self.position:end])
        self.position = end
        return value

    def _append_object(self, value: Any) -> None:
        if len(self.objects) >= MAX_REFERENCES:
            raise GachaBannerError("invalid_atlas", "AMF3 object count exceeds the configured limit")
        self.objects.append(value)

    def _object_reference(self, index: int) -> Any:
        if index >= len(self.objects):
            raise GachaBannerError("invalid_atlas", "AMF3 object reference is out of range")
        return self.objects[index]


def _decode_atlas(data: bytes) -> list[dict[str, Any]]:
    if len(data) > MAX_COMPRESSED_ATLAS_BYTES:
        raise GachaBannerError("atlas_too_large", "compressed item atlas exceeds the configured limit")
    decompressor = zlib.decompressobj(-zlib.MAX_WBITS)
    try:
        inflated = decompressor.decompress(data, MAX_ATLAS_BYTES + 1)
        inflated += decompressor.flush(MAX_ATLAS_BYTES - len(inflated) + 1)
    except zlib.error as error:
        raise GachaBannerError("invalid_atlas", "item atlas raw DEFLATE is invalid") from error
    if (
        len(inflated) > MAX_ATLAS_BYTES
        or decompressor.unconsumed_tail
        or decompressor.unused_data
        or not decompressor.eof
    ):
        raise GachaBannerError("invalid_atlas", "item atlas raw DEFLATE boundary is invalid")
    decoded = _Amf3Decoder(inflated).decode()
    if not isinstance(decoded, list) or not all(isinstance(value, dict) for value in decoded):
        raise GachaBannerError("invalid_atlas", "item atlas root must be an object array")
    return decoded


# //// /解码图集使用的有界 AMF3 子集 ////


# //// 校验并还原 Equipment pixelart 子纹理 [@x380kkm 2026-08-24] ////
def _integer_field(record: dict[str, Any], field: str, default: int | None = None) -> int:
    if field not in record:
        if default is None:
            raise GachaBannerError("invalid_atlas", f"atlas entry is missing field: {field}")
        return default
    value = record[field]
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise GachaBannerError("invalid_atlas", f"atlas entry field is not an integer: {field}")
    if isinstance(value, float) and (not math.isfinite(value) or not value.is_integer()):
        raise GachaBannerError("invalid_atlas", f"atlas entry field is not an integer: {field}")
    return int(value)


def _boolean_field(record: dict[str, Any], field: str) -> bool:
    value = record.get(field, False)
    if isinstance(value, bool):
        return value
    if isinstance(value, int) and value in {0, 1}:
        return bool(value)
    raise GachaBannerError("invalid_atlas", f"atlas entry field is not a boolean: {field}")


def _atlas_index(records: list[dict[str, Any]]) -> dict[str, AtlasEntry]:
    entries: dict[str, AtlasEntry] = {}
    for record in records:
        name = record.get("n")
        if not isinstance(name, str) or not name:
            raise GachaBannerError("invalid_atlas", "atlas entry name is invalid")
        width = _integer_field(record, "w")
        height = _integer_field(record, "h")
        is_rotated = _boolean_field(record, "r")
        content_width = height if is_rotated else width
        content_height = width if is_rotated else height
        frame_width = _integer_field(record, "fw", 0)
        frame_height = _integer_field(record, "fh", 0)
        if frame_width == 0 and frame_height == 0:
            frame_width = content_width
            frame_height = content_height
        entry = AtlasEntry(
            name=name,
            x=_integer_field(record, "x"),
            y=_integer_field(record, "y"),
            width=width,
            height=height,
            frame_x=_integer_field(record, "fx", 0),
            frame_y=_integer_field(record, "fy", 0),
            frame_width=frame_width,
            frame_height=frame_height,
            is_rotated=is_rotated,
        )
        content_width = entry.height if entry.is_rotated else entry.width
        content_height = entry.width if entry.is_rotated else entry.height
        if (
            entry.x < 0
            or entry.y < 0
            or entry.width <= 0
            or entry.height <= 0
            or entry.frame_width <= 0
            or entry.frame_height <= 0
            or entry.frame_x > 0
            or entry.frame_y > 0
            or -entry.frame_x + content_width > entry.frame_width
            or -entry.frame_y + content_height > entry.frame_height
        ):
            raise GachaBannerError("invalid_atlas", "atlas entry geometry is invalid", name=name)
        if name in entries:
            raise GachaBannerError("invalid_atlas", "atlas contains a duplicate entry", name=name)
        entries[name] = entry
    return entries


def _sprite_sheet(data: bytes) -> Image.Image:
    if len(data) > MAX_SPRITE_SHEET_BYTES:
        raise GachaBannerError("sprite_sheet_too_large", "item sprite sheet exceeds the configured limit")
    try:
        with Image.open(io.BytesIO(standard_png(data))) as source:
            source.load()
            if source.width * source.height > MAX_IMAGE_PIXELS:
                raise GachaBannerError(
                    "sprite_sheet_too_large", "item sprite sheet dimensions exceed the configured limit"
                )
            return source.convert("RGBA")
    except GachaBannerError:
        raise
    except (OSError, SyntaxError, UnidentifiedImageError) as error:
        raise GachaBannerError("invalid_sprite_sheet", "item sprite sheet cannot be decoded") from error


def _encode_png(image: Image.Image) -> bytes:
    output = io.BytesIO()
    image.save(output, format="PNG", optimize=False, compress_level=9)
    return output.getvalue()


def extract_item_atlas_icons(
    atlas_data: bytes,
    sprite_sheet_data: bytes,
    names: set[str],
) -> tuple[dict[str, bytes], list[str]]:
    entries = _atlas_index(_decode_atlas(atlas_data))
    sprite_sheet = _sprite_sheet(sprite_sheet_data)
    icons: dict[str, bytes] = {}
    for name in sorted(names.intersection(entries)):
        entry = entries[name]
        right = entry.x + entry.width
        bottom = entry.y + entry.height
        if right > sprite_sheet.width or bottom > sprite_sheet.height:
            raise GachaBannerError("invalid_atlas", "atlas crop exceeds the item sprite sheet", name=name)
        region = sprite_sheet.crop((entry.x, entry.y, right, bottom))
        if entry.is_rotated:
            region = region.transpose(Image.Transpose.ROTATE_90)
        icon = Image.new("RGBA", (entry.frame_width, entry.frame_height), (0, 0, 0, 0))
        icon.alpha_composite(region, (-entry.frame_x, -entry.frame_y))
        icons[name] = _encode_png(icon)
    return icons, sorted(names - entries.keys())


# //// /校验并还原 Equipment pixelart 子纹理 ////
