# audience: internal
# # regional-zip-range
# 此模块使用 HTTP Range 读取远程 ZIP 和 ZIP64 的中央目录及指定成员.
# 成员读取验证压缩方式, 解压大小和 CRC-32, 适用于大型客户端缓存归档.

from __future__ import annotations

import binascii
import hashlib
import json
import os
import struct
import time
import urllib.error
import urllib.request
import uuid
import zlib
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable


END_DIRECTORY = struct.Struct("<4s4H2LH")
ZIP64_END_DIRECTORY = struct.Struct("<4sQ2H2L4Q")
ZIP64_LOCATOR = struct.Struct("<4sLQL")
CENTRAL_DIRECTORY = struct.Struct("<4s6H3L5H2L")
LOCAL_FILE_HEADER = struct.Struct("<4s5H3L2H")
END_DIRECTORY_SIGNATURE = b"PK\x05\x06"
ZIP64_END_DIRECTORY_SIGNATURE = b"PK\x06\x06"
ZIP64_LOCATOR_SIGNATURE = b"PK\x06\x07"
CENTRAL_DIRECTORY_SIGNATURE = b"PK\x01\x02"
LOCAL_FILE_SIGNATURE = b"PK\x03\x04"
ZIP64_EXTRA_FIELD_ID = 0x0001
MAX_DIRECTORY_BYTES = 512 * 1024 * 1024
MAX_MEMBER_BYTES = 256 * 1024 * 1024
MAX_MEMBER_COUNT = 1_000_000
HTTP_ATTEMPTS = 3


class RegionalZipError(RuntimeError):
    pass


@dataclass(frozen=True)
class ZipDirectoryMetrics:
    archive_size: int
    directory_offset: int
    directory_size: int
    member_count: int


@dataclass(frozen=True)
class ZipRangeEntry:
    name: str
    flags: int
    compression: int
    crc32: int
    compressed_size: int
    uncompressed_size: int
    local_header_offset: int


# //// 读取严格匹配响应范围的远端字节 [@x380kkm 2026-08-29] ////
class HttpRangeSource:
    def __init__(self, url: str) -> None:
        if not url.startswith(("http://", "https://")):
            raise RegionalZipError("archive URL must use HTTP or HTTPS")
        self.url = url
        self._size: int | None = None

    @property
    def size(self) -> int:
        if self._size is None:
            response, body = self._request({"Range": "bytes=0-0"})
            content_range = response.headers.get("Content-Range", "")
            if response.status != 206 or not content_range.startswith("bytes 0-0/"):
                raise RegionalZipError("archive server does not provide byte ranges")
            try:
                self._size = int(content_range.rsplit("/", 1)[1])
            except ValueError as error:
                raise RegionalZipError("archive Content-Range is invalid") from error
            if body != b"PK"[:1]:
                raise RegionalZipError("archive does not begin with a ZIP signature")
        return self._size

    def read(self, offset: int, length: int) -> bytes:
        if offset < 0 or length < 0 or offset + length > self.size:
            raise RegionalZipError("archive byte range is out of bounds")
        if length == 0:
            return b""
        end = offset + length - 1
        response, body = self._request({"Range": f"bytes={offset}-{end}"})
        expected_range = f"bytes {offset}-{end}/{self.size}"
        if response.status != 206 or response.headers.get("Content-Range") != expected_range:
            raise RegionalZipError("archive server returned a mismatched byte range")
        if len(body) != length:
            raise RegionalZipError("archive byte range is truncated")
        return body

    def _request(
        self, headers: dict[str, str]
    ) -> tuple[urllib.response.addinfourl, bytes]:
        request = urllib.request.Request(
            self.url,
            headers={"Accept-Encoding": "identity", "User-Agent": "StarpointVoice/1", **headers},
        )
        last_error: BaseException | None = None
        for attempt in range(HTTP_ATTEMPTS):
            try:
                with urllib.request.urlopen(request, timeout=240) as response:
                    return response, response.read()
            except (OSError, urllib.error.URLError) as error:
                last_error = error
                if attempt + 1 < HTTP_ATTEMPTS:
                    time.sleep(0.5 * (attempt + 1))
        raise RegionalZipError("archive range request failed") from last_error


# //// /读取严格匹配响应范围的远端字节 ////


# //// 解析 ZIP64 中央目录和成员数据 [@x380kkm 2026-08-29] ////
class RemoteZip64Archive:
    def __init__(self, url: str, cache_root: Path | None = None) -> None:
        self.source = HttpRangeSource(url)
        self.cache_root = cache_root
        self.metrics = self._read_directory_metrics()
        self.entries = self._read_entries()

    @property
    def url(self) -> str:
        return self.source.url

    def find_hashed_asset(
        self,
        asset_hash: str,
        variants: Iterable[str] = (
            "upload",
            "android_upload",
            "ios_upload",
            "medium_upload",
        ),
    ) -> ZipRangeEntry | None:
        if len(asset_hash) != 40 or any(character not in "0123456789abcdef" for character in asset_hash):
            raise RegionalZipError("asset hash must be lowercase SHA-1")
        suffix = f"/{asset_hash[:2]}/{asset_hash[2:]}"
        priority = {variant: index for index, variant in enumerate(variants)}
        candidates: list[tuple[int, str, ZipRangeEntry]] = []
        for entry in self.entries:
            if not entry.name.endswith(suffix):
                continue
            marker = "/production/"
            marker_offset = entry.name.rfind(marker)
            if marker_offset < 0:
                continue
            variant = entry.name[marker_offset + len(marker) :].split("/", 1)[0]
            if variant in priority:
                candidates.append((priority[variant], entry.name, entry))
        return min(candidates, default=None, key=lambda candidate: candidate[:2])[2] if candidates else None

    def read(self, entry: ZipRangeEntry) -> bytes:
        if entry.flags & 0x1:
            raise RegionalZipError(f"encrypted ZIP member is unsupported: {entry.name}")
        if entry.uncompressed_size > MAX_MEMBER_BYTES:
            raise RegionalZipError(f"ZIP member exceeds the configured limit: {entry.name}")
        header = self.source.read(entry.local_header_offset, LOCAL_FILE_HEADER.size)
        values = LOCAL_FILE_HEADER.unpack(header)
        if values[0] != LOCAL_FILE_SIGNATURE:
            raise RegionalZipError(f"ZIP local header is invalid: {entry.name}")
        name_length, extra_length = values[-2:]
        data_offset = entry.local_header_offset + LOCAL_FILE_HEADER.size + name_length + extra_length
        compressed = self.source.read(data_offset, entry.compressed_size)
        if entry.compression == 0:
            data = compressed
        elif entry.compression == 8:
            try:
                data = zlib.decompress(compressed, -zlib.MAX_WBITS)
            except zlib.error as error:
                raise RegionalZipError(f"ZIP member cannot be inflated: {entry.name}") from error
        else:
            raise RegionalZipError(
                f"ZIP compression method {entry.compression} is unsupported: {entry.name}"
            )
        if len(data) != entry.uncompressed_size:
            raise RegionalZipError(f"ZIP member size is inconsistent: {entry.name}")
        if binascii.crc32(data) & 0xFFFFFFFF != entry.crc32:
            raise RegionalZipError(f"ZIP member CRC-32 is inconsistent: {entry.name}")
        return data

    def _read_directory_metrics(self) -> ZipDirectoryMetrics:
        tail_size = min(self.source.size, END_DIRECTORY.size + 65_535 + ZIP64_LOCATOR.size)
        tail_offset = self.source.size - tail_size
        tail = self.source.read(tail_offset, tail_size)
        end_offset = tail.rfind(END_DIRECTORY_SIGNATURE)
        if end_offset < 0 or end_offset + END_DIRECTORY.size > len(tail):
            raise RegionalZipError("ZIP end directory is missing")
        end_record = END_DIRECTORY.unpack_from(tail, end_offset)
        member_count = end_record[4]
        directory_size = end_record[5]
        directory_offset = end_record[6]
        if member_count == 0xFFFF or directory_size == 0xFFFFFFFF or directory_offset == 0xFFFFFFFF:
            locator_offset = tail_offset + end_offset - ZIP64_LOCATOR.size
            locator = self.source.read(locator_offset, ZIP64_LOCATOR.size)
            locator_record = ZIP64_LOCATOR.unpack(locator)
            if locator_record[0] != ZIP64_LOCATOR_SIGNATURE or locator_record[3] != 1:
                raise RegionalZipError("ZIP64 locator is invalid")
            zip64_record = ZIP64_END_DIRECTORY.unpack(
                self.source.read(locator_record[2], ZIP64_END_DIRECTORY.size)
            )
            if zip64_record[0] != ZIP64_END_DIRECTORY_SIGNATURE:
                raise RegionalZipError("ZIP64 end directory is invalid")
            member_count = zip64_record[7]
            directory_size = zip64_record[8]
            directory_offset = zip64_record[9]
        if member_count > MAX_MEMBER_COUNT:
            raise RegionalZipError("ZIP member count exceeds the configured limit")
        if directory_size > MAX_DIRECTORY_BYTES:
            raise RegionalZipError("ZIP central directory exceeds the configured limit")
        if directory_offset + directory_size > self.source.size:
            raise RegionalZipError("ZIP central directory is out of bounds")
        return ZipDirectoryMetrics(
            archive_size=self.source.size,
            directory_offset=directory_offset,
            directory_size=directory_size,
            member_count=member_count,
        )

    def _read_entries(self) -> tuple[ZipRangeEntry, ...]:
        directory = self._read_cached_directory()
        entries: list[ZipRangeEntry] = []
        offset = 0
        while offset < len(directory):
            if offset + CENTRAL_DIRECTORY.size > len(directory):
                raise RegionalZipError("ZIP central directory is truncated")
            values = CENTRAL_DIRECTORY.unpack_from(directory, offset)
            if values[0] != CENTRAL_DIRECTORY_SIGNATURE:
                raise RegionalZipError("ZIP central directory entry is invalid")
            name_length, extra_length, comment_length = values[10:13]
            entry_end = offset + CENTRAL_DIRECTORY.size + name_length + extra_length + comment_length
            if entry_end > len(directory):
                raise RegionalZipError("ZIP central directory entry is out of bounds")
            name_bytes = directory[
                offset + CENTRAL_DIRECTORY.size : offset + CENTRAL_DIRECTORY.size + name_length
            ]
            encoding = "utf-8" if values[3] & 0x800 else "cp437"
            try:
                name = name_bytes.decode(encoding)
            except UnicodeDecodeError as error:
                raise RegionalZipError("ZIP member name cannot be decoded") from error
            extra_start = offset + CENTRAL_DIRECTORY.size + name_length
            extra = directory[extra_start : extra_start + extra_length]
            uncompressed_size, compressed_size, local_offset, disk_number = _read_zip64_values(
                extra,
                values[9],
                values[8],
                values[16],
                values[13],
            )
            if disk_number != 0:
                raise RegionalZipError("multi-disk ZIP archives are unsupported")
            entries.append(
                ZipRangeEntry(
                    name=name,
                    flags=values[3],
                    compression=values[4],
                    crc32=values[7],
                    compressed_size=compressed_size,
                    uncompressed_size=uncompressed_size,
                    local_header_offset=local_offset,
                )
            )
            offset = entry_end
        if len(entries) != self.metrics.member_count:
            raise RegionalZipError("ZIP member count does not match its central directory")
        return tuple(entries)

    def _read_cached_directory(self) -> bytes:
        if self.cache_root is None:
            return self.source.read(self.metrics.directory_offset, self.metrics.directory_size)
        self.cache_root.mkdir(parents=True, exist_ok=True)
        identity = hashlib.sha256(self.url.encode("utf-8")).hexdigest()
        data_path = self.cache_root / f"{identity}.central-directory"
        metadata_path = self.cache_root / f"{identity}.json"
        expected = {
            "archive_size": self.metrics.archive_size,
            "directory_offset": self.metrics.directory_offset,
            "directory_size": self.metrics.directory_size,
            "url": self.url,
        }
        try:
            metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
            if metadata == expected and data_path.stat().st_size == self.metrics.directory_size:
                return data_path.read_bytes()
        except (OSError, UnicodeError, json.JSONDecodeError):
            pass
        data = self.source.read(self.metrics.directory_offset, self.metrics.directory_size)
        _write_atomic(data_path, data)
        _write_atomic(
            metadata_path,
            (json.dumps(expected, ensure_ascii=False, sort_keys=True) + "\n").encode("utf-8"),
        )
        return data


def _read_zip64_values(
    extra: bytes,
    uncompressed_size: int,
    compressed_size: int,
    local_offset: int,
    disk_number: int,
) -> tuple[int, int, int, int]:
    body: bytes | None = None
    offset = 0
    while offset + 4 <= len(extra):
        field_id, field_length = struct.unpack_from("<HH", extra, offset)
        field_end = offset + 4 + field_length
        if field_end > len(extra):
            raise RegionalZipError("ZIP extra field is truncated")
        if field_id == ZIP64_EXTRA_FIELD_ID:
            body = extra[offset + 4 : field_end]
            break
        offset = field_end
    values = [uncompressed_size, compressed_size, local_offset, disk_number]
    sentinels = [0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF, 0xFFFF]
    widths = [8, 8, 8, 4]
    body_offset = 0
    for index, (sentinel, width) in enumerate(zip(sentinels, widths)):
        if values[index] != sentinel:
            continue
        if body is None or body_offset + width > len(body):
            raise RegionalZipError("ZIP64 extra field is incomplete")
        values[index] = int.from_bytes(body[body_offset : body_offset + width], "little")
        body_offset += width
    return values[0], values[1], values[2], values[3]


def _write_atomic(path: Path, data: bytes) -> None:
    temporary = path.with_name(f".{path.name}.{uuid.uuid4().hex}.tmp")
    try:
        temporary.write_bytes(data)
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


# //// /解析 ZIP64 中央目录和成员数据 ////
