# audience: internal
# # ios-package-version
# 分发版本以原客户端主版本的下一个主版本为起点, 叠加 Starpoint 发布版本.
# App 和内嵌 Framework 使用同一分发版本与构建号.

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Any


VERSION_PATTERN = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")


# //// 保存发布版本与 Apple 分发版本 [@x380kkm 2026-09-21] ////
@dataclass(frozen=True)
class PackageVersion:
    release: str
    bundle: str
# //// /保存发布版本与 Apple 分发版本 ////


# //// 将发布版本映射到原客户端之上的分发版本 [@x380kkm 2026-09-21] ////
def create_package_version(client_version: str, release_version: str) -> PackageVersion:
    client = VERSION_PATTERN.fullmatch(client_version)
    release = VERSION_PATTERN.fullmatch(release_version)
    if client is None or release is None:
        raise ValueError("客户端版本和发布版本必须各含三个以点分隔的非负整数.")
    release_major, minor, patch = map(int, release.groups())
    major = int(client.group(1)) + 1 + release_major
    if major > 9999 or minor > 99 or patch > 99:
        raise ValueError("Apple 构建号的主版本最多四位, 次版本和补丁版本各最多两位.")
    return PackageVersion(release_version, f"{major}.{minor}.{patch}")
# //// /将发布版本映射到原客户端之上的分发版本 ////


# //// 将同一发布身份写入包信息 [@x380kkm 2026-09-21] ////
def apply_package_version(info: dict[str, Any], version: PackageVersion) -> None:
    info["StarpointReleaseVersion"] = version.release
    info["CFBundleShortVersionString"] = version.bundle
    info["CFBundleVersion"] = version.bundle
# //// /将同一发布身份写入包信息 ////
