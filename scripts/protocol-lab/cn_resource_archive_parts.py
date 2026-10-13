# audience: internal
# # cn-resource-archive-parts
#
# 资源按约 20 MiB 的经典 ZIP 分档, 单个成员保持完整.
# 输出文件按写入顺序编号, 图像内容和逻辑路径保持原样.

from __future__ import annotations

import mmap
import os
import zipfile
from pathlib import Path

from cn_voice_overlay_archive import standard_archive_digest
from restore_cn_gacha_feature_images import zip_entry


MAX_ARCHIVE_BYTES = 20 * 1024 * 1024
ARCHIVE_DIRECTORY = "archive-common-diff"


# //// 顺序写入有大小上限的资源归档 [@x380kkm 2026-09-20] ////
class ResourceArchiveWriter:
    def __init__(self, directory: Path):
        self.directory = directory
        self.paths: list[Path] = []
        self.archive = None
        self.byte_budget = 22

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()

    def close(self):
        if self.archive is not None:
            self.archive.close()
            self.archive = None
            if self.paths[-1].stat().st_size > MAX_ARCHIVE_BYTES:
                raise RuntimeError(f"资源归档超过大小上限: {self.paths[-1]}")

    def write(self, name: str, data: bytes):
        entry_budget = len(data) + len(data) // 1000 + len(name.encode("utf-8")) * 2 + 512
        if entry_budget + 22 > MAX_ARCHIVE_BYTES:
            raise RuntimeError(f"资源成员超过归档大小上限: {name}")
        if self.archive is not None and self.byte_budget + entry_budget > MAX_ARCHIVE_BYTES:
            self.close()
        if self.archive is None:
            path = self.directory / f"resource-{len(self.paths) + 1:03d}.zip"
            self.archive = zipfile.ZipFile(path, "w", allowZip64=False)
            self.paths.append(path)
            self.byte_budget = 22
        self.archive.writestr(zip_entry(name), data, compresslevel=1)
        self.byte_budget += entry_budget
# //// /顺序写入有大小上限的资源归档 ////


# //// 生成分档清单并发布完整归档文件 [@x380kkm 2026-09-20] ////
def archive_part_records(paths: list[Path], original_version: str, target_version: str, prefix: str) -> list[dict]:
    records = []
    for number, path in enumerate(paths, 1):
        name = f"{prefix}{original_version}-{target_version}-{number:03d}.zip"
        with path.open("rb") as stream, mmap.mmap(stream.fileno(), 0, access=mmap.ACCESS_READ) as data:
            digest = standard_archive_digest(data)
        records.append({"relative_path": f"{ARCHIVE_DIRECTORY}/{name}",
                        "byte_length": path.stat().st_size, "sha256": digest})
    return records


def install_archive_parts(cdn_root: Path, paths: list[Path], records: list[dict]) -> None:
    destinations = [cdn_root / record["relative_path"] for record in records]
    for destination in destinations:
        if destination.exists():
            raise RuntimeError(f"资源归档目标已存在: {destination}")
    for path, destination in zip(paths, destinations, strict=True):
        os.replace(path, destination)
# //// /生成分档清单并发布完整归档文件 ////
