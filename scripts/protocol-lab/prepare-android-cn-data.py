# audience: internal
# # prepare-android-cn-data
#
# 此程序从 CN CDN 生成 Android 侧载目录, 内容清单和完成标记.

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
from dataclasses import dataclass
from pathlib import Path


ANDROID_ENTITY_LIST = "entities/10939-android_medium.csv"
ANDROID_PATH_FILE = "entities/PathFile.csv"
ANDROID_ARCHIVE_DIRECTORIES = (
    "archive-common-full",
    "archive-common-diff",
    "archive-android-full",
    "archive-android-diff",
)
IOS_DATA_PREFIXES = (
    "archive-ios-",
    "production/ios_upload/",
    "production/ios_bundle/",
    "production/ios_medium_bundle/",
    "production/ios_small_bundle/",
)
MANIFEST_NAME = "manifest.sha256"
COMPLETE_MARKER = ".complete"


# //// 保存 Android CDN 文件记录 [@x380kkm 2026-08-31] ////
@dataclass(frozen=True)
class DataFile:
    source: Path
    relative: str
    size: int
    sha256: str
# //// /保存 Android CDN 文件记录 ////


# //// 计算文件 SHA-256 [@x380kkm 2026-08-31] ////
def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(4 * 1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()
# //// /计算文件 SHA-256 ////


# //// 判断路径是否属于 Android 分发数据 [@x380kkm 2026-08-31] ////
def is_android_data_path(relative: str) -> bool:
    first = relative.split("/", 1)[0]
    if first.startswith("activity-validation-"):
        return False
    if any(relative.startswith(prefix) for prefix in IOS_DATA_PREFIXES):
        return False
    if relative == "entities/10939-ios_medium.csv":
        return False
    if relative in ("materialization-manifest.json", MANIFEST_NAME, COMPLETE_MARKER):
        return False
    return True
# //// /判断路径是否属于 Android 分发数据 ////


# //// 收集 Android CDN 文件并固定顺序 [@x380kkm 2026-08-31] ////
def collect_data_files(source_root: Path) -> list[DataFile]:
    root = source_root.resolve(strict=True)
    if not (root / ANDROID_ENTITY_LIST).is_file():
        raise ValueError(f"Android EntityLists 文件不存在: {root / ANDROID_ENTITY_LIST}")
    if not (root / ANDROID_PATH_FILE).is_file():
        raise ValueError(f"Android PathFile 文件不存在: {root / ANDROID_PATH_FILE}")
    for relative in ANDROID_ARCHIVE_DIRECTORIES:
        archive_root = root / relative
        if not archive_root.is_dir() or not any(archive_root.iterdir()):
            raise ValueError(f"Android 分发 archive 目录为空: {archive_root}")

    records: list[DataFile] = []
    for path in root.rglob("*"):
        if path.is_symlink():
            raise ValueError(f"CDN 包含符号链接: {path}")
        if not path.is_file():
            continue
        relative = path.relative_to(root).as_posix()
        if not is_android_data_path(relative):
            continue
        if any(segment in ("", ".", "..") for segment in relative.split("/")):
            raise ValueError(f"CDN 路径无效: {relative}")
        if any(character in relative for character in ("\t", "\n", "\r", "\\")):
            raise ValueError(f"CDN 路径无效: {relative}")
        records.append(
            DataFile(
                source=path,
                relative=relative,
                size=path.stat().st_size,
                sha256=sha256_file(path),
            )
        )
    records.sort(key=lambda record: record.relative)
    if not records:
        raise ValueError("Android CDN 选择结果为空")
    return records
# //// /收集 Android CDN 文件并固定顺序 ////


# //// 生成 Android CDN 内容清单 [@x380kkm 2026-08-31] ////
def build_manifest(records: list[DataFile]) -> bytes:
    lines = [f"{record.sha256}\t{record.size}\t{record.relative}" for record in records]
    return ("\n".join(lines) + "\n").encode("utf-8")
# //// /生成 Android CDN 内容清单 ////


# //// 使用硬链接或复制物化分发数据 [@x380kkm 2026-08-31] ////
def materialize_data(records: list[DataFile], destination: Path) -> tuple[int, int]:
    linked = 0
    copied = 0
    for record in records:
        target = destination / Path(record.relative)
        target.parent.mkdir(parents=True, exist_ok=True)
        try:
            os.link(record.source, target)
            linked += 1
        except OSError:
            shutil.copy2(record.source, target)
            copied += 1
    return linked, copied
# //// /使用硬链接或复制物化分发数据 ////


# //// 生成版本化 Android CDN 分发目录 [@x380kkm 2026-08-31] ////
def prepare_distribution(source_root: Path, output_root: Path) -> dict[str, object]:
    source = source_root.resolve(strict=True)
    output = output_root.resolve()
    if output == source or output.is_relative_to(source):
        raise ValueError("Android CDN 输出目录位于源目录内部")
    if output.exists():
        raise ValueError(f"Android CDN 输出目录已存在: {output}")

    records = collect_data_files(source)
    manifest = build_manifest(records)
    manifest_sha256 = hashlib.sha256(manifest).hexdigest()
    manifest_prefix = manifest_sha256[:16]
    data_directory = output / manifest_prefix
    data_directory.mkdir(parents=True)
    linked, copied = materialize_data(records, data_directory)
    (data_directory / MANIFEST_NAME).write_bytes(manifest)
    (data_directory / COMPLETE_MARKER).write_text(
        manifest_sha256 + "\n",
        encoding="utf-8",
        newline="",
    )

    report = {
        "schema_version": 1,
        "platform": "android",
        "package_id": "dev.starpoint.personalservice",
        "source_root": str(source),
        "manifest_sha256": manifest_sha256,
        "manifest_prefix": manifest_prefix,
        "manifest_file": str(data_directory / MANIFEST_NAME),
        "data_directory": str(data_directory),
        "files": len(records),
        "bytes": sum(record.size for record in records),
        "linked_files": linked,
        "copied_files": copied,
        "required_entries": [
            ANDROID_ENTITY_LIST,
            ANDROID_PATH_FILE,
            *ANDROID_ARCHIVE_DIRECTORIES,
        ],
    }
    report_path = output / "android-cdn-report.json"
    report_path.write_text(
        json.dumps(report, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
        newline="",
    )
    return report
# //// /生成版本化 Android CDN 分发目录 ////


# //// 解析参数并输出 Android CDN 报告 [@x380kkm 2026-08-31] ////
def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    report = prepare_distribution(args.source, args.output)
    print(json.dumps(report, ensure_ascii=False))
    return 0
# //// /解析参数并输出 Android CDN 报告 ////


# //// 运行 Android CDN 准备命令 [@x380kkm 2026-08-31] ////
if __name__ == "__main__":
    raise SystemExit(main())
# //// /运行 Android CDN 准备命令 ////
