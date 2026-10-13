# audience: internal
# # prepare-android-game-resources
#
# 此脚本将生成图片加入游戏 bundle.zip 和 PNG 文件清单, 并写出 AssetExtractor 使用的 SHA-1 标记.
# 输入 APK 的其余内置资源保持原内容, bundle 摘要让覆盖安装重新展开变化后的内置资源.

from __future__ import annotations

import argparse
import hashlib
import io
import json
import zipfile
from pathlib import Path

from cn_game_resource_overlay import PNG_FILELIST_ENTRY, merge_png_filelist, validate_gacha_banner_assets


# //// 构造游戏内置资源与文件清单 [@x380kkm 2026-09-20] ////
def prepare_bundle(input_apk: Path, cdn_root: Path, output_bundle: Path, output_digest: Path) -> dict:
    banners = validate_gacha_banner_assets(cdn_root)
    root = cdn_root / "production/bundle"
    overlay = {"production/bundle/" + source.relative_to(root).as_posix(): source.read_bytes() for source in banners}
    with zipfile.ZipFile(input_apk) as apk:
        original_bundle = apk.read("assets/bundle.zip")
    with zipfile.ZipFile(io.BytesIO(original_bundle)) as source:
        original_filelist = source.read(PNG_FILELIST_ENTRY)
        hashed_paths = [source_path.relative_to(root).as_posix() for source_path in banners]
        overlay[PNG_FILELIST_ENTRY] = merge_png_filelist(original_filelist, hashed_paths)
        output_bundle.parent.mkdir(parents=True, exist_ok=True)
        with zipfile.ZipFile(output_bundle, "w", compression=zipfile.ZIP_DEFLATED, allowZip64=False) as target:
            for entry in source.infolist():
                if entry.filename not in overlay:
                    target.writestr(entry, source.read(entry))
            for name, data in sorted(overlay.items()):
                entry = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                entry.compress_type = zipfile.ZIP_DEFLATED
                target.writestr(entry, data)
    with output_bundle.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha1").hexdigest()
    output_digest.write_text(digest, encoding="utf-8")
    with zipfile.ZipFile(output_bundle) as bundle:
        filelist = set(bundle.read(PNG_FILELIST_ENTRY).decode("utf-8").splitlines())
        for suffix in hashed_paths:
            if suffix not in filelist or bundle.read("production/bundle/" + suffix) != overlay["production/bundle/" + suffix]:
                raise ValueError(f"generated asset cannot be resolved by FileReader: {suffix}")
    return {"generatedBannerCount": len(banners), "bundle": str(output_bundle), "bundleBytes": output_bundle.stat().st_size,
            "digestFile": str(output_digest), "filelist": PNG_FILELIST_ENTRY, "digest": digest}
# //// /构造游戏内置资源与文件清单 ////


# //// 输出可用于 APK 重建的内置资源 [@x380kkm 2026-09-20] ////
def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input-apk", required=True, type=Path)
    parser.add_argument("--cdn-root", required=True, type=Path)
    parser.add_argument("--output-bundle", required=True, type=Path)
    parser.add_argument("--output-digest", required=True, type=Path)
    args = parser.parse_args()
    print(json.dumps(prepare_bundle(args.input_apk, args.cdn_root, args.output_bundle, args.output_digest), ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
# //// /输出可用于 APK 重建的内置资源 ////
