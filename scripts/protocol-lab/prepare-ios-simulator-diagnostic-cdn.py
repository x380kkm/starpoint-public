# audience: internal
# # prepare-ios-simulator-diagnostic-cdn
# 该脚本生成 Simulator 协议链使用的微型 CN CDN 归档, 其中资源清单与本地 ZIP 的大小和摘要一致.

from __future__ import annotations

import argparse
import base64
import hashlib
import io
import json
import tarfile
import zipfile
from pathlib import Path
from typing import Optional


TITLE_ENTITY_LIST_PATH = "EntityLists/empty.csv"
GAME_ENTITY_LIST_PATH = "entities/empty.csv"
IOS_TITLE_ENTITY_LIST_NAME = "10939-ios_medium.csv"
ANDROID_TITLE_ENTITY_LIST_NAME = "10939-android_medium.csv"
IOS_TITLE_ENTITY_LIST_PATH = "entities/%s" % IOS_TITLE_ENTITY_LIST_NAME
ANDROID_TITLE_ENTITY_LIST_PATH = "entities/%s" % ANDROID_TITLE_ENTITY_LIST_NAME
ENTITY_LIST_BODY = b""
WF_CONFIG_PATH = "wf/210009_config_20200415.json"
DEFAULT_WF_CONFIG_BODY = json.dumps(
    {
        "token": "00000000000000000000000000000000",
        "config": "00",
    },
    separators=(",", ":"),
).encode("utf-8")
STARTUP_STATIC_PATHS = (
    WF_CONFIG_PATH,
    "area/config.json",
    "protocols/leiting/sensitive/part/common_version.txt",
    "protocols/leiting/sensitive/part/common-text_version.txt",
    "protocols/leiting/switch/switch.txt",
)
FULL_ARCHIVE_SPECS = (
    (
        "archive-common-full",
        "starpoint-common-1.4.0-ios-simulator.zip",
        "starpoint-simulator-common.txt",
        b"starpoint simulator common asset\n",
    ),
    (
        "archive-ios-full",
        "starpoint-ios-1.4.0-ios-simulator.zip",
        "starpoint-simulator-ios.txt",
        b"starpoint simulator ios asset\n",
    ),
)
DIFF_ARCHIVE_SPECS = (
    (
        "archive-ios-diff",
        "starpoint-ios-1.4.0-1.4.54-ios-simulator.zip",
        "starpoint-simulator-ios-diff.txt",
        b"starpoint simulator ios diff asset\n",
    ),
)
ARCHIVE_SPECS = FULL_ARCHIVE_SPECS + DIFF_ARCHIVE_SPECS
SOURCE_BANNER_ARCHIVE_PREFIX = "starpoint-ios-gacha-banners-"
DEFAULT_ACTIVITY_CATALOG = {
    "format_version": 1,
    "region": "cn",
    "client_version": "1.8.4",
    "asset_version": "1.4.54",
    "generated_at": "2030-01-01T00:00:00Z",
    "activities": [
        {
            "activity_id": "story:simulator",
            "name": "Simulator Activity",
            "kind": "story",
            "default_start_at_ms": 1_893_456_000_000,
            "default_end_at_ms": 1_893_542_400_000,
        }
    ],
}


class SourceBannerDiff:
    def __init__(
        self,
        original_version: str,
        target_version: str,
        directory: str,
        name: str,
        body: bytes,
        entry_names: tuple[str, ...],
    ) -> None:
        self.original_version = original_version
        self.target_version = target_version
        self.directory = directory
        self.name = name
        self.body = body
        self.entry_names = entry_names


# //// 生成可由 ZIP 客户端打开的微型归档 [@x380kkm 2026-08-22] ////
def build_asset_archive(entry_name: str, entry_body: bytes) -> bytes:
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_STORED) as archive:
        entry = zipfile.ZipInfo(entry_name)
        entry.date_time = (1980, 1, 1, 0, 0, 0)
        entry.external_attr = 0o644 << 16
        archive.writestr(entry, entry_body)
    return output.getvalue()


# //// /生成可由 ZIP 客户端打开的微型归档 ////


# //// 生成单个微型归档的下载元数据 [@x380kkm 2026-08-22] ////
def build_archive_metadata(directory: str, name: str, body: bytes) -> dict[str, object]:
    return {
        "location": "https://retired.invalid/%s/%s" % (directory, name),
        "size": len(body),
        "sha256": base64.b64encode(hashlib.sha256(body).digest()).decode("ascii"),
    }


# //// /生成单个微型归档的下载元数据 ////


# //// 读取来源 CDN 的 iOS 卡池 banner 差分 [@x380kkm 2026-08-28] ////
def source_archive_relative_path(location: object) -> tuple[str, str]:
    if not isinstance(location, str):
        raise ValueError("source CDN archive location is invalid")
    parts = [part for part in location.replace("\\", "/").split("/") if part]
    if len(parts) < 2:
        raise ValueError("source CDN archive location is incomplete")
    return parts[-2], parts[-1]


def read_source_banner_diff(source_root: Optional[Path]) -> Optional[SourceBannerDiff]:
    if source_root is None or not (source_root / "path").is_file():
        return None
    manifest = json.loads((source_root / "path").read_text(encoding="utf-8"))
    info = manifest.get("info")
    diffs = manifest.get("diff")
    if not isinstance(info, dict) or not isinstance(diffs, list):
        raise ValueError("source CDN path manifest is invalid")
    target_version = info.get("target_asset_version")
    matches: list[SourceBannerDiff] = []
    for group in diffs:
        if not isinstance(group, dict) or group.get("version") != target_version:
            continue
        archives = group.get("archive")
        if not isinstance(archives, list):
            raise ValueError("source CDN diff archive is invalid")
        for metadata in archives:
            if not isinstance(metadata, dict):
                raise ValueError("source CDN archive metadata is invalid")
            directory, name = source_archive_relative_path(metadata.get("location"))
            if directory != "archive-ios-diff" or not name.startswith(
                SOURCE_BANNER_ARCHIVE_PREFIX
            ):
                continue
            archive_path = source_root / directory / name
            body = archive_path.read_bytes()
            expected_digest = base64.b64encode(hashlib.sha256(body).digest()).decode("ascii")
            if metadata.get("size") != len(body) or metadata.get("sha256") != expected_digest:
                raise ValueError("source CDN banner archive metadata is inconsistent")
            with zipfile.ZipFile(io.BytesIO(body)) as archive:
                entry_names = tuple(archive.namelist())
            original_version = group.get("original_version")
            if not isinstance(original_version, str) or not isinstance(target_version, str):
                raise ValueError("source CDN banner archive version is invalid")
            matches.append(
                SourceBannerDiff(
                    original_version,
                    target_version,
                    directory,
                    name,
                    body,
                    entry_names,
                )
            )
    if len(matches) > 1:
        raise ValueError("source CDN contains duplicate current banner archives")
    return matches[0] if matches else None
# //// /读取来源 CDN 的 iOS 卡池 banner 差分 ////


# //// 生成与微型归档一致的 get_path 清单 [@x380kkm 2026-08-22] ////
def build_path_manifest(
    archive_bodies: dict[tuple[str, str], bytes],
    source_banner_diff: Optional[SourceBannerDiff] = None,
) -> bytes:
    full_archives = [
        build_archive_metadata(directory, name, archive_bodies[(directory, name)])
        for directory, name, _, _ in FULL_ARCHIVE_SPECS
    ]
    diff_archives = [
        build_archive_metadata(directory, name, archive_bodies[(directory, name)])
        for directory, name, _, _ in DIFF_ARCHIVE_SPECS
    ]
    target_version = (
        source_banner_diff.target_version if source_banner_diff is not None else "1.4.54"
    )
    diff_groups = [
        {
            "version": "1.4.54",
            "original_version": "1.4.0",
            "archive": diff_archives,
        }
    ]
    if source_banner_diff is not None:
        diff_groups.append(
            {
                "version": source_banner_diff.target_version,
                "original_version": source_banner_diff.original_version,
                "archive": [
                    build_archive_metadata(
                        source_banner_diff.directory,
                        source_banner_diff.name,
                        source_banner_diff.body,
                    )
                ],
            }
        )
    combined_digest = hashlib.sha256()
    digest_specs = list(ARCHIVE_SPECS)
    if source_banner_diff is not None:
        digest_specs.append(
            (
                source_banner_diff.directory,
                source_banner_diff.name,
                "",
                b"",
            )
        )
    for directory, name, _, _ in digest_specs:
        combined_digest.update(archive_bodies[(directory, name)])
    manifest = {
        "info": {
            "client_asset_version": "1.4.0",
            "target_asset_version": target_version,
            "eventual_target_asset_version": target_version,
            "is_initial": True,
            "latest_maj_first_version": "1.4.0",
        },
        "full": {
            "version": "1.4.0",
            "archive": full_archives,
        },
        "diff": diff_groups,
        "asset_version_hash": combined_digest.hexdigest(),
    }
    return json.dumps(manifest, ensure_ascii=False, separators=(",", ":")).encode(
        "utf-8"
    )


# //// /生成与微型归档一致的 get_path 清单 ////


# //// 读取真实活动目录或返回内置目录 [@x380kkm 2026-08-21] ////
def read_activity_catalog(source_root: Optional[Path]) -> bytes:
    if source_root is not None:
        catalog_path = source_root / "activity-catalog.json"
        if catalog_path.is_file():
            catalog = json.loads(catalog_path.read_text(encoding="utf-8"))
            if isinstance(catalog.get("activities"), list) and catalog["activities"]:
                return json.dumps(catalog, ensure_ascii=False, separators=(",", ":")).encode(
                    "utf-8"
                )
    return json.dumps(
        DEFAULT_ACTIVITY_CATALOG,
        ensure_ascii=False,
        separators=(",", ":"),
    ).encode("utf-8")


# //// /读取真实活动目录或返回内置目录 ////


# //// 将一个字节文件加入确定的 tar 路径 [@x380kkm 2026-08-21] ////
def add_bytes(archive: tarfile.TarFile, path: str, body: bytes) -> None:
    metadata = tarfile.TarInfo(path)
    metadata.size = len(body)
    metadata.mode = 0o644
    metadata.mtime = 0
    archive.addfile(metadata, io.BytesIO(body))


# //// /将一个字节文件加入确定的 tar 路径 ////


# //// 复制活动目录使用的管理图标 [@x380kkm 2026-08-21] ////
def add_management_icons(
    archive: tarfile.TarFile,
    source_root: Optional[Path],
) -> None:
    if source_root is None:
        return
    icon_root = source_root / "management-assets" / "item-icons"
    if not icon_root.is_dir():
        return
    for icon_path in sorted(path for path in icon_root.rglob("*") if path.is_file()):
        relative = icon_path.relative_to(source_root).as_posix()
        add_bytes(archive, relative, icon_path.read_bytes())


# //// /复制活动目录使用的管理图标 ////


# //// 生成与来源目录一致的标题页和游戏实体清单 [@x380kkm 2026-08-25] ////
def filtered_entity_list(
    source_root: Path,
    directory: str,
    name: str,
    entry_names: set[str],
) -> bytes:
    source_path = source_root / directory / name
    if not source_path.is_file():
        return ENTITY_LIST_BODY
    lines = [
        line
        for line in source_path.read_text(encoding="utf-8-sig").splitlines()
        if line.split(",", 1)[0] in entry_names
    ]
    return (("\n".join(lines) + "\n") if lines else "").encode("utf-8")


def add_entity_lists(
    archive: tarfile.TarFile,
    source_root: Optional[Path],
    source_banner_diff: Optional[SourceBannerDiff] = None,
) -> None:
    if source_root is not None and (source_root / "EntityLists").is_dir():
        directory = "EntityLists"
    elif source_root is not None and (source_root / "entities").is_dir():
        directory = "entities"
    else:
        directory = "EntityLists"
    entry_names = set(source_banner_diff.entry_names) if source_banner_diff else set()
    bodies = {
        "empty.csv": ENTITY_LIST_BODY,
        IOS_TITLE_ENTITY_LIST_NAME: (
            filtered_entity_list(
                source_root, directory, IOS_TITLE_ENTITY_LIST_NAME, entry_names
            )
            if source_root is not None and source_banner_diff is not None
            else ENTITY_LIST_BODY
        ),
        ANDROID_TITLE_ENTITY_LIST_NAME: ENTITY_LIST_BODY,
    }
    if source_root is not None and source_banner_diff is not None:
        bodies["PathFile.csv"] = filtered_entity_list(
            source_root, directory, "PathFile.csv", entry_names
        )
    for name, body in bodies.items():
        add_bytes(archive, "%s/%s" % (directory, name), body)


# //// /生成与来源目录一致的标题页和游戏实体清单 ////


# //// 复制启动链读取的静态资源 [@x380kkm 2026-08-22] ////
def add_startup_static_assets(
    archive: tarfile.TarFile,
    source_root: Optional[Path],
) -> None:
    for relative_path in STARTUP_STATIC_PATHS:
        if source_root is not None:
            source_path = source_root / relative_path
            if source_path.is_file():
                add_bytes(archive, relative_path, source_path.read_bytes())
                continue
        if relative_path == WF_CONFIG_PATH:
            add_bytes(archive, relative_path, DEFAULT_WF_CONFIG_BODY)


# //// /复制启动链读取的静态资源 ////


# //// 写出 Simulator CN CDN 归档 [@x380kkm 2026-08-22] ////
def write_diagnostic_cdn(output_path: Path, source_root: Optional[Path]) -> None:
    source_banner_diff = read_source_banner_diff(source_root)
    archive_bodies = {
        (directory, name): build_asset_archive(entry_name, entry_body)
        for directory, name, entry_name, entry_body in ARCHIVE_SPECS
    }
    if source_banner_diff is not None:
        archive_bodies[(source_banner_diff.directory, source_banner_diff.name)] = (
            source_banner_diff.body
        )
    output_path.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(output_path, "w:gz") as archive:
        add_bytes(archive, "activity-catalog.json", read_activity_catalog(source_root))
        add_entity_lists(archive, source_root, source_banner_diff)
        add_bytes(
            archive,
            "path",
            build_path_manifest(archive_bodies, source_banner_diff),
        )
        for (directory, name), body in archive_bodies.items():
            add_bytes(archive, "%s/%s" % (directory, name), body)
        add_management_icons(archive, source_root)
        add_startup_static_assets(archive, source_root)


# //// /写出 Simulator CN CDN 归档 ////


# //// 解析生成器命令行 [@x380kkm 2026-08-21] ////
def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--source-root", type=Path)
    return parser.parse_args()


# //// /解析生成器命令行 ////


if __name__ == "__main__":
    arguments = parse_arguments()
    write_diagnostic_cdn(arguments.output, arguments.source_root)
