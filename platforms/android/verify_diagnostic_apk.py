# audience: internal
# # verify-android-diagnostic
#
# 该脚本验证诊断 APK 的 arm64 JNI 库, DEX, Android 二进制清单和资源表.

from __future__ import annotations

import argparse
import json
import struct
import zipfile
from pathlib import Path

REQUIRED_ENTRIES = {
    "AndroidManifest.xml",
    "classes.dex",
    "lib/arm64-v8a/libstarpoint_android_bridge.so",
    "resources.arsc",
}


# //// 验证 Android 诊断包结构和 ELF 架构 [@x380kkm 2026-07-23] ////
def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--apk", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    with zipfile.ZipFile(args.apk) as package:
        names = set(package.namelist())
        missing = REQUIRED_ENTRIES - names
        if missing:
            raise SystemExit(f"diagnostic APK entries are missing: {sorted(missing)}")
        unexpected_native = sorted(
            name
            for name in names
            if name.startswith("lib/")
            and name != "lib/arm64-v8a/libstarpoint_android_bridge.so"
        )
        if unexpected_native:
            raise SystemExit(f"unexpected Android native libraries: {unexpected_native}")
        dex = package.read("classes.dex")
        native = package.read("lib/arm64-v8a/libstarpoint_android_bridge.so")
        manifest = package.read("AndroidManifest.xml")
        resources = package.read("resources.arsc")

    if not dex.startswith(b"dex\n"):
        raise SystemExit("classes.dex has an invalid magic value")
    if len(native) < 20 or native[:4] != b"\x7fELF":
        raise SystemExit("Android JNI library is not ELF")
    if native[4] != 2 or native[5] != 1:
        raise SystemExit("Android JNI library is not little-endian ELF64")
    machine = struct.unpack_from("<H", native, 18)[0]
    if machine != 183:
        raise SystemExit(f"Android JNI library is not AArch64: e_machine={machine}")
    if len(manifest) < 8 or manifest[:4] != b"\x03\x00\x08\x00":
        raise SystemExit("AndroidManifest.xml is not Android binary XML")
    if len(resources) < 12 or resources[:4] != b"\x02\x00\x0c\x00":
        raise SystemExit("resources.arsc is not an Android resource table")

    inventory = {
        "apk": str(args.apk),
        "dex_bytes": len(dex),
        "native_bytes": len(native),
        "native_machine": "AArch64",
        "resources_bytes": len(resources),
        "required_entries": sorted(REQUIRED_ENTRIES),
    }
    rendered = json.dumps(inventory, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered, encoding="utf-8")
    print(rendered, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
# //// /验证 Android 诊断包结构和 ELF 架构 ////
