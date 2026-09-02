# audience: internal
# # ios-cn-compatibility-patch
# 此模块对 CN iOS 1.8.4 的 AIR 打包保护, 雷霆登录界面, 抽卡资源预载和公网 IP 探测应用已确认补丁.
# 每项补丁使用唯一二进制上下文, 并同时接受原始状态和已应用状态.

from __future__ import annotations

import hashlib
from dataclasses import dataclass
from typing import Any


# //// 表示一项带唯一上下文的等长二进制补丁 [@x380kkm 2026-07-23] ////
@dataclass(frozen=True)
class BinaryPatch:
    name: str
    preferred_offset: int
    source_window: bytes
    target_window: bytes
    change_offset: int
    change_length: int
# //// /表示一项带唯一上下文的等长二进制补丁 ////


# //// 定义 CN iOS 1.8.4 兼容补丁集 [@x380kkm 2026-07-23] ////
CN_1_8_4_COMPATIBILITY_PATCHES = (
    BinaryPatch(
        "air_packaging_guard",
        0xB00C,
        bytes.fromhex(
            "f32d5f95080080d2e9dd9752a9d5bb72090100b9fd7b42a9"
            "f44f41a9f657c3a8c0035fd6"
        ),
        bytes.fromhex(
            "f32d5f95080080d2e9dd9752a9d5bb721f2003d5fd7b42a9"
            "f44f41a9f657c3a8c0035fd6"
        ),
        16,
        4,
    ),
    BinaryPatch(
        "air_safe_fallthrough_guard_0008e6f0",
        0x8E6F0,
        bytes.fromhex(
            "c0035fd6080080d2e9dd9752a9d5bb72090100b9c0035fd6"
            "fd7bbfa9fd030091f9ffff97482103f0"
        ),
        bytes.fromhex(
            "c0035fd6080080d2e9dd9752a9d5bb721f2003d5c0035fd6"
            "fd7bbfa9fd030091f9ffff97482103f0"
        ),
        16,
        4,
    ),
    BinaryPatch(
        "air_safe_fallthrough_guard_00256b30",
        0x256B30,
        bytes.fromhex(
            "a2000054080080d2e9dd9752a9d5bb72090100b9fd7b42a9"
            "f44f41a9f657c3a8c0035fd67f6e0bb9"
        ),
        bytes.fromhex(
            "a2000054080080d2e9dd9752a9d5bb721f2003d5fd7b42a9"
            "f44f41a9f657c3a8c0035fd67f6e0bb9"
        ),
        16,
        4,
    ),
    BinaryPatch(
        "air_bundle_identifier_resource_check",
        0x312230,
        bytes.fromhex("fc6fbda9f44f01a9fd7b02a9fd830091"),
        bytes.fromhex("000080d2c0035fd61f2003d51f2003d5"),
        0,
        16,
    ),
    BinaryPatch(
        "air_safe_fallthrough_guard_004932d4",
        0x4932D4,
        bytes.fromhex(
            "6af8ff17080080d2e9dd9752a9d5bb72090100b9c0035fd6"
            "ff8304d1fa670da9f85f0ea9f6570fa9"
        ),
        bytes.fromhex(
            "6af8ff17080080d2e9dd9752a9d5bb721f2003d5c0035fd6"
            "ff8304d1fa670da9f85f0ea9f6570fa9"
        ),
        16,
        4,
    ),
    BinaryPatch(
        "air_safe_fallthrough_guard_004ac098",
        0x4AC098,
        bytes.fromhex(
            "965e00b9080080d2e9dd9752a9d5bb72090100b9fd7b43a9"
            "f44f42a9f65741a9f85fc4a8c0035fd6"
        ),
        bytes.fromhex(
            "965e00b9080080d2e9dd9752a9d5bb721f2003d5fd7b43a9"
            "f44f42a9f65741a9f85fc4a8c0035fd6"
        ),
        16,
        4,
    ),
    BinaryPatch(
        "air_safe_fallthrough_guard_004ac578",
        0x4AC578,
        bytes.fromhex(
            "c0035fd6080080d2e9dd9752a9d5bb72090100b9c0035fd6"
            "fd7bbfa9fd0300916000039000e01891"
        ),
        bytes.fromhex(
            "c0035fd6080080d2e9dd9752a9d5bb721f2003d5c0035fd6"
            "fd7bbfa9fd0300916000039000e01891"
        ),
        16,
        4,
    ),
    BinaryPatch(
        "air_safe_fallthrough_guard_004d089c",
        0x4D089C,
        bytes.fromhex(
            "c0035fd6080080d2e9dd9752a9d5bb72090100b9c0035fd6"
            "fa67bba9f85f01a9f65702a9f44f03a9"
        ),
        bytes.fromhex(
            "c0035fd6080080d2e9dd9752a9d5bb721f2003d5c0035fd6"
            "fa67bba9f85f01a9f65702a9f44f03a9"
        ),
        16,
        4,
    ),
    BinaryPatch(
        "air_safe_fallthrough_guard_004def4c",
        0x4DEF4C,
        bytes.fromhex(
            "c0035fd6080080d2e9dd9752a9d5bb72090100b9c0035fd6"
            "e30302aa024c218b498c41f8490000b4"
        ),
        bytes.fromhex(
            "c0035fd6080080d2e9dd9752a9d5bb721f2003d5c0035fd6"
            "e30302aa024c218b498c41f8490000b4"
        ),
        16,
        4,
    ),
    BinaryPatch(
        "air_safe_fallthrough_guard_00520960",
        0x520960,
        bytes.fromhex(
            "c0035fd6080080d2e9dd9752a9d5bb72090100b9c0035fd6"
            "f657bda9f44f01a9fd7b02a9fd830091"
        ),
        bytes.fromhex(
            "c0035fd6080080d2e9dd9752a9d5bb721f2003d5c0035fd6"
            "f657bda9f44f01a9fd7b02a9fd830091"
        ),
        16,
        4,
    ),
    BinaryPatch(
        "air_safe_fallthrough_guard_00533240",
        0x533240,
        bytes.fromhex(
            "c0035fd6080080d2e9dd9752a9d5bb72090100b9c0035fd6"
            "080080d2e9dd9752a9d5bb72"
        ),
        bytes.fromhex(
            "c0035fd6080080d2e9dd9752a9d5bb721f2003d5c0035fd6"
            "080080d2e9dd9752a9d5bb72"
        ),
        16,
        4,
    ),
    BinaryPatch(
        "air_safe_fallthrough_guard_00533254",
        0x533254,
        bytes.fromhex(
            "c0035fd6080080d2e9dd9752a9d5bb72090100b9c0035fd6"
            "c0035fd6e00000b4e10300aa08cc7492"
        ),
        bytes.fromhex(
            "c0035fd6080080d2e9dd9752a9d5bb721f2003d5c0035fd6"
            "c0035fd6e00000b4e10300aa08cc7492"
        ),
        16,
        4,
    ),
    BinaryPatch(
        "gdpr_privacy_view",
        0x60F15C,
        bytes.fromhex("f44fc2a8901d4715f44fbea9fd7b01a9fd43009128f002d0"),
        bytes.fromhex("f44fc2a8901d471500008052c0035fd6fd43009128f002d0"),
        8,
        8,
    ),
    BinaryPatch(
        "login_welcome_view_call",
        0x634890,
        bytes.fromhex("1f2003d502d945f9f4da469508ef02b0009545f9"),
        bytes.fromhex("1f2003d502d945f91f2003d508ef02b0009545f9"),
        8,
        4,
    ),
    BinaryPatch(
        "lt_welcome_view",
        0x64B238,
        bytes.fromhex("c0035fd6c0035fd6ff0307d1ef3b126ded33136deb2b146d"),
        bytes.fromhex("c0035fd6c0035fd6000080d2c0035fd6ed33136deb2b146d"),
        8,
        8,
    ),
    BinaryPatch(
        "login_token_presence",
        0x68E878,
        bytes.fromhex("fd031daa31274595200800b4f50300aae00314aa"),
        bytes.fromhex("fd031daa312745951f2003d5f50300aae00314aa"),
        8,
        4,
    ),
    BinaryPatch(
        "login_token_length",
        0x68E898,
        bytes.fromhex("f60300aa9b3a4595a00600b4e00314aac8594595"),
        bytes.fromhex("f60300aa9b3a45951f2003d5e00314aac8594595"),
        8,
        4,
    ),
    BinaryPatch(
        "login_privacy_gate",
        0x68E8D8,
        bytes.fromhex("e00315aab11f45953805003548ec029000ad47f9"),
        bytes.fromhex("e00315aab11f45951f2003d548ec029000ad47f9"),
        8,
        4,
    ),
    BinaryPatch(
        "leiting_privacy_view",
        0x698990,
        bytes.fromhex("94223b91f6ffff17f44fbea9fd7b01a9fd430091f3eb02d0"),
        bytes.fromhex("94223b91f6ffff1700008052c0035fd6fd430091f3eb02d0"),
        8,
        8,
    ),
    BinaryPatch(
        "login_manager_welcome",
        0x6ADB14,
        bytes.fromhex("f85fc4a822a34415ff4301d1f44f03a9fd7b04a9fd030191"),
        bytes.fromhex("f85fc4a822a34415000080d2c0035fd6fd7b04a9fd030191"),
        8,
        8,
    ),
    BinaryPatch(
        "first_login_tip",
        0x6AE0DC,
        bytes.fromhex("14000052f4ffff17f657bda9f44f01a9fd7b02a9fd830091"),
        bytes.fromhex("14000052f4ffff1700008052c0035fd6fd7b02a9fd830091"),
        8,
        8,
    ),
    BinaryPatch(
        "login_license_gate",
        0x6C6CFC,
        bytes.fromhex("42203d91325644950007003797ea02b0e0f645f9"),
        bytes.fromhex("42203d91325644953800001497ea02b0e0f645f9"),
        8,
        4,
    ),
    BinaryPatch(
        "gacha_equipment_odds_preload",
        0x3E9864C,
        bytes.fromhex(
            "40013fd6e8ab41f9e09b01fde898f0b4e4030032e8ab41f9"
            "090940f9295140f92a0940f9"
        ),
        bytes.fromhex(
            "40013fd6e8ab41f9e09b01fde898f0b4e4031f2ae8ab41f9"
            "090940f9295140f92a0940f9"
        ),
        16,
        4,
    ),
    BinaryPatch(
        "public_ip_probe",
        0x5884229,
        b"https://pv.sohu.com/cityjson?ie=utf-8",
        b"http://127.0.0.1:17171/cityjson?ie=u8",
        0,
        37,
    ),
    BinaryPatch(
        "device_ip_country_probe",
        0x588D139,
        b"http://www.ip138.com/ips138.asp?ip=%@&action=2",
        b"http://127.0.0.1:17171/ips138.asp?ip=%@&x=0000",
        0,
        46,
    ),
    BinaryPatch(
        "sobot_qq_support_link",
        0x589416C,
        b"http://wpa.qq.com/msgrd?v=3&uin=%@&site=qq&menu=yes",
        b"http://127.1:17171/msgrd?v=3&uin=%@&site=q&menu=yes",
        0,
        51,
    ),
)
# //// /定义 CN iOS 1.8.4 兼容补丁集 ////


# //// 验证补丁只改动声明的等长区间 [@x380kkm 2026-07-23] ////
def validate_binary_patch(patch: BinaryPatch) -> None:
    if len(patch.source_window) != len(patch.target_window):
        raise ValueError(f"二进制补丁窗口长度不同: patch={patch.name}.")
    change_end = patch.change_offset + patch.change_length
    if (
        patch.change_offset < 0
        or change_end > len(patch.source_window)
        or patch.preferred_offset < patch.change_offset
    ):
        raise ValueError(f"二进制补丁区间越界: patch={patch.name}.")
    if (
        patch.source_window[: patch.change_offset]
        != patch.target_window[: patch.change_offset]
        or patch.source_window[change_end:] != patch.target_window[change_end:]
        or patch.source_window[patch.change_offset:change_end]
        == patch.target_window[patch.change_offset:change_end]
    ):
        raise ValueError(f"二进制补丁改动超出声明区间: patch={patch.name}.")
# //// /验证补丁只改动声明的等长区间 ////


# //// 查找二进制模式的全部偏移 [@x380kkm 2026-07-23] ////
def find_all_offsets(data: bytes, pattern: bytes) -> list[int]:
    offsets: list[int] = []
    offset = data.find(pattern)
    while offset >= 0:
        offsets.append(offset)
        offset = data.find(pattern, offset + 1)
    return offsets
# //// /查找二进制模式的全部偏移 ////


# //// 应用或确认一项唯一二进制补丁 [@x380kkm 2026-07-23] ////
def apply_binary_patch(
    data: bytes, patch: BinaryPatch
) -> tuple[bytes, dict[str, Any]]:
    validate_binary_patch(patch)
    preferred_window_offset = patch.preferred_offset - patch.change_offset
    preferred_window = data[
        preferred_window_offset : preferred_window_offset + len(patch.source_window)
    ]
    if preferred_window == patch.source_window:
        source_offsets = [preferred_window_offset]
        target_offsets: list[int] = []
        located_by = "preferred_offset"
    elif preferred_window == patch.target_window:
        source_offsets = []
        target_offsets = [preferred_window_offset]
        located_by = "preferred_offset"
    else:
        source_offsets = find_all_offsets(data, patch.source_window)
        target_offsets = find_all_offsets(data, patch.target_window)
        located_by = "unique_search"
    if len(source_offsets) == 1 and not target_offsets:
        window_offset = source_offsets[0]
        patched = bytearray(data)
        patched[
            window_offset : window_offset + len(patch.target_window)
        ] = patch.target_window
        patched_data = bytes(patched)
        status = "applied"
    elif not source_offsets and len(target_offsets) == 1:
        window_offset = target_offsets[0]
        patched_data = data
        status = "already_applied"
    else:
        raise ValueError(
            "CN iOS 兼容补丁上下文数量不正确: "
            f"patch={patch.name}, source={len(source_offsets)}, "
            f"target={len(target_offsets)}."
        )

    change_end = patch.change_offset + patch.change_length
    source_bytes = patch.source_window[patch.change_offset:change_end]
    target_bytes = patch.target_window[patch.change_offset:change_end]
    return patched_data, {
        "patch": patch.name,
        "status": status,
        "located_by": located_by,
        "preferred_offset": patch.preferred_offset,
        "offset": window_offset + patch.change_offset,
        "bytes": patch.change_length,
        "source_sha256": hashlib.sha256(source_bytes).hexdigest(),
        "target_sha256": hashlib.sha256(target_bytes).hexdigest(),
        "source_window_sha256": hashlib.sha256(patch.source_window).hexdigest(),
        "target_window_sha256": hashlib.sha256(patch.target_window).hexdigest(),
    }
# //// /应用或确认一项唯一二进制补丁 ////


# //// 应用 CN iOS 1.8.4 兼容补丁集 [@x380kkm 2026-07-23] ////
def patch_cn_1_8_4_compatibility(
    data: bytes,
) -> tuple[bytes, list[dict[str, Any]]]:
    patched = data
    evidence: list[dict[str, Any]] = []
    for patch in CN_1_8_4_COMPATIBILITY_PATCHES:
        patched, item = apply_binary_patch(patched, patch)
        evidence.append(item)
    return patched, evidence
# //// /应用 CN iOS 1.8.4 兼容补丁集 ////
