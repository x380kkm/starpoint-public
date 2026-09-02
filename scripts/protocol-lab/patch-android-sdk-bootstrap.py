# audience: internal
# # patch-android-sdk-bootstrap
# 此程序只为协议取证移除 CN Android 引导壳中一次可定位的 LeitingApplication 初始化调用. 结果不伪造登录状态或服务器响应.

from __future__ import annotations

import argparse
import hashlib
import struct
import zlib
from dataclasses import dataclass
from pathlib import Path


TARGET_CLASS = "Ls/h/e/l/l/S;"
TARGET_METHOD = "onCreate"
SKIP_BOOTSTRAP_CALLEE = ("Ls/h/e/l/l/N;", "ra", 0x71)
SKIP_DELEGATE_CALLEE = ("Landroid/app/Application;", "onCreate", 0x6E)


def read_u16(data: bytes, offset: int) -> int:
    return struct.unpack_from("<H", data, offset)[0]


def read_u32(data: bytes, offset: int) -> int:
    return struct.unpack_from("<I", data, offset)[0]


def read_uleb128(data: bytes, offset: int) -> tuple[int, int]:
    value = 0
    shift = 0
    while True:
        if offset >= len(data):
            raise ValueError("uleb128 exceeds input")
        byte = data[offset]
        offset += 1
        value |= (byte & 0x7F) << shift
        if byte < 0x80:
            return value, offset
        shift += 7
        if shift >= 35:
            raise ValueError("uleb128 is too long")


@dataclass(frozen=True)
class DexHeader:
    string_ids_size: int
    string_ids_off: int
    type_ids_size: int
    type_ids_off: int
    proto_ids_size: int
    proto_ids_off: int
    method_ids_size: int
    method_ids_off: int
    class_defs_size: int
    class_defs_off: int


class DexReader:
    def __init__(self, data: bytes) -> None:
        self.data = data
        if not data.startswith(b"dex\n"):
            raise ValueError("input is not a DEX file")
        self.header = DexHeader(
            string_ids_size=read_u32(data, 0x38),
            string_ids_off=read_u32(data, 0x3C),
            type_ids_size=read_u32(data, 0x40),
            type_ids_off=read_u32(data, 0x44),
            proto_ids_size=read_u32(data, 0x48),
            proto_ids_off=read_u32(data, 0x4C),
            method_ids_size=read_u32(data, 0x58),
            method_ids_off=read_u32(data, 0x5C),
            class_defs_size=read_u32(data, 0x60),
            class_defs_off=read_u32(data, 0x64),
        )

    def string_at(self, index: int) -> str:
        if index < 0 or index >= self.header.string_ids_size:
            raise ValueError(f"invalid string index: {index}")
        offset = read_u32(self.data, self.header.string_ids_off + index * 4)
        _, offset = read_uleb128(self.data, offset)
        end = self.data.index(0, offset)
        return self.data[offset:end].decode("utf-8", "replace")

    def type_at(self, index: int) -> str:
        if index < 0 or index >= self.header.type_ids_size:
            raise ValueError(f"invalid type index: {index}")
        string_index = read_u32(self.data, self.header.type_ids_off + index * 4)
        return self.string_at(string_index)

    def method_at(self, index: int) -> tuple[str, str, int]:
        if index < 0 or index >= self.header.method_ids_size:
            raise ValueError(f"invalid method index: {index}")
        offset = self.header.method_ids_off + index * 8
        class_index = read_u16(self.data, offset)
        proto_index = read_u16(self.data, offset + 2)
        name_index = read_u32(self.data, offset + 4)
        return self.type_at(class_index), self.string_at(name_index), proto_index

    def class_data_offset(self, descriptor: str) -> int:
        for index in range(self.header.class_defs_size):
            offset = self.header.class_defs_off + index * 32
            class_index = read_u32(self.data, offset)
            if self.type_at(class_index) == descriptor:
                return read_u32(self.data, offset + 24)
        raise ValueError(f"class not found: {descriptor}")

    def method_code_offset(self, descriptor: str, name: str) -> int:
        offset = self.class_data_offset(descriptor)
        static_fields, offset = read_uleb128(self.data, offset)
        instance_fields, offset = read_uleb128(self.data, offset)
        direct_methods, offset = read_uleb128(self.data, offset)
        virtual_methods, offset = read_uleb128(self.data, offset)
        for _ in range(static_fields + instance_fields):
            _, offset = read_uleb128(self.data, offset)
            _, offset = read_uleb128(self.data, offset)
        for count in (direct_methods, virtual_methods):
            previous_index = 0
            for _ in range(count):
                index_delta, offset = read_uleb128(self.data, offset)
                _, offset = read_uleb128(self.data, offset)
                code_offset, offset = read_uleb128(self.data, offset)
                previous_index += index_delta
                _, method_name, _ = self.method_at(previous_index)
                if method_name == name:
                    return code_offset
        raise ValueError(f"method not found: {descriptor}->{name}")

    def find_call(self, code_offset: int, class_name: str, method_name: str, opcode: int) -> list[int]:
        if code_offset == 0:
            raise ValueError("target method has no code")
        instruction_count = read_u32(self.data, code_offset + 12)
        instruction_offset = code_offset + 16
        matches: list[int] = []
        for index in range(instruction_count):
            word = read_u16(self.data, instruction_offset + index * 2)
            if (word & 0xFF) != opcode:
                continue
            if index + 2 >= instruction_count:
                continue
            method_index = read_u16(self.data, instruction_offset + (index + 1) * 2)
            callee_class, callee_name, _ = self.method_at(method_index)
            if callee_class == class_name and callee_name == method_name:
                matches.append(instruction_offset + index * 2)
        return matches


def patch_bootstrap(data: bytes, mode: str) -> tuple[bytes, int]:
    reader = DexReader(data)
    code_offset = reader.method_code_offset(TARGET_CLASS, TARGET_METHOD)
    if mode == "skip-sdk-bootstrap":
        class_name, method_name, opcode = SKIP_BOOTSTRAP_CALLEE
    elif mode == "skip-delegated-sdk-oncreate":
        class_name, method_name, opcode = SKIP_DELEGATE_CALLEE
    else:
        raise ValueError(f"unknown patch mode: {mode}")
    matches = reader.find_call(code_offset, class_name, method_name, opcode)
    if len(matches) != 1:
        raise ValueError(f"expected one SDK bootstrap call, found {len(matches)}")
    patched = bytearray(data)
    instruction_offset = matches[0]
    patched[instruction_offset : instruction_offset + 6] = b"\x00" * 6
    patched[0x0C:0x20] = hashlib.sha1(patched[0x20:]).digest()
    checksum = zlib.adler32(patched[0x0C:]) & 0xFFFFFFFF
    struct.pack_into("<I", patched, 0x08, checksum)
    return bytes(patched), instruction_offset


def main() -> None:
    parser = argparse.ArgumentParser(description="为协议取证移除一次确定的 LeitingApplication 初始化调用.")
    parser.add_argument(
        "--mode",
        choices=("skip-sdk-bootstrap", "skip-delegated-sdk-oncreate"),
        default="skip-delegated-sdk-oncreate",
    )
    parser.add_argument("--input", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    source = args.input.read_bytes()
    patched, instruction_offset = patch_bootstrap(source, args.mode)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(patched)
    print(
        f"android-sdk-bootstrap-patch: mode={args.mode} patched={instruction_offset:#x} "
        f"input_bytes={len(source)} output_bytes={len(patched)}"
    )


if __name__ == "__main__":
    main()
