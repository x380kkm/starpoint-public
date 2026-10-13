# audience: internal
# # stage-cn-character-full-shots
#
# 此工具把显式映射的角色全身立绘写入新的 CN CDN 目录.
# 输出保留源 CDN, 原子发布派生目录并生成变更清单.

# /// script
# requires-python = ">=3.12"
# dependencies = [
#   "Pillow",
# ]
# ///

from __future__ import annotations

import argparse
import copy
import csv
import hashlib
import io
import json
import os
import shutil
import sys
import tempfile
import uuid
import zipfile
from collections.abc import Sequence
from pathlib import Path

from cn_asset_path_manifest import (
    PATH_MANIFEST_NAME,
    refresh_cn_asset_path_manifest_entries,
)
from cn_character_full_shot_assets import (
    OUTPUT_MANIFEST_NAME,
    PortraitStagingError,
    load_mapping_spec,
)
from cn_character_full_shot_plan import (
    PayloadArtifact,
    build_replacement_plan,
)
from cn_character_full_shot_previews import add_illustration_previews
from cn_character_portrait_diff import publish_portrait_diff

ZIP64_EXTRA_FIELD_ID = 0x0001
LEGACY_EXTRACT_VERSION_BY_COMPRESSION = {
    zipfile.ZIP_STORED: 10,
    zipfile.ZIP_DEFLATED: 20,
}


# //// 保留 ZIP 成员的非 ZIP64 扩展字段 [@x380kkm 2026-08-22] ////
def _remove_zip64_extra_fields(extra: bytes) -> bytes:
    output = bytearray()
    offset = 0
    while offset + 4 <= len(extra):
        field_id = int.from_bytes(extra[offset : offset + 2], "little")
        field_size = int.from_bytes(extra[offset + 2 : offset + 4], "little")
        field_end = offset + 4 + field_size
        if field_end > len(extra):
            break
        if field_id != ZIP64_EXTRA_FIELD_ID:
            output.extend(extra[offset:field_end])
        offset = field_end
    output.extend(extra[offset:])
    return bytes(output)


# //// 原子构造独立的可挂载 CDN 目录 [@x380kkm 2026-08-21] ////
def _link_or_copy(source: str, target: str) -> str:
    try:
        os.link(source, target)
    except OSError:
        shutil.copy2(source, target)
    return target


def _clone_cdn(source_root: Path, staging_root: Path) -> None:
    for current_root, directory_names, file_names in os.walk(source_root):
        current = Path(current_root)
        if current.is_symlink():
            raise PortraitStagingError(
                "unsupported_symlink", "CN CDN cannot contain symlink directories"
            )
        directory_names.sort()
        file_names.sort()
        relative = current.relative_to(source_root)
        target_directory = staging_root / relative
        target_directory.mkdir(parents=True, exist_ok=True)
        for directory_name in directory_names:
            if (current / directory_name).is_symlink():
                raise PortraitStagingError(
                    "unsupported_symlink", "CN CDN cannot contain symlink directories"
                )
        for file_name in file_names:
            source_file = current / file_name
            if source_file.is_symlink():
                raise PortraitStagingError(
                    "unsupported_symlink", "CN CDN cannot contain symlink files"
                )
            if relative == Path(".") and file_name in {
                ".starpoint-cn-cdn.json",
                OUTPUT_MANIFEST_NAME,
            }:
                continue
            _link_or_copy(str(source_file), str(target_directory / file_name))


def _rewrite_zip(
    source_path: Path,
    output_path: Path,
    replacements: dict[str, PayloadArtifact],
) -> None:
    temporary_path = output_path.with_name(
        f".{output_path.name}.{uuid.uuid4().hex}.tmp"
    )
    try:
        with (
            zipfile.ZipFile(source_path, "r") as source,
            zipfile.ZipFile(temporary_path, "w") as output,
        ):
            output.comment = source.comment
            replaced: set[str] = set()
            for source_info in source.infolist():
                target_info = copy.copy(source_info)
                target_info.extra = _remove_zip64_extra_fields(target_info.extra)
                target_info.extract_version = LEGACY_EXTRACT_VERSION_BY_COMPRESSION.get(
                    target_info.compress_type, target_info.extract_version
                )
                replacement = replacements.get(source_info.filename)
                if replacement is not None:
                    target_info.file_size = replacement.size
                    with (
                        replacement.payload_path.open("rb") as replacement_entry,
                        output.open(target_info, "w") as output_entry,
                    ):
                        shutil.copyfileobj(
                            replacement_entry, output_entry, length=1024 * 1024
                        )
                    replaced.add(source_info.filename)
                else:
                    with (
                        source.open(source_info, "r") as source_entry,
                        output.open(target_info, "w") as output_entry,
                    ):
                        shutil.copyfileobj(
                            source_entry, output_entry, length=1024 * 1024
                        )
            if replaced != set(replacements):
                raise PortraitStagingError(
                    "archive_entry_disappeared",
                    "a planned full-shot archive entry is missing",
                    archive=str(source_path),
                )
        os.replace(temporary_path, output_path)
    finally:
        temporary_path.unlink(missing_ok=True)


def _patch_entity_csv(
    path: Path,
    replacements: dict[str, PayloadArtifact],
    target_version: str,
) -> int:
    original = path.read_bytes()
    has_bom = original.startswith(b"\xef\xbb\xbf")
    text = original.decode("utf-8-sig")
    output_lines: list[str] = []
    changed = 0
    for line in text.splitlines(keepends=True):
        body = line.rstrip("\r\n")
        ending = line[len(body) :]
        if not body:
            output_lines.append(line)
            continue
        fields = next(csv.reader([body], strict=True))
        replacement = replacements.get(fields[0] if fields else "")
        if replacement is None:
            output_lines.append(line)
            continue
        if len(fields) < 4:
            raise PortraitStagingError(
                "invalid_entity_list", "matching entity row is truncated"
            )
        fields[1] = target_version
        fields[2] = str(replacement.size)
        fields[3] = replacement.sha256_encoded
        writer_buffer = io.StringIO(newline="")
        csv.writer(writer_buffer, lineterminator="").writerow(fields)
        output_lines.append(writer_buffer.getvalue() + ending)
        changed += 1
    encoded = "".join(output_lines).encode("utf-8")
    if has_bom:
        encoded = b"\xef\xbb\xbf" + encoded
    temporary_path = path.with_name(f".{path.name}.{uuid.uuid4().hex}.tmp")
    try:
        temporary_path.write_bytes(encoded)
        os.replace(temporary_path, path)
    finally:
        temporary_path.unlink(missing_ok=True)
    return changed


def _file_record(path: Path, root: Path) -> dict[str, object]:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return {
        "bytes": path.stat().st_size,
        "path": path.relative_to(root).as_posix(),
        "sha256": digest.hexdigest(),
    }


def _write_json_atomic(path: Path, payload: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary_path = path.with_name(f".{path.name}.{uuid.uuid4().hex}.tmp")
    try:
        temporary_path.write_text(
            json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        os.replace(temporary_path, path)
    finally:
        temporary_path.unlink(missing_ok=True)


def _coverage_summary(entries: list[dict[str, object]]) -> dict[str, object]:
    by_region: dict[str, int] = {}
    replacement_successes = 0
    cn_fallbacks = 0
    for entry in entries:
        region = str(entry["source_region"])
        by_region[region] = by_region.get(region, 0) + 1
        if entry["replacement_status"] == "fallback_only":
            if region == "cn":
                cn_fallbacks += 1
            continue
        replacement_successes += 1
    return {
        "by_region": dict(sorted(by_region.items())),
        "cn_fallbacks": cn_fallbacks,
        "replacement_successes": replacement_successes,
        "total": len(entries),
    }


def _load_existing_manifest(cdn_root: Path) -> dict[str, object] | None:
    path = cdn_root / OUTPUT_MANIFEST_NAME
    if not path.is_file():
        return None
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise PortraitStagingError(
            "invalid_existing_manifest",
            "existing full-shot manifest cannot be read",
        ) from error
    if (
        not isinstance(payload, dict)
        or payload.get("schema_version") != 1
        or not isinstance(payload.get("entries"), list)
        or not isinstance(payload.get("changed_files"), list)
    ):
        raise PortraitStagingError(
            "invalid_existing_manifest",
            "existing full-shot manifest has an unsupported shape",
        )
    return payload


def _merge_manifest_entries(
    existing: Sequence[object],
    additions: Sequence[dict[str, object]],
) -> list[dict[str, object]]:
    merged: dict[str, dict[str, object]] = {}
    for raw_entry in [*existing, *additions]:
        if not isinstance(raw_entry, dict):
            raise PortraitStagingError(
                "invalid_existing_manifest",
                "full-shot manifest entry must be an object",
            )
        target = raw_entry.get("target")
        if not isinstance(target, str) or not target:
            raise PortraitStagingError(
                "invalid_existing_manifest",
                "full-shot manifest entry target is invalid",
            )
        merged[target] = raw_entry
    return [merged[target] for target in sorted(merged)]


def _merge_changed_files(
    existing: Sequence[object],
    additions: Sequence[dict[str, object]],
) -> list[dict[str, object]]:
    merged: dict[str, dict[str, object]] = {}
    for raw_change in existing:
        if (
            not isinstance(raw_change, dict)
            or not isinstance(raw_change.get("original"), dict)
            or not isinstance(raw_change.get("staged"), dict)
        ):
            raise PortraitStagingError(
                "invalid_existing_manifest",
                "full-shot changed-file record is invalid",
            )
        path = raw_change["staged"].get("path")
        if not isinstance(path, str) or not path:
            raise PortraitStagingError(
                "invalid_existing_manifest",
                "full-shot changed-file path is invalid",
            )
        merged[path] = raw_change
    for addition in additions:
        path = addition["staged"]["path"]
        previous = merged.get(path)
        merged[path] = {
            "original": previous["original"] if previous is not None else addition["original"],
            "staged": addition["staged"],
        }
    return [merged[path] for path in sorted(merged)]


def stage_portrait_cdn(
    cdn_root: Path,
    source_root: Path,
    spec_path: Path,
    output_root: Path,
) -> dict[str, object]:
    resolved_cdn = cdn_root.resolve(strict=True)
    resolved_source = source_root.resolve(strict=True)
    resolved_output = output_root.resolve(strict=False)
    if not resolved_cdn.is_dir() or not resolved_source.is_dir():
        raise PortraitStagingError(
            "invalid_input_root", "CDN and source roots must be directories"
        )
    if resolved_output.exists():
        raise PortraitStagingError(
            "output_exists", "output CDN already exists", output=str(resolved_output)
        )
    if resolved_output.is_relative_to(resolved_cdn) or resolved_cdn.is_relative_to(
        resolved_output
    ):
        raise PortraitStagingError(
            "unsafe_output_root",
            "output CDN must not contain or be inside the source CDN",
        )
    if resolved_output.is_relative_to(
        resolved_source
    ) or resolved_source.is_relative_to(resolved_output):
        raise PortraitStagingError(
            "unsafe_output_root",
            "output CDN must be separate from the portrait source root",
        )

    resolved_output.parent.mkdir(parents=True, exist_ok=True)
    staging_root: Path | None = None
    spool_root: Path | None = None
    try:
        staging_root = Path(
            tempfile.mkdtemp(
                prefix=f".{resolved_output.name}.portrait-staging-",
                dir=resolved_output.parent,
            )
        )
        spool_root = Path(
            tempfile.mkdtemp(
                prefix=f".{resolved_output.name}.portrait-spool-",
                dir=resolved_output.parent,
            )
        )
        existing_manifest = _load_existing_manifest(resolved_cdn)
        mappings = load_mapping_spec(spec_path)
        archive_replacements, asset_replacements, manifest_entries = (
            build_replacement_plan(
                resolved_cdn,
                resolved_source,
                mappings,
                spool_root,
            )
        )
        _clone_cdn(resolved_cdn, staging_root)
        preview_entries = add_illustration_previews(
            resolved_cdn, resolved_source, mappings, spool_root,
            archive_replacements, asset_replacements,
        )
        changed_relative_paths: set[Path] = set()
        for relative_archive, replacements in sorted(
            archive_replacements.items(), key=lambda item: str(item[0])
        ):
            _rewrite_zip(
                resolved_cdn / relative_archive,
                staging_root / relative_archive,
                replacements,
            )
            changed_relative_paths.add(relative_archive)
        path_manifest_changes = refresh_cn_asset_path_manifest_entries(
            staging_root,
            archive_replacements,
        )
        if path_manifest_changes:
            changed_relative_paths.add(Path(PATH_MANIFEST_NAME))

        incremental_update = publish_portrait_diff(staging_root, asset_replacements, spool_root)
        changed_relative_paths.add(Path(PATH_MANIFEST_NAME))
        entity_changes = 0
        for directory_name in ("entities", "EntityLists"):
            for source_csv in sorted((resolved_cdn / directory_name).glob("*.csv")):
                if not source_csv.is_file():
                    continue
                relative_csv = source_csv.relative_to(resolved_cdn)
                changed = _patch_entity_csv(
                    staging_root / relative_csv, asset_replacements, incremental_update["target_version"]
                )
                if changed:
                    entity_changes += changed
                    changed_relative_paths.add(relative_csv)
        if entity_changes == 0:
            raise PortraitStagingError(
                "entity_patch_missing", "no entity CSV rows were updated"
            )

        changed_files = []
        for relative_path in sorted(
            changed_relative_paths, key=lambda path: path.as_posix()
        ):
            changed_files.append(
                {
                    "original": _file_record(
                        resolved_cdn / relative_path, resolved_cdn
                    ),
                    "staged": _file_record(staging_root / relative_path, staging_root),
                }
            )
        if existing_manifest is not None:
            manifest_entries = _merge_manifest_entries(
                existing_manifest["entries"], manifest_entries
            )
            changed_files = _merge_changed_files(
                existing_manifest["changed_files"], changed_files
            )
        coverage = _coverage_summary(manifest_entries)
        manifest: dict[str, object] = {
            "incremental_update": incremental_update,
            "illustration_previews": preview_entries,
            "changed_files": changed_files,
            "coverage": coverage,
            "entries": manifest_entries,
            "omitted_base_markers": [".starpoint-cn-cdn.json"],
            "schema_version": 1,
            "status": "ready_with_fallbacks" if coverage["cn_fallbacks"] else "ready",
        }
        _write_json_atomic(staging_root / OUTPUT_MANIFEST_NAME, manifest)
        os.replace(staging_root, resolved_output)
        return manifest
    finally:
        if staging_root is not None and staging_root.exists():
            shutil.rmtree(staging_root)
        if spool_root is not None and spool_root.exists():
            shutil.rmtree(spool_root)


# //// /原子构造独立的可挂载 CDN 目录 ////


# //// 运行 full-shot staging 命令 [@x380kkm 2026-08-21] ////
def build_argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Stage mapped CN character full-shot replacements."
    )
    parser.add_argument("--cdn-root", type=Path, required=True)
    parser.add_argument("--source-root", type=Path, required=True)
    parser.add_argument("--spec", type=Path, required=True)
    parser.add_argument("--output-root", type=Path, required=True)
    parser.add_argument("--report", type=Path)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_argument_parser().parse_args(argv)
    report_path = args.report or args.output_root.with_name(
        f"{args.output_root.name}.portrait-report.json"
    )
    try:
        manifest = stage_portrait_cdn(
            args.cdn_root, args.source_root, args.spec, args.output_root
        )
    except PortraitStagingError as error:
        failure = {
            "errors": [error.as_record()],
            "schema_version": 1,
            "status": "failed",
        }
        _write_json_atomic(report_path, failure)
        print(json.dumps(failure, ensure_ascii=False, sort_keys=True), file=sys.stderr)
        return 1
    success = {
        "manifest": str((args.output_root / OUTPUT_MANIFEST_NAME).resolve()),
        "schema_version": 1,
        "status": manifest["status"],
    }
    _write_json_atomic(report_path, success)
    print(json.dumps(success, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
# //// /运行 full-shot staging 命令 ////
