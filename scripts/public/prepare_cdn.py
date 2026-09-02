# audience: external
# # prepare-cdn
#
# 此程序把一个完整 CN CDN 根和若干覆盖目录合并到本地开发目录.
# 输入目录保持原有相对路径, 后提供的覆盖目录替换同名文件.

from __future__ import annotations

import argparse
import json
import os
import shutil
import tempfile
from collections.abc import Iterator, Sequence
from dataclasses import dataclass
from pathlib import Path

DEFAULT_DESTINATION = Path(".cdn/cn")
LAYOUT_NAME = ".starpoint-cdn-layout.json"
LAYOUT_SCHEMA = "starpoint-cdn-layout/v1"


# //// 表示一个输入目录 [@x380kkm 2026-09-02] ////
@dataclass(frozen=True)
class InputLayer:
    root: Path
    display_path: str
# //// /表示一个输入目录 ////


# //// 表示一个待写入文件 [@x380kkm 2026-09-02] ////
@dataclass(frozen=True)
class PlannedFile:
    source: Path
    relative_path: str
    size: int
# //// /表示一个待写入文件 ////


# //// 表示 CDN 准备错误 [@x380kkm 2026-09-02] ////
class CdnPreparationError(ValueError):
    pass
# //// /表示 CDN 准备错误 ////


# //// 生成可公开记录的显示路径 [@x380kkm 2026-09-02] ////
def display_path(path: Path) -> str:
    if path.is_absolute():
        return path.name or "."
    return path.as_posix()
# //// /生成可公开记录的显示路径 ////


# //// 解析输入目录 [@x380kkm 2026-09-02] ////
def resolve_layer(path: Path, label: str) -> InputLayer:
    try:
        root = path.expanduser().resolve(strict=True)
    except OSError as error:
        raise CdnPreparationError(f"{label}目录不存在: {display_path(path)}") from error
    if not root.is_dir():
        raise CdnPreparationError(f"{label}路径不是目录: {display_path(path)}")
    return InputLayer(root=root, display_path=display_path(path))
# //// /解析输入目录 ////


# //// 按精确大小写查找目录项 [@x380kkm 2026-09-02] ////
def exact_entry(parent: Path, name: str) -> Path | None:
    try:
        with os.scandir(parent) as iterator:
            entries = list(iterator)
    except OSError:
        return None
    exact = next((entry for entry in entries if entry.name == name), None)
    if exact is not None:
        return Path(exact.path)
    folded = next((entry for entry in entries if entry.name.casefold() == name.casefold()), None)
    if folded is not None:
        raise CdnPreparationError(f"输入包含大小写冲突: {folded.name} 与 {name}")
    return None
# //// /按精确大小写查找目录项 ////


# //// 校验完整 CDN 根的入口文件 [@x380kkm 2026-09-02] ////
def validate_source_layout(root: Path) -> None:
    path_entry = exact_entry(root, "path")
    if path_entry is None or not path_entry.is_file():
        raise CdnPreparationError("源 CDN 缺少根级 path 文件")
    entities_entry = exact_entry(root, "entities")
    path_file_entry = (
        exact_entry(entities_entry, "PathFile.csv")
        if entities_entry is not None and entities_entry.is_dir()
        else None
    )
    entity_lists_entry = exact_entry(root, "EntityLists")
    has_path_file = path_file_entry is not None and path_file_entry.is_file()
    has_entity_lists = entity_lists_entry is not None and entity_lists_entry.is_dir()
    if not has_path_file and not has_entity_lists:
        raise CdnPreparationError(
            "源 CDN 缺少 entities/PathFile.csv 或 EntityLists 目录"
        )
# //// /校验完整 CDN 根的入口文件 ////


# //// 校验输入和输出目录彼此分离 [@x380kkm 2026-09-02] ////
def validate_directory_boundaries(destination: Path, layers: Sequence[InputLayer]) -> None:
    for layer in layers:
        if (
            destination == layer.root
            or destination.is_relative_to(layer.root)
            or layer.root.is_relative_to(destination)
        ):
            raise CdnPreparationError(
                f"目标目录与输入目录重叠: {layer.display_path}"
            )
# //// /校验输入和输出目录彼此分离 ////


# //// 判断目录项是否通过链接改变路径边界 [@x380kkm 2026-09-02] ////
def is_link(path: Path) -> bool:
    return path.is_symlink() or path.is_junction()
# //// /判断目录项是否通过链接改变路径边界 ////


# //// 校验一个相对文件路径 [@x380kkm 2026-09-02] ////
def validate_relative_path(relative_path: Path) -> str:
    if relative_path.is_absolute() or any(
        part in ("", ".", "..") for part in relative_path.parts
    ):
        raise CdnPreparationError(f"输入包含越界路径: {relative_path}")
    normalized = relative_path.as_posix()
    if any(character in normalized for character in ("\\", "\x00", "\t", "\n", "\r")):
        raise CdnPreparationError(f"输入包含无效路径字符: {normalized}")
    if normalized.casefold() == LAYOUT_NAME.casefold():
        if normalized != LAYOUT_NAME:
            raise CdnPreparationError(
                f"路径与布局文件发生大小写冲突: {normalized}"
            )
        return ""
    return normalized
# //// /校验一个相对文件路径 ////


# //// 按稳定顺序枚举输入目录文件 [@x380kkm 2026-09-02] ////
def iter_layer_files(layer: InputLayer) -> Iterator[PlannedFile]:
    pending = [layer.root]
    while pending:
        directory = pending.pop()
        try:
            entries = sorted(os.scandir(directory), key=lambda entry: entry.name.casefold())
        except OSError as error:
            raise CdnPreparationError(
                f"无法读取输入目录: {layer.display_path}"
            ) from error
        for entry in entries:
            path = Path(entry.path)
            relative_path = path.relative_to(layer.root)
            normalized = validate_relative_path(relative_path)
            if is_link(path):
                raise CdnPreparationError(
                    f"输入链接可能越过目录边界: {relative_path.as_posix()}"
                )
            if entry.is_dir(follow_symlinks=False):
                if not normalized:
                    raise CdnPreparationError(
                        f"输入目录占用布局文件名: {relative_path.as_posix()}"
                    )
                pending.append(path)
                continue
            if not entry.is_file(follow_symlinks=False):
                raise CdnPreparationError(
                    f"输入包含不支持的目录项: {relative_path.as_posix()}"
                )
            if not normalized:
                continue
            yield PlannedFile(
                source=path,
                relative_path=normalized,
                size=entry.stat(follow_symlinks=False).st_size,
            )
# //// /按稳定顺序枚举输入目录文件 ////


# //// 注册路径大小写和文件类型 [@x380kkm 2026-09-02] ////
def register_path(
    registry: dict[str, tuple[str, str]], relative_path: str, entry_type: str
) -> None:
    folded = relative_path.casefold()
    existing = registry.get(folded)
    if existing is None:
        registry[folded] = (relative_path, entry_type)
        return
    existing_path, existing_type = existing
    if existing_path != relative_path:
        raise CdnPreparationError(
            f"输入包含大小写冲突: {existing_path} 与 {relative_path}"
        )
    if existing_type != entry_type:
        raise CdnPreparationError(
            f"输入包含文件和目录冲突: {relative_path}"
        )
# //// /注册路径大小写和文件类型 ////


# //// 合并基础 CDN 和覆盖目录 [@x380kkm 2026-09-02] ////
def merge_layers(
    layers: Sequence[InputLayer],
) -> tuple[dict[str, PlannedFile], dict[str, tuple[str, str]]]:
    files: dict[str, PlannedFile] = {}
    registry: dict[str, tuple[str, str]] = {}
    for layer in layers:
        for planned_file in iter_layer_files(layer):
            parts = planned_file.relative_path.split("/")
            for index in range(1, len(parts)):
                register_path(registry, "/".join(parts[:index]), "directory")
            register_path(registry, planned_file.relative_path, "file")
            files[planned_file.relative_path] = planned_file
    return files, registry
# //// /合并基础 CDN 和覆盖目录 ////


# //// 生成目标目录内的安全路径 [@x380kkm 2026-09-02] ////
def destination_path(destination: Path, relative_path: str) -> Path:
    target = destination.joinpath(*relative_path.split("/"))
    resolved_target = target.resolve(strict=False)
    if not resolved_target.is_relative_to(destination):
        raise CdnPreparationError(f"目标路径越过 CDN 根: {relative_path}")
    return target
# //// /生成目标目录内的安全路径 ////


# //// 校验目标目录中的路径大小写 [@x380kkm 2026-09-02] ////
def validate_destination_case(
    destination: Path, registry: dict[str, tuple[str, str]]
) -> None:
    if not destination.exists():
        return
    pending = [destination]
    while pending:
        directory = pending.pop()
        for entry in os.scandir(directory):
            path = Path(entry.path)
            relative_path = path.relative_to(destination).as_posix()
            expected = registry.get(relative_path.casefold())
            if expected is not None and expected[0] != relative_path:
                raise CdnPreparationError(
                    f"目标目录包含大小写冲突: {relative_path} 与 {expected[0]}"
                )
            if entry.is_dir(follow_symlinks=False) and not is_link(path):
                pending.append(path)
# //// /校验目标目录中的路径大小写 ////


# //// 删除一个受目标根约束的目录项 [@x380kkm 2026-09-02] ////
def remove_entry(path: Path) -> None:
    if is_link(path):
        if path.is_junction():
            path.rmdir()
        else:
            path.unlink()
    elif path.is_dir():
        shutil.rmtree(path)
    else:
        path.unlink()
# //// /删除一个受目标根约束的目录项 ////


# //// 准备目标目录的文件和目录类型 [@x380kkm 2026-09-02] ////
def prepare_destination_entries(
    destination: Path,
    registry: dict[str, tuple[str, str]],
    prune: bool,
) -> None:
    destination.mkdir(parents=True, exist_ok=True)
    directories = sorted(
        (value[0] for value in registry.values() if value[1] == "directory"),
        key=lambda value: (value.count("/"), value),
    )
    for relative_path in directories:
        target = destination_path(destination, relative_path)
        if target.exists() or is_link(target):
            if target.is_dir() and not is_link(target):
                continue
            if not prune:
                raise CdnPreparationError(
                    f"目标文件阻挡 CDN 目录: {relative_path}"
                )
            remove_entry(target)
        target.mkdir()

    file_paths = [value[0] for value in registry.values() if value[1] == "file"]
    for relative_path in file_paths:
        target = destination_path(destination, relative_path)
        if is_link(target):
            if not prune:
                raise CdnPreparationError(
                    f"目标链接阻挡 CDN 文件: {relative_path}"
                )
            remove_entry(target)
            continue
        if target.is_dir() and not is_link(target):
            if not prune:
                raise CdnPreparationError(
                    f"目标目录阻挡 CDN 文件: {relative_path}"
                )
            remove_entry(target)
# //// /准备目标目录的文件和目录类型 ////


# //// 通过临时文件原子更新一个 CDN 文件 [@x380kkm 2026-09-02] ////
def materialize_file(source: Path, target: Path) -> None:
    descriptor, temporary_name = tempfile.mkstemp(
        prefix=f".{target.name}.", suffix=".tmp", dir=target.parent
    )
    os.close(descriptor)
    temporary_path = Path(temporary_name)
    temporary_path.unlink()
    try:
        shutil.copy2(source, temporary_path)
        os.replace(temporary_path, target)
    finally:
        temporary_path.unlink(missing_ok=True)
# //// /通过临时文件原子更新一个 CDN 文件 ////


# //// 通过临时文件原子更新布局记录 [@x380kkm 2026-09-02] ////
def write_layout(target: Path, layout: dict[str, object]) -> None:
    body = (json.dumps(layout, ensure_ascii=False, indent=2) + "\n").encode("utf-8")
    descriptor, temporary_name = tempfile.mkstemp(
        prefix=f".{target.name}.", suffix=".tmp", dir=target.parent
    )
    temporary_path = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "wb") as output:
            output.write(body)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary_path, target)
    finally:
        temporary_path.unlink(missing_ok=True)
# //// /通过临时文件原子更新布局记录 ////


# //// 删除目标目录中合并结果之外的文件 [@x380kkm 2026-09-02] ////
def prune_destination(destination: Path, retained_paths: set[str]) -> None:
    def prune_directory(directory: Path) -> None:
        for entry in list(os.scandir(directory)):
            path = Path(entry.path)
            relative_path = path.relative_to(destination).as_posix()
            destination_path(destination, relative_path)
            if entry.is_dir(follow_symlinks=False) and not is_link(path):
                prune_directory(path)
                try:
                    path.rmdir()
                except OSError:
                    pass
                continue
            if relative_path not in retained_paths:
                remove_entry(path)

    prune_directory(destination)
# //// /删除目标目录中合并结果之外的文件 ////


# //// 准备本地 CN CDN 目录 [@x380kkm 2026-09-02] ////
def prepare_cdn(
    source: Path,
    destination: Path = DEFAULT_DESTINATION,
    overlays: Sequence[Path] = (),
    prune: bool = False,
) -> dict[str, object]:
    source_layer = resolve_layer(source, "源 CDN")
    validate_source_layout(source_layer.root)
    overlay_layers = [
        resolve_layer(path, f"覆盖层 {index}")
        for index, path in enumerate(overlays, start=1)
    ]
    layers = [source_layer, *overlay_layers]

    resolved_destination = destination.expanduser().resolve(strict=False)
    if resolved_destination.exists() and not resolved_destination.is_dir():
        raise CdnPreparationError("目标路径不是目录")
    validate_directory_boundaries(resolved_destination, layers)

    files, registry = merge_layers(layers)
    registry[LAYOUT_NAME.casefold()] = (LAYOUT_NAME, "file")
    validate_destination_case(resolved_destination, registry)
    prepare_destination_entries(resolved_destination, registry, prune)

    for relative_path in sorted(files):
        planned_file = files[relative_path]
        target = destination_path(resolved_destination, relative_path)
        materialize_file(planned_file.source, target)

    retained_paths = set(files)
    retained_paths.add(LAYOUT_NAME)
    if prune:
        prune_destination(resolved_destination, retained_paths)

    layout: dict[str, object] = {
        "schema": LAYOUT_SCHEMA,
        "source": source_layer.display_path,
        "overlays": [layer.display_path for layer in overlay_layers],
        "file_count": len(files),
        "total_bytes": sum(planned_file.size for planned_file in files.values()),
    }
    write_layout(resolved_destination / LAYOUT_NAME, layout)
    return layout
# //// /准备本地 CN CDN 目录 ////


# //// 解析命令参数并准备 CDN [@x380kkm 2026-09-02] ////
def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="合并并准备本地 CN CDN 资源目录.")
    parser.add_argument("--source", required=True, type=Path, help="完整 CDN 根目录.")
    parser.add_argument(
        "--destination",
        type=Path,
        default=DEFAULT_DESTINATION,
        help="输出目录, 默认写入 .cdn/cn.",
    )
    parser.add_argument(
        "--overlay",
        action="append",
        default=[],
        type=Path,
        help="覆盖目录, 可重复提供, 后者优先.",
    )
    parser.add_argument(
        "--prune",
        action="store_true",
        help="删除输出目录中不属于合并结果的文件.",
    )
    arguments = parser.parse_args(argv)
    try:
        layout = prepare_cdn(
            source=arguments.source,
            destination=arguments.destination,
            overlays=arguments.overlay,
            prune=arguments.prune,
        )
    except (CdnPreparationError, OSError) as error:
        parser.exit(1, f"CDN 准备失败: {error}\n")
    print(json.dumps(layout, ensure_ascii=False))
    return 0
# //// /解析命令参数并准备 CDN ////


# //// 运行 CDN 准备命令 [@x380kkm 2026-09-02] ////
if __name__ == "__main__":
    raise SystemExit(main())
# //// /运行 CDN 准备命令 ////
