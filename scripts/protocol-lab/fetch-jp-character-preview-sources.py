# audience: internal
# # fetch-jp-character-preview-sources
# 此入口按当前角色目录读取日服归档中的立绘选择页纹理和坐标.
# HTTP Range 缓存目录和命中成员, 生成器使用同名 PNG 与 atlas 作为一组输入.
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import struct
import tempfile
import urllib.request
import zipfile
import zlib
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

from cn_character_full_shot_assets import hash_cn_asset_path
from cn_character_portrait_sources import collect_portrait_sources
from cn_gacha_banner_assets import EntityCatalog

ARCHIVE_URL = ("https://ia601507.us.archive.org/21/items/gacha-archive/Online/World%20Flipper/"
               "air.jp.co.cygames.worldflipper-1.780.0.1780000.zip")
RANGE_BYTES = 2 * 1024 * 1024


# //// 按有限范围缓存远端 ZIP 字节 [@x380kkm 2026-09-27] ////
class ArchiveReader(io.RawIOBase):
    def __init__(self, url: str, cache_root: Path):
        self.url = url
        self.position = 0
        with urllib.request.urlopen(urllib.request.Request(url, method="HEAD"), timeout=30) as response:
            self.length = int(response.headers["Content-Length"])
            identity = url + str(response.headers.get("ETag", self.length))
        self.cache_root = cache_root / hashlib.sha256(identity.encode()).hexdigest()[:16]
        self.cache_root.mkdir(parents=True, exist_ok=True)

    def readable(self):
        return True

    def seekable(self):
        return True

    def tell(self):
        return self.position

    def seek(self, offset, whence=0):
        self.position = offset + (self.position if whence == 1 else self.length if whence == 2 else 0)
        if self.position < 0:
            raise ValueError("ZIP 字节位置超出归档.")
        return self.position

    def read(self, size=-1):
        end = self.length if size < 0 else min(self.length, self.position + size)
        ranges = [(start, min(start + RANGE_BYTES, end)) for start in range(self.position, end, RANGE_BYTES)]
        with ThreadPoolExecutor(max_workers=8) as executor:
            data = b"".join(executor.map(lambda span: self.read_range(*span), ranges))
        self.position = end
        return data

    def read_range(self, start, end):
        path = self.cache_root / f"{start}-{end}.bin"
        if path.is_file() and path.stat().st_size == end - start:
            return path.read_bytes()
        request = urllib.request.Request(self.url, headers={"Range": f"bytes={start}-{end - 1}"})
        with urllib.request.urlopen(request, timeout=60) as response:
            expected = f"bytes {start}-{end - 1}/{self.length}"
            if response.status != 206 or response.headers.get("Content-Range") != expected:
                raise ValueError("日服归档返回的字节区间不匹配.")
            data = response.read()
        if len(data) != end - start:
            raise ValueError("日服归档字节区间未完整下载.")
        with tempfile.NamedTemporaryFile(dir=self.cache_root, delete=False) as temporary:
            temporary.write(data)
            temporary_path = Path(temporary.name)
        os.replace(temporary_path, path)
        return data

    def read_member(self, info):
        start = info.header_offset
        end = min(self.length, start + 30 + len(info.filename.encode("utf-8")) + len(info.extra)
                  + info.compress_size + 1024)
        data = self.read_range(start, end)
        if data[:4] != b"PK\x03\x04":
            raise ValueError("日服归档成员头部无效.")
        name_length, extra_length = struct.unpack_from("<HH", data, 26)
        payload_start = 30 + name_length + extra_length
        payload_end = payload_start + info.compress_size
        if payload_end > len(data):
            data += self.read_range(end, start + payload_end)
        compressed = data[payload_start:payload_end]
        if info.compress_type == zipfile.ZIP_STORED:
            result = compressed
        elif info.compress_type == zipfile.ZIP_DEFLATED:
            result = zlib.decompress(compressed, -15)
        else:
            raise ValueError("日服归档成员的压缩格式不受支持.")
        if len(result) != info.file_size or zlib.crc32(result) != info.CRC:
            raise ValueError("日服归档成员解压结果校验失败.")
        return result
# //// /按有限范围缓存远端 ZIP 字节 ////


# //// 检索当前角色对应的日服预览资源 [@x380kkm 2026-09-27] ////
def fetch_previews(cdn_root, sources_root, output_root, archive_url):
    config_path = Path(__file__).with_name("jp-character-full-shot-sources.json")
    portraits, _ = collect_portrait_sources(cdn_root, sources_root, config_path)
    catalog = EntityCatalog.load(cdn_root, cdn_root / "entities/PathFile.csv")
    targets = []
    for prefix in sorted({target.rsplit("/ui/", 1)[0] for target in portraits}):
        paths = [prefix + "/ui/illustration_setting_sprite_sheet" + suffix
                 for suffix in (".png", ".atlas.amf3.deflate")]
        if all(catalog.find(path) is not None for path in paths):
            targets.extend(paths)
    by_suffix = {}
    for target in targets:
        digest = hash_cn_asset_path(target)
        by_suffix[f"{digest[:2]}/{digest[2:]}"] = target
    output_root.mkdir(parents=True, exist_ok=True)
    reader = ArchiveReader(archive_url, output_root / ".archive-cache")
    with zipfile.ZipFile(reader) as archive:
        selected = {}
        for info in archive.infolist():
            if "/asset_download/" not in info.filename:
                continue
            target = by_suffix.get("/".join(info.filename.split("/")[-2:]))
            if target is None:
                continue
            current = selected.get(target)
            if current is None or info.file_size > current.file_size:
                selected[target] = info
    print(json.dumps({"requested": len(targets), "available": len(selected)}), flush=True)
    failures = []
    completed = 0

    def download(target):
        path = output_root / target
        if path.is_file():
            return
        data = reader.read_member(selected[target])
        path.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as temporary:
            temporary.write(data)
            temporary_path = Path(temporary.name)
        os.replace(temporary_path, path)

    with ThreadPoolExecutor(max_workers=8) as executor:
        jobs = {executor.submit(download, target): target for target in selected}
        for future in as_completed(jobs):
            target = jobs[future]
            try:
                future.result()
            except Exception as error:
                failures.append({"target": target, "error": str(error)})
            completed += 1
            if completed % 50 == 0:
                print(json.dumps({"completed": completed, "failures": len(failures)}), flush=True)
    report = {"archive_url": archive_url, "requested": len(targets),
              "available": len(selected), "missing": sorted(set(targets) - set(selected)),
              "failures": failures}
    (output_root / "preview-source-report.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8",
    )
    return report
# //// /检索当前角色对应的日服预览资源 ////


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cdn-root", type=Path, required=True)
    parser.add_argument("--sources-root", type=Path, required=True)
    parser.add_argument("--output-root", type=Path, required=True)
    parser.add_argument("--archive-url", default=ARCHIVE_URL)
    args = parser.parse_args()
    report = fetch_previews(args.cdn_root, args.sources_root, args.output_root, args.archive_url)
    print(json.dumps(report, ensure_ascii=False))
    return 1 if report["failures"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
