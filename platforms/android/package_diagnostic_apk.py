# audience: internal
# # package-android-diagnostic
#
# 该脚本把 DEX 和 arm64 JNI 库加入 aapt2 生成的基础 APK.

from __future__ import annotations

import argparse
import shutil
import zipfile
from pathlib import Path


# //// 组装 Android 诊断 APK [@x380kkm 2026-07-23] ////
def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", type=Path, required=True)
    parser.add_argument("--dex", type=Path, required=True)
    parser.add_argument("--native-library", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    for path in (args.base, args.dex, args.native_library):
        if not path.is_file():
            raise SystemExit(f"required Android package input is missing: {path}")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(args.base, args.output)
    with zipfile.ZipFile(args.output, "a") as package:
        existing = set(package.namelist())
        additions = {
            "classes.dex": args.dex,
            "lib/arm64-v8a/libstarpoint_android_bridge.so": args.native_library,
        }
        duplicates = existing.intersection(additions)
        if duplicates:
            raise SystemExit(f"base APK already contains generated entries: {sorted(duplicates)}")
        for archive_name, source in additions.items():
            package.write(source, archive_name, compress_type=zipfile.ZIP_DEFLATED)
    print(args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
# //// /组装 Android 诊断 APK ////
