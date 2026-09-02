# audience: internal
# # inspect-android-apk-bundle
#
# 此程序解析 Android APK 尾随 bundle, 按内嵌清单检查 payload.

from __future__ import annotations

import argparse
import hashlib
import json
import struct
import zipfile
from pathlib import Path


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


# //// 向 ZIP 解析器暴露基础 APK 前缀 [@x380kkm 2026-08-31] ////
class PrefixReader:
    def __init__(self, path: Path, length: int) -> None:
        self.source = path.open("rb")
        self.length = length
        self.position = 0

    def read(self, size: int = -1) -> bytes:
        remaining = self.length - self.position
        if size < 0 or size > remaining:
            size = remaining
        data = self.source.read(size)
        self.position += len(data)
        return data

    def seek(self, offset: int, whence: int = 0) -> int:
        if whence == 0:
            position = offset
        elif whence == 1:
            position = self.position + offset
        elif whence == 2:
            position = self.length + offset
        else:
            raise ValueError(f"seek whence 无效: {whence}")
        if position < 0 or position > self.length:
            raise ValueError(f"seek 越过基础 APK: {position}")
        self.source.seek(position)
        self.position = position
        return position

    def tell(self) -> int:
        return self.position

    def seekable(self) -> bool:
        return True

    def readable(self) -> bool:
        return True

    def close(self) -> None:
        self.source.close()

    def __enter__(self) -> PrefixReader:
        return self

    def __exit__(self, exc_type: object, exc_value: object, traceback: object) -> None:
        self.close()


# //// 计算文件摘要 [@x380kkm 2026-08-31] ////
def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(COPY_BUFFER_SIZE), b""):
            digest.update(block)
    return digest.hexdigest()


# //// 查找文件末尾的 ZIP 末尾记录 [@x380kkm 2026-08-31] ////
def find_final_eocd(path: Path) -> tuple[int, int, bytes]:
    file_size = path.stat().st_size
    read_size = min(file_size, EOCD_SEARCH_LIMIT)
    with path.open("rb") as source:
        source.seek(file_size - read_size)
        tail = source.read(read_size)
    cursor = len(tail) - EOCD_MIN_SIZE
    while cursor >= 0:
        if tail[cursor : cursor + 4] == EOCD_SIGNATURE:
            comment_length = struct.unpack_from("<H", tail, cursor + 20)[0]
            length = EOCD_MIN_SIZE + comment_length
            if cursor + length == len(tail):
                offset = file_size - read_size + cursor
                return offset, length, tail[cursor : cursor + length]
        cursor -= 1
    raise ValueError("bundle 末尾没有 ZIP 末尾记录.")


# //// 读取并验证尾部 footer [@x380kkm 2026-08-31] ////
def read_footer(path: Path) -> tuple[dict[str, int], bytes]:
    eocd_offset, eocd_length, _ = find_final_eocd(path)
    footer_offset = eocd_offset - FOOTER_SIZE
    if footer_offset < 0:
        raise ValueError("bundle 没有 footer 空间.")
    with path.open("rb") as source:
        source.seek(footer_offset)
        footer_bytes = source.read(FOOTER_SIZE)
    magic, version, flags, payload_offset, payload_length, manifest_digest = FOOTER.unpack(footer_bytes)
    if magic != MAGIC or version != VERSION or not (flags & FLAG_DUPLICATE_EOCD):
        raise ValueError("bundle footer 标识无效.")
    if payload_offset <= 0 or payload_offset + payload_length != footer_offset:
        raise ValueError("bundle payload 与 footer 不连续.")
    return {
        "payload_offset": payload_offset,
        "payload_length": payload_length,
        "footer_offset": footer_offset,
        "eocd_offset": eocd_offset,
        "eocd_length": eocd_length,
    }, manifest_digest


# //// 读取清单并计算顺序偏移 [@x380kkm 2026-08-31] ////
def read_manifest(path: Path, location: dict[str, int], expected_digest: bytes) -> list[tuple[str, int, str, int]]:
    try:
        with PrefixReader(path, location["payload_offset"]) as prefix:
            with zipfile.ZipFile(prefix, "r") as archive:
                manifest = archive.read(MANIFEST_ASSET)
    except (KeyError, zipfile.BadZipFile) as error:
        raise ValueError("bundle 的基础 APK ZIP 不可读或缺少清单 asset.") from error
    if hashlib.sha256(manifest).digest() != expected_digest:
        raise ValueError("bundle footer 的清单摘要与 APK 内清单不一致.")
    entries: list[tuple[str, int, str, int]] = []
    cursor = location["payload_offset"]
    seen: set[str] = set()
    for line_number, raw_line in enumerate(manifest.decode("utf-8-sig").splitlines(), 1):
        if not raw_line:
            continue
        fields = raw_line.split("\t", 2)
        if len(fields) != 3:
            raise ValueError(f"清单字段数量无效: line={line_number}")
        digest, raw_size, relative = fields
        try:
            size = int(raw_size)
        except ValueError as error:
            raise ValueError(f"清单长度无效: line={line_number}") from error
        if size < 0 or len(digest) != 64 or any(character not in "0123456789abcdef" for character in digest):
            raise ValueError(f"清单摘要或长度无效: line={line_number}")
        if (
            not relative
            or relative.startswith("/")
            or "\\" in relative
            or any(segment in ("", ".", "..") for segment in relative.split("/"))
        ):
            raise ValueError(f"清单路径无效: line={line_number}")
        if relative in seen:
            raise ValueError(f"清单路径重复: {relative}")
        seen.add(relative)
        entries.append((digest, size, relative, cursor))
        cursor += size
    if cursor != location["payload_offset"] + location["payload_length"]:
        raise ValueError("清单总长度与 payload 长度不一致.")
    return entries


# //// 验证 payload 文件摘要 [@x380kkm 2026-08-31] ////
def verify_payload(path: Path, entries: list[tuple[str, int, str, int]]) -> None:
    with path.open("rb") as source:
        for expected_digest, size, relative, offset in entries:
            source.seek(offset)
            digest = hashlib.sha256()
            remaining = size
            while remaining:
                block = source.read(min(COPY_BUFFER_SIZE, remaining))
                if not block:
                    raise ValueError(f"payload 提前结束: {relative}")
                digest.update(block)
                remaining -= len(block)
            if digest.hexdigest() != expected_digest:
                raise ValueError(f"payload 摘要不一致: {relative}")


# //// 验证基础 ZIP 的重复末尾记录 [@x380kkm 2026-08-31] ////
def verify_duplicate_eocd(path: Path, location: dict[str, int]) -> None:
    base_length = location["payload_offset"]
    read_size = min(base_length, EOCD_SEARCH_LIMIT)
    with path.open("rb") as source:
        source.seek(base_length - read_size)
        base_tail = source.read(read_size)
    cursor = len(base_tail) - EOCD_MIN_SIZE
    base_eocd = None
    while cursor >= 0:
        if base_tail[cursor : cursor + 4] == EOCD_SIGNATURE:
            comment_length = struct.unpack_from("<H", base_tail, cursor + 20)[0]
            length = EOCD_MIN_SIZE + comment_length
            if cursor + length == len(base_tail):
                base_eocd = base_tail[cursor : cursor + length]
                break
        cursor -= 1
    if base_eocd is None:
        raise ValueError("基础 APK 找不到 ZIP 末尾记录.")
    with path.open("rb") as source:
        source.seek(location["eocd_offset"])
        final_eocd = source.read(location["eocd_length"])
    if base_eocd != final_eocd:
        raise ValueError("bundle 重复 EOCD 与基础 APK 不一致.")


# //// 运行解析校验并输出报告 [@x380kkm 2026-08-31] ////
def main() -> int:
    parser = argparse.ArgumentParser(description="inspect a Starpoint Android APK bundle")
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--skip-digests", action="store_true")
    args = parser.parse_args()
    bundle = args.bundle.resolve(strict=True)
    location, manifest_digest = read_footer(bundle)
    entries = read_manifest(bundle, location, manifest_digest)
    if not args.skip_digests:
        verify_payload(bundle, entries)
    verify_duplicate_eocd(bundle, location)
    bundle_report = {"path": str(bundle), "bytes": bundle.stat().st_size}
    if not args.skip_digests:
        bundle_report["sha256"] = sha256_file(bundle)
    report = {
        "schema_version": 1,
        "format": "starpoint-apk-bundle",
        "bundle": bundle_report,
        "payload": {"offset": location["payload_offset"], "bytes": location["payload_length"], "files": len(entries)},
        "footer": {"offset": location["footer_offset"], "bytes": FOOTER_SIZE, "magic": MAGIC.decode("ascii"), "version": VERSION},
        "manifest_sha256": manifest_digest.hex(),
        "digests_verified": not args.skip_digests,
    }
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
