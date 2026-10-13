# audience: internal
# # cn-packaged-resource-reader
# 此模块按客户端文件清单, 平台和图像规格读取分发包内的最终 CDN 资源.
# 下载差分按版本链覆盖, 包内 bundle 按 FileReader 的目录规则回退.

from __future__ import annotations

import io
import zipfile
from contextlib import ExitStack
from pathlib import Path

from audit_cn_client_resource_resolution import (
    ResourceReference, download_chain, hashed_suffix, is_platform_dependent, read_bundle, resource_paths, texture_reference,
)
from client_package_archive import ClientPackageCdn, DirectoryCdn, open_game_zip


# //// 定位最终资源版本中的文件成员 [@x380kkm 2026-09-21] ////
class PackagedResourceReader:
    def __init__(self, path: Path, overlay_archives: list[Path] | None = None, cdn_root: Path | None = None):
        self.context = ExitStack()
        self.path = path
        self.cdn = self.context.enter_context(DirectoryCdn(cdn_root) if cdn_root else ClientPackageCdn(path))
        self.document = self.cdn.read_json("path")
        ordered, self.versions = download_chain(self.document)
        package = self.context.enter_context(open_game_zip(path))
        if path.suffix.lower() == ".apk":
            self.prefix = ""
            self.bundle_zip = self.context.enter_context(zipfile.ZipFile(io.BytesIO(package.read("assets/bundle.zip"))))
            platform = "android"
        else:
            roots = {name.split(".app/", 1)[0] + ".app/asset/" for name in package.namelist() if ".app/asset/" in name}
            if len(roots) != 1:
                raise ValueError(f"IPA asset 根目录数量异常: {roots}")
            self.prefix = roots.pop()
            self.bundle_zip = package
            platform = "ios"
        self.bundle = read_bundle(self.bundle_zip, self.prefix, platform, path)
        self.archives = {}
        self.downloaded = {}
        for group in [ordered["full"], *ordered["diff"]]:
            for item in group["archive"]:
                name = "/".join(item["location"].rsplit("/", 2)[-2:])
                family = name.split("/", 1)[0].split("-")[1]
                if family not in ("common", "medium", platform):
                    continue
                archive = self.context.enter_context(self.cdn.open_archive(name))
                self.archives[name] = archive
                self.downloaded.update({entry.filename: (name, entry) for entry in archive.infolist()
                                        if entry.filename.startswith("production/") and not entry.is_dir()})
        for path in overlay_archives or []:
            name = "overlay:" + str(path.resolve())
            archive = self.context.enter_context(zipfile.ZipFile(path))
            self.archives[name] = archive
            self.downloaded.update({entry.filename: (name, entry) for entry in archive.infolist()
                                    if entry.filename.startswith("production/") and not entry.is_dir()})

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.context.close()

    def locate(self, logical: str, resolution: str = "medium") -> dict | None:
        reference = texture_reference(logical) if logical.endswith(".png") else ResourceReference(
            logical, hashed_suffix(logical), False, is_platform_dependent(logical))
        download, bundle = resource_paths(reference, self.bundle, resolution)
        if download in self.downloaded:
            name, entry = self.downloaded[download]
            return {"storage": "cdn", "archive": name, "entry": download, "bytes": entry.file_size, "crc": entry.CRC}
        if bundle in self.bundle.entries:
            entry = self.bundle_zip.getinfo(self.prefix + bundle)
            return {"storage": "bundle", "entry": bundle, "bytes": entry.file_size, "crc": entry.CRC}
        return None

    def read(self, logical: str, resolution: str = "medium") -> bytes | None:
        location = self.locate(logical, resolution)
        if location is None:
            return None
        return self.read_location(location)

    def read_location(self, location: dict) -> bytes:
        if location["storage"] == "cdn":
            return self.archives[location["archive"]].read(location["entry"])
        return self.bundle_zip.read(self.prefix + location["entry"])
# //// /定位最终资源版本中的文件成员 ////
