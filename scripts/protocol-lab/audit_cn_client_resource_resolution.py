# audience: internal
# # audit-cn-client-resource-resolution
# FileReader 的文件清单成员, 图像规格和平台共同决定落盘资源路径.
# CN 客户端加载 bundle 与 ios_bundle 文件清单, 教程资源目录采用运行平台前缀.
# 审计从 full 版本沿 original_version 索引的 diff 链模拟下载覆盖, 分别检查两种图像规格.

from __future__ import annotations

import argparse
import hashlib
import io
import json
import re
import zipfile
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

from audit_cn_cdn_storage import ASSET_SALT, archive_paths, read_entities, read_json
from client_package_archive import open_game_zip


PLATFORMS = ("ios", "android")
RESOLUTIONS = ("medium", "small")
FILELIST_KINDS = ("amf", "encrypt", "copy", "compress", "png", "atf")
SCALED_PREFIXES = (
    "scene/title_bundled/key_visual/sprite_sheet",
    "scene/title_bundled/key_visual/separate/",
    "dynamic/gacha_banner/",
    "dynamic/home_banner/",
)


# //// 表示带来源的资源及客户端包 [@x380kkm 2026-09-20] ////
@dataclass(frozen=True)
class StoredAsset:
    size: int
    source: str


@dataclass
class ClientBundle:
    platform: str
    path: Path
    entries: dict[str, StoredAsset]
    common_members: set[str]
    tutorial_members: set[str]
    filelists: dict[str, int]


@dataclass(frozen=True)
class ResourceReference:
    logical_path: str | None
    suffix: str
    scaled: bool
    platform_dependent: bool = False
    source_path: str | None = None
# //// /表示带来源的资源及客户端包 ////


# //// 按客户端规则选择资源文件名和目录 [@x380kkm 2026-09-20] ////
def hashed_suffix(logical_path: str) -> str:
    logical_path = re.sub(r"[/\\]+", "/", logical_path).lstrip("/")
    digest = hashlib.sha1((logical_path + ASSET_SALT).encode("utf-8")).hexdigest()
    return digest[:2] + "/" + digest[2:]


def is_platform_dependent(path: str) -> bool:
    return path.startswith("character/") and "/ui/skill_cutin_" in path


def is_scaled_asset(path: str) -> bool:
    return not is_platform_dependent(path) and (
        (path.startswith("character/") and "/ui/" in path)
        or path.startswith(SCALED_PREFIXES)
    )


def texture_reference(path: str) -> ResourceReference:
    path = path.replace("//", "/")
    platform_dependent = is_platform_dependent(path)
    selected_path = path[:-4] + ".atf.deflate" if platform_dependent else path
    return ResourceReference(
        selected_path, hashed_suffix(selected_path),
        is_scaled_asset(path), platform_dependent,
    )


def resource_paths(
    reference: ResourceReference, bundle: ClientBundle, resolution: str,
) -> tuple[str, str]:
    is_common = reference.suffix in bundle.common_members
    is_tutorial = not is_common and reference.suffix in bundle.tutorial_members
    family = "bundle" if is_common or is_tutorial else "upload"
    if reference.scaled:
        family = resolution + "_" + family
    if reference.platform_dependent or is_tutorial:
        family = bundle.platform + "_" + family
    bundle_path = "production/" + family + "/" + reference.suffix
    return bundle_path.replace("bundle", "upload"), bundle_path
# //// /按客户端规则选择资源文件名和目录 ////


# //// 从客户端包读取实体目录和实际文件清单 [@x380kkm 2026-09-20] ////
def read_bundle(source: zipfile.ZipFile, prefix: str, platform: str, path: Path) -> ClientBundle:
    entries = {
        member.filename.removeprefix(prefix): StoredAsset(member.file_size, str(path))
        for member in source.infolist()
        if member.filename.startswith(prefix + "production/") and not member.is_dir()
    }
    members: dict[str, set[str]] = {"bundle": set(), "ios_bundle": set()}
    filelists = {}
    for family in members:
        for kind in FILELIST_KINDS:
            entry = "production/bundle/" + hashed_suffix(f"{family}_{kind}.filelist")
            if entry in entries:
                values = source.read(prefix + entry).decode("utf-8-sig").splitlines()
                members[family].update(value for value in values if value)
                filelists[f"{family}_{kind}"] = len(values)
    return ClientBundle(platform, path, entries, members["bundle"], members["ios_bundle"], filelists)


def read_client_package(path: Path) -> ClientBundle:
    with open_game_zip(path) as package:
        if path.suffix.lower() == ".apk":
            with zipfile.ZipFile(io.BytesIO(package.read("assets/bundle.zip"))) as bundle:
                return read_bundle(bundle, "", "android", path)
        roots = {
            name.split(".app/", 1)[0] + ".app/asset/"
            for name in package.namelist() if ".app/asset/" in name
        }
        if len(roots) != 1:
            raise ValueError(f"IPA asset 根目录数量异常: {path}: {len(roots)}")
        return read_bundle(package, roots.pop(), "ios", path)
# //// /从客户端包读取实体目录和实际文件清单 ////


# //// 从公布归档建立分平台的解压结果目录 [@x380kkm 2026-09-20] ////
def download_chain(document: dict) -> tuple[dict, list[str]]:
    by_origin = {group["original_version"]: group for group in document.get("diff", [])}
    version = document["full"]["version"]
    versions = [version]
    groups = []
    while version in by_origin:
        group = by_origin[version]
        version = group["version"]
        if version in versions:
            raise ValueError(f"资源版本链形成循环: {versions}: {version}")
        groups.append(group)
        versions.append(version)
    return {**document, "diff": groups}, versions


def read_download_entries(root: Path, document: dict) -> tuple[dict, list, list[str]]:
    ordered, versions = download_chain(document)
    archives, issues = archive_paths(root, ordered)
    target = document.get("info", {}).get("eventual_target_asset_version")
    if target and versions[-1] != target:
        issues.append({"kind": "incomplete_version_chain", "reached": versions[-1], "target": target})
    result: dict[str, dict[str, StoredAsset]] = {platform: {} for platform in PLATFORMS}
    for path in archives:
        if not path.is_file():
            continue
        family = path.parent.name.split("-")[1]
        platforms = PLATFORMS if family in ("common", "medium") else (family,)
        with zipfile.ZipFile(path) as archive:
            entries = {
                member.filename: StoredAsset(member.file_size, path.relative_to(root).as_posix())
                for member in archive.infolist()
                if member.filename.startswith("production/") and not member.is_dir()
            }
        for platform in platforms:
            if platform in result:
                result[platform].update(entries)
    return result, issues, versions


def read_png_references(paths: list[Path]) -> dict:
    groups: dict[str, dict[str, set[str]]] = {"typedPngReferences": {}, "nonstandardTemplateUsage": {}}
    for path in paths:
        document = read_json(path)
        sections = {
            "typedPngReferences": document["references"],
            "nonstandardTemplateUsage": document.get("nonstandardTemplateUsage", {}).get("references", []),
        }
        for scope, references in sections.items():
            for reference in references:
                logical = reference["logicalPath"]
                if logical.endswith(".png"):
                    groups[scope].setdefault(logical, set()).update(reference.get("owners", []))
    return {scope: resolve_reference_names(owners) for scope, owners in groups.items()}


def resolve_reference_names(owners: dict[str, set[str]]) -> tuple[list[ResourceReference], dict]:
    references = [texture_reference(path) for path in sorted(owners)]
    return references, {
        reference.logical_path: sorted(owners[path])
        for path, reference in zip(sorted(owners), references)
    }
# //// /从公布归档建立分平台的解压结果目录 ////


# //// 检查下载目录优先和包内同规格回退 [@x380kkm 2026-09-20] ////
def locate_reference(
    reference: ResourceReference, bundle: ClientBundle, resolution: str,
    downloaded: dict[str, StoredAsset],
) -> tuple[str, str, StoredAsset] | None:
    download_path, bundle_path = resource_paths(reference, bundle, resolution)
    if download_path in downloaded:
        return "download", download_path, downloaded[download_path]
    if bundle_path in bundle.entries:
        return "bundle", bundle_path, bundle.entries[bundle_path]
    return None


def suffix_locations(*storages: dict[str, StoredAsset]) -> dict[str, list[str]]:
    result: dict[str, list[str]] = {}
    for storage in storages:
        for path in storage:
            parts = path.split("/")
            if len(parts) == 4:
                result.setdefault("/".join(parts[-2:]), []).append(path)
    return result


def audit_references(
    references: list[ResourceReference], bundle: ClientBundle, resolution: str,
    downloaded: dict[str, StoredAsset], manifest: dict, owners: dict,
) -> dict:
    required = Counter()
    resolved = Counter()
    missing = []
    unregistered = []
    size_mismatches = []
    locations = suffix_locations(downloaded, bundle.entries)
    for reference in references:
        download_path, bundle_path = resource_paths(reference, bundle, resolution)
        required[bundle_path.split("/")[1]] += 1
        found = locate_reference(reference, bundle, resolution, downloaded)
        if found is None:
            other_resolution = "medium" if resolution == "small" else "small"
            other = locate_reference(reference, bundle, other_resolution, downloaded)
            missing.append({
                "logicalPath": reference.logical_path, "suffix": reference.suffix,
                "sourcePath": reference.source_path, "downloadPath": download_path,
                "bundlePath": bundle_path, "otherResolutionReachable": other is not None,
                "sameHashOtherPaths": sorted(set(locations.get(reference.suffix, []))),
                "owners": owners.get(reference.logical_path, []),
            })
            continue
        storage, path, asset = found
        resolved[storage + ":" + path.split("/")[1]] += 1
        if storage == "download":
            expected = manifest.get(path)
            if expected is None:
                unregistered.append(path)
            elif expected[0] != asset.size:
                size_mismatches.append({"path": path, "expected": expected[0], "actual": asset.size})
    return {
        "referenceCount": len(references), "resolvedCount": sum(resolved.values()),
        "requiredFamilies": dict(sorted(required.items())),
        "resolvedFamilies": dict(sorted(resolved.items())),
        "resolutionOnlyGapCount": sum(item["otherResolutionReachable"] for item in missing),
        "missingCount": len(missing), "missing": missing,
        "unregisteredDownloads": sorted(set(unregistered)), "sizeMismatches": size_mismatches,
    }
# //// /检查下载目录优先和包内同规格回退 ////


# //// 生成两平台的图像规格可达性报告 [@x380kkm 2026-09-20] ////
def audit(root: Path, packages: list[Path], reference_paths: list[Path]) -> dict:
    root = root.resolve(strict=True)
    document = read_json(root / "path")
    manifests = read_entities(root)
    downloaded, issues, versions = read_download_entries(root, document)
    reference_scopes = read_png_references(reference_paths)
    bundles = {bundle.platform: bundle for bundle in map(read_client_package, packages)}
    if set(bundles) != set(PLATFORMS):
        raise ValueError("客户端输入需要各提供一个 iOS IPA 和 Android APK")
    report = {
        "cdnRoot": str(root), "referenceReports": list(map(str, reference_paths)),
        "contract": {
            "assetDirectories": ["production"], "assetReadKind": 2,
            "tutorialFilelistFamily": "ios_bundle", "pngTextureScale": {"medium": 1, "small": 0.7},
            "platformDependentTextureExtension": ".atf.deflate",
        },
        "downloadVersionChain": versions, "archiveIssues": issues, "platforms": {},
    }
    for platform, bundle in bundles.items():
        manifest_names = [name for name in manifests if name.endswith(f"-{platform}_medium.csv")]
        if len(manifest_names) != 1:
            raise ValueError(f"平台实体清单数量异常: {platform}: {manifest_names}")
        manifest = manifests[manifest_names[0]]
        medium = [
            ResourceReference(None, "/".join(path.split("/")[-2:]), True, source_path=path)
            for path in sorted(manifest) if path.startswith("production/medium_upload/")
        ]
        platform_report = {
            "package": str(bundle.path), "manifest": manifest_names[0], "filelists": bundle.filelists,
            "commonFilelistMembers": len(bundle.common_members),
            "tutorialFilelistMembers": len(bundle.tutorial_members), "resolutions": {},
        }
        for resolution in RESOLUTIONS:
            results = {
                "mediumInventory": audit_references(medium, bundle, resolution, downloaded[platform], manifest, {}),
            }
            for scope, (references, owners) in reference_scopes.items():
                png_references = [reference for reference in references if not reference.platform_dependent]
                results[scope] = audit_references(png_references, bundle, resolution, downloaded[platform], manifest, owners)
            platform_report["resolutions"][resolution] = results
        platform_report["platformTextures"] = {
            scope: audit_references(
                [reference for reference in references if reference.platform_dependent],
                bundle, "medium", downloaded[platform], manifest, owners,
            ) for scope, (references, owners) in reference_scopes.items()
        }
        report["platforms"][platform] = platform_report
    return report


def main() -> int:
    parser = argparse.ArgumentParser(description="按 CN 客户端寻址规则审计两种图像规格")
    parser.add_argument("--cdn-root", required=True, type=Path)
    parser.add_argument("--client-package", required=True, action="append", type=Path)
    parser.add_argument("--references", required=True, action="append", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    report = audit(args.cdn_root, args.client_package, args.references)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    summary = {
        platform: {
            resolution: {name: {key: result[key] for key in ("referenceCount", "resolvedCount", "missingCount", "resolutionOnlyGapCount")}
                         for name, result in results.items()}
            for resolution, results in data["resolutions"].items()
        } for platform, data in report["platforms"].items()
    }
    print(json.dumps({"output": str(args.output), "platforms": summary}, ensure_ascii=False))
    return 0
# //// /生成两平台的图像规格可达性报告 ////


if __name__ == "__main__":
    raise SystemExit(main())
