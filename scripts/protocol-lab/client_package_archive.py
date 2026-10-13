# audience: internal
# # client-package-archive
# 此模块按原始 ZIP 或 SPAPKBDL 的基础 APK 边界读取最终客户端包.
# Android 尾随或同名独立资源文档由安装包内嵌清单定位, ZIP 读取只覆盖基础 APK.

from __future__ import annotations

import io
import hashlib
import json
import struct
import zipfile
from contextlib import contextmanager, ExitStack
from pathlib import Path


BUNDLE_FOOTER = struct.Struct("<8sIIQQ32s")
EOCD_SIGNATURE = b"PK\x05\x06"


# //// 以文件片段提供随机读取 [@x380kkm 2026-09-21] ////
class FileSlice(io.RawIOBase):
    def __init__(self, path: Path, offset: int, length: int):
        self.source = path.open("rb")
        self.offset = offset
        self.length = length
        self.position = 0

    def read(self, size: int = -1) -> bytes:
        remaining = self.length - self.position
        size = remaining if size < 0 else min(size, remaining)
        self.source.seek(self.offset + self.position)
        data = self.source.read(size)
        self.position += len(data)
        return data

    def seek(self, offset: int, whence: int = 0) -> int:
        position = offset if whence == 0 else self.position + offset if whence == 1 else self.length + offset
        if whence not in (0, 1, 2) or not 0 <= position <= self.length:
            raise ValueError(f"文件片段读取越界: {position}")
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
        super().close()
# //// /以文件片段提供随机读取 ////


# //// 读取 Android 包的基础 ZIP 和尾随资源边界 [@x380kkm 2026-09-21] ////
def android_bundle_location(path: Path) -> dict | None:
    size = path.stat().st_size
    with path.open("rb") as source:
        tail_size = min(size, 65_557 + BUNDLE_FOOTER.size)
        source.seek(size - tail_size)
        tail = source.read(tail_size)
    for cursor in range(len(tail) - 22, BUNDLE_FOOTER.size - 1, -1):
        if tail[cursor:cursor + 4] != EOCD_SIGNATURE:
            continue
        comment_size = struct.unpack_from("<H", tail, cursor + 20)[0]
        if cursor + 22 + comment_size != len(tail):
            continue
        footer_offset = size - tail_size + cursor - BUNDLE_FOOTER.size
        magic, version, flags, offset, length, digest = BUNDLE_FOOTER.unpack_from(tail, cursor - BUNDLE_FOOTER.size)
        if magic != b"SPAPKBDL":
            return None
        if version != 1 or not flags & 1 or offset <= 0 or offset + length != footer_offset:
            raise ValueError("Android 尾随资源边界无效")
        return {"offset": offset, "length": length, "manifest_digest": digest.hex()}
    return None


# //// 核对独立资源文档与签名清单的边界 [@x380kkm 2026-09-26] ////
def android_resource_document_location(path: Path) -> dict:
    with path.open("rb") as source:
        header = source.read(BUNDLE_FOOTER.size)
    if len(header) != BUNDLE_FOOTER.size:
        raise ValueError("Android 独立资源文档头部不完整")
    magic, version, flags, offset, length, digest = BUNDLE_FOOTER.unpack(header)
    if (magic != b"SPAPKBDL" or version != 1 or flags != 0
            or offset != BUNDLE_FOOTER.size or offset + length != path.stat().st_size):
        raise ValueError("Android 独立资源文档边界无效")
    return {"offset": offset, "length": length, "manifest_digest": digest.hex()}
# //// /核对独立资源文档与签名清单的边界 ////


@contextmanager
def open_client_zip(path: Path):
    location = android_bundle_location(path) if path.suffix.lower() == ".apk" else None
    if location is None:
        with zipfile.ZipFile(path) as archive:
            yield archive
    else:
        with FileSlice(path, 0, location["offset"]) as prefix, zipfile.ZipFile(prefix) as archive:
            yield archive


@contextmanager
def open_game_zip(path: Path):
    with open_client_zip(path) as package:
        if "assets/starpoint-game.apk" in package.namelist():
            with zipfile.ZipFile(io.BytesIO(package.read("assets/starpoint-game.apk"))) as game:
                yield game
        else:
            yield package
# //// /读取 Android 包的基础 ZIP 和尾随资源边界 ////


# //// 从最终分发包读取内嵌 CDN 文件 [@x380kkm 2026-09-21] ////
class ClientPackageCdn:
    def __init__(self, path: Path):
        self.path = path
        self.payload_path = path
        self.context = ExitStack()
        try:
            self.open_package(path)
        except BaseException:
            self.context.close()
            raise

    def open_package(self, path: Path):
        self.package = self.context.enter_context(open_client_zip(path))
        self.location = android_bundle_location(path) if path.suffix.lower() == ".apk" else None
        delivery = "assets/starpoint-personal-service-cdn/resource-delivery.txt"
        if self.location is None and delivery in self.package.namelist():
            if self.package.read(delivery).decode("utf-8").strip() == "external":
                self.payload_path = path.with_suffix(".starpoint-resources")
                self.location = android_resource_document_location(self.payload_path)
        self.entries = {}
        if self.location is not None:
            cursor = self.location["offset"]
            manifest = self.package.read("assets/starpoint-personal-service-cdn/manifest.sha256")
            if hashlib.sha256(manifest).hexdigest() != self.location["manifest_digest"]:
                raise ValueError("Android 资源文档与安装包清单不匹配")
            for line in manifest.decode("utf-8-sig").splitlines():
                if not line:
                    continue
                digest, size, name = line.split("\t", 2)
                size = int(size)
                self.entries[name] = {"offset": cursor, "size": size, "digest": digest}
                cursor += size
            if cursor != self.location["offset"] + self.location["length"]:
                raise ValueError("Android CDN 清单和文件边界不一致")
        else:
            candidates = [name[:-4] for name in self.package.namelist() if name.endswith("/StarpointCNCDN/path")]
            if len(candidates) != 1:
                raise ValueError(f"包内 CDN 根目录数量异常: {candidates}")
            prefix = candidates[0]
            for entry in self.package.infolist():
                if entry.filename.startswith(prefix) and not entry.is_dir():
                    self.entries[entry.filename[len(prefix):]] = {"entry": entry, "size": entry.file_size}

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.context.close()

    @contextmanager
    def open(self, name: str):
        record = self.entries[name]
        if "offset" in record:
            with FileSlice(self.payload_path, record["offset"], record["size"]) as source:
                yield source
        else:
            with self.package.open(record["entry"]) as source:
                yield source

    def read(self, name: str) -> bytes:
        with self.open(name) as source:
            return source.read()

    def read_json(self, name: str) -> dict:
        return json.loads(self.read(name))

    @contextmanager
    def open_archive(self, name: str):
        with self.open(name) as source, zipfile.ZipFile(source) as archive:
            yield archive
# //// /从最终分发包读取内嵌 CDN 文件 ////


# //// 从物化目录读取同一 CDN 文件接口 [@x380kkm 2026-09-21] ////
class DirectoryCdn:
    def __init__(self, root: Path):
        self.root = root.resolve(strict=True)
        self.entries = {path.relative_to(self.root).as_posix(): {"size": path.stat().st_size}
                        for path in self.root.rglob("*") if path.is_file()}

    def __enter__(self):
        return self

    def __exit__(self, *args):
        return None

    def read(self, name: str) -> bytes:
        return (self.root / name).read_bytes()

    def read_json(self, name: str):
        return json.loads(self.read(name))

    @contextmanager
    def open_archive(self, name: str):
        with zipfile.ZipFile(self.root / name) as archive:
            yield archive
# //// /从物化目录读取同一 CDN 文件接口 ////
