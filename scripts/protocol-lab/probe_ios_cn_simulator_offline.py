# audience: internal
# # probe-ios-cn-simulator-offline
# 此程序在一台无头 Simulator 中验证真实 CN 候选启动, loopback 管理页和进程日志零非 loopback URL.
# Simulator 生命周期只通过 xcrun simctl 管理, 程序创建的设备在报告写入前删除.

from __future__ import annotations

import argparse
import hashlib
import json
import os
import plistlib
import re
import shutil
import struct
import subprocess
import sys
import time
import urllib.request
import zipfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Optional
from urllib.parse import urlsplit


MH_MAGIC_64 = 0xFEEDFACF
LC_BUILD_VERSION = 0x32
PLATFORM_IOS = 2
PLATFORM_IOS_SIMULATOR = 7
MACH_HEADER_64_SIZE = 32
STARTUP_GUARD_OFFSET = 3_221_360
STARTUP_GUARD_SOURCE = bytes.fromhex("d7040034")
STARTUP_GUARD_TARGET = bytes.fromhex("c0040034")
NETWORK_URL_PATTERN = re.compile(r"https?://[^\s\]\[\)\(\}\{<>'\"]+", re.IGNORECASE)
LOOPBACK_HOSTS = {"127.0.0.1", "127.1", "localhost", "::1"}
BUNDLED_CDN_DIRECTORY = "StarpointCNCDN"
HTTP_OBSERVATIONS_EXPORTER = "export-ios-simulator-http-observations.py"
PASSIVE_APP_REQUIRED_REQUESTS = (
    ("POST", "/sync_data"),
    ("GET", "/chat-sdk/sdk/user/v2/config.action"),
    ("POST", "/chat-sdk/sdk/user/v2/appInit.action"),
    ("GET", "/wf/210009_config_20200415.json"),
)
RESOURCE_REQUEST_PREFIXES = ("/asset/", "/assets/", "/cdn/", "/ios/")
RESOURCE_REQUEST_SUFFIXES = (".bundle", ".csv", ".manifest", ".pack", ".zip")
RESOURCE_REQUEST_PATHS = frozenset({"/api/index.php/asset/get_path"})
LEGACY_CITYJSON_URL = b"http://@127.0.0.1:1/cityjson?ie=utf-8"
CURRENT_CITYJSON_URL = b"http://127.0.0.1:17171/cityjson?ie=u8"
EXPECTED_LEGACY_SOBOT_AUTHORITY_COUNT = 4
PERSONAL_SERVICE_FRAMEWORK_DIRECTORY = "PersonalServiceBootstrap.framework"
PERSONAL_SERVICE_FRAMEWORK_BINARY = "PersonalServiceBootstrap"
FRAMEWORK_OVERLAY_STAGING_DIRECTORY = ".PersonalServiceBootstrap.framework.overlay"


# //// 计算文件 SHA-256 [@x380kkm 2026-08-21] ////
def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()
# //// /计算文件 SHA-256 ////


# //// 执行外部命令并保留有限结果 [@x380kkm 2026-08-21] ////
def run_command(
    arguments: list[str],
    *,
    check: bool = True,
    timeout: int = 120,
) -> dict[str, Any]:
    result = subprocess.run(
        arguments,
        check=False,
        capture_output=True,
        text=True,
        timeout=timeout,
    )
    record = {
        "arguments": arguments,
        "exit_code": result.returncode,
        "stdout": result.stdout,
        "stderr": result.stderr,
    }
    if check and result.returncode != 0:
        detail = (result.stderr or result.stdout).strip()
        raise RuntimeError(
            f"命令失败: command={arguments[0]}, exit_code={result.returncode}, detail={detail}"
        )
    return record
# //// /执行外部命令并保留有限结果 ////


# //// 把薄 arm64 Mach-O 的 iOS 平台改为 iOS Simulator [@x380kkm 2026-08-21] ////
def patch_macho_platform(data: bytes) -> tuple[bytes, int]:
    if len(data) < MACH_HEADER_64_SIZE:
        raise ValueError("Mach-O 文件过短.")
    magic, _, _, _, command_count, command_bytes, _, _ = struct.unpack_from(
        "<IiiIIIII", data, 0
    )
    if magic != MH_MAGIC_64:
        raise ValueError("文件不是薄 64-bit Mach-O.")
    command_offset = MACH_HEADER_64_SIZE
    command_limit = command_offset + command_bytes
    if command_limit > len(data):
        raise ValueError("Mach-O 加载命令越界.")

    patched = bytearray(data)
    patch_count = 0
    for _ in range(command_count):
        if command_offset + 8 > command_limit:
            raise ValueError("Mach-O 加载命令头越界.")
        command, command_size = struct.unpack_from("<II", data, command_offset)
        if command_size < 8 or command_offset + command_size > command_limit:
            raise ValueError("Mach-O 加载命令长度无效.")
        if command == LC_BUILD_VERSION:
            if command_size < 24:
                raise ValueError("LC_BUILD_VERSION 长度无效.")
            platform = struct.unpack_from("<I", data, command_offset + 8)[0]
            if platform == PLATFORM_IOS:
                struct.pack_into(
                    "<I", patched, command_offset + 8, PLATFORM_IOS_SIMULATOR
                )
                patch_count += 1
            elif platform != PLATFORM_IOS_SIMULATOR:
                raise ValueError(f"不支持的 Mach-O 平台: {platform}.")
        command_offset += command_size
    if command_offset != command_limit:
        raise ValueError("Mach-O 加载命令总长度不一致.")
    return bytes(patched), patch_count
# //// /把薄 arm64 Mach-O 的 iOS 平台改为 iOS Simulator ////


# //// 关闭 CN 1.8.4 的 device-only 启动守卫 [@x380kkm 2026-08-21] ////
def patch_startup_guard(data: bytes) -> tuple[bytes, dict[str, Any]]:
    end = STARTUP_GUARD_OFFSET + len(STARTUP_GUARD_SOURCE)
    if end > len(data):
        raise ValueError("CN 启动守卫偏移越界.")
    source = data[STARTUP_GUARD_OFFSET:end]
    if source != STARTUP_GUARD_SOURCE:
        raise ValueError(
            "CN 启动守卫字节不一致: "
            f"expected={STARTUP_GUARD_SOURCE.hex()}, actual={source.hex()}."
        )
    patched = bytearray(data)
    patched[STARTUP_GUARD_OFFSET:end] = STARTUP_GUARD_TARGET
    return bytes(patched), {
        "offset": STARTUP_GUARD_OFFSET,
        "source_instruction": STARTUP_GUARD_SOURCE.hex(),
        "target_instruction": STARTUP_GUARD_TARGET.hex(),
    }
# //// /关闭 CN 1.8.4 的 device-only 启动守卫 ////


# //// 提取进程网络日志中的非 loopback URL [@x380kkm 2026-08-21] ////
def find_non_loopback_network_urls(log_text: str) -> list[str]:
    urls: set[str] = set()
    for line in log_text.splitlines():
        if not any(
            marker in line
            for marker in ("com.apple.network", "com.apple.CFNetwork", "url:")
        ):
            continue
        for match in NETWORK_URL_PATTERN.finditer(line):
            candidate = match.group(0).rstrip(".,;:")
            parsed = urlsplit(candidate)
            if parsed.hostname is None:
                continue
            if parsed.hostname.lower() not in LOOPBACK_HOSTS:
                urls.add(candidate)
    return sorted(urls)
# //// /提取进程网络日志中的非 loopback URL ////


# //// 等待个人服务 health 端点 [@x380kkm 2026-08-21] ////
def wait_for_health(timeout_seconds: int) -> dict[str, Any]:
    deadline = time.monotonic() + timeout_seconds
    last_error = ""
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen(
                "http://127.0.0.1:17171/health", timeout=1.0
            ) as response:
                body = response.read().decode("utf-8")
                if response.status == 200:
                    return {"status": response.status, "body": body}
        except Exception as error:  # noqa: BLE001
            last_error = str(error)
        time.sleep(0.5)
    raise RuntimeError(f"个人服务 health 超时: {last_error}")
# //// /等待个人服务 health 端点 ////


# //// 选择可用的 iOS runtime 和 iPhone 设备类型 [@x380kkm 2026-08-21] ////
def select_simulator_configuration() -> tuple[str, str]:
    runtime_data = json.loads(
        run_command(["xcrun", "simctl", "list", "runtimes", "-j"])["stdout"]
    )
    runtimes = [
        runtime
        for runtime in runtime_data["runtimes"]
        if runtime.get("isAvailable") and runtime.get("platform") == "iOS"
    ]
    if not runtimes:
        raise RuntimeError("Mac 没有可用的 iOS Simulator runtime.")
    runtime = max(runtimes, key=lambda item: item.get("version", ""))["identifier"]

    device_data = json.loads(
        run_command(["xcrun", "simctl", "list", "devicetypes", "-j"])["stdout"]
    )
    iphones = [
        device
        for device in device_data["devicetypes"]
        if device.get("productFamily") == "iPhone" or device["name"].startswith("iPhone")
    ]
    if not iphones:
        raise RuntimeError("Mac 没有可用的 iPhone Simulator 设备类型.")
    preferred = next(
        (device for device in iphones if device["name"] == "iPhone 17 Pro"),
        iphones[0],
    )
    return runtime, preferred["identifier"]
# //// /选择可用的 iOS runtime 和 iPhone 设备类型 ////


# //// 安全复制目录内容 [@x380kkm 2026-08-22] ////
def copy_directory_contents_safely(
    source_root: Path, destination_root: Path
) -> list[Path]:
    if not source_root.is_dir():
        raise ValueError(f"覆盖目录不存在: {source_root}.")
    if source_root.is_symlink():
        raise ValueError(f"覆盖根目录不能是符号链接: {source_root}.")
    if destination_root.is_symlink():
        raise ValueError(f"覆盖目标根目录不能是符号链接: {destination_root}.")
    if destination_root.exists() and not destination_root.is_dir():
        raise ValueError(f"覆盖目标根路径不是目录: {destination_root}.")
    resolved_source_root = source_root.resolve()
    resolved_destination_root = destination_root.resolve()
    if (
        resolved_destination_root.is_relative_to(resolved_source_root)
        or resolved_source_root.is_relative_to(resolved_destination_root)
    ):
        raise ValueError(
            "覆盖源目录与目标目录不能互相包含: "
            f"source={source_root}, destination={destination_root}."
        )
    destination_root.mkdir(parents=True, exist_ok=True)
    copied_files: list[Path] = []
    for source in sorted(source_root.rglob("*")):
        if source.is_symlink():
            raise ValueError(f"覆盖内容不能是符号链接: {source}.")
        relative_path = source.relative_to(source_root)
        destination = destination_root / relative_path
        if not destination.resolve().is_relative_to(resolved_destination_root):
            raise ValueError(f"覆盖目标越界: {relative_path}.")
        if source.is_dir():
            if destination.is_symlink():
                raise ValueError(f"覆盖目标不能是符号链接: {destination}.")
            destination.mkdir(parents=True, exist_ok=True)
            continue
        if not source.is_file():
            raise ValueError(f"覆盖内容不是常规文件: {source}.")
        destination.parent.mkdir(parents=True, exist_ok=True)
        if destination.is_symlink():
            raise ValueError(f"覆盖目标不能是符号链接: {destination}.")
        shutil.copy2(source, destination)
        copied_files.append(relative_path)
    return copied_files
# //// /安全复制目录内容 ////


# //// 安全复制 Simulator CDN 覆盖文件 [@x380kkm 2026-08-22] ////
def inject_cdn_overlay(app_root: Path, overlay_root: Path) -> list[dict[str, Any]]:
    cdn_root = app_root / BUNDLED_CDN_DIRECTORY
    copied_files = copy_directory_contents_safely(overlay_root, cdn_root)
    injected_files = [
        {
            "path": relative_path.as_posix(),
            "size": (cdn_root / relative_path).stat().st_size,
        }
        for relative_path in copied_files
    ]
    if not injected_files:
        raise ValueError(f"CDN 覆盖目录没有文件: {overlay_root}.")
    return injected_files
# //// /安全复制 Simulator CDN 覆盖文件 ////


# //// 完整替换个人服务 Framework [@x380kkm 2026-08-22] ////
def replace_personal_service_framework(
    app_root: Path, overlay_root: Path
) -> dict[str, Any]:
    if overlay_root.name != PERSONAL_SERVICE_FRAMEWORK_DIRECTORY:
        raise ValueError(
            "Framework 覆盖目录名称不正确: "
            f"expected={PERSONAL_SERVICE_FRAMEWORK_DIRECTORY}, "
            f"actual={overlay_root.name}."
        )
    frameworks_root = app_root / "Frameworks"
    if frameworks_root.is_symlink() or not frameworks_root.is_dir():
        raise ValueError(f"App Frameworks 目录无效: {frameworks_root}.")
    destination = frameworks_root / PERSONAL_SERVICE_FRAMEWORK_DIRECTORY
    if destination.is_symlink() or not destination.is_dir():
        raise ValueError(f"App 个人服务 Framework 目录无效: {destination}.")
    staged_destination = frameworks_root / FRAMEWORK_OVERLAY_STAGING_DIRECTORY
    if staged_destination.exists() or staged_destination.is_symlink():
        raise ValueError(f"Framework 覆盖暂存目录已存在: {staged_destination}.")

    try:
        copied_files = copy_directory_contents_safely(
            overlay_root, staged_destination
        )
        binary = staged_destination / PERSONAL_SERVICE_FRAMEWORK_BINARY
        info_plist = staged_destination / "Info.plist"
        if not binary.is_file():
            raise ValueError(f"Framework 覆盖缺少主二进制: {binary}.")
        if not info_plist.is_file():
            raise ValueError(f"Framework 覆盖缺少 Info.plist: {info_plist}.")
        evidence = {
            "source": str(overlay_root),
            "destination": (
                f"Frameworks/{PERSONAL_SERVICE_FRAMEWORK_DIRECTORY}"
            ),
            "file_count": len(copied_files),
            "binary_sha256": sha256_file(binary),
        }
        shutil.rmtree(destination)
        staged_destination.rename(destination)
        return evidence
    except Exception:
        if staged_destination.is_dir() and not staged_destination.is_symlink():
            shutil.rmtree(staged_destination)
        raise
# //// /完整替换个人服务 Framework ////


# //// 迁移旧 CN loopback 端点 [@x380kkm 2026-08-22] ////
def upgrade_legacy_cn_network_patches(
    data: bytes,
) -> tuple[bytes, dict[str, Any]]:
    from ios_cn_aot_patch import patch_sobot_authorities

    patched, sobot_evidence = patch_sobot_authorities(data)
    if (
        sobot_evidence.get("source_mode") != "port_one_loopback"
        or sobot_evidence.get("count") != EXPECTED_LEGACY_SOBOT_AUTHORITY_COUNT
    ):
        raise ValueError(
            "旧 Sobot authority 不是 4 个 port-one loopback 地址: "
            f"mode={sobot_evidence.get('source_mode')}, "
            f"count={sobot_evidence.get('count')}."
        )

    source_count = patched.count(LEGACY_CITYJSON_URL)
    target_count = patched.count(CURRENT_CITYJSON_URL)
    if source_count != 1 or target_count != 0:
        raise ValueError(
            "旧 cityjson 地址数量不正确: "
            f"source={source_count}, target={target_count}."
        )
    if len(LEGACY_CITYJSON_URL) != len(CURRENT_CITYJSON_URL):
        raise ValueError("cityjson 地址长度不一致.")
    cityjson_offset = patched.find(LEGACY_CITYJSON_URL)
    cityjson_end = cityjson_offset + len(LEGACY_CITYJSON_URL)
    migrated = bytearray(patched)
    migrated[cityjson_offset:cityjson_end] = CURRENT_CITYJSON_URL
    migrated_bytes = bytes(migrated)
    if (
        migrated_bytes.count(LEGACY_CITYJSON_URL) != 0
        or migrated_bytes.count(CURRENT_CITYJSON_URL) != 1
    ):
        raise ValueError("cityjson 地址写入后无法唯一复读.")
    return migrated_bytes, {
        "sobot": sobot_evidence,
        "cityjson": {
            "count": 1,
            "offset": cityjson_offset,
            "source": LEGACY_CITYJSON_URL.decode("ascii"),
            "target": CURRENT_CITYJSON_URL.decode("ascii"),
        },
    }
# //// /迁移旧 CN loopback 端点 ////


# //// 把候选 App 转为 Simulator 可安装目录 [@x380kkm 2026-08-21] ////
def prepare_simulator_app(
    ipa_path: Path,
    work_root: Path,
    cdn_overlay_root: Optional[Path] = None,
    framework_overlay: Optional[Path] = None,
    upgrade_legacy_cn_patches: bool = False,
) -> tuple[
    Path,
    str,
    str,
    list[dict[str, Any]],
    dict[str, Any],
    Optional[dict[str, Any]],
    Optional[dict[str, Any]],
    dict[str, Any],
]:
    archive_root = work_root / "archive"
    with zipfile.ZipFile(ipa_path) as archive:
        archive.extractall(archive_root)
    applications = list((archive_root / "Payload").glob("*.app"))
    if len(applications) != 1:
        raise ValueError(f"IPA 根 App 数量不正确: {len(applications)}.")
    app_root = applications[0]
    info = plistlib.loads((app_root / "Info.plist").read_bytes())
    executable_name = info.get("CFBundleExecutable")
    bundle_id = info.get("CFBundleIdentifier")
    if not isinstance(executable_name, str) or not executable_name:
        raise ValueError("Info.plist 缺少 CFBundleExecutable.")
    if not isinstance(bundle_id, str) or not bundle_id:
        raise ValueError("Info.plist 缺少 CFBundleIdentifier.")

    cdn_root = app_root / BUNDLED_CDN_DIRECTORY
    original_cdn_present = cdn_root.is_dir()
    injected_files = (
        inject_cdn_overlay(app_root, cdn_overlay_root)
        if cdn_overlay_root is not None
        else []
    )
    cdn_input = {
        "original_bundle_present": original_cdn_present,
        "effective_cdn_root_present": cdn_root.is_dir(),
        "overlay_root": str(cdn_overlay_root) if cdn_overlay_root is not None else None,
        "injected_files": injected_files,
    }
    framework_overlay_evidence = (
        replace_personal_service_framework(app_root, framework_overlay)
        if framework_overlay is not None
        else None
    )

    main_executable = app_root / executable_name
    main_data = main_executable.read_bytes()
    legacy_patch_upgrade = None
    if upgrade_legacy_cn_patches:
        main_data, legacy_patch_upgrade = upgrade_legacy_cn_network_patches(
            main_data
        )
    main_data, startup_guard = patch_startup_guard(main_data)
    main_executable.write_bytes(main_data)

    platform_patches: list[dict[str, Any]] = []
    macho_paths = [main_executable]
    frameworks_root = app_root / "Frameworks"
    if frameworks_root.is_dir():
        for framework in frameworks_root.glob("*.framework"):
            binary = framework / framework.stem
            if binary.is_file():
                macho_paths.append(binary)
    for macho_path in macho_paths:
        patched, command_count = patch_macho_platform(macho_path.read_bytes())
        if command_count != 1:
            raise ValueError(
                f"Mach-O 平台命令数量不正确: path={macho_path.name}, count={command_count}."
            )
        macho_path.write_bytes(patched)
        macho_path.chmod(0o755)
        platform_patches.append(
            {"path": str(macho_path.relative_to(app_root)), "commands": command_count}
        )

    for signature in app_root.rglob("_CodeSignature"):
        if signature.is_dir():
            shutil.rmtree(signature)
    if frameworks_root.is_dir():
        for framework in sorted(frameworks_root.glob("*.framework")):
            run_command(
                ["codesign", "--force", "--sign", "-", "--timestamp=none", str(framework)]
            )
    run_command(
        ["codesign", "--force", "--sign", "-", "--timestamp=none", str(app_root)]
    )
    run_command(["codesign", "--verify", "--deep", "--strict", str(app_root)])
    return (
        app_root,
        bundle_id,
        executable_name,
        platform_patches,
        startup_guard,
        legacy_patch_upgrade,
        framework_overlay_evidence,
        cdn_input,
    )
# //// /把候选 App 转为 Simulator 可安装目录 ////


# //// 预授权 Simulator 通知权限 [@x380kkm 2026-08-22] ////
def grant_notification_permission(
    simulator_udid: str, bundle_id: str
) -> dict[str, Any]:
    result = run_command(
        [
            "xcrun",
            "simctl",
            "privacy",
            simulator_udid,
            "grant",
            "notifications",
            bundle_id,
        ],
        check=False,
    )
    result["outcome"] = "granted" if result["exit_code"] == 0 else "unavailable"
    return result
# //// /预授权 Simulator 通知权限 ////


# //// 判断失败请求是否读取 App CDN 资源 [@x380kkm 2026-08-22] ////
def is_bundled_resource_request(path: str) -> bool:
    normalized = path.split("?", 1)[0]
    return (
        normalized in RESOURCE_REQUEST_PATHS
        or normalized.startswith(RESOURCE_REQUEST_PREFIXES)
        or normalized.lower().endswith(RESOURCE_REQUEST_SUFFIXES)
    )
# //// /判断失败请求是否读取 App CDN 资源 ////


# //// 汇总核心 HTTP 观察结果 [@x380kkm 2026-08-22] ////
def summarize_http_observations(
    observation_report: dict[str, Any],
    original_cdn_present: bool,
) -> dict[str, Any]:
    raw_observations = observation_report.get("observations")
    observations = raw_observations if isinstance(raw_observations, list) else []
    core_failures = [
        observation
        for observation in observations
        if observation.get("core") is True
        and isinstance(observation.get("status"), int)
        and not 200 <= observation["status"] < 300
    ]

    def select_status(status: int) -> list[dict[str, Any]]:
        return [
            {
                "method": observation.get("method"),
                "path": observation.get("path"),
                "count": observation.get("count"),
            }
            for observation in core_failures
            if observation["status"] == status
        ]

    resource_failures = [
        observation
        for observation in core_failures
        if isinstance(observation.get("path"), str)
        and is_bundled_resource_request(observation["path"])
    ]
    resource_failure_classification = None
    if resource_failures:
        resource_failure_classification = (
            "bundled_resource_or_service_failure"
            if original_cdn_present
            else "probe_input_missing_bundled_cdn"
        )
    return {
        "status": observation_report.get("status"),
        "error_code": observation_report.get("error_code"),
        "first_failure": observation_report.get("first_failure"),
        "core_observation_count": observation_report.get("core_observation_count"),
        "core_failure_count": observation_report.get("core_failure_count"),
        "required_failure_count": observation_report.get("required_failure_count"),
        "required_failures": observation_report.get("required_failures", []),
        "core_404": select_status(404),
        "core_500": select_status(500),
        "missing_required_requests": observation_report.get(
            "missing_required_requests", []
        ),
        "resource_failure_classification": resource_failure_classification,
    }
# //// /汇总核心 HTTP 观察结果 ////


# //// 从运行中的 Simulator 导出 HTTP 观察结果 [@x380kkm 2026-08-22] ////
def export_http_observations(
    simulator_udid: str,
    bundle_id: str,
    started_at: str,
    output_root: Path,
    original_cdn_present: bool,
) -> dict[str, Any]:
    output_path = output_root / "http-observations.json"
    scenario_report_path = output_root / "http-observation-window.json"
    data_container_query = run_command(
        [
            "xcrun",
            "simctl",
            "get_app_container",
            simulator_udid,
            bundle_id,
            "data",
        ],
        check=False,
    )
    result: dict[str, Any] = {
        "status": "failed",
        "output_path": str(output_path),
        "scenario_report_path": str(scenario_report_path),
        "data_container_query": data_container_query,
    }
    if data_container_query["exit_code"] != 0:
        result["error_code"] = "DATA_CONTAINER_LOOKUP_FAILED"
        return result
    data_container = data_container_query["stdout"].strip()
    if not data_container:
        result["error_code"] = "DATA_CONTAINER_PATH_EMPTY"
        return result
    result["data_container"] = data_container
    result["consistency"] = "sqlite_read_transaction_with_wal"

    scenario_report_path.write_text(
        json.dumps(
            {
                "started_at": started_at,
                "required_requests": [
                    {"method": method, "path": path}
                    for method, path in PASSIVE_APP_REQUIRED_REQUESTS
                ],
            },
            ensure_ascii=False,
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    exporter_path = Path(__file__).with_name(HTTP_OBSERVATIONS_EXPORTER)
    result["exporter_path"] = str(exporter_path)
    if not exporter_path.is_file():
        result["error_code"] = "OBSERVATIONS_EXPORTER_MISSING"
        return result
    output_path.unlink(missing_ok=True)
    export_command = run_command(
        [
            sys.executable,
            str(exporter_path),
            "--data-container",
            data_container,
            "--scenario-report",
            str(scenario_report_path),
            "--output",
            str(output_path),
        ],
        check=False,
    )
    result["export_command"] = export_command
    if not output_path.is_file():
        result["error_code"] = "OBSERVATIONS_REPORT_MISSING"
        return result
    try:
        observation_report = json.loads(output_path.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        result["error_code"] = "OBSERVATIONS_REPORT_INVALID"
        return result
    if not isinstance(observation_report, dict):
        result["error_code"] = "OBSERVATIONS_REPORT_INVALID"
        return result
    summary = summarize_http_observations(
        observation_report,
        original_cdn_present,
    )
    result["status"] = (
        summary["status"] if summary["status"] in {"passed", "failed"} else "failed"
    )
    result["error_code"] = summary["error_code"]
    result["summary"] = summary
    return result
# //// /从运行中的 Simulator 导出 HTTP 观察结果 ////


# //// 将 HTTP 观察状态写入 probe 报告 [@x380kkm 2026-08-22] ////
def record_http_observation_result(
    report: dict[str, Any], observation_result: dict[str, Any]
) -> None:
    report["http_observations"] = observation_result
    if observation_result.get("status") == "passed":
        return
    report["status"] = "failed"
    if "error" not in report:
        error_code = observation_result.get("error_code") or "UNKNOWN"
        report["error"] = f"HTTP 观察失败: error_code={error_code}."
# //// /将 HTTP 观察状态写入 probe 报告 ////


# //// 在单台无头 Simulator 中运行离线验收 [@x380kkm 2026-08-21] ////
def probe_candidate(
    ipa_path: Path,
    expected_sha256: str,
    output_root: Path,
    wait_seconds: int,
    cdn_overlay_root: Optional[Path] = None,
    framework_overlay: Optional[Path] = None,
    upgrade_legacy_cn_patches: bool = False,
) -> dict[str, Any]:
    actual_sha256 = sha256_file(ipa_path)
    if actual_sha256 != expected_sha256.lower():
        raise ValueError(
            f"IPA SHA-256 不一致: expected={expected_sha256}, actual={actual_sha256}."
        )
    output_root.mkdir(parents=True, exist_ok=True)
    work_root = output_root / "work"
    if work_root.exists():
        raise ValueError(f"工作目录已存在: {work_root}.")
    work_root.mkdir()
    started_at = datetime.now(timezone.utc).isoformat()
    report: dict[str, Any] = {
        "schema_version": 1,
        "status": "failed",
        "ipa_sha256": actual_sha256,
        "expected_sha256": expected_sha256.lower(),
        "started_at_utc": started_at,
    }
    simulator_udid: Optional[str] = None
    bundle_id: Optional[str] = None
    cdn_input: Optional[dict[str, Any]] = None
    try:
        booted = json.loads(
            run_command(["xcrun", "simctl", "list", "devices", "booted", "-j"])[
                "stdout"
            ]
        )
        booted_count = sum(len(devices) for devices in booted["devices"].values())
        if booted_count != 0:
            raise RuntimeError(
                f"远端已有 booted Simulator, 为避免并行运行已停止: count={booted_count}."
            )

        (
            app_root,
            bundle_id,
            executable_name,
            platform_patches,
            startup_guard,
            legacy_patch_upgrade,
            framework_overlay_evidence,
            cdn_input,
        ) = prepare_simulator_app(
            ipa_path,
            work_root,
            cdn_overlay_root=cdn_overlay_root,
            framework_overlay=framework_overlay,
            upgrade_legacy_cn_patches=upgrade_legacy_cn_patches,
        )
        report["cdn_input"] = cdn_input
        if legacy_patch_upgrade is not None:
            report["legacy_cn_patch_upgrade"] = legacy_patch_upgrade
        if framework_overlay_evidence is not None:
            report["framework_overlay"] = framework_overlay_evidence
        runtime, device_type = select_simulator_configuration()
        simulator_name = f"Starpoint Offline {os.getpid()}"
        simulator_udid = run_command(
            ["xcrun", "simctl", "create", simulator_name, device_type, runtime]
        )["stdout"].strip()
        (output_root / "simulator-udid.txt").write_text(
            simulator_udid + "\n", encoding="utf-8"
        )
        run_command(["xcrun", "simctl", "boot", simulator_udid])
        run_command(
            ["xcrun", "simctl", "bootstatus", simulator_udid, "-b"], timeout=300
        )
        install = run_command(
            ["xcrun", "simctl", "install", simulator_udid, str(app_root)], timeout=300
        )
        report["install"] = install
        notification_permission = grant_notification_permission(
            simulator_udid, bundle_id
        )
        report["notification_permission"] = notification_permission
        launch = run_command(
            ["xcrun", "simctl", "launch", simulator_udid, bundle_id], timeout=60
        )
        launched_process = re.search(r":\s*([0-9]+)\s*$", launch["stdout"])
        if launched_process is None:
            raise RuntimeError("simctl launch 没有返回进程 PID.")
        process = {"pid": int(launched_process.group(1))}
        health = wait_for_health(30)
        time.sleep(wait_seconds)
        app_screenshot = output_root / "app.png"
        run_command(
            ["xcrun", "simctl", "io", simulator_udid, "screenshot", str(app_screenshot)]
        )
        logs = run_command(
            [
                "xcrun",
                "simctl",
                "spawn",
                simulator_udid,
                "log",
                "show",
                "--last",
                f"{max(wait_seconds + 20, 30)}s",
                "--style",
                "compact",
                "--predicate",
                f'process == "{executable_name}"',
            ],
            timeout=180,
        )
        log_path = output_root / "worldflipper.log"
        log_path.write_text(logs["stdout"], encoding="utf-8")
        non_loopback_urls = find_non_loopback_network_urls(logs["stdout"])
        if non_loopback_urls:
            raise RuntimeError(
                "进程日志包含非 loopback URL: " + ", ".join(non_loopback_urls)
            )

        management_open = run_command(
            [
                "xcrun",
                "simctl",
                "openurl",
                simulator_udid,
                "http://127.0.0.1:17171/manage/",
            ]
        )
        time.sleep(4)
        management_screenshot = output_root / "management-safari.png"
        run_command(
            [
                "xcrun",
                "simctl",
                "io",
                simulator_udid,
                "screenshot",
                str(management_screenshot),
            ]
        )
        report.update(
            {
                "status": "passed",
                "bundle_id": bundle_id,
                "executable": executable_name,
                "simulator": {
                    "udid": simulator_udid,
                    "runtime": runtime,
                    "device_type": device_type,
                },
                "startup_guard": startup_guard,
                "platform_patches": platform_patches,
                "install": install,
                "launch": launch,
                "process": process,
                "health": health,
                "network_negative_assertion": {
                    "status": "passed",
                    "scope": f'process == "{executable_name}"',
                    "non_loopback_urls": [],
                    "log_path": str(log_path),
                },
                "management_loopback_open": management_open,
                "screenshots": {
                    "app": str(app_screenshot),
                    "management": str(management_screenshot),
                },
            }
        )
    except Exception as error:  # noqa: BLE001
        report["error"] = str(error)
    finally:
        cleanup: dict[str, Any] = {}
        if simulator_udid is not None:
            if bundle_id is not None:
                try:
                    observation_result = export_http_observations(
                        simulator_udid,
                        bundle_id,
                        started_at,
                        output_root,
                        bool(cdn_input and cdn_input["original_bundle_present"]),
                    )
                except Exception as error:  # noqa: BLE001
                    observation_result = {
                        "status": "failed",
                        "error_code": "OBSERVATIONS_EXPORT_FAILED",
                        "error": str(error),
                        "output_path": str(output_root / "http-observations.json"),
                    }
                record_http_observation_result(report, observation_result)
                cleanup["terminate"] = run_command(
                    ["xcrun", "simctl", "terminate", simulator_udid, bundle_id],
                    check=False,
                )
            cleanup["shutdown"] = run_command(
                ["xcrun", "simctl", "shutdown", simulator_udid], check=False
            )
            cleanup["delete"] = run_command(
                ["xcrun", "simctl", "delete", simulator_udid], check=False
            )
        report["cleanup"] = cleanup
        report["ended_at_utc"] = datetime.now(timezone.utc).isoformat()
        report_path = output_root / "offline-probe-report.json"
        temporary_report = report_path.with_suffix(".json.tmp")
        temporary_report.write_text(
            json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        os.replace(temporary_report, report_path)
    return report
# //// /在单台无头 Simulator 中运行离线验收 ////


# //// 解析命令行并返回验收状态 [@x380kkm 2026-08-21] ////
def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--ipa", required=True, type=Path)
    parser.add_argument("--expected-sha256", required=True)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--wait-seconds", type=int, default=20)
    parser.add_argument("--cdn-overlay-root", type=Path)
    parser.add_argument("--framework-overlay", type=Path)
    parser.add_argument("--upgrade-legacy-cn-patches", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if not re.fullmatch(r"[0-9a-fA-F]{64}", args.expected_sha256):
        raise ValueError("--expected-sha256 必须是 64 位十六进制.")
    if not 5 <= args.wait_seconds <= 120:
        raise ValueError("--wait-seconds 必须介于 5 和 120 之间.")
    report = probe_candidate(
        args.ipa.resolve(),
        args.expected_sha256,
        args.output.resolve(),
        args.wait_seconds,
        cdn_overlay_root=(
            args.cdn_overlay_root.resolve() if args.cdn_overlay_root else None
        ),
        framework_overlay=(
            args.framework_overlay.resolve() if args.framework_overlay else None
        ),
        upgrade_legacy_cn_patches=args.upgrade_legacy_cn_patches,
    )
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0 if report["status"] == "passed" else 1
# //// /解析命令行并返回验收状态 ////


if __name__ == "__main__":
    sys.exit(main())
