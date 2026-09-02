# audience: external
# # ios-inventory
# 此程序读取 IPA 的 Info.plist, Mach-O 架构, 最低系统, 加密标记, 代码签名段和 SWF 清单.
# 此程序不修改 IPA, 输出用于判断文件能否直接以开发者证书重新签名.

from __future__ import annotations

import argparse
import hashlib
import json
import plistlib
import struct
import zipfile
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any


MH_MAGIC_64 = 0xFEEDFACF
FAT_MAGIC = 0xCAFEBABE
FAT_MAGIC_64 = 0xCAFEBABF
LC_LOAD_DYLIB = 0xC
LC_LOAD_WEAK_DYLIB = 0x80000018
LC_REEXPORT_DYLIB = 0x8000001F
LC_LOAD_UPWARD_DYLIB = 0x80000023
LC_CODE_SIGNATURE = 0x1D
LC_VERSION_MIN_IPHONEOS = 0x25
LC_ENCRYPTION_INFO = 0x21
LC_ENCRYPTION_INFO_64 = 0x2C
LC_BUILD_VERSION = 0x32
CPU_TYPES = {
    0x0100000C: "arm64",
    0x0200000C: "arm64_32",
    0x01000007: "x86_64",
}
PLATFORMS = {
    2: "iOS",
    6: "Mac Catalyst",
    7: "iOS Simulator",
}


# //// 表示一个 Mach-O 切片的文件范围 [@x380kkm 2026-07-20] ////
@dataclass(frozen=True)
class MachOSlice:
    offset: int
    size: int


# //// /表示一个 Mach-O 切片的文件范围 ////


# //// 把 Mach-O packed version 转换为可读版本 [@x380kkm 2026-07-20] ////
def render_version(value: int) -> str:
    return f"{value >> 16}.{(value >> 8) & 0xFF}.{value & 0xFF}"


# //// /把 Mach-O packed version 转换为可读版本 ////


# //// 读取通用二进制中的 Mach-O 切片 [@x380kkm 2026-07-20] ////
def find_macho_slices(data: bytes) -> list[MachOSlice]:
    if len(data) < 8:
        raise ValueError("Mach-O 文件过短.")
    big_magic = struct.unpack_from(">I", data, 0)[0]
    if big_magic not in {FAT_MAGIC, FAT_MAGIC_64}:
        return [MachOSlice(0, len(data))]

    architecture_count = struct.unpack_from(">I", data, 4)[0]
    entry_size = 32 if big_magic == FAT_MAGIC_64 else 20
    slices: list[MachOSlice] = []
    for index in range(architecture_count):
        entry_offset = 8 + index * entry_size
        if entry_size == 20:
            _, _, offset, size, _ = struct.unpack_from(">IIIII", data, entry_offset)
        else:
            _, _, offset, size, _, _ = struct.unpack_from(">IIQQII", data, entry_offset)
        slices.append(MachOSlice(offset, size))
    return slices


# //// /读取通用二进制中的 Mach-O 切片 ////


# //// 读取一个 64 位 Mach-O 切片的加载命令 [@x380kkm 2026-07-20] ////
def analyze_macho_slice(data: bytes, macho_slice: MachOSlice) -> dict[str, Any]:
    offset = macho_slice.offset
    if struct.unpack_from("<I", data, offset)[0] != MH_MAGIC_64:
        raise ValueError(f"不支持的 Mach-O magic, offset={offset}.")
    _, cpu_type, cpu_subtype, file_type, command_count, command_bytes, flags, _ = struct.unpack_from(
        "<IiiIIIII", data, offset
    )
    command_offset = offset + 32
    encryption: dict[str, Any] | None = None
    code_signature: dict[str, Any] | None = None
    minimum_ios: str | None = None
    build_version: dict[str, Any] | None = None
    dylibs: list[str] = []

    for _ in range(command_count):
        command, command_size = struct.unpack_from("<II", data, command_offset)
        if command_size < 8 or command_offset + command_size > offset + macho_slice.size:
            raise ValueError(f"无效 Mach-O load command, offset={command_offset}.")
        if command in {LC_ENCRYPTION_INFO, LC_ENCRYPTION_INFO_64}:
            crypt_offset, crypt_size, crypt_id = struct.unpack_from("<III", data, command_offset + 8)
            encryption = {
                "offset": crypt_offset,
                "size": crypt_size,
                "cryptid": crypt_id,
                "encrypted": crypt_id != 0,
            }
        elif command == LC_CODE_SIGNATURE:
            data_offset, data_size = struct.unpack_from("<II", data, command_offset + 8)
            code_signature = {"offset": data_offset, "size": data_size}
        elif command == LC_VERSION_MIN_IPHONEOS:
            version, _ = struct.unpack_from("<II", data, command_offset + 8)
            minimum_ios = render_version(version)
        elif command == LC_BUILD_VERSION:
            platform, minimum, sdk, tool_count = struct.unpack_from("<IIII", data, command_offset + 8)
            build_version = {
                "platform": PLATFORMS.get(platform, str(platform)),
                "minimum_os": render_version(minimum),
                "sdk": render_version(sdk),
                "tool_count": tool_count,
            }
        elif command in {LC_LOAD_DYLIB, LC_LOAD_WEAK_DYLIB, LC_REEXPORT_DYLIB, LC_LOAD_UPWARD_DYLIB}:
            name_offset = struct.unpack_from("<I", data, command_offset + 8)[0]
            name_start = command_offset + name_offset
            name_end = data.find(b"\0", name_start, command_offset + command_size)
            if name_end >= 0:
                dylibs.append(data[name_start:name_end].decode("utf-8", errors="replace"))
        command_offset += command_size

    return {
        "offset": offset,
        "size": macho_slice.size,
        "architecture": CPU_TYPES.get(cpu_type & 0xFFFFFFFF, hex(cpu_type & 0xFFFFFFFF)),
        "cpu_subtype": cpu_subtype,
        "file_type": file_type,
        "command_count": command_count,
        "command_bytes": command_bytes,
        "flags": flags,
        "minimum_ios": minimum_ios,
        "build_version": build_version,
        "encryption": encryption,
        "code_signature": code_signature,
        "linked_dylibs": dylibs,
    }


# //// /读取一个 64 位 Mach-O 切片的加载命令 ////


# //// 分析一个 Mach-O 文件的全部架构 [@x380kkm 2026-07-20] ////
def analyze_macho(data: bytes) -> dict[str, Any]:
    slices = [analyze_macho_slice(data, macho_slice) for macho_slice in find_macho_slices(data)]
    return {
        "sha256": hashlib.sha256(data).hexdigest(),
        "bytes": len(data),
        "slices": slices,
        "fairplay_encrypted": any(
            bool(slice_info["encryption"] and slice_info["encryption"]["encrypted"])
            for slice_info in slices
        ),
        "has_code_signature_command": any(slice_info["code_signature"] for slice_info in slices),
    }


# //// /分析一个 Mach-O 文件的全部架构 ////


# //// 定位 IPA 中唯一的 app bundle [@x380kkm 2026-07-20] ////
def find_app_root(archive: zipfile.ZipFile) -> PurePosixPath:
    roots = {
        PurePosixPath(name).parents[0]
        for name in archive.namelist()
        if name.startswith("Payload/") and name.endswith(".app/Info.plist")
    }
    if len(roots) != 1:
        raise ValueError(f"IPA 必须包含一个 app bundle, 实际为 {len(roots)}.")
    return roots.pop()


# //// /定位 IPA 中唯一的 app bundle ////


# //// 读取 IPA bundle, 签名, Mach-O 和 SWF 信息 [@x380kkm 2026-07-20] ////
def analyze_ipa(ipa_path: Path) -> dict[str, Any]:
    ipa_path = ipa_path.resolve()
    with zipfile.ZipFile(ipa_path) as archive:
        app_root = find_app_root(archive)
        info_path = app_root / "Info.plist"
        info = plistlib.loads(archive.read(str(info_path)))
        executable_name = info["CFBundleExecutable"]
        executable_path = app_root / executable_name
        executable = archive.read(str(executable_path))
        names = set(archive.namelist())
        swf_files = []
        for name in sorted(names):
            path = PurePosixPath(name)
            if app_root not in path.parents or name.endswith("/"):
                continue
            with archive.open(name) as source:
                prefix = source.read(3)
                if prefix not in {b"FWS", b"CWS", b"ZWS"}:
                    continue
                content = prefix + source.read()
            swf_files.append(
                {
                    "path": path.relative_to(app_root).as_posix(),
                    "bytes": len(content),
                    "sha256": hashlib.sha256(content).hexdigest(),
                    "signature": prefix.decode("ascii"),
                }
            )

        signature_prefix = str(app_root / "_CodeSignature") + "/"
        has_signature_directory = any(name.startswith(signature_prefix) for name in names)
        provision_path = str(app_root / "embedded.mobileprovision")
        selected_info = {
            key: info.get(key)
            for key in (
                "CFBundleIdentifier",
                "CFBundleExecutable",
                "CFBundleName",
                "CFBundleDisplayName",
                "CFBundleShortVersionString",
                "CFBundleVersion",
                "MinimumOSVersion",
                "UIDeviceFamily",
                "UIRequiredDeviceCapabilities",
                "DTPlatformName",
                "DTPlatformVersion",
                "DTSDKName",
                "DTXcode",
                "DTXcodeBuild",
            )
        }
        macho = analyze_macho(executable)
        return {
            "schema_version": 1,
            "ipa_path": str(ipa_path),
            "ipa_bytes": ipa_path.stat().st_size,
            "ipa_sha256": hashlib.sha256(ipa_path.read_bytes()).hexdigest(),
            "app_root": str(app_root),
            "info": selected_info,
            "macho": macho,
            "has_code_signature_directory": has_signature_directory,
            "has_embedded_mobileprovision": provision_path in names,
            "swf_files": swf_files,
            "can_resign_without_decryption": not macho["fairplay_encrypted"],
        }


# //// /读取 IPA bundle, 签名, Mach-O 和 SWF 信息 ////


# //// 解析命令行并写入 IPA 分析结果 [@x380kkm 2026-07-20] ////
def main() -> None:
    parser = argparse.ArgumentParser(description="检查 IPA 的架构, 加密, 签名和 SWF 资源.")
    parser.add_argument("--ipa", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    result = analyze_ipa(arguments.ipa)
    arguments.output.parent.mkdir(parents=True, exist_ok=True)
    arguments.output.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(result, ensure_ascii=False))


if __name__ == "__main__":
    main()
# //// /解析命令行并写入 IPA 分析结果 ////
