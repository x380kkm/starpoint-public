# audience: external
# # package-ios-personal-service
# 此程序把已构建的 arm64 Framework 加入解密 IPA, 注入加载命令, 并移除旧签名文件.
# CN 模式应用已确认的 1.8.4 兼容补丁, 并接受原始 SDK 地址或 Emulator 代理地址.
# 原始 SDK 地址与 Emulator 代理地址不能混用.
# CN CDN 根 path 清单按写入 App 的最终 ZIP 同步.
# 生成的卡池 banner 同时进入 CN CDN 和 App asset bundle.
# 输出宿主 App 保留 iPhone 方向并启用 iPad 多任务动态尺寸.
# 输出仍是未签名实验包, 不能直接安装到设备.
# 输出报告记录 App 内打开管理页面的手势.
# App 和内嵌 Framework 的版本来自同一个显式发布版本.

from __future__ import annotations

import argparse
import hashlib
import json
import os
import plistlib
import re
import shutil
import struct
import zipfile
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from cn_asset_path_manifest import (
    PATH_MANIFEST_NAME,
    render_synchronized_cn_asset_path_manifest,
)
from ios_cn_aot_patch import patch_cn_aot_endpoints
from ios_cn_compatibility_patch import patch_cn_1_8_4_compatibility
from ios_inventory import MH_MAGIC_64, analyze_macho, find_app_root
from ios_package_version import apply_package_version, create_package_version


CPU_TYPE_ARM64 = 0x0100000C
LC_LOAD_DYLIB = 0xC
LC_SEGMENT_64 = 0x19
MH_DYLIB = 6
MACH_HEADER_64_SIZE = 32
SECTION_64_SIZE = 80
DYLIB_COMMAND_HEADER_SIZE = 24
PERSONAL_SERVICE_PORT = 17171
MANAGEMENT_ACTIVATION_METHOD = "three_finger_long_press"
IOS_IPHONE_DEVICE_FAMILY = 1
IOS_IPAD_DEVICE_FAMILY = 2
IOS_IPAD_INTERFACE_ORIENTATIONS = (
    "UIInterfaceOrientationPortrait",
    "UIInterfaceOrientationPortraitUpsideDown",
    "UIInterfaceOrientationLandscapeLeft",
    "UIInterfaceOrientationLandscapeRight",
)
from cn_game_resource_overlay import PNG_FILELIST_ENTRY, merge_png_filelist, validate_gacha_banner_assets
CN_CDN_BUNDLE_ARCHIVE_PATH = "StarpointCNCDN"
CN_CDN_BUNDLE_INFO_PLIST_KEY = "StarpointCNCDNBundlePath"
CN_CDN_BUNDLE_MODE_INFO_PLIST_KEY = "StarpointCNCDNBundleMode"
CN_CDN_BUNDLE_MODE_DIRECT = "direct"
BUNDLE_IDENTIFIER_PATTERN = re.compile(r"^[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+$")


@dataclass(frozen=True)
class MachOLayout:
    command_count: int
    command_bytes: int
    command_end: int
    first_section_offset: int
    linked_dylibs: tuple[str, ...]

    @property
    def header_padding(self) -> int:
        return self.first_section_offset - self.command_end


# //// 表示一次客户端主二进制转换结果 [@x380kkm 2026-07-23] ////
@dataclass(frozen=True)
class ExecutableTransformation:
    data: bytes
    endpoint_replacements: tuple[dict[str, Any], ...]
    compatibility_patches: tuple[dict[str, Any], ...]
# //// /表示一次客户端主二进制转换结果 ////


ExecutableTransform = Callable[[bytes], ExecutableTransformation]


# //// 读取薄 arm64 Mach-O 的加载命令空间 [@x380kkm 2026-07-22] ////
def read_macho_layout(data: bytes) -> MachOLayout:
    if len(data) < MACH_HEADER_64_SIZE:
        raise ValueError("Mach-O 文件过短.")
    magic, cpu_type, _, _, command_count, command_bytes, _, _ = struct.unpack_from(
        "<IiiIIIII", data, 0
    )
    if magic != MH_MAGIC_64:
        raise ValueError("仅支持薄 64 位 Mach-O.")
    if cpu_type & 0xFFFFFFFF != CPU_TYPE_ARM64:
        raise ValueError("仅支持 arm64 Mach-O.")

    command_end = MACH_HEADER_64_SIZE + command_bytes
    if command_end > len(data):
        raise ValueError("Mach-O 加载命令超出文件范围.")
    command_offset = MACH_HEADER_64_SIZE
    section_offsets: list[int] = []
    linked_dylibs: list[str] = []
    for _ in range(command_count):
        if command_offset + 8 > command_end:
            raise ValueError("Mach-O 加载命令头超出声明范围.")
        command, command_size = struct.unpack_from("<II", data, command_offset)
        if command_size < 8 or command_offset + command_size > command_end:
            raise ValueError("Mach-O 加载命令大小无效.")
        if command == LC_LOAD_DYLIB:
            name_offset = struct.unpack_from("<I", data, command_offset + 8)[0]
            name_start = command_offset + name_offset
            name_end = data.find(b"\0", name_start, command_offset + command_size)
            if name_start >= command_offset + command_size or name_end < 0:
                raise ValueError("Mach-O dylib 名称无效.")
            linked_dylibs.append(data[name_start:name_end].decode("utf-8"))
        elif command == LC_SEGMENT_64:
            if command_size < 72:
                raise ValueError("Mach-O segment 命令过短.")
            section_count = struct.unpack_from("<I", data, command_offset + 64)[0]
            section_offset = command_offset + 72
            if (
                section_offset + section_count * SECTION_64_SIZE
                > command_offset + command_size
            ):
                raise ValueError("Mach-O section 表超出 segment 命令.")
            for index in range(section_count):
                file_offset = struct.unpack_from(
                    "<I", data, section_offset + index * SECTION_64_SIZE + 48
                )[0]
                if file_offset > 0:
                    section_offsets.append(file_offset)
        command_offset += command_size

    if command_offset != command_end:
        raise ValueError("Mach-O 加载命令总长度不一致.")
    if not section_offsets:
        raise ValueError("Mach-O 没有可定位的 section.")
    first_section_offset = min(section_offsets)
    if first_section_offset < command_end or first_section_offset > len(data):
        raise ValueError("Mach-O 首个 section offset 无效.")
    return MachOLayout(
        command_count=command_count,
        command_bytes=command_bytes,
        command_end=command_end,
        first_section_offset=first_section_offset,
        linked_dylibs=tuple(linked_dylibs),
    )


def align(value: int, alignment: int) -> int:
    return (value + alignment - 1) & ~(alignment - 1)


def build_load_dylib_command(load_path: str) -> bytes:
    encoded_path = load_path.encode("utf-8") + b"\0"
    command_size = align(DYLIB_COMMAND_HEADER_SIZE + len(encoded_path), 8)
    command = bytearray(command_size)
    struct.pack_into(
        "<IIIIII",
        command,
        0,
        LC_LOAD_DYLIB,
        command_size,
        DYLIB_COMMAND_HEADER_SIZE,
        0,
        0,
        0,
    )
    command[
        DYLIB_COMMAND_HEADER_SIZE : DYLIB_COMMAND_HEADER_SIZE + len(encoded_path)
    ] = encoded_path
    return bytes(command)


# //// 在 Mach-O 头部空白区加入 Framework 加载命令 [@x380kkm 2026-07-22] ////
def inject_load_dylib(data: bytes, load_path: str) -> tuple[bytes, MachOLayout]:
    layout = read_macho_layout(data)
    if load_path in layout.linked_dylibs:
        return data, layout

    command = build_load_dylib_command(load_path)
    if len(command) > layout.header_padding:
        raise ValueError(
            f"Mach-O header padding 不足, need={len(command)}, available={layout.header_padding}."
        )
    padding = data[layout.command_end : layout.command_end + len(command)]
    if any(padding):
        raise ValueError("Mach-O header padding 包含非零数据, 拒绝覆盖.")

    patched = bytearray(data)
    patched[layout.command_end : layout.command_end + len(command)] = command
    struct.pack_into("<I", patched, 16, layout.command_count + 1)
    struct.pack_into("<I", patched, 20, layout.command_bytes + len(command))
    patched_layout = read_macho_layout(patched)
    if load_path not in patched_layout.linked_dylibs:
        raise ValueError("Framework 加载命令写入后无法复读.")
    return bytes(patched), patched_layout


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def validate_framework(framework_path: Path) -> tuple[Path, dict[str, Any]]:
    if not framework_path.is_dir() or framework_path.suffix != ".framework":
        raise ValueError("--framework 必须指向 .framework 目录.")
    framework_binary = framework_path / framework_path.stem
    if not framework_binary.is_file():
        raise ValueError(f"Framework 主二进制不存在: {framework_binary}.")
    analysis = analyze_macho(framework_binary.read_bytes())
    slices = analysis["slices"]
    if len(slices) != 1 or slices[0]["architecture"] != "arm64":
        raise ValueError("Framework 必须仅包含 arm64 iPhone slice.")
    if slices[0]["file_type"] != MH_DYLIB:
        raise ValueError("Framework 主二进制不是 Mach-O dylib.")
    if analysis["fairplay_encrypted"]:
        raise ValueError("Framework 主二进制不能包含加密 slice.")
    return framework_binary, analysis


def should_remove_original_entry(
    name: str,
    app_root: str,
    framework_name: str,
    cn_cdn_bundle_path: str,
) -> bool:
    signature_prefix = f"{app_root}/_CodeSignature/"
    framework_prefix = f"{app_root}/Frameworks/{framework_name}/"
    cn_cdn_prefix = f"{app_root}/{cn_cdn_bundle_path}/"
    return (
        name.startswith(signature_prefix)
        or name == f"{app_root}/embedded.mobileprovision"
        or name.startswith(framework_prefix)
        or name.startswith(cn_cdn_prefix)
    )


def add_framework_files(
    output: zipfile.ZipFile,
    framework_path: Path,
    framework_binary: Path,
    app_root: str,
    framework_info: bytes,
) -> None:
    for source in sorted(framework_path.rglob("*")):
        if source.is_symlink():
            raise ValueError(
                f"Framework 包含符号链接, 请先生成扁平 iOS Framework: {source}."
            )
        if not source.is_file():
            continue
        relative = source.relative_to(framework_path).as_posix()
        archive_path = f"{app_root}/Frameworks/{framework_path.name}/{relative}"
        archive_info = zipfile.ZipInfo(archive_path, date_time=(1980, 1, 1, 0, 0, 0))
        archive_info.create_system = 3
        mode = 0o100755 if source == framework_binary else 0o100644
        archive_info.external_attr = mode << 16
        archive_info.compress_type = zipfile.ZIP_DEFLATED
        data = framework_info if relative == "Info.plist" else source.read_bytes()
        output.writestr(archive_info, data)


def validate_cn_cdn_bundle(
    cdn_bundle_path: Path,
) -> tuple[list[Path], int, list[Path]]:
    if not cdn_bundle_path.is_dir():
        raise ValueError("--cn-cdn-bundle 必须指向目录.")
    gacha_banner_files = validate_gacha_banner_assets(cdn_bundle_path)
    files: list[Path] = []
    total_size = 0
    for source in sorted(cdn_bundle_path.rglob("*")):
        if source.is_symlink():
            raise ValueError(f"CN CDN 资源不能包含符号链接: {source}.")
        if not source.is_file():
            continue
        relative = source.relative_to(cdn_bundle_path).as_posix()
        if not relative or relative.startswith("../") or "/../" in f"/{relative}":
            raise ValueError(f"CN CDN 资源路径越界: {relative}.")
        files.append(source)
        total_size += source.stat().st_size
    if not files:
        raise ValueError("--cn-cdn-bundle 不能为空.")
    return files, total_size, gacha_banner_files


def add_cn_cdn_bundle_files(
    output: zipfile.ZipFile,
    cdn_bundle_path: Path,
    files: list[Path],
    app_root: str,
) -> int:
    synchronized_manifest = render_synchronized_cn_asset_path_manifest(cdn_bundle_path)
    total_size_delta = 0
    for source in files:
        relative = source.relative_to(cdn_bundle_path).as_posix()
        archive_path = f"{app_root}/{CN_CDN_BUNDLE_ARCHIVE_PATH}/{relative}"
        archive_info = zipfile.ZipInfo(archive_path, date_time=(1980, 1, 1, 0, 0, 0))
        archive_info.create_system = 3
        archive_info.external_attr = 0o100644 << 16
        archive_info.compress_type = (
            zipfile.ZIP_STORED
            if source.suffix.lower() == ".zip"
            else zipfile.ZIP_DEFLATED
        )
        if relative == PATH_MANIFEST_NAME and synchronized_manifest is not None:
            total_size_delta += len(synchronized_manifest) - source.stat().st_size
            output.writestr(archive_info, synchronized_manifest)
        else:
            archive_info.file_size = source.stat().st_size
            with source.open("rb") as stream, output.open(archive_info, "w") as destination:
                shutil.copyfileobj(stream, destination)
    return total_size_delta


# //// 将生成的卡池 banner 加入 App asset bundle [@x380kkm 2026-08-28] ////
def add_gacha_banner_bundle_files(
    output: zipfile.ZipFile,
    cdn_bundle_path: Path,
    banner_files: list[Path],
    app_root: str,
    original_filelist: bytes = b"",
) -> int:
    total_size = 0
    bundle_root = cdn_bundle_path / "production" / "bundle"
    for source in banner_files:
        relative = source.relative_to(bundle_root).as_posix()
        archive_path = f"{app_root}/asset/production/bundle/{relative}"
        archive_info = zipfile.ZipInfo(archive_path, date_time=(1980, 1, 1, 0, 0, 0))
        archive_info.create_system = 3
        archive_info.external_attr = 0o100644 << 16
        archive_info.compress_type = zipfile.ZIP_DEFLATED
        content = source.read_bytes()
        output.writestr(archive_info, content)
        total_size += len(content)
    if banner_files:
        hashed_paths = [source.relative_to(bundle_root).as_posix() for source in banner_files]
        filelist = merge_png_filelist(original_filelist, hashed_paths)
        output.writestr(f"{app_root}/asset/{PNG_FILELIST_ENTRY}", filelist)
    return total_size
# //// /将生成的卡池 banner 加入 App asset bundle ////


# //// 生成独立 bundle ID 的未签名 IPA [@x380kkm 2026-07-24] ////
def package_personal_service_ipa(
    input_ipa: Path,
    framework_path: Path,
    output_ipa: Path,
    bundle_id: str,
    display_name: str,
    release_version: str,
    cn_cdn_bundle_path: Path | None = None,
) -> dict[str, Any]:
    return package_transformed_personal_service_ipa(
        input_ipa,
        framework_path,
        output_ipa,
        bundle_id,
        display_name,
        release_version,
        preserve_client_executable,
        cn_cdn_bundle_path,
    )


def package_cn_personal_service_ipa(
    input_ipa: Path,
    framework_path: Path,
    output_ipa: Path,
    bundle_id: str,
    display_name: str,
    release_version: str,
    cn_cdn_bundle_path: Path | None = None,
) -> dict[str, Any]:
    return package_transformed_personal_service_ipa(
        input_ipa,
        framework_path,
        output_ipa,
        bundle_id,
        display_name,
        release_version,
        transform_cn_executable,
        cn_cdn_bundle_path,
    )


# //// 保持通用客户端主二进制内容 [@x380kkm 2026-07-23] ////
def preserve_client_executable(executable: bytes) -> ExecutableTransformation:
    return ExecutableTransformation(executable, (), ())
# //// /保持通用客户端主二进制内容 ////


# //// 转换 CN iOS 1.8.4 主二进制 [@x380kkm 2026-07-23] ////
def transform_cn_executable(executable: bytes) -> ExecutableTransformation:
    compatible, compatibility_patches = patch_cn_1_8_4_compatibility(executable)
    routed, endpoint_replacements = patch_cn_aot_endpoints(compatible)
    return ExecutableTransformation(
        data=routed,
        endpoint_replacements=tuple(endpoint_replacements),
        compatibility_patches=tuple(compatibility_patches),
    )
# //// /转换 CN iOS 1.8.4 主二进制 ////


# //// 设置宿主 App 的 iPad 多任务契约 [@x380kkm 2026-08-31] ////
def configure_ipad_multitasking_info(info: dict[str, Any]) -> None:
    info["UIDeviceFamily"] = [IOS_IPHONE_DEVICE_FAMILY, IOS_IPAD_DEVICE_FAMILY]
    info["UISupportedInterfaceOrientations~ipad"] = list(
        IOS_IPAD_INTERFACE_ORIENTATIONS
    )
    info["UIRequiresFullScreen"] = False
    launch_storyboard = info.get("UILaunchStoryboardName")
    if not isinstance(launch_storyboard, str) or not launch_storyboard.strip():
        launch_screen = info.get("UILaunchScreen")
        if not isinstance(launch_screen, dict):
            info["UILaunchScreen"] = {}
# //// /设置宿主 App 的 iPad 多任务契约 ////


# //// 重打包注入个人服务的 IPA [@x380kkm 2026-07-24] ////
def package_transformed_personal_service_ipa(
    input_ipa: Path,
    framework_path: Path,
    output_ipa: Path,
    bundle_id: str,
    display_name: str,
    release_version: str,
    transform_executable: ExecutableTransform,
    cn_cdn_bundle_path: Path | None = None,
) -> dict[str, Any]:
    if input_ipa.resolve() == output_ipa.resolve():
        raise ValueError("输入和输出 IPA 必须使用不同路径.")
    if not BUNDLE_IDENTIFIER_PATTERN.fullmatch(bundle_id):
        raise ValueError("--bundle-id 不是有效的 Apple bundle identifier.")
    if not display_name.strip():
        raise ValueError("--display-name 不能为空.")
    cn_cdn_files: list[Path] = []
    cn_cdn_total_size = 0
    gacha_banner_files: list[Path] = []
    if cn_cdn_bundle_path is not None:
        cn_cdn_files, cn_cdn_total_size, gacha_banner_files = validate_cn_cdn_bundle(
            cn_cdn_bundle_path
        )
    framework_binary, framework_analysis = validate_framework(framework_path)
    load_path = (
        f"@executable_path/Frameworks/{framework_path.name}/{framework_binary.name}"
    )
    output_ipa.parent.mkdir(parents=True, exist_ok=True)
    temporary_output = output_ipa.with_suffix(output_ipa.suffix + ".tmp")
    temporary_output.unlink(missing_ok=True)

    try:
        with zipfile.ZipFile(input_ipa) as source:
            app_root = find_app_root(source)
            info_path = f"{app_root}/Info.plist"
            info = plistlib.loads(source.read(info_path))
            executable_name = info.get("CFBundleExecutable")
            if not isinstance(executable_name, str) or not executable_name:
                raise ValueError("Info.plist 缺少 CFBundleExecutable.")
            if bundle_id == info.get("CFBundleIdentifier"):
                raise ValueError("实验 IPA 必须使用不同于原包的 bundle ID.")
            package_version = create_package_version(
                info.get("CFBundleShortVersionString", ""), release_version
            )
            framework_info = plistlib.loads((framework_path / "Info.plist").read_bytes())
            apply_package_version(info, package_version)
            apply_package_version(framework_info, package_version)
            updated_framework_info = plistlib.dumps(
                framework_info, fmt=plistlib.FMT_BINARY, sort_keys=False
            )
            executable_path = f"{app_root}/{executable_name}"
            executable = source.read(executable_path)
            executable_analysis = analyze_macho(executable)
            if executable_analysis["fairplay_encrypted"]:
                raise ValueError("IPA 主二进制仍受 FairPlay 加密, 不能注入.")
            if len(executable_analysis["slices"]) != 1:
                raise ValueError("当前注入器仅支持单一 arm64 slice 的 IPA.")
            transformation = transform_executable(executable)
            executable = transformation.data
            original_layout = read_macho_layout(executable)
            patched_executable, patched_layout = inject_load_dylib(
                executable, load_path
            )

            info["CFBundleIdentifier"] = bundle_id
            info["CFBundleDisplayName"] = display_name
            configure_ipad_multitasking_info(info)
            if cn_cdn_bundle_path is None:
                info.pop(CN_CDN_BUNDLE_INFO_PLIST_KEY, None)
                info.pop(CN_CDN_BUNDLE_MODE_INFO_PLIST_KEY, None)
            else:
                info[CN_CDN_BUNDLE_INFO_PLIST_KEY] = CN_CDN_BUNDLE_ARCHIVE_PATH
                info[CN_CDN_BUNDLE_MODE_INFO_PLIST_KEY] = CN_CDN_BUNDLE_MODE_DIRECT
            transport_security = info.setdefault("NSAppTransportSecurity", {})
            if not isinstance(transport_security, dict):
                raise ValueError("NSAppTransportSecurity 必须是字典.")
            transport_security["NSAllowsLocalNetworking"] = True
            updated_info = plistlib.dumps(
                info, fmt=plistlib.FMT_BINARY, sort_keys=False
            )
            embedded_gacha_banner_paths = {
                (
                    f"{app_root}/asset/production/bundle/"
                    f"{source.relative_to(cn_cdn_bundle_path / 'production' / 'bundle').as_posix()}"
                )
                for source in gacha_banner_files
            }
            filelist_path = f"{app_root}/asset/{PNG_FILELIST_ENTRY}"
            original_filelist = source.read(filelist_path) if filelist_path in source.namelist() else b""
            if gacha_banner_files:
                embedded_gacha_banner_paths.add(filelist_path)

            with zipfile.ZipFile(temporary_output, "w") as output:
                for entry in source.infolist():
                    if should_remove_original_entry(
                        entry.filename,
                        app_root,
                        framework_path.name,
                        CN_CDN_BUNDLE_ARCHIVE_PATH,
                    ) or entry.filename in embedded_gacha_banner_paths:
                        continue
                    if entry.filename == executable_path:
                        output.writestr(entry, patched_executable)
                    elif entry.filename == info_path:
                        output.writestr(entry, updated_info)
                    else:
                        output.writestr(entry, source.read(entry.filename))
                add_framework_files(
                    output, framework_path, framework_binary, app_root, updated_framework_info
                )
                if cn_cdn_bundle_path is not None:
                    cn_cdn_total_size += add_cn_cdn_bundle_files(
                        output,
                        cn_cdn_bundle_path,
                        cn_cdn_files,
                        app_root,
                    )
                    embedded_gacha_banner_size = add_gacha_banner_bundle_files(
                        output,
                        cn_cdn_bundle_path,
                        gacha_banner_files,
                        app_root,
                        original_filelist,
                    )
                else:
                    embedded_gacha_banner_size = 0
        os.replace(temporary_output, output_ipa)
    except BaseException:
        temporary_output.unlink(missing_ok=True)
        raise

    return {
        "schema_version": 4,
        "input_ipa": str(input_ipa.resolve()),
        "output_ipa": str(output_ipa.resolve()),
        "output_sha256": sha256_file(output_ipa),
        "bundle_id": bundle_id,
        "display_name": display_name,
        "release_version": package_version.release,
        "bundle_version": package_version.bundle,
        "build_version": package_version.bundle,
        "framework": framework_path.name,
        "framework_bundle_version": framework_info["CFBundleShortVersionString"],
        "framework_build_version": framework_info["CFBundleVersion"],
        "framework_sha256": sha256_file(framework_binary),
        "framework_architecture": framework_analysis["slices"][0]["architecture"],
        "load_path": load_path,
        "header_padding_before": original_layout.header_padding,
        "header_padding_after": patched_layout.header_padding,
        "requires_resigning": True,
        "installable": False,
        "personal_service_port": PERSONAL_SERVICE_PORT,
        "management_activation_method": MANAGEMENT_ACTIVATION_METHOD,
        "cn_endpoint_replacements": list(transformation.endpoint_replacements),
        "cn_compatibility_patches": list(transformation.compatibility_patches),
        "cn_cdn_bundle_path": (
            CN_CDN_BUNDLE_ARCHIVE_PATH if cn_cdn_bundle_path is not None else None
        ),
        "cn_cdn_bundle_mode": (
            CN_CDN_BUNDLE_MODE_DIRECT if cn_cdn_bundle_path is not None else None
        ),
        "cn_cdn_file_count": len(cn_cdn_files),
        "cn_cdn_total_size": cn_cdn_total_size,
        "embedded_gacha_banner_count": len(gacha_banner_files),
        "embedded_gacha_banner_size": embedded_gacha_banner_size,
    }
# //// /重打包注入个人服务的 IPA ////


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="把个人服务 Framework 注入解密 IPA, 输出未签名实验包."
    )
    parser.add_argument("input_ipa", type=Path)
    parser.add_argument("--framework", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--bundle-id", required=True)
    parser.add_argument("--display-name", required=True)
    parser.add_argument("--release-version", required=True)
    parser.add_argument("--patch-cn-endpoints", action="store_true")
    parser.add_argument("--cn-cdn-bundle", type=Path)
    parser.add_argument("--report", type=Path)
    return parser.parse_args()


def write_report(report_path: Path, result: dict[str, Any]) -> None:
    report_path.parent.mkdir(parents=True, exist_ok=True)
    temporary_path = report_path.with_suffix(report_path.suffix + ".tmp")
    try:
        temporary_path.write_text(
            json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        os.replace(temporary_path, report_path)
    except BaseException:
        temporary_path.unlink(missing_ok=True)
        raise


def main() -> None:
    args = parse_args()
    package_ipa = (
        package_cn_personal_service_ipa
        if args.patch_cn_endpoints
        else package_personal_service_ipa
    )
    result = package_ipa(
        args.input_ipa,
        args.framework,
        args.output,
        args.bundle_id,
        args.display_name,
        args.release_version,
        args.cn_cdn_bundle,
    )
    if args.report is not None:
        write_report(args.report, result)
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
# //// /生成独立 bundle ID 的未签名 IPA ////
