# audience: external
# # binary-inventory
# 此程序为已解包客户端建立 DEX, ELF, SWF 和资源文件清单, 并提取 CN 8003 与历史 18888 端口常量.
# 此程序只读取输入目录, 分析结果和字符串文件写入明确指定的输出位置.

from __future__ import annotations

import argparse
import hashlib
import json
import re
import struct
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable


ASCII_STRINGS = re.compile(rb"[\x20-\x7e]{4,}")
UTF16LE_STRINGS = re.compile(rb"(?:[\x20-\x7e]\x00){4,}")
INDICATORS = (
    "8003",
    "18888",
    "multibattle",
    "multi_battle",
    "room_sequence",
    "socket",
    "connect",
    "send",
    "recv",
    "messagepack",
    "msgpack",
    "aes",
    "rc4",
    "zlib",
    "leiting.com",
    "shijtswygamegf",
    "shijtswydl",
    "kakaogames.com",
    "wdfp",
)
PROTOCOL_PORTS = (8003, 18888)
ELF_MACHINES = {
    3: "x86",
    40: "arm",
    62: "x86_64",
    183: "aarch64",
}


# //// 表示从二进制文件提取的一个可打印字符串 [@x380kkm 2026-07-20] ////
@dataclass(frozen=True)
class ExtractedString:
    offset: int
    encoding: str
    value: str


# //// /表示从二进制文件提取的一个可打印字符串 ////


# //// 提取 ASCII 和 UTF-16LE 字符串 [@x380kkm 2026-07-20] ////
def extract_strings(data: bytes) -> Iterable[ExtractedString]:
    for match in ASCII_STRINGS.finditer(data):
        yield ExtractedString(match.start(), "ascii", match.group().decode("ascii"))
    for match in UTF16LE_STRINGS.finditer(data):
        yield ExtractedString(match.start(), "utf-16le", match.group().decode("utf-16le"))


# //// /提取 ASCII 和 UTF-16LE 字符串 ////


# //// 识别 Android 客户端常见二进制格式 [@x380kkm 2026-07-20] ////
def detect_kind(data: bytes) -> str:
    if data.startswith(b"dex\n"):
        return "dex"
    if data.startswith(b"\x7fELF"):
        return "elf"
    if data[:3] in {b"FWS", b"CWS", b"ZWS"}:
        return "swf"
    if data.startswith(b"PK\x03\x04"):
        return "zip"
    return "resource"


# //// /识别 Android 客户端常见二进制格式 ////


# //// 读取 ELF 类别, 字节序和目标架构 [@x380kkm 2026-07-20] ////
def describe_elf(data: bytes) -> dict[str, Any]:
    if len(data) < 20 or not data.startswith(b"\x7fELF"):
        return {}
    bitness = {1: 32, 2: 64}.get(data[4])
    endianness = {1: "little", 2: "big"}.get(data[5])
    if endianness is None:
        return {"bitness": bitness, "endianness": "unknown"}
    machine = struct.unpack("<H" if endianness == "little" else ">H", data[18:20])[0]
    return {
        "bitness": bitness,
        "endianness": endianness,
        "machine": machine,
        "architecture": ELF_MACHINES.get(machine, "unknown"),
    }


# //// /读取 ELF 类别, 字节序和目标架构 ////


# //// 查找一个端口的文本与 16 位整数表示 [@x380kkm 2026-07-20] ////
def find_port_encodings(data: bytes, port: int) -> list[dict[str, Any]]:
    encodings = {
        "ascii": str(port).encode("ascii"),
        "uint16_little": struct.pack("<H", port),
        "uint16_big": struct.pack(">H", port),
    }
    hits: list[dict[str, Any]] = []
    for encoding, needle in encodings.items():
        offset = 0
        while (found := data.find(needle, offset)) >= 0:
            hits.append({"encoding": encoding, "offset": found})
            offset = found + 1
    return hits


# //// /查找一个端口的文本与 16 位整数表示 ////


# //// 把目标二进制的字符串写入独立文本文件 [@x380kkm 2026-07-20] ////
def write_strings(
    root: Path,
    file_path: Path,
    strings_directory: Path,
    strings: list[ExtractedString],
) -> str:
    relative_path = file_path.relative_to(root)
    output_path = strings_directory / relative_path.with_suffix(relative_path.suffix + ".strings.txt")
    output_path.parent.mkdir(parents=True, exist_ok=True)
    with output_path.open("w", encoding="utf-8", newline="\n") as output:
        for item in strings:
            output.write(f"{item.offset:08x}\t{item.encoding}\t{item.value}\n")
    return str(output_path.resolve())


# //// /把目标二进制的字符串写入独立文本文件 ////


# //// 分析一个已解包客户端文件 [@x380kkm 2026-07-20] ////
def analyze_file(root: Path, file_path: Path, strings_directory: Path) -> dict[str, Any]:
    data = file_path.read_bytes()
    kind = detect_kind(data)
    protocol_port_hits = {
        str(port): find_port_encodings(data, port)
        for port in PROTOCOL_PORTS
    }
    strings = list(extract_strings(data))
    interesting_strings = [
        {
            "offset": item.offset,
            "encoding": item.encoding,
            "value": item.value,
        }
        for item in strings
        if any(indicator in item.value.lower() for indicator in INDICATORS)
    ]
    result: dict[str, Any] = {
        "path": file_path.relative_to(root).as_posix(),
        "bytes": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
        "kind": kind,
        "protocol_port_hits": protocol_port_hits,
        "port_18888_hits": protocol_port_hits["18888"],
        "indicator_strings": interesting_strings,
    }
    if kind == "elf":
        result["elf"] = describe_elf(data)
    if kind in {"dex", "elf", "swf"}:
        result["strings_path"] = write_strings(root, file_path, strings_directory, strings)
    return result


# //// /分析一个已解包客户端文件 ////


# //// 为输入目录生成稳定排序的二进制清单 [@x380kkm 2026-07-20] ////
def build_inventory(root: Path, output_path: Path, strings_directory: Path) -> dict[str, Any]:
    root = root.resolve()
    output_path = output_path.resolve()
    strings_directory = strings_directory.resolve()
    files = [
        analyze_file(root, file_path, strings_directory)
        for file_path in sorted(path for path in root.rglob("*") if path.is_file())
        if file_path.resolve() != output_path and strings_directory not in file_path.resolve().parents
    ]
    inventory = {
        "schema_version": 2,
        "root": str(root),
        "files": files,
        "summary": {
            "file_count": len(files),
            "dex_count": sum(file["kind"] == "dex" for file in files),
            "elf_count": sum(file["kind"] == "elf" for file in files),
            "swf_count": sum(file["kind"] == "swf" for file in files),
            "files_with_protocol_ports": sum(
                any(file["protocol_port_hits"].values())
                for file in files
            ),
            "files_with_18888": sum(bool(file["port_18888_hits"]) for file in files),
            "files_with_indicators": sum(bool(file["indicator_strings"]) for file in files),
        },
    }
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(inventory, ensure_ascii=False, indent=2), encoding="utf-8")
    return inventory


# //// /为输入目录生成稳定排序的二进制清单 ////


# //// 解析命令行参数并执行清单生成 [@x380kkm 2026-07-20] ////
def main() -> None:
    parser = argparse.ArgumentParser(description="分析已解包 Android 客户端中的 DEX, ELF, SWF 和协议常量.")
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--strings", type=Path, required=True)
    arguments = parser.parse_args()
    inventory = build_inventory(arguments.root, arguments.output, arguments.strings)
    print(json.dumps(inventory["summary"], ensure_ascii=False))


if __name__ == "__main__":
    main()
# //// /解析命令行参数并执行清单生成 ////
