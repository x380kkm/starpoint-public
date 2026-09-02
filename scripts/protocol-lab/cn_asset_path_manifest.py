# audience: internal
# # cn-asset-path-manifest
# 此模块按 CDN 内最终 ZIP 同步根 path 清单的大小和标准 Base64 SHA-256.

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import uuid
from collections.abc import Iterable, Iterator
from pathlib import Path, PurePosixPath
from typing import Any


ARCHIVE_DIRECTORIES = frozenset(
    {
        "archive-android-diff",
        "archive-android-full",
        "archive-common-diff",
        "archive-common-full",
        "archive-ios-diff",
        "archive-ios-full",
        "archive-medium-diff",
        "archive-medium-full",
    }
)
PATH_MANIFEST_NAME = "path"


# //// 枚举 path 清单中的归档记录 [@x380kkm 2026-08-21] ////
def _archive_entries(manifest: Any) -> Iterator[dict[str, Any]]:
    if not isinstance(manifest, dict):
        raise ValueError("CN asset path manifest root must be an object")
    full = manifest.get("full")
    diff = manifest.get("diff")
    if not isinstance(full, dict) or not isinstance(diff, list):
        raise ValueError("CN asset path manifest groups are invalid")

    for group in [full, *diff]:
        if not isinstance(group, dict) or not isinstance(group.get("archive"), list):
            raise ValueError("CN asset path manifest archive group is invalid")
        for entry in group["archive"]:
            if not isinstance(entry, dict):
                raise ValueError("CN asset path manifest archive entry is invalid")
            yield entry
# //// /枚举 path 清单中的归档记录 ////


# //// 解析清单归档的 CDN 相对路径 [@x380kkm 2026-08-21] ////
def _archive_relative_path(entry: dict[str, Any]) -> str:
    location = entry.get("location")
    if not isinstance(location, str):
        raise ValueError("CN asset path manifest archive location is invalid")
    parts = [part for part in location.replace("\\", "/").split("/") if part]
    if len(parts) < 2:
        raise ValueError("CN asset path manifest archive location is incomplete")
    directory, file_name = parts[-2:]
    if (
        directory not in ARCHIVE_DIRECTORIES
        or not file_name.endswith(".zip")
        or Path(file_name).name != file_name
    ):
        raise ValueError("CN asset path manifest archive path is invalid")
    return f"{directory}/{file_name}"
# //// /解析清单归档的 CDN 相对路径 ////


# //// 计算归档的标准 Base64 SHA-256 [@x380kkm 2026-08-21] ////
def _archive_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(4 * 1024 * 1024), b""):
            digest.update(block)
    return base64.b64encode(digest.digest()).decode("ascii")
# //// /计算归档的标准 Base64 SHA-256 ////


# //// 规范化调用方指定的归档路径 [@x380kkm 2026-08-21] ////
def _selected_archive_paths(archive_paths: Iterable[Path]) -> set[str]:
    selected: set[str] = set()
    for archive_path in archive_paths:
        relative = PurePosixPath(archive_path.as_posix())
        parts = relative.parts
        if (
            relative.is_absolute()
            or len(parts) != 2
            or parts[0] not in ARCHIVE_DIRECTORIES
            or not parts[1].endswith(".zip")
        ):
            raise ValueError(f"CN archive path is invalid: {archive_path}")
        selected.add(relative.as_posix())
    return selected
# //// /规范化调用方指定的归档路径 ////


# //// 按最终归档内容生成同步后的 path 清单 [@x380kkm 2026-08-21] ////
def _synchronized_manifest(
    cdn_root: Path,
    selected_paths: set[str] | None,
) -> tuple[bytes, bytes, int]:
    manifest_path = cdn_root / PATH_MANIFEST_NAME
    original = manifest_path.read_bytes()
    try:
        manifest = json.loads(original)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError(f"CN asset path manifest is invalid: {error}") from error

    matched: set[str] = set()
    archive_metadata: dict[str, tuple[int, str]] = {}
    changed = 0
    for entry in _archive_entries(manifest):
        relative = _archive_relative_path(entry)
        if selected_paths is not None and relative not in selected_paths:
            continue
        matched.add(relative)
        if relative not in archive_metadata:
            archive_path = cdn_root.joinpath(*relative.split("/"))
            if not archive_path.is_file():
                raise ValueError(
                    f"CN asset path manifest archive is missing: {relative}"
                )
            archive_metadata[relative] = (
                archive_path.stat().st_size,
                _archive_sha256(archive_path),
            )
        size, sha256 = archive_metadata[relative]
        if entry.get("size") == size and entry.get("sha256") == sha256:
            continue
        entry["size"] = size
        entry["sha256"] = sha256
        changed += 1

    if selected_paths is not None:
        missing = sorted(selected_paths - matched)
        if missing:
            raise ValueError(
                "CN asset path manifest does not reference: " + ", ".join(missing)
            )
    if changed == 0:
        return original, original, changed

    synchronized = json.dumps(
        manifest,
        ensure_ascii=False,
        separators=(",", ":"),
    ).encode("utf-8")
    if original.endswith(b"\r\n"):
        synchronized += b"\r\n"
    elif original.endswith(b"\n"):
        synchronized += b"\n"
    return original, synchronized, changed
# //// /按最终归档内容生成同步后的 path 清单 ////


# //// 返回包装器使用的同步清单正文 [@x380kkm 2026-08-21] ////
def render_synchronized_cn_asset_path_manifest(cdn_root: Path) -> bytes | None:
    if not (cdn_root / PATH_MANIFEST_NAME).is_file():
        return None
    _, synchronized, _ = _synchronized_manifest(cdn_root, None)
    return synchronized
# //// /返回包装器使用的同步清单正文 ////


# //// 原子刷新指定归档对应的 path 清单记录 [@x380kkm 2026-08-21] ////
def refresh_cn_asset_path_manifest_entries(
    cdn_root: Path,
    archive_paths: Iterable[Path],
) -> int:
    selected = _selected_archive_paths(archive_paths)
    original, synchronized, changed = _synchronized_manifest(cdn_root, selected)
    if synchronized == original:
        return changed

    manifest_path = cdn_root / PATH_MANIFEST_NAME
    temporary_path = manifest_path.with_name(
        f".{manifest_path.name}.{uuid.uuid4().hex}.tmp"
    )
    try:
        temporary_path.write_bytes(synchronized)
        os.replace(temporary_path, manifest_path)
    finally:
        temporary_path.unlink(missing_ok=True)
    return changed
# //// /原子刷新指定归档对应的 path 清单记录 ////


# //// 原子刷新 path 清单中的全部归档记录 [@x380kkm 2026-08-21] ////
def refresh_cn_asset_path_manifest(cdn_root: Path) -> int:
    archive_paths = [
        Path(_archive_relative_path(entry))
        for entry in _archive_entries(
            json.loads((cdn_root / PATH_MANIFEST_NAME).read_bytes())
        )
    ]
    return refresh_cn_asset_path_manifest_entries(cdn_root, archive_paths)
# //// /原子刷新 path 清单中的全部归档记录 ////


# //// 运行 path 清单刷新命令 [@x380kkm 2026-08-21] ////
def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("cdn_root", type=Path)
    arguments = parser.parse_args()
    changed = refresh_cn_asset_path_manifest(arguments.cdn_root.resolve(strict=True))
    print(json.dumps({"updated_entries": changed}, separators=(",", ":")))
    return 0
# //// /运行 path 清单刷新命令 ////


if __name__ == "__main__":
    raise SystemExit(main())
