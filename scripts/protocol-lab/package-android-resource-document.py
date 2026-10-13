# audience: internal
# # package-android-resource-document
#
# 资源文档按安装包签名清单的顺序保存文件, 头部复用 APK bundle 的长度和清单摘要字段.

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import sys
import zipfile
from pathlib import Path


# //// 读取共享 Android bundle 格式 [@x380kkm 2026-09-26] ////
def load_bundle_format():
    path = Path(__file__).with_name("append-android-apk-bundle.py")
    spec = importlib.util.spec_from_file_location("android_apk_bundle_format", path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


# //// 将清单资源写入可顺序读取的文档 [@x380kkm 2026-09-26] ////
def package_resources(base_apk: Path, cdn_root: Path, output: Path) -> dict[str, object]:
    bundle = load_bundle_format()
    base = base_apk.resolve(strict=True)
    root = cdn_root.resolve(strict=True)
    target = output.resolve()
    if target.exists():
        raise ValueError(f"资源输出已存在: {target}")
    manifest_path = root / "manifest.sha256"
    manifest, entries = bundle.read_manifest(manifest_path)
    bundle.assert_embedded_manifest(base, manifest)
    with zipfile.ZipFile(base) as archive:
        mode = archive.read("assets/starpoint-personal-service-cdn/resource-delivery.txt")
    if mode.decode("utf-8").strip() != "external":
        raise ValueError("安装包需要使用 External 资源分发模式.")
    sources = bundle.resolve_manifest_files(root, entries)
    payload_length = sum(entry.size for entry in entries)
    manifest_digest = hashlib.sha256(manifest).digest()
    header = bundle.FOOTER.pack(bundle.MAGIC, bundle.VERSION, 0, bundle.FOOTER_SIZE, payload_length, manifest_digest)
    target.parent.mkdir(parents=True, exist_ok=True)
    partial = target.with_name(target.name + ".partial")
    digest = hashlib.sha256()
    if partial.exists():
        raise ValueError(f"资源临时输出已存在: {partial}")
    try:
        with partial.open("xb") as destination:
            destination.write(header)
            digest.update(header)
            for entry, source in sources:
                size, actual_digest = bundle.copy_file(source, destination, digest)
                if size != entry.size or actual_digest != entry.digest:
                    raise ValueError(f"资源内容与签名清单不一致: {entry.relative}")
        partial.replace(target)
    except BaseException:
        partial.unlink(missing_ok=True)
        raise
    return {
        "format": "starpoint-resource-document",
        "resources": {"path": str(target), "bytes": target.stat().st_size, "sha256": digest.hexdigest()},
        "payload": {"offset": bundle.FOOTER_SIZE, "bytes": payload_length, "files": len(entries)},
        "manifest_sha256": manifest_digest.hex(),
        "digests_verified": True,
    }


# //// 处理资源文档的构建参数 [@x380kkm 2026-09-26] ////
def main() -> int:
    parser = argparse.ArgumentParser(description="Package CDN resources for Android document import")
    parser.add_argument("--base-apk", type=Path, required=True)
    parser.add_argument("--cdn-root", type=Path, required=True)
    parser.add_argument("--output-resources", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(package_resources(args.base_apk, args.cdn_root, args.output_resources), ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
