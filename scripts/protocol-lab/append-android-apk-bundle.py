# audience: internal
# # append-android-apk-bundle
#
# 此程序把 Android CDN 清单中的文件按清单顺序追加到已签名伴随 APK.

from __future__ import annotations

import argparse
import hashlib
import json
import struct
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import BinaryIO, Protocol


MAGIC = b"SPAPKBDL"
VERSION = 1
FLAG_DUPLICATE_EOCD = 1
FOOTER_SIZE = 64
FOOTER = struct.Struct("<8sIIQQ32s")
EOCD_SIGNATURE = b"PK\x05\x06"
EOCD_MIN_SIZE = 22
EOCD_SEARCH_LIMIT = 65_557
COPY_BUFFER_SIZE = 4 * 1024 * 1024
MANIFEST_ASSET = "assets/starpoint-personal-service-cdn/manifest.sha256"


# //// 提供增量摘要写入边界 [@x380kkm 2026-08-31] ////
class DigestSink(Protocol):
    def update(self, data: bytes) -> None: ...


# //// 保存 ZIP 末尾定位结果 [@x380kkm 2026-08-31] ////
@dataclass(frozen=True)
class ZipEnd:
    offset: int
    length: int
    bytes: bytes
    central_directory_offset: int
    central_directory_size: int
    entry_count: int


# //// 保存 CDN 清单记录 [@x380kkm 2026-08-31] ////
@dataclass(frozen=True)
class ManifestEntry:
    digest: str
    size: int
    relative: str


# //// 复制文件并返回摘要 [@x380kkm 2026-08-31] ////
def copy_file(source: Path, target: BinaryIO, bundle_digest: DigestSink) -> tuple[int, str]:
    digest = hashlib.sha256()
    size = 0
    with source.open("rb") as input_file:
        for block in iter(lambda: input_file.read(COPY_BUFFER_SIZE), b""):
            target.write(block)
            bundle_digest.update(block)
            size += len(block)
            digest.update(block)
    return size, digest.hexdigest()


# //// 查找并验证 ZIP 末尾记录 [@x380kkm 2026-08-31] ////
def find_zip_end(path: Path) -> ZipEnd:
    file_size = path.stat().st_size
    if file_size < EOCD_MIN_SIZE:
        raise ValueError("基础 APK 小于 ZIP 末尾记录.")
    read_size = min(file_size, EOCD_SEARCH_LIMIT)
    with path.open("rb") as source:
        source.seek(file_size - read_size)
        tail = source.read(read_size)
    cursor = len(tail) - EOCD_MIN_SIZE
    while cursor >= 0:
        if tail[cursor : cursor + 4] != EOCD_SIGNATURE:
            cursor -= 1
            continue
        comment_length = struct.unpack_from("<H", tail, cursor + 20)[0]
        end = cursor + EOCD_MIN_SIZE + comment_length
        if end != len(tail):
            cursor -= 1
            continue
        fields = struct.unpack_from("<4s4H2IH", tail, cursor)
        _, disk, central_disk, disk_entries, total_entries, central_size, central_offset, _ = fields
        if disk != 0 or central_disk != 0 or disk_entries != total_entries:
            raise ValueError("基础 APK 使用了多磁盘 ZIP.")
        if (
            disk_entries == 0xFFFF
            or total_entries == 0xFFFF
            or central_size == 0xFFFFFFFF
            or central_offset == 0xFFFFFFFF
        ):
            raise ValueError("基础 APK 使用 ZIP64, 当前尾随格式要求普通 ZIP.")
        eocd_offset = file_size - read_size + cursor
        if central_offset + central_size > eocd_offset:
            raise ValueError("基础 APK 中央目录越过 ZIP 末尾记录.")
        return ZipEnd(
            offset=eocd_offset,
            length=EOCD_MIN_SIZE + comment_length,
            bytes=tail[cursor:end],
            central_directory_offset=central_offset,
            central_directory_size=central_size,
            entry_count=total_entries,
        )
    raise ValueError("基础 APK 找不到 ZIP 末尾记录.")


# //// 规范化并检查相对路径 [@x380kkm 2026-08-31] ////
def validate_relative_path(value: str) -> str:
    normalized = value.replace("\\", "/")
    if (
        not normalized
        or normalized.startswith("/")
        or "\x00" in normalized
        or any(segment in ("", ".", "..") for segment in normalized.split("/"))
    ):
        raise ValueError(f"CDN 路径无效: {value!r}")
    if any(character in normalized for character in "\t\r\n"):
        raise ValueError(f"CDN 路径包含控制字符: {value!r}")
    return normalized


# //// 读取 CDN 清单并检查字段 [@x380kkm 2026-08-31] ////
def read_manifest(path: Path) -> tuple[bytes, list[ManifestEntry]]:
    data = path.read_bytes()
    if not data:
        raise ValueError(f"CDN 清单为空: {path}")
    entries: list[ManifestEntry] = []
    seen: set[str] = set()
    for line_number, raw_line in enumerate(data.decode("utf-8-sig").splitlines(), 1):
        if not raw_line:
            continue
        fields = raw_line.split("\t", 2)
        if len(fields) != 3:
            raise ValueError(f"CDN 清单字段数量无效: line={line_number}")
        digest, raw_size, relative = fields
        if len(digest) != 64 or any(character not in "0123456789abcdef" for character in digest):
            raise ValueError(f"CDN 清单摘要无效: line={line_number}")
        try:
            size = int(raw_size)
        except ValueError as error:
            raise ValueError(f"CDN 清单长度无效: line={line_number}") from error
        if size < 0:
            raise ValueError(f"CDN 清单长度为负: line={line_number}")
        relative = validate_relative_path(relative)
        if relative in seen:
            raise ValueError(f"CDN 清单路径重复: {relative}")
        seen.add(relative)
        entries.append(ManifestEntry(digest, size, relative))
    if not entries:
        raise ValueError("CDN 清单没有有效记录.")
    return data, entries


# //// 验证基础 APK 内嵌清单 [@x380kkm 2026-08-31] ////
def assert_embedded_manifest(base_apk: Path, manifest: bytes) -> None:
    try:
        with zipfile.ZipFile(base_apk, "r") as archive:
            embedded = archive.read(MANIFEST_ASSET)
    except (KeyError, zipfile.BadZipFile) as error:
        raise ValueError("基础 APK 没有可读的 CDN 清单 asset.") from error
    if embedded != manifest:
        raise ValueError("基础 APK 内嵌清单与 CDN 清单不一致.")


# //// 检查源文件并固定追加顺序 [@x380kkm 2026-08-31] ////
def resolve_manifest_files(root: Path, entries: list[ManifestEntry]) -> list[tuple[ManifestEntry, Path]]:
    resolved_root = root.resolve(strict=True)
    result: list[tuple[ManifestEntry, Path]] = []
    for entry in entries:
        candidate = resolved_root / Path(entry.relative)
        if candidate.is_symlink():
            raise ValueError(f"CDN 文件是符号链接: {candidate}")
        source = candidate.resolve()
        if not source.is_relative_to(resolved_root):
            raise ValueError(f"CDN 路径越过根目录: {entry.relative}")
        if not source.is_file():
            raise ValueError(f"CDN 文件不存在或不是普通文件: {source}")
        actual_size = source.stat().st_size
        if actual_size != entry.size:
            raise ValueError(
                f"CDN 清单与文件长度不一致: {entry.relative} "
                f"expected={entry.size} actual={actual_size}"
            )
        result.append((entry, source))
    return result


# //// 写出追加 bundle [@x380kkm 2026-08-31] ////
def append_bundle(base_apk: Path, output_apk: Path, cdn_root: Path) -> dict[str, object]:
    base = base_apk.resolve(strict=True)
    output = output_apk.resolve()
    if output.exists():
        raise ValueError(f"输出 APK 已存在: {output}")
    if output == base:
        raise ValueError("输出 APK 不能覆盖基础 APK.")
    manifest_path = cdn_root.resolve(strict=True) / "manifest.sha256"
    manifest, manifest_entries = read_manifest(manifest_path)
    assert_embedded_manifest(base, manifest)
    resolved_files = resolve_manifest_files(cdn_root, manifest_entries)
    zip_end = find_zip_end(base)
    base_length = base.stat().st_size
    manifest_digest = hashlib.sha256(manifest).digest()

    output.parent.mkdir(parents=True, exist_ok=True)
    partial = output.with_name(output.name + ".partial")
    if partial.exists():
        raise ValueError(f"临时输出 APK 已存在: {partial}")
    bundle_digest = hashlib.sha256()
    try:
        with base.open("rb") as source, partial.open("wb") as target:
            remaining = base_length
            while remaining:
                block = source.read(min(COPY_BUFFER_SIZE, remaining))
                if not block:
                    raise IOError("复制基础 APK 时提前遇到文件末尾.")
                target.write(block)
                bundle_digest.update(block)
                remaining -= len(block)
            payload_offset = target.tell()
            for entry, source_path in resolved_files:
                actual_size, actual_digest = copy_file(source_path, target, bundle_digest)
                if actual_size != entry.size or actual_digest != entry.digest:
                    raise ValueError(
                        f"CDN 清单与文件摘要不一致: {entry.relative} "
                        f"expected={entry.size}/{entry.digest} actual={actual_size}/{actual_digest}"
                    )
            payload_length = target.tell() - payload_offset
            footer = FOOTER.pack(MAGIC, VERSION, FLAG_DUPLICATE_EOCD, payload_offset, payload_length, manifest_digest)
            target.write(footer)
            target.write(zip_end.bytes)
            bundle_digest.update(footer)
            bundle_digest.update(zip_end.bytes)
        partial.replace(output)
    except BaseException:
        partial.unlink(missing_ok=True)
        raise

    output_length = output.stat().st_size
    expected_length = base_length + payload_length + FOOTER_SIZE + zip_end.length
    if output_length != expected_length:
        raise IOError(f"bundle 长度不一致: expected={expected_length} actual={output_length}")
    output_digest = bundle_digest.hexdigest()
    return {
        "schema_version": 1,
        "format": "starpoint-apk-bundle",
        "apk": {"path": str(output), "bytes": output_length, "sha256": output_digest},
        "base": {"path": str(base), "bytes": base_length, "eocd_offset": zip_end.offset, "eocd_length": zip_end.length, "zip_entries": zip_end.entry_count},
        "payload": {"offset": payload_offset, "bytes": payload_length, "files": len(resolved_files)},
        "manifest": {"path": str(manifest_path), "bytes": len(manifest), "sha256": manifest_digest.hex()},
        "footer": {"offset": base_length + payload_length, "bytes": FOOTER_SIZE, "magic": MAGIC.decode("ascii"), "version": VERSION},
    }


# //// 运行 bundle 生成命令 [@x380kkm 2026-08-31] ////
def main() -> int:
    parser = argparse.ArgumentParser(description="append a manifest-ordered CDN payload to an APK")
    parser.add_argument("--base-apk", type=Path, required=True)
    parser.add_argument("--cdn-root", type=Path, required=True)
    parser.add_argument("--output-apk", type=Path, required=True)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    report = append_bundle(args.base_apk, args.output_apk, args.cdn_root)
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
