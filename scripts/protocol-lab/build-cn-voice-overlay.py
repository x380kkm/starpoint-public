# audience: internal
# # build-cn-voice-overlay
#
# 此脚本合并 CN 角色资料与 JP 音频资源, 生成可审计的 iOS 语音增量归档.
# 角色台词保留 CN 文本, 缺失声优字段按 JP, EN, CN 顺序回退.
#
# /// script
# requires-python = ">=3.12,<3.13"
# ///

from __future__ import annotations

import argparse
import base64
import csv
import hashlib
import io
import json
import os
import struct
import tempfile
import zipfile
import zlib
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass, replace
from pathlib import Path
from typing import Any, Iterable

from regional_zip_range import RemoteZip64Archive, ZipRangeEntry


CN_ASSET_HASH_SALT = "K6R9T9Hz22OpeIGEWB0ui6c6PYFQnJGy"
DEFAULT_JP_ARCHIVE_URL = (
    "https://ia601507.us.archive.org/21/items/gacha-archive/Online/World%20Flipper/"
    "air.jp.co.cygames.worldflipper-1.780.0.1780000.zip"
)
MASTER_SPECS = (
    ("character", "master/character/character.orderedmap"),
    ("character_speech", "master/character/character_speech.orderedmap"),
    ("character_text", "master/character/character_text.orderedmap"),
    ("ui_string", "master/string/ui_string.orderedmap"),
    ("voice_asset", "master/asset/voice_asset.orderedmap"),
)
BATTLE_PREFIXES = (
    "battle/battle_start_",
    "battle/skill_",
    "battle/matched_skill_",
    "battle/power_flip_",
    "battle/outhole_",
    "battle/win_",
)
BATTLE_READY_PATHS = ("battle/skill_ready", "battle/matched_skill_ready")
REMOTE_VARIANT_ORDER = (
    "upload", "ios_upload", "android_upload", "medium_upload", "small_upload",
)
MAX_BATTLE_INDEX = 512


class VoiceOverlayError(RuntimeError):
    pass


@dataclass(frozen=True)
class VoiceAsset:
    logical_path: str
    data: bytes
    source: str

    @property
    def asset_hash(self) -> str:
        return hashlib.sha1(
            (self.logical_path + CN_ASSET_HASH_SALT).encode("utf-8")
        ).hexdigest()

    @property
    def entry_path(self) -> str:
        digest = self.asset_hash
        return f"production/upload/{digest[:2]}/{digest[2:]}"

    @property
    def entity_digest(self) -> str:
        encoded = base64.b64encode(hashlib.sha256(self.data).digest()).decode("ascii")
        return encoded.rstrip("=").replace("+", "_").replace("/", "-")


@dataclass(frozen=True)
class RoleAudit:
    character_id: str
    string_id: str
    cn_voice_actor: str
    jp_voice_actor: str
    en_voice_actor: str
    selected_voice_actor: str
    selected_source: str
    speech_mp3_count: int
    battle_mp3_count: int
    speech_paths: tuple[str, ...]
    battle_paths: tuple[str, ...]
    missing_paths: tuple[str, ...]


# //// 读取 JSON 和原子写入结果文件 [@x380kkm 2026-08-29] ////
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
# //// /读取 JSON 和原子写入结果文件 ////


# //// 解码与编码多行 CSV orderedmap [@x380kkm 2026-08-29] ////
def _read_u32(data: bytes, offset: int) -> int:
    if offset < 0 or offset + 4 > len(data):
        raise VoiceOverlayError("orderedmap 整数超出范围")
    return struct.unpack_from("<I", data, offset)[0]


def _is_nested_container(data: bytes) -> bool:
    if len(data) < 6:
        return False
    index_length = _read_u32(data, 0)
    return index_length > 0 and 4 + index_length <= len(data)


def _decode_container(data: bytes, depth: int = 1) -> dict[str, Any]:
    if depth > 64:
        raise VoiceOverlayError("orderedmap 嵌套深度超出限制")
    index_length = _read_u32(data, 0)
    index_end = 4 + index_length
    if index_length == 0 or index_end > len(data):
        raise VoiceOverlayError("orderedmap 索引无效")
    try:
        index = zlib.decompress(data[4:index_end])
    except zlib.error as error:
        raise VoiceOverlayError("orderedmap 索引无法解压") from error
    count = _read_u32(index, 0)
    table_end = 4 + count * 8
    if table_end > len(index):
        raise VoiceOverlayError("orderedmap 索引表被截断")
    key_bytes = index[table_end:]
    key_offset = 0
    data_offset = 0
    result: dict[str, Any] = {}
    for entry_index in range(count):
        table_offset = 4 + entry_index * 8
        key_end = _read_u32(index, table_offset)
        data_end = _read_u32(index, table_offset + 4)
        if key_end < key_offset or key_end > len(key_bytes):
            raise VoiceOverlayError("orderedmap 键表无效")
        data_start = index_end + data_offset
        data_stop = index_end + data_end
        if data_end < data_offset or data_stop > len(data):
            raise VoiceOverlayError("orderedmap 数据表无效")
        key = key_bytes[key_offset:key_end].decode("utf-8")
        if key in result:
            raise VoiceOverlayError(f"orderedmap 存在重复键: {key}")
        value_bytes = data[data_start:data_stop]
        if _is_nested_container(value_bytes):
            value: Any = _decode_container(value_bytes, depth + 1)
        else:
            try:
                raw = zlib.decompress(value_bytes)
                value = list(csv.reader(io.StringIO(raw.decode("utf-8"), newline="")))
            except (UnicodeError, csv.Error, zlib.error) as error:
                raise VoiceOverlayError(f"orderedmap CSV 无法解码: {key}") from error
        result[key] = value
        key_offset = key_end
        data_offset = data_end
    if key_offset != len(key_bytes) or index_end + data_offset != len(data):
        raise VoiceOverlayError("orderedmap 包含尾随字节")
    return result


def decode_ordered_map(data: bytes) -> dict[str, Any]:
    if not data:
        raise VoiceOverlayError("orderedmap 内容为空")
    return _decode_container(data)


def _encode_csv_rows(rows: list[list[str]]) -> bytes:
    stream = io.StringIO(newline="")
    writer = csv.writer(stream, lineterminator="\n")
    writer.writerows(rows)
    return stream.getvalue().removesuffix("\n").encode("utf-8")


def _encode_container(values: dict[str, Any]) -> bytes:
    keys: list[bytes] = []
    chunks: list[bytes] = []
    offsets: list[tuple[int, int]] = []
    key_length = 0
    data_length = 0
    for key, value in values.items():
        key_bytes = key.encode("utf-8")
        if isinstance(value, dict):
            chunk = _encode_container(value)
        elif isinstance(value, list) and all(isinstance(row, list) for row in value):
            chunk = zlib.compress(_encode_csv_rows(value))
        else:
            raise VoiceOverlayError(f"orderedmap 值类型不支持: {key}")
        keys.append(key_bytes)
        chunks.append(chunk)
        key_length += len(key_bytes)
        data_length += len(chunk)
        offsets.append((key_length, data_length))
    index = bytearray(4 + len(offsets) * 8)
    struct.pack_into("<I", index, 0, len(offsets))
    for index_offset, (key_end, data_end) in enumerate(offsets):
        struct.pack_into("<II", index, 4 + index_offset * 8, key_end, data_end)
    compressed_index = zlib.compress(bytes(index) + b"".join(keys))
    return struct.pack("<I", len(compressed_index)) + compressed_index + b"".join(chunks)


def encode_ordered_map(value: dict[str, Any]) -> bytes:
    return _encode_container(value)
# //// /解码与编码多行 CSV orderedmap ////


# //// 定位 master 文件和角色行 [@x380kkm 2026-08-29] ////
def find_master_file(root: Path, logical_path: str) -> Path:
    filename = Path(logical_path).name
    direct_candidates = (root / Path(logical_path), root / filename)
    for candidate in direct_candidates:
        if candidate.is_file():
            return candidate.resolve()
    existing = sorted({path.resolve() for path in root.rglob(filename) if path.is_file()})
    if not existing:
        raise VoiceOverlayError(f"master 文件缺失: {logical_path} root={root}")
    if len(existing) > 1:
        raise VoiceOverlayError(f"master 文件存在多个候选: {logical_path} root={root}")
    return existing[0]


def load_master_set(root: Path) -> dict[str, bytes]:
    return {
        logical_path: find_master_file(root, logical_path).read_bytes()
        for _, logical_path in MASTER_SPECS
    }


def first_row(mapping: dict[str, Any], key: str) -> list[str]:
    value = mapping.get(key)
    if not isinstance(value, list) or not value or not isinstance(value[0], list):
        raise VoiceOverlayError(f"orderedmap 缺少行: {key}")
    return value[0]


def get_optional_first_row(mapping: dict[str, Any] | None, key: str) -> list[str]:
    if mapping is None:
        return []
    value = mapping.get(key)
    if not isinstance(value, list) or not value or not isinstance(value[0], list):
        return []
    return value[0]


def valid_voice_actor(value: str) -> bool:
    return bool(value and value not in {"--", "(None)"})


def read_character_records(
    character_master: dict[str, Any],
    character_text: dict[str, Any],
    character_speech: dict[str, Any],
    role_ids: Iterable[str],
) -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    for character_id in role_ids:
        character_row = first_row(character_master, character_id)
        text_row = first_row(character_text, character_id)
        speeches = character_speech.get(character_id)
        if not isinstance(speeches, list):
            raise VoiceOverlayError(f"角色语音表缺少角色: {character_id}")
        string_id = character_row[0]
        speech_paths = tuple(
            row[-1]
            for row in speeches
            if isinstance(row, list)
            and row
            and isinstance(row[-1], str)
            and row[-1]
            and not row[-1].startswith("(")
        )
        records.append(
            {
                "character_id": character_id,
                "string_id": string_id,
                "cn_voice_actor": text_row[-1] if text_row else "",
                "speech_paths": speech_paths,
            }
        )
    return records


# //// 从区域 master 自动选择缺失语音的角色 [@x380kkm 2026-08-29] ////
def discover_missing_voice_role_ids(
    cn_character: dict[str, Any],
    cn_text: dict[str, Any],
    cn_speech: dict[str, Any],
    jp_text: dict[str, Any],
    en_text: dict[str, Any] | None,
    remote_index: dict[str, ZipRangeEntry],
    fallback_roots: tuple[tuple[str, Path | None], ...],
) -> tuple[str, ...]:
    role_ids: list[str] = []
    for character_id in sorted(cn_text):
        cn_row = get_optional_first_row(cn_text, character_id)
        cn_actor = cn_row[-1] if cn_row else ""
        if valid_voice_actor(cn_actor):
            continue
        jp_row = get_optional_first_row(jp_text, character_id)
        en_row = get_optional_first_row(en_text, character_id)
        if not (
            valid_voice_actor(jp_row[-1] if jp_row else "")
            or valid_voice_actor(en_row[-1] if en_row else "")
        ):
            continue
        character_row = get_optional_first_row(cn_character, character_id)
        if not character_row or not character_row[0]:
            continue
        string_id = character_row[0]
        speech_paths = tuple(
            f"character/{string_id}/voice/{row[-1]}.mp3"
            for row in cn_speech.get(character_id, [])
            if isinstance(row, list)
            and row
            and isinstance(row[-1], str)
            and row[-1]
            and not row[-1].startswith("(")
        )
        battle_paths = enumerate_battle_paths(string_id, remote_index, fallback_roots)
        if any(
            path_exists(path, remote_index, fallback_roots)
            for path in [*speech_paths, *battle_paths]
        ):
            role_ids.append(character_id)
    return tuple(role_ids)
# //// /从区域 master 自动选择缺失语音的角色 ////


def merge_voice_actor(
    records: list[dict[str, Any]],
    jp_text: dict[str, Any],
    en_text: dict[str, Any] | None,
    cn_text: dict[str, Any],
) -> tuple[list[RoleAudit], int]:
    audits: list[RoleAudit] = []
    changed = 0
    for record in records:
        character_id = record["character_id"]
        cn_row = cn_text[character_id][0]
        jp_row = first_row(jp_text, character_id)
        en_row = get_optional_first_row(en_text, character_id)
        cn_actor = cn_row[-1] if cn_row else ""
        jp_actor = jp_row[-1] if jp_row else ""
        en_actor = en_row[-1] if en_row else ""
        if valid_voice_actor(jp_actor):
            selected_actor, selected_source = jp_actor, "jp"
        elif valid_voice_actor(en_actor):
            selected_actor, selected_source = en_actor, "en"
        elif valid_voice_actor(cn_actor):
            selected_actor, selected_source = cn_actor, "cn"
        else:
            selected_actor, selected_source = cn_actor, "cn"
        if selected_actor != cn_actor:
            cn_row[-1] = selected_actor
            changed += 1
        record.update(
            {
                "jp_voice_actor": jp_actor,
                "en_voice_actor": en_actor,
                "selected_voice_actor": selected_actor,
                "selected_source": selected_source,
            }
        )
        audits.append(
            RoleAudit(
                character_id=character_id,
                string_id=record["string_id"],
                cn_voice_actor=cn_actor,
                jp_voice_actor=jp_actor,
                en_voice_actor=en_actor,
                selected_voice_actor=selected_actor,
                selected_source=selected_source,
                speech_mp3_count=0,
                battle_mp3_count=0,
                speech_paths=tuple(record["speech_paths"]),
                battle_paths=(),
                missing_paths=(),
            )
        )
    return audits, changed
# //// /定位 master 文件和角色行 ////


# //// 建立远程索引并读取本地回退音频 [@x380kkm 2026-08-29] ////
def remote_asset_index(archive: RemoteZip64Archive) -> dict[str, ZipRangeEntry]:
    priorities = {variant: index for index, variant in enumerate(REMOTE_VARIANT_ORDER)}
    selected: dict[str, tuple[int, ZipRangeEntry]] = {}
    for entry in archive.entries:
        marker_offset = entry.name.rfind("/production/")
        if marker_offset < 0:
            continue
        parts = entry.name[marker_offset + len("/production/") :].split("/")
        if len(parts) != 3 or len(parts[1]) != 2 or len(parts[2]) != 38:
            continue
        priority = priorities.get(parts[0])
        if priority is None:
            continue
        asset_hash = parts[1] + parts[2]
        previous = selected.get(asset_hash)
        if previous is None or priority < previous[0]:
            selected[asset_hash] = (priority, entry)
    return {asset_hash: entry for asset_hash, (_, entry) in selected.items()}


def local_hashed_asset(root: Path | None, asset_hash: str) -> bytes | None:
    if root is None:
        return None
    for variant in REMOTE_VARIANT_ORDER:
        path = root / "production" / variant / asset_hash[:2] / asset_hash[2:]
        if path.is_file():
            return path.read_bytes()
    return None


def read_voice_asset(
    logical_path: str,
    remote: RemoteZip64Archive,
    remote_index: dict[str, ZipRangeEntry],
    fallback_roots: tuple[tuple[str, Path | None], ...],
) -> tuple[bytes | None, str]:
    asset_hash = hashlib.sha1(
        (logical_path + CN_ASSET_HASH_SALT).encode("utf-8")
    ).hexdigest()
    remote_entry = remote_index.get(asset_hash)
    if remote_entry is not None:
        return remote.read(remote_entry), "jp"
    for source, root in fallback_roots:
        data = local_hashed_asset(root, asset_hash)
        if data is not None:
            return data, source
    return None, "missing"


def path_exists(
    logical_path: str,
    remote_index: dict[str, ZipRangeEntry],
    fallback_roots: tuple[tuple[str, Path | None], ...],
) -> bool:
    asset_hash = hashlib.sha1(
        (logical_path + CN_ASSET_HASH_SALT).encode("utf-8")
    ).hexdigest()
    if asset_hash in remote_index:
        return True
    return any(
        local_hashed_asset(root, asset_hash) is not None
        for _, root in fallback_roots
    )


def enumerate_battle_paths(
    string_id: str,
    remote_index: dict[str, ZipRangeEntry],
    fallback_roots: tuple[tuple[str, Path | None], ...],
) -> tuple[str, ...]:
    paths: list[str] = []
    for prefix in BATTLE_PREFIXES:
        for index in range(MAX_BATTLE_INDEX):
            logical = f"character/{string_id}/voice/{prefix}{index}.mp3"
            if not path_exists(logical, remote_index, fallback_roots):
                break
            paths.append(logical)
    for suffix in BATTLE_READY_PATHS:
        logical = f"character/{string_id}/voice/{suffix}.mp3"
        if path_exists(logical, remote_index, fallback_roots):
            paths.append(logical)
    return tuple(paths)
# //// /建立远程索引并读取本地回退音频 ////


# //// 合并排除项并构造 master 载荷 [@x380kkm 2026-08-29] ////
def remove_voice_exclusions(
    ui_string: dict[str, Any], string_ids: set[str]
) -> tuple[str, ...]:
    rows = ui_string.get("character_voice_exclude")
    if not isinstance(rows, list) or not rows or not isinstance(rows[0], list):
        raise VoiceOverlayError("ui_string 缺少 character_voice_exclude")
    original = rows[0][0] if rows[0] else ""
    tokens = [token for token in original.split("|") if token]
    removed = tuple(token for token in tokens if token in string_ids)
    remaining = [token for token in tokens if token not in string_ids]
    if remaining:
        rows[0][0] = "|".join(remaining)
    else:
        rows[0][0] = "(None)"
    return removed


def merge_master_payloads(
    master_bytes: dict[str, bytes],
    cn_master_root: Path,
    jp_master_root: Path,
    en_master_root: Path | None,
    role_ids: tuple[str, ...],
) -> tuple[list[VoiceAsset], list[RoleAudit], dict[str, Any]]:
    del cn_master_root
    cn_character = decode_ordered_map(
        master_bytes["master/character/character.orderedmap"]
    )
    cn_speech = decode_ordered_map(
        master_bytes["master/character/character_speech.orderedmap"]
    )
    cn_text = decode_ordered_map(
        master_bytes["master/character/character_text.orderedmap"]
    )
    jp_text = decode_ordered_map(
        find_master_file(
            jp_master_root, "master/character/character_text.orderedmap"
        ).read_bytes()
    )
    en_text = (
        decode_ordered_map(
            find_master_file(
                en_master_root, "master/character/character_text.orderedmap"
            ).read_bytes()
        )
        if en_master_root is not None
        else None
    )
    records = read_character_records(cn_character, cn_text, cn_speech, role_ids)
    role_audits, changed_actor_count = merge_voice_actor(
        records, jp_text, en_text, cn_text
    )
    ui_string = decode_ordered_map(
        master_bytes["master/string/ui_string.orderedmap"]
    )
    removed_exclusions = remove_voice_exclusions(
        ui_string, {record["string_id"] for record in records}
    )
    merged = dict(master_bytes)
    merged["master/character/character_text.orderedmap"] = encode_ordered_map(cn_text)
    merged["master/string/ui_string.orderedmap"] = encode_ordered_map(ui_string)
    master_assets = [
        VoiceAsset(
            logical_path,
            merged[logical_path],
            "merged"
            if logical_path
            in {
                "master/character/character_text.orderedmap",
                "master/string/ui_string.orderedmap",
            }
            else "cn-master",
        )
        for _, logical_path in MASTER_SPECS
    ]
    metadata = {
        "changed_actor_count": changed_actor_count,
        "removed_voice_exclusions": removed_exclusions,
        "master_record_count": len(master_assets),
        "master_logical_paths": [asset.logical_path for asset in master_assets],
    }
    return master_assets, role_audits, metadata
# //// /合并排除项并构造 master 载荷 ////


# //// 构建确定性 iOS 差分归档 [@x380kkm 2026-08-29] ////
def zip_entry(name: str) -> zipfile.ZipInfo:
    info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
    info.create_system = 3
    info.external_attr = 0o100644 << 16
    info.compress_type = zipfile.ZIP_DEFLATED
    return info


def build_archive(assets: Iterable[VoiceAsset]) -> bytes:
    unique: dict[str, VoiceAsset] = {}
    for asset in assets:
        existing = unique.get(asset.entry_path)
        if existing is not None and existing.data != asset.data:
            raise VoiceOverlayError(
                f"资源哈希冲突: {asset.logical_path} {asset.entry_path}"
            )
        unique.setdefault(asset.entry_path, asset)
    output = io.BytesIO()
    with zipfile.ZipFile(
        output,
        "w",
        compression=zipfile.ZIP_DEFLATED,
        compresslevel=9,
        allowZip64=False,
    ) as archive:
        for asset in assets:
            if unique.get(asset.entry_path) is not asset:
                continue
            archive.writestr(zip_entry(asset.entry_path), asset.data)
    data = output.getvalue()
    if b"PK\x06\x06" in data or b"PK\x06\x07" in data:
        raise VoiceOverlayError("iOS 语音差分归档不应使用 ZIP64")
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        if len(archive.infolist()) != len(unique):
            raise VoiceOverlayError("iOS 语音归档包含重复条目")
    return data
# //// /构建确定性 iOS 差分归档 ////


# //// 生成全量语音报告和归档 [@x380kkm 2026-08-29] ////
def build_voice_overlay(
    cn_master_root: Path,
    jp_master_root: Path,
    en_master_root: Path | None,
    output_root: Path,
    jp_archive_url: str,
    cache_root: Path,
    cn_asset_root: Path | None,
    en_asset_root: Path | None,
    role_ids: tuple[str, ...],
) -> dict[str, Any]:
    master_bytes = load_master_set(cn_master_root)
    remote_archive = RemoteZip64Archive(jp_archive_url, cache_root)
    remote_index = remote_asset_index(remote_archive)
    fallback_roots = (("en", en_asset_root), ("cn", cn_asset_root))
    if not role_ids:
        cn_character = decode_ordered_map(
            master_bytes["master/character/character.orderedmap"]
        )
        cn_text = decode_ordered_map(
            master_bytes["master/character/character_text.orderedmap"]
        )
        cn_speech = decode_ordered_map(
            master_bytes["master/character/character_speech.orderedmap"]
        )
        jp_text = decode_ordered_map(
            find_master_file(
                jp_master_root, "master/character/character_text.orderedmap"
            ).read_bytes()
        )
        en_text = (
            decode_ordered_map(
                find_master_file(
                    en_master_root, "master/character/character_text.orderedmap"
                ).read_bytes()
            )
            if en_master_root is not None
            else None
        )
        role_ids = discover_missing_voice_role_ids(
            cn_character,
            cn_text,
            cn_speech,
            jp_text,
            en_text,
            remote_index,
            fallback_roots,
        )
        if not role_ids:
            raise VoiceOverlayError("未找到需要补齐的 CN 角色语音")
    master_assets, role_audits, master_metadata = merge_master_payloads(
        master_bytes, cn_master_root, jp_master_root, en_master_root, role_ids
    )
    audio_assets: list[VoiceAsset] = []
    completed_roles: list[RoleAudit] = []
    missing_paths: list[str] = []
    pending: list[tuple[str, str]] = []
    role_path_sets: dict[str, tuple[tuple[str, ...], tuple[str, ...]]] = {}
    for audit in role_audits:
        speech_logical = tuple(
            f"character/{audit.string_id}/voice/{suffix}.mp3"
            for suffix in audit.speech_paths
        )
        battle_logical = enumerate_battle_paths(
            audit.string_id, remote_index, fallback_roots
        )
        role_path_sets[audit.character_id] = (speech_logical, battle_logical)
        pending.extend(
            (audit.character_id, logical_path)
            for logical_path in [*speech_logical, *battle_logical]
        )
    fetched: dict[str, VoiceAsset] = {}
    with ThreadPoolExecutor(max_workers=16) as executor:
        futures = {
            executor.submit(
                read_voice_asset,
                logical_path,
                remote_archive,
                remote_index,
                fallback_roots,
            ): (character_id, logical_path)
            for character_id, logical_path in pending
        }
        for future in as_completed(futures):
            character_id, logical_path = futures[future]
            data, source = future.result()
            if data is None:
                missing_paths.append(logical_path)
                continue
            fetched.setdefault(logical_path, VoiceAsset(logical_path, data, source))
    audio_assets.extend(fetched.values())
    for audit in role_audits:
        speech_logical, battle_logical = role_path_sets[audit.character_id]
        role_missing = tuple(
            logical_path
            for logical_path in [*speech_logical, *battle_logical]
            if logical_path not in fetched
        )
        completed_roles.append(
            replace(
                audit,
                speech_mp3_count=len(speech_logical),
                battle_mp3_count=len(battle_logical),
                speech_paths=speech_logical,
                battle_paths=battle_logical,
                missing_paths=role_missing,
            )
        )
    if missing_paths:
        raise VoiceOverlayError(
            f"语音资源缺失 {len(missing_paths)} 个: "
            + ", ".join(missing_paths[:8])
        )
    all_assets = [
        *master_assets,
        *sorted(audio_assets, key=lambda asset: asset.logical_path),
    ]
    archive_data = build_archive(all_assets)
    archive_name = "starpoint-cn-voice-overlay-ios.zip"
    archive_path = output_root / "archive-ios-diff" / archive_name
    atomic_write(archive_path, archive_data)
    asset_records = [
        {
            "logical_path": asset.logical_path,
            "entry_path": asset.entry_path,
            "asset_hash": asset.asset_hash,
            "source": asset.source,
            "byte_length": len(asset.data),
            "digest": asset.entity_digest,
        }
        for asset in all_assets
    ]
    role_records = [
        {
            "character_id": role.character_id,
            "string_id": role.string_id,
            "cn_voice_actor": role.cn_voice_actor,
            "jp_voice_actor": role.jp_voice_actor,
            "en_voice_actor": role.en_voice_actor,
            "selected_voice_actor": role.selected_voice_actor,
            "selected_source": role.selected_source,
            "speech_mp3_count": role.speech_mp3_count,
            "battle_mp3_count": role.battle_mp3_count,
            "speech_paths": list(role.speech_paths),
            "battle_paths": list(role.battle_paths),
            "missing_paths": list(role.missing_paths),
        }
        for role in completed_roles
    ]
    report = {
        "schema": 1,
        "master_scope": "normal",
        "bundled_master_scope": "preserved",
        "region_priority": ["jp", "en", "cn"],
        "role_count": len(completed_roles),
        "speech_mp3_count": sum(role.speech_mp3_count for role in completed_roles),
        "battle_mp3_count": sum(role.battle_mp3_count for role in completed_roles),
        "master_record_count": master_metadata["master_record_count"],
        "total_asset_count": len(all_assets),
        "total_bytes": sum(len(asset.data) for asset in all_assets),
        "missing_count": 0,
        "jp_archive_url": jp_archive_url,
        "archive": {
            "relative_path": f"archive-ios-diff/{archive_name}",
            "byte_length": len(archive_data),
            "sha256": base64.b64encode(
                hashlib.sha256(archive_data).digest()
            ).decode("ascii"),
            "zip64": False,
            "entry_count": len(asset_records),
        },
        "masters": asset_records[: len(master_assets)],
        "assets": asset_records[len(master_assets) :],
        "roles": role_records,
        "master_changes": {
            "changed_actor_count": master_metadata["changed_actor_count"],
            "removed_voice_exclusions": list(
                master_metadata["removed_voice_exclusions"]
            ),
            "logical_paths": master_metadata["master_logical_paths"],
        },
    }
    atomic_write(
        output_root / "voice-overlay-report.json",
        (json.dumps(report, ensure_ascii=False, indent=2) + "\n").encode("utf-8"),
    )
    atomic_write(
        output_root / "role-audit.json",
        (json.dumps(role_records, ensure_ascii=False, indent=2) + "\n").encode("utf-8"),
    )
    return report
# //// /生成全量语音报告和归档 ////


# //// 执行语音覆盖生成命令 [@x380kkm 2026-08-29] ////
def main() -> int:
    project_root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--cn-master-root",
        type=Path,
        default=project_root.parent / "archive",
    )
    parser.add_argument(
        "--jp-master-root",
        type=Path,
        default=project_root.parent / "archive" / "jp-voice-masters",
    )
    parser.add_argument("--en-master-root", type=Path)
    parser.add_argument("--cn-asset-root", type=Path)
    parser.add_argument("--en-asset-root", type=Path)
    parser.add_argument("--output-root", type=Path, required=True)
    parser.add_argument("--jp-archive-url", default=DEFAULT_JP_ARCHIVE_URL)
    parser.add_argument(
        "--cache-root",
        type=Path,
        default=project_root.parent / "archive" / "regional-zip-cache",
    )
    parser.add_argument("--role-id", action="append", dest="role_ids")
    arguments = parser.parse_args()
    role_ids = tuple(arguments.role_ids or ())
    if len(set(role_ids)) != len(role_ids):
        raise VoiceOverlayError("角色 ID 不应重复")
    report = build_voice_overlay(
        arguments.cn_master_root.resolve(strict=True),
        arguments.jp_master_root.resolve(strict=True),
        arguments.en_master_root.resolve(strict=True)
        if arguments.en_master_root
        else None,
        arguments.output_root.resolve(),
        arguments.jp_archive_url,
        arguments.cache_root.resolve(),
        arguments.cn_asset_root.resolve(strict=True)
        if arguments.cn_asset_root
        else None,
        arguments.en_asset_root.resolve(strict=True)
        if arguments.en_asset_root
        else None,
        role_ids,
    )
    print(json.dumps(report, ensure_ascii=False, separators=(",", ":")))
    return 0
# //// /执行语音覆盖生成命令 ////


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except VoiceOverlayError as error:
        print(
            json.dumps({"error": str(error)}, ensure_ascii=False),
            file=os.sys.stderr,
        )
        raise SystemExit(1) from None
