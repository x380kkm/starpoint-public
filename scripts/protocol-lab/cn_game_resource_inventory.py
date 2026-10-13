# audience: internal
# # cn-game-resource-inventory
# 此模块从当前客户端主表和加载代码提取资源引用, 保留字段与路径解析的证据边界.
# 主表目录候选来自客户端引用报告和活动投影, 候选以包内实际寻址结果确认.

from __future__ import annotations

import json
import re
from collections import Counter
from pathlib import Path

from audit_cn_gacha_candidate_assets import decode_multirow_ordered_map
from audit_cn_master_resource_references import (
    MASTER_CLASSES, MASTER_DIRECTORIES, RESOURCE_FIELD, References, collect_references, rows, source_fields,
)
from cn_startup_resource_references import CALL_PATTERN, add_literal_reference, add_startup_references


# //// 读取当前包内的客户端主表字段 [@x380kkm 2026-09-21] ////
def collect_master_references(reader, source: Path, projection: Path, path_constants: Path | None, pcode_root: Path | None):
    projected = json.loads(projection.read_text(encoding="utf-8-sig"))["masters"]
    directories = set(MASTER_DIRECTORIES)
    directories.update(record["logical_path"].removeprefix("master/").rsplit("/", 1)[0] for record in projected)
    constant_paths = {} if path_constants is None else {
        value.split("\t", 1)[0].lstrip("/"): value.split("\t", 1)[1]
        for value in path_constants.read_text(encoding="utf-8-sig").splitlines()}
    fields_root = source / "pinball/master/generated"
    specs, masters = [], {}
    for file in sorted(fields_root.glob("*Values.as")):
        class_name = file.stem.removesuffix("Values")
        text = file.read_text(encoding="utf-8-sig")
        if "Decompilation error" in text and pcode_root:
            file = pcode_root / "pinball/master/generated" / (file.stem + ".pcode")
            fields, unresolved = pcode_fields(file.read_text(encoding="utf-8-sig"))
        else:
            fields, unresolved = source_fields(text)
        if not fields and not unresolved and class_name not in MASTER_CLASSES:
            continue
        name = re.sub(r"(?<!^)(?=[A-Z])", "_", class_name).lower()
        candidates = [f"master/{directory}/{name}.orderedmap" for directory in sorted(directories)]
        candidates.extend("master/" + path.removeprefix("master/").removesuffix(".orderedmap") + ".orderedmap"
                          for path in constant_paths if path.removesuffix(".orderedmap").endswith("/" + name))
        candidates = list(dict.fromkeys(candidates))
        found = [path for path in candidates if reader.locate(path) is not None]
        spec = {"name": name, "source": str(file), "fields": fields, "unresolvedFields": unresolved,
                "resolution": "unique" if len(found) == 1 else "ambiguous" if found else "unresolved",
                "logicalPaths": found}
        if len(found) == 1:
            masters[name] = decode_multirow_ordered_map(reader.read(found[0]))
            spec["rowCount"] = sum(1 for _ in rows(masters[name]))
        specs.append(spec)
    references, nonstandard, scripted, assists = collect_references(specs, masters)
    startup = add_startup_references(references, source, masters)
    scenarios = collect_scenario_references(reader, references)
    for spec in specs:
        if spec["name"] == "scenario_command":
            spec.update({"resolution": "dynamic_quest_paths", "logicalPaths": scenarios["paths"],
                         "rowCount": scenarios["commandCount"]})
    return references, {"masters": specs, "startup": startup, "scriptedCharacterCount": len(scripted),
                        "scriptedCharacterReferences": nonstandard.document(), "assistDefinitions": assists,
                        "scenarios": scenarios}
# //// /读取当前包内的客户端主表字段 ////


# //// 沿剧情任务主表读取每段脚本中的背景和声音 [@x380kkm 2026-09-21] ////
def collect_scenario_references(reader, references):
    paths = sorted(item["logicalPath"] for item in references.resources.values() if item["kind"] == "scenario_master")
    count = 0
    for path in paths:
        payload = reader.read(path)
        if payload is None:
            continue
        for key, row in rows(decode_multirow_ordered_map(payload)):
            count += 1
            owner = f"scenario:{path}:{key}"
            field = {"0": (6, "voice_path"), "1": (36, "bgm"), "3": (39, "bgm"), "21": (34, "image")}.get(row[0])
            if field is not None:
                index, kind = field
                references.field(kind, row[index], owner, f"ScenarioCommandValues.as:{kind}:{index};ScenarioCommandSource.as:resolvePathCollection,getImagePaths")
    return {"pathCount": len(paths), "commandCount": count, "paths": paths}
# //// /沿剧情任务主表读取每段脚本中的背景和声音 ////


# //// 提取全部生产资源加载器的字面路径 [@x380kkm 2026-09-21] ////
def collect_loader_references(references: References, source: Path):
    extra = re.compile(r'(?P<method>movie\.add(?:All)?|soundEffect\.add(?:All)?|bgm\.add(?:All)?)'
                       r'\((?P<argument>\[[^\]]*\]|"[^"]*")\)', re.DOTALL)
    loaders = []
    for file in sorted((source / "pinball").rglob("*.as")):
        text = file.read_text(encoding="utf-8-sig")
        calls = [*CALL_PATTERN.finditer(text), *extra.finditer(text)]
        if not calls:
            continue
        count = 0
        for call in calls:
            for path in re.findall(r'"([^"\n]+)"', call["argument"]):
                if "/" not in path or path.endswith("/") or path.startswith(("http:", "https:")):
                    continue
                evidence = f"{file}:{text[:call.start()].count(chr(10)) + 1}"
                owner = "client:" + file.stem
                method = call["method"]
                if method.startswith("movie."):
                    references.add([path + ".movie.amf3.deflate"], owner, evidence, "movie")
                elif method.startswith(("bgm.", "soundEffect.")):
                    references.add([path + ".mp3"], owner, evidence, "audio")
                else:
                    add_literal_reference(references, method, path, owner, evidence)
                count += 1
        loaders.append({"source": str(file), "literalReferenceCount": count})
    return loaders
# //// /提取全部生产资源加载器的字面路径 ////


# //// 汇总实际引用和未解析的动态字段 [@x380kkm 2026-09-21] ////
def collect_game_references(reader, source: Path, projection: Path, path_constants: Path | None = None,
                            pcode_root: Path | None = None):
    references, scope = collect_master_references(reader, source, projection, path_constants, pcode_root)
    scope["loaders"] = collect_loader_references(references, source)
    scope["constantResources"] = collect_constant_resources(reader, references, path_constants)
    document = references.document()
    scope.update({"referenceGroups": len(document), "atlasMemberCount": len(references.members),
                  "referencesByKind": dict(Counter(item["kind"] for item in document)),
                  "unresolvedReferences": references.unresolved, "dynamicPrefixReferences": references.dynamic,
                  "masterResolutionCounts": dict(Counter(item["resolution"] for item in scope["masters"]))})
    return references, scope
# //// /汇总实际引用和未解析的动态字段 ////


# //// 核对客户端常量直接指向的实际资源类型 [@x380kkm 2026-09-21] ////
def collect_constant_resources(reader, references, path_constants):
    if path_constants is None:
        return {"pathConstants": 0, "resourceMatches": 0}
    count = 0
    constants = path_constants.read_text(encoding="utf-8-sig").splitlines()
    for line in constants:
        path, anchor = line.split("\t", 1)
        if path.startswith("/") or path.endswith(".hx"):
            continue
        for suffix in (".png", ".atlas.amf3.deflate", ".parts.amf3.deflate", ".frame.amf3.deflate",
                       ".timeline.amf3.deflate", ".movie.amf3.deflate", ".ui.amf3.deflate", ".mp3"):
            logical = path + suffix
            if reader.locate(logical) is not None:
                count += 1
                references.add([logical], "client:constant", f"{path_constants}:abc:{anchor}", "constant_resource")
    return {"pathConstants": len(constants), "resourceMatches": count}
# //// /核对客户端常量直接指向的实际资源类型 ////


# //// 从字节码字段赋值读取资源列索引 [@x380kkm 2026-09-21] ////
def pcode_fields(source):
    declared = {name for name, kind in re.findall(r"public var (\w+):(\w+);", source)
                if kind in {"String", "Option", "Array"} and RESOURCE_FIELD.search(name)}
    fields = {}
    rows_read = list(re.finditer(r'getlocal1\s+push(?:byte|short) (\d+)\s+getproperty MultinameL', source))
    for assignment in re.finditer(r'initproperty QName\(PackageNamespace\(""\),"(\w+)"\)', source):
        if assignment[1] not in declared:
            continue
        prior = [item for item in rows_read if item.end() < assignment.start()]
        if prior:
            row = prior[-1]
            fields[assignment[1]] = {"index": int(row[1]), "line": source[:assignment.start()].count("\n") + 1,
                                     "confidence": "unique", "encoding": "pcode_row_assignment"}
    return fields, sorted(declared - fields.keys())
# //// /从字节码字段赋值读取资源列索引 ////
