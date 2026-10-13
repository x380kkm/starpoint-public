# audience: internal
# # audit-cn-cdn-storage
#
# 此脚本按 path 实际列出的归档核对两个平台的 EntityLists, 并检查指定逻辑资源的可达性.
# verify-contents 读取客户端摘要所对应的实体字节, 报告区分目录存在与内容匹配.

from __future__ import annotations

import argparse
import base64
import csv
import hashlib
import io
import json
import re
import sys
import zipfile
from collections import Counter, defaultdict
from pathlib import Path
from urllib.parse import unquote, urlparse


ASSET_SALT = "K6R9T9Hz22OpeIGEWB0ui6c6PYFQnJGy"
ASSET_DIRECTORIES = ("upload", "medium_upload", "ios_upload", "android_upload")


# //// 读取资源清单和命令参数 [@x380kkm 2026-09-20] ////
def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cdn-root", type=Path, required=True)
    parser.add_argument("--references", type=Path)
    parser.add_argument("--client-package", action="append", default=[], type=Path)
    parser.add_argument("--android-bundle", type=Path)
    parser.add_argument("--verify-contents", action="store_true")
    parser.add_argument("--output", type=Path)
    return parser.parse_args()


def read_json(path: Path) -> object:
    return json.loads(path.read_text(encoding="utf-8-sig"))


def read_entities(root: Path) -> dict[str, dict[str, tuple[int, str, str]]]:
    manifests = {}
    for path in sorted((root / "entities").glob("*-*_medium.csv")):
        records = {}
        with path.open(encoding="utf-8-sig", newline="") as stream:
            for row in csv.reader(stream):
                if not row:
                    continue
                if len(row) != 5:
                    raise ValueError(f"invalid EntityLists row: {path}")
                entry, _, size, digest, kind = row
                value = (int(size), digest, kind)
                if entry in records and records[entry] != value:
                    raise ValueError(f"conflicting entity: {path.name}: {entry}")
                records[entry] = value
        manifests[path.name] = records
    if not manifests:
        raise ValueError("CDN has no platform EntityLists")
    return manifests


def archive_paths(root: Path, document: dict) -> tuple[list[Path], list[dict]]:
    groups = [document["full"], *document.get("diff", [])]
    paths = {}
    issues = []
    for group in groups:
        for item in group.get("archive", []):
            parts = Path(unquote(urlparse(item["location"]).path)).parts
            indexes = [index for index, part in enumerate(parts) if part.startswith("archive-")]
            if len(indexes) != 1:
                raise ValueError(f"invalid archive location: {item['location']}")
            relative = Path(*parts[indexes[0]:])
            path = (root / relative).resolve()
            if not path.is_relative_to(root):
                raise ValueError(f"archive escapes CDN: {relative}")
            if path in paths:
                continue
            paths[path] = True
            if not path.is_file():
                issues.append({"kind": "missing_download_archive", "path": relative.as_posix()})
            elif path.stat().st_size != item["size"]:
                issues.append({"kind": "download_size_mismatch", "path": relative.as_posix(),
                               "expected": item["size"], "actual": path.stat().st_size})
    return list(paths), issues
# //// /读取资源清单和命令参数 ////


# //// 按客户端摘要核对实际实体 [@x380kkm 2026-09-20] ////
def entity_digest(stream) -> str:
    value = hashlib.file_digest(stream, "sha256").digest()
    return base64.b64encode(value).decode("ascii").rstrip("=").replace("+", "_").replace("/", "-")


def verify_entities(root: Path, archives: list[Path], manifests: dict, verify_contents: bool) -> tuple[dict, list]:
    expected = defaultdict(set)
    for records in manifests.values():
        for entry, record in records.items():
            expected[entry].add(record)
    resolved = {}
    issues = []
    for archive_path in archives:
        if not archive_path.is_file():
            continue
        try:
            with zipfile.ZipFile(archive_path) as archive:
                for member in archive.infolist():
                    candidates = [record for record in expected.get(member.filename, ())
                                  if record[0] == member.file_size
                                  and (member.filename, record) not in resolved]
                    if not candidates:
                        continue
                    digest = None
                    if verify_contents:
                        with archive.open(member) as stream:
                            digest = entity_digest(stream)
                    for record in candidates:
                        if not verify_contents or digest == record[1]:
                            resolved[(member.filename, record)] = archive_path.relative_to(root).as_posix()
        except (OSError, RuntimeError, zipfile.BadZipFile) as error:
            issues.append({"kind": "unreadable_archive", "path": archive_path.relative_to(root).as_posix(),
                           "message": str(error)})
    return resolved, issues


def platform_coverage(manifests: dict, resolved: dict) -> dict:
    result = {}
    for name, records in manifests.items():
        missing = [{"entry": entry, "size": record[0], "digest": record[1], "assetKind": record[2]}
                   for entry, record in records.items() if (entry, record) not in resolved]
        result[name] = {"entityCount": len(records), "matchedCount": len(records) - len(missing),
                        "missingByKind": dict(Counter(item["assetKind"] for item in missing)), "missing": missing}
    return result
# //// /按客户端摘要核对实际实体 ////


# //// 将逻辑资源关联到每个平台的清单与归档 [@x380kkm 2026-09-20] ////
def logical_entries(logical_path: str) -> list[str]:
    normalized = logical_path.replace("\\", "/").lstrip("/")
    digest = hashlib.sha1((normalized + ASSET_SALT).encode("utf-8")).hexdigest()
    return [f"production/{directory}/{digest[:2]}/{digest[2:]}" for directory in ASSET_DIRECTORIES]


def read_bundle_files(archive: zipfile.ZipFile, prefix: str) -> set[str]:
    files = set()
    registered = set()
    for member in archive.infolist():
        relative = member.filename.removeprefix(prefix)
        if not member.filename.startswith(prefix) or not re.fullmatch(r"production/(?:[a-z_]+_)?bundle/[a-f0-9]{2}/[a-f0-9]{38}", relative):
            continue
        suffix = "/".join(relative.split("/")[-2:])
        files.add(suffix)
    for family in ("bundle", "ios_bundle"):
        for kind in ("amf", "encrypt", "copy", "compress", "png", "atf"):
            entry = logical_entries(f"{family}_{kind}.filelist")[0].replace("/upload/", "/bundle/")
            name = prefix + entry
            if name in archive.namelist():
                registered.update(archive.read(name).decode("utf-8-sig").splitlines())
    return files & registered


def client_bundle_files(packages: list[Path], android_bundle: Path | None) -> dict:
    result = {}
    for package in packages:
        with zipfile.ZipFile(package) as source:
            if package.suffix.lower() == ".apk":
                with zipfile.ZipFile(io.BytesIO(source.read("assets/bundle.zip"))) as bundle:
                    result["android"] = read_bundle_files(bundle, "")
            else:
                roots = {name.split(".app/", 1)[0] + ".app/asset/" for name in source.namelist() if ".app/asset/" in name}
                if len(roots) != 1:
                    raise ValueError(f"IPA asset root is ambiguous: {package}")
                result["ios"] = read_bundle_files(source, roots.pop())
    if android_bundle:
        with zipfile.ZipFile(android_bundle) as bundle:
            result["android"] = read_bundle_files(bundle, "")
    return result


def reference_coverage(document: dict, manifests: dict, resolved: dict, bundle_files: dict) -> dict:
    references = document.get("references", [])
    gaps = []
    matched = Counter()
    bundled = Counter()
    for reference in references:
        logical_path = reference["logicalPath"]
        paths = [entry for logical in [logical_path, *reference.get("alternativePaths", [])] for entry in logical_entries(logical)]
        for manifest, records in manifests.items():
            platform = "android" if "android" in manifest else "ios"
            suffixes = {"/".join(entry.split("/")[-2:]) for entry in paths}
            matches = [(entry, records[entry]) for entry in paths if entry in records]
            if suffixes & bundle_files.get(platform, set()):
                matched[manifest] += 1
                bundled[manifest] += 1
            elif not matches:
                gaps.append({**reference, "manifest": manifest, "kind": "missing_logical_entity"})
            elif not any(match in resolved for match in matches):
                gaps.append({**reference, "manifest": manifest, "kind": "missing_logical_storage"})
            else:
                matched[manifest] += 1
    return {"scope": document.get("scope"), "referenceCount": len(references),
            "matchedByManifest": dict(matched), "bundledByManifest": dict(bundled), "gaps": gaps}
# //// /将逻辑资源关联到每个平台的清单与归档 ////


# //// 输出可达性与内容证据 [@x380kkm 2026-09-20] ////
def main() -> int:
    args = parse_args()
    root = args.cdn_root.resolve(strict=True)
    document = read_json(root / "path")
    manifests = read_entities(root)
    archives, issues = archive_paths(root, document)
    resolved, archive_issues = verify_entities(root, archives, manifests, args.verify_contents)
    issues.extend(archive_issues)
    platforms = platform_coverage(manifests, resolved)
    report = {"cdnRoot": str(root), "targetAssetVersion": document["info"]["target_asset_version"],
              "verification": "client_entity_digest" if args.verify_contents else "entry_path_and_size",
              "archiveCount": len(archives), "issues": issues, "platforms": platforms}
    if args.references:
        bundles = client_bundle_files(args.client_package, args.android_bundle)
        report["references"] = reference_coverage(read_json(args.references), manifests, resolved, bundles)
    report["status"] = "gaps" if issues or any(item["missing"] for item in platforms.values()) \
        or report.get("references", {}).get("gaps") else "ok"
    output = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(output, encoding="utf-8")
        print(json.dumps({"report": str(args.output), "status": report["status"],
                          "platforms": {name: {key: value for key, value in data.items() if key != "missing"}
                                        for name, data in platforms.items()}, "issues": issues}, ensure_ascii=False))
    else:
        sys.stdout.write(output)
    return 0 if report["status"] == "ok" else 1


if __name__ == "__main__":
    raise SystemExit(main())
# //// /输出可达性与内容证据 ////
