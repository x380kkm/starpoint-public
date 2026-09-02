# audience: internal
# # ios-cn-aot-patch
# 此模块等长替换 AIR iOS AOT 字符串池中的 CN API, 版本, 雷霆 SDK 和 Sobot 端点.
# 每类已知端点必须符合固定数量, 原始地址与 Emulator 代理地址不能混用.

from __future__ import annotations

import hashlib
import re
from dataclasses import dataclass
from typing import Any


PERSONAL_SERVICE_AUTHORITY = "127.0.0.1:17171"
VERSION_PATH = "/shijtswy/version/"
CN_API_SOURCE = "https://shijtswygamegf.leiting.com"
CN_API_STRING_POOL_PATTERN = b"\x05https\x1ashijtswygamegf.leiting.com"
CN_PRIMARY_VERSION_CANDIDATES = (
    b"https://update.leiting.com/shijtswy/version/",
    b"http://00000@10.0.2.2:8001/shijtswy/version/",
)
CN_BACKUP_VERSION_CANDIDATES = (
    b"https://update.roguelike.com/shijtswy/version/",
    b"http://0000000@10.0.2.2:8001/shijtswy/version/",
)
SDK_PROXY_PATTERN = re.compile(rb"http://(0*)@10\.0\.2\.2:8001")
ORIGINAL_SDK_AUTHORITY_PATTERN = re.compile(
    rb"https?://(?:[a-z0-9-]+\.)*(?:leiting\.com|roguelike\.com)(?::[0-9]{1,5})?"
)
EXPECTED_SDK_URL_COUNT = 148
SOBOT_AUTHORITY_PATTERN = re.compile(
    rb"https?://(?P<host>api\.sobot\.com|img\.sobot\.com|www\.sobot\.com)"
)
PORT_ONE_SOBOT_PROXY_PATTERN = re.compile(
    rb"http://00@127\.0\.0\.1:1(?![0-9])"
)
SOBOT_LOOPBACK_PATTERN = re.compile(rb"http://(0*)@127\.1:17171")
EXPECTED_SOBOT_AUTHORITY_COUNTS = {
    "api.sobot.com": 1,
    "img.sobot.com": 2,
    "www.sobot.com": 1,
}


# //// 表示一类雷霆 SDK 地址来源 [@x380kkm 2026-07-23] ////
@dataclass(frozen=True)
class SdkUrlSource:
    mode: str
    description: str
    matches: tuple[re.Match[bytes], ...]
    zero_padding_counts: tuple[int, ...] | None
# //// /表示一类雷霆 SDK 地址来源 ////


def pad_authority_with_userinfo(authority: str, target_length: int) -> bytes:
    encoded_authority = f"@{authority}".encode("ascii")
    padding_length = target_length - len(encoded_authority)
    if padding_length < 0:
        raise ValueError(
            f"目标 authority 过长, need={len(encoded_authority)}, available={target_length}."
        )
    return b"0" * padding_length + encoded_authority


def build_loopback_url(total_length: int) -> bytes:
    base = f"http://@{PERSONAL_SERVICE_AUTHORITY}{VERSION_PATH}".encode("ascii")
    padding_length = total_length - len(base)
    if padding_length < 0:
        raise ValueError(
            f"目标版本 URL 过长, need={len(base)}, available={total_length}."
        )
    return b"http://" + b"0" * padding_length + base[len(b"http://") :]


def find_unique_candidate(
    data: bytes, candidates: tuple[bytes, ...], endpoint_name: str
) -> tuple[bytes, int]:
    matches: list[tuple[bytes, int]] = []
    for candidate in candidates:
        offset = data.find(candidate)
        while offset >= 0:
            matches.append((candidate, offset))
            offset = data.find(candidate, offset + 1)
    if len(matches) != 1:
        raise ValueError(f"{endpoint_name} 端点数量不正确: count={len(matches)}.")
    return matches[0]


def replace_equal_length(
    data: bytes, offset: int, source: bytes, target: bytes
) -> bytes:
    if len(source) != len(target):
        raise ValueError(
            f"AOT 字符串替换长度不同: source={len(source)}, target={len(target)}."
        )
    patched = bytearray(data)
    patched[offset : offset + len(source)] = target
    return bytes(patched)


def replacement_evidence(
    endpoint_name: str,
    offset: int,
    source: bytes,
    target: bytes,
    source_url: str,
    target_url: str,
) -> dict[str, Any]:
    return {
        "endpoint": endpoint_name,
        "offset": offset,
        "bytes": len(source),
        "source": source_url,
        "target": target_url,
        "source_sha256": hashlib.sha256(source).hexdigest(),
        "target_sha256": hashlib.sha256(target).hexdigest(),
    }


# //// 构造与源地址等长的 loopback authority [@x380kkm 2026-07-23] ////
def build_loopback_authority_prefix(total_length: int) -> bytes:
    scheme = b"http://"
    target_authority = PERSONAL_SERVICE_AUTHORITY.encode("ascii")
    available_authority_length = total_length - len(scheme)
    if available_authority_length == len(target_authority):
        return scheme + target_authority
    return scheme + pad_authority_with_userinfo(
        PERSONAL_SERVICE_AUTHORITY, available_authority_length
    )
# //// /构造与源地址等长的 loopback authority ////


# //// 选择唯一一种雷霆 SDK 地址来源 [@x380kkm 2026-07-23] ////
def select_cn_sdk_url_matches(
    data: bytes, expected_count: int
) -> SdkUrlSource:
    original_matches = tuple(ORIGINAL_SDK_AUTHORITY_PATTERN.finditer(data))
    proxy_matches = tuple(SDK_PROXY_PATTERN.finditer(data))
    if len(original_matches) == expected_count and not proxy_matches:
        return SdkUrlSource(
            mode="original_authorities",
            description="https?://<leiting-or-roguelike-authority>",
            matches=original_matches,
            zero_padding_counts=None,
        )
    if len(proxy_matches) == expected_count and not original_matches:
        return SdkUrlSource(
            mode="emulator_proxy",
            description="http://<padding>@10.0.2.2:8001",
            matches=proxy_matches,
            zero_padding_counts=tuple(
                len(match.group(1)) for match in proxy_matches
            ),
        )
    raise ValueError(
        "CN SDK URL 数量不正确: "
        f"expected={expected_count}, original={len(original_matches)}, "
        f"proxy={len(proxy_matches)}."
    )
# //// /选择唯一一种雷霆 SDK 地址来源 ////


# //// 等长替换雷霆 SDK 原始或代理地址 [@x380kkm 2026-07-23] ////
def patch_cn_sdk_urls(
    data: bytes, expected_count: int = EXPECTED_SDK_URL_COUNT
) -> tuple[bytes, dict[str, Any]]:
    url_source = select_cn_sdk_url_matches(data, expected_count)

    patched = bytearray(data)
    source_values: list[bytes] = []
    target_values: list[bytes] = []
    offsets: list[int] = []
    for match in url_source.matches:
        source = match.group(0)
        target = build_loopback_authority_prefix(len(source))
        if len(source) != len(target):
            raise ValueError(
                f"SDK URL 替换长度不同: source={len(source)}, target={len(target)}."
            )
        patched[match.start() : match.end()] = target
        source_values.append(source)
        target_values.append(target)
        offsets.append(match.start())

    patched_bytes = bytes(patched)
    if (
        SDK_PROXY_PATTERN.search(patched_bytes) is not None
        or ORIGINAL_SDK_AUTHORITY_PATTERN.search(patched_bytes) is not None
    ):
        raise ValueError("CN SDK URL 写入后仍存在旧地址.")
    for offset, target in zip(offsets, target_values):
        if patched_bytes[offset : offset + len(target)] != target:
            raise ValueError(f"CN SDK URL 写入后无法复读: offset={offset}.")

    offsets_text = ",".join(str(offset) for offset in offsets).encode("ascii")
    evidence = {
        "endpoint": "sdk_urls",
        "source_mode": url_source.mode,
        "count": len(url_source.matches),
        "first_offset": offsets[0],
        "last_offset": offsets[-1],
        "minimum_source_bytes": min(map(len, source_values)),
        "maximum_source_bytes": max(map(len, source_values)),
        "source": url_source.description,
        "target": (
            f"http://<optional-padding-and-userinfo>{PERSONAL_SERVICE_AUTHORITY}"
        ),
        "offsets_sha256": hashlib.sha256(offsets_text).hexdigest(),
        "source_sha256": hashlib.sha256(b"\0".join(source_values)).hexdigest(),
        "target_sha256": hashlib.sha256(b"\0".join(target_values)).hexdigest(),
    }
    if url_source.zero_padding_counts is not None:
        evidence["minimum_zero_padding"] = min(url_source.zero_padding_counts)
        evidence["maximum_zero_padding"] = max(url_source.zero_padding_counts)
    return patched_bytes, evidence


# //// /等长替换雷霆 SDK 原始或代理地址 ////


# //// 构造与 Sobot authority 等长的 17171 loopback authority [@x380kkm 2026-08-22] ////
def build_sobot_loopback_authority_prefix(total_length: int) -> bytes:
    scheme = b"http://"
    authority = "127.1:17171"
    return scheme + pad_authority_with_userinfo(
        authority,
        total_length - len(scheme),
    )
# //// /构造与 Sobot authority 等长的 17171 loopback authority ////


# //// 选择 Sobot 原始 authority 或端口 1 代理 [@x380kkm 2026-08-22] ////
def select_sobot_authority_matches(
    data: bytes,
) -> tuple[str, tuple[re.Match[bytes], ...], dict[str, int]]:
    original_matches = tuple(SOBOT_AUTHORITY_PATTERN.finditer(data))
    port_one_matches = tuple(PORT_ONE_SOBOT_PROXY_PATTERN.finditer(data))
    expected_count = sum(EXPECTED_SOBOT_AUTHORITY_COUNTS.values())
    if len(original_matches) == expected_count and not port_one_matches:
        authority_counts = {
            authority: sum(
                match.group("host").decode("ascii") == authority
                for match in original_matches
            )
            for authority in EXPECTED_SOBOT_AUTHORITY_COUNTS
        }
        if authority_counts != EXPECTED_SOBOT_AUTHORITY_COUNTS:
            raise ValueError(
                f"Sobot authority 分布不正确: counts={authority_counts}."
            )
        return "original_authorities", original_matches, authority_counts
    if len(port_one_matches) == expected_count and not original_matches:
        return (
            "port_one_loopback",
            port_one_matches,
            dict(EXPECTED_SOBOT_AUTHORITY_COUNTS),
        )
    raise ValueError(
        "Sobot authority 数量不正确: "
        f"expected={expected_count}, original={len(original_matches)}, "
        f"port_one={len(port_one_matches)}."
    )
# //// /选择 Sobot 原始 authority 或端口 1 代理 ////


# //// 等长替换 Sobot authority 到个人服务 [@x380kkm 2026-08-22] ////
def patch_sobot_authorities(data: bytes) -> tuple[bytes, dict[str, Any]]:
    source_mode, matches, authority_counts = select_sobot_authority_matches(data)
    patched = bytearray(data)
    source_values: list[bytes] = []
    target_values: list[bytes] = []
    offsets: list[int] = []
    for match in matches:
        source = match.group(0)
        target = build_sobot_loopback_authority_prefix(len(source))
        patched[match.start() : match.end()] = target
        source_values.append(source)
        target_values.append(target)
        offsets.append(match.start())

    patched_bytes = bytes(patched)
    if (
        SOBOT_AUTHORITY_PATTERN.search(patched_bytes) is not None
        or PORT_ONE_SOBOT_PROXY_PATTERN.search(patched_bytes) is not None
    ):
        raise ValueError("Sobot authority 写入后仍存在旧地址.")
    if len(tuple(SOBOT_LOOPBACK_PATTERN.finditer(patched_bytes))) != len(matches):
        raise ValueError("Sobot authority 写入后无法完整复读.")

    offsets_text = ",".join(str(offset) for offset in offsets).encode("ascii")
    return patched_bytes, {
        "endpoint": "observed_third_party_urls",
        "source_mode": source_mode,
        "count": len(matches),
        "first_offset": offsets[0],
        "last_offset": offsets[-1],
        "minimum_source_bytes": min(map(len, source_values)),
        "maximum_source_bytes": max(map(len, source_values)),
        "source": (
            "https?://<api-or-image-or-web>.sobot.com"
            if source_mode == "original_authorities"
            else "http://<padding>@127.0.0.1:1"
        ),
        "target": "http://<optional-padding-and-userinfo>127.1:17171",
        "authority_counts": authority_counts,
        "offsets_sha256": hashlib.sha256(offsets_text).hexdigest(),
        "source_sha256": hashlib.sha256(b"\0".join(source_values)).hexdigest(),
        "target_sha256": hashlib.sha256(b"\0".join(target_values)).hexdigest(),
    }
# //// /等长替换 Sobot authority 到个人服务 ////


# //// 等长替换 CN iOS AOT 端点 [@x380kkm 2026-07-22] ////
def patch_cn_aot_endpoints(data: bytes) -> tuple[bytes, list[dict[str, Any]]]:
    api_source, api_offset = find_unique_candidate(
        data, (CN_API_STRING_POOL_PATTERN,), "CN API"
    )
    api_authority = pad_authority_with_userinfo(PERSONAL_SERVICE_AUTHORITY, 27)
    api_target = b"\x04http\x1b" + api_authority
    patched = replace_equal_length(data, api_offset, api_source, api_target)
    target_api_url = f"http://{api_authority.decode('ascii')}"
    evidence = [
        replacement_evidence(
            "api_server",
            api_offset,
            api_source,
            api_target,
            CN_API_SOURCE,
            target_api_url,
        )
    ]

    version_endpoints = (
        ("primary_version", CN_PRIMARY_VERSION_CANDIDATES),
        ("backup_version", CN_BACKUP_VERSION_CANDIDATES),
    )
    for endpoint_name, candidates in version_endpoints:
        source, offset = find_unique_candidate(patched, candidates, endpoint_name)
        target = build_loopback_url(len(source))
        patched = replace_equal_length(patched, offset, source, target)
        evidence.append(
            replacement_evidence(
                endpoint_name,
                offset,
                source,
                target,
                source.decode("ascii"),
                target.decode("ascii"),
            )
        )

    patched, sdk_evidence = patch_cn_sdk_urls(patched)
    evidence.append(sdk_evidence)
    patched, sobot_evidence = patch_sobot_authorities(patched)
    evidence.append(sobot_evidence)
    if patched.count(api_target) != 1:
        raise ValueError("CN API 目标端点写入后无法唯一复读.")
    for item in evidence[1:3]:
        if patched.count(item["target"].encode("ascii")) != 1:
            raise ValueError(f"{item['endpoint']} 目标端点写入后无法唯一复读.")
    return patched, evidence


# //// /等长替换 CN iOS AOT 端点 ////
