# audience: external
# # prepare-ios-signing
# 此脚本验证解码后的 Apple provisioning profile, 并生成绑定目标 bundle identifier 的签名 entitlements.

from __future__ import annotations

import argparse
import copy
from datetime import UTC, datetime
import json
from pathlib import Path
import plistlib
import re
from typing import Any


class SigningProfileError(ValueError):
    pass


# //// 验证 profile 基础字段 [@x380kkm 2026-07-23] ////
def validate_bundle_identifier(bundle_identifier: str) -> None:
    parts = bundle_identifier.split(".")
    if (
        not bundle_identifier
        or "*" in bundle_identifier
        or any(not part or re.fullmatch(r"[A-Za-z0-9-]+", part) is None for part in parts)
    ):
        raise SigningProfileError("bundle identifier is invalid")


def profile_pattern_matches(pattern: str, bundle_identifier: str) -> bool:
    if "*" not in pattern:
        return pattern == bundle_identifier
    if pattern.count("*") != 1 or not pattern.endswith("*"):
        raise SigningProfileError("profile application identifier contains an unsupported wildcard")
    return bundle_identifier.startswith(pattern[:-1])


def utc_datetime(value: Any, field: str) -> datetime:
    if not isinstance(value, datetime):
        raise SigningProfileError(f"profile {field} is missing or invalid")
    if value.tzinfo is None:
        return value.replace(tzinfo=UTC)
    return value.astimezone(UTC)


def first_profile_string(profile: dict[str, Any], field: str) -> str:
    values = profile.get(field)
    if not isinstance(values, list) or not values or not isinstance(values[0], str):
        raise SigningProfileError(f"profile {field} is missing or invalid")
    return values[0]
# //// /验证 profile 基础字段 ////


# //// 验证 profile 并生成签名 entitlements [@x380kkm 2026-07-23] ////
def prepare_signing_entitlements(
    profile: dict[str, Any],
    bundle_identifier: str,
    now: datetime | None = None,
) -> tuple[dict[str, Any], dict[str, Any]]:
    validate_bundle_identifier(bundle_identifier)
    expiration = utc_datetime(profile.get("ExpirationDate"), "ExpirationDate")
    current_time = now or datetime.now(UTC)
    if current_time.tzinfo is None:
        current_time = current_time.replace(tzinfo=UTC)
    if expiration <= current_time.astimezone(UTC):
        raise SigningProfileError("provisioning profile is expired")

    devices = profile.get("ProvisionedDevices")
    if (
        not isinstance(devices, list)
        or not devices
        or not all(isinstance(device, str) and device for device in devices)
    ):
        raise SigningProfileError("provisioning profile contains no registered devices")

    source_entitlements = profile.get("Entitlements")
    if not isinstance(source_entitlements, dict):
        raise SigningProfileError("profile Entitlements is missing or invalid")
    entitlements = copy.deepcopy(source_entitlements)

    application_prefix = first_profile_string(profile, "ApplicationIdentifierPrefix")
    application_identifier = entitlements.get("application-identifier")
    prefix = f"{application_prefix}."
    if not isinstance(application_identifier, str) or not application_identifier.startswith(prefix):
        raise SigningProfileError("profile application identifier prefix is invalid")
    profile_bundle_pattern = application_identifier[len(prefix) :]
    if not profile_pattern_matches(profile_bundle_pattern, bundle_identifier):
        raise SigningProfileError("bundle identifier is not covered by the provisioning profile")

    exact_application_identifier = f"{application_prefix}.{bundle_identifier}"
    entitlements["application-identifier"] = exact_application_identifier
    keychain_groups = entitlements.get("keychain-access-groups")
    if isinstance(keychain_groups, list):
        entitlements["keychain-access-groups"] = [
            exact_application_identifier if group == f"{application_prefix}.*" else group
            for group in keychain_groups
        ]

    team_identifier = entitlements.get("com.apple.developer.team-identifier")
    if not isinstance(team_identifier, str):
        team_identifier = first_profile_string(profile, "TeamIdentifier")
        entitlements["com.apple.developer.team-identifier"] = team_identifier

    summary = {
        "profile_name": profile.get("Name"),
        "profile_uuid": profile.get("UUID"),
        "expiration_utc": expiration.isoformat().replace("+00:00", "Z"),
        "bundle_identifier": bundle_identifier,
        "application_identifier": exact_application_identifier,
        "team_identifier": team_identifier,
        "provisioned_device_count": len(devices),
        "get_task_allow": bool(entitlements.get("get-task-allow", False)),
    }
    return entitlements, summary
# //// /验证 profile 并生成签名 entitlements ////


# //// 读取 profile 并写入 entitlements [@x380kkm 2026-07-23] ////
def load_profile(path: Path) -> dict[str, Any]:
    with path.open("rb") as source:
        profile = plistlib.load(source)
    if not isinstance(profile, dict):
        raise SigningProfileError("profile plist root is invalid")
    return profile


def write_entitlements(path: Path, entitlements: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("wb") as destination:
        plistlib.dump(entitlements, destination, fmt=plistlib.FMT_XML, sort_keys=True)
# //// /读取 profile 并写入 entitlements ////


# //// 提供 profile 验证命令 [@x380kkm 2026-07-23] ////
def main() -> None:
    parser = argparse.ArgumentParser(description="验证 provisioning profile 并生成签名 entitlements.")
    parser.add_argument("--profile-plist", required=True, type=Path)
    parser.add_argument("--bundle-id", required=True)
    parser.add_argument("--output", required=True, type=Path)
    arguments = parser.parse_args()

    try:
        entitlements, summary = prepare_signing_entitlements(
            load_profile(arguments.profile_plist),
            arguments.bundle_id,
        )
        write_entitlements(arguments.output, entitlements)
    except (OSError, plistlib.InvalidFileException, SigningProfileError) as error:
        parser.error(str(error))
    print(json.dumps(summary, ensure_ascii=False, sort_keys=True))


if __name__ == "__main__":
    main()
# //// /提供 profile 验证命令 ////
