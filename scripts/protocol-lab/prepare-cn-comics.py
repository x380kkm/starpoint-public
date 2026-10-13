# audience: internal
# # prepare-cn-comics
# 漫画库保留原始话数, 真实图片, 插画替代页与缺页说明使用独立的来源类型.
# Pillow 生成最长边 2048 像素的正文 PNG 和两种顶部缩略图, 缺页说明需要中文字体.
# 全部图片通过解码后原子替换目录清单, 同源成品通过已有清单或复用清单识别.
# substitute 来源的 image_file 相对于输入清单, 发布清单从相同话数的正文图片恢复本地输入.

import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass, replace
from io import BytesIO
import json
from pathlib import Path
from urllib.parse import urlsplit
from urllib.request import urlopen

from PIL import Image, ImageDraw, ImageFont, ImageOps


# //// 读取公开漫画原图 [@x380kkm 2026-09-20] ////
@dataclass(frozen=True)
class ComicSource:
    kind: int
    episode: int
    title: str
    source_url: str = ""
    image_url: str = ""
    availability: str = "available"
    notice: str = ""

    @classmethod
    def from_record(cls, record: dict, directory: Path):
        return cls(**{name: record[name] for name in ComicSource.__dataclass_fields__ if name in record})

    def validate(self) -> None:
        image_url = urlsplit(self.image_url)
        if image_url.scheme != "https" or not (image_url.hostname or "").endswith(".tapimg.com"):
            raise ValueError(f"漫画 {self.kind}/{self.episode} 的图片地址必须来自公开 TapTap 图片域名")

    def read_image(self, font_path: Path | None) -> Image.Image:
        return download_image(self)


# //// 读取明确标记的缺页说明 [@x380kkm 2026-09-20] ////
@dataclass(frozen=True)
class UnavailableComicSource(ComicSource):
    availability: str = "unavailable"

    def validate(self) -> None:
        if self.image_url or not self.notice.strip():
            raise ValueError(f"漫画 {self.kind}/{self.episode} 的缺页来源需要说明文字和空图片地址")

    def read_image(self, font_path: Path | None) -> Image.Image:
        return render_unavailable_page(self, font_path)


# //// 读取具有来源说明的本地插画替代页 [@x380kkm 2026-09-21] ////
@dataclass(frozen=True)
class SubstituteComicSource(ComicSource):
    availability: str = "substitute"
    image_path: Path | None = None

    @classmethod
    def from_record(cls, record: dict, directory: Path):
        source = super().from_record(record, directory)
        image_file = record.get("image_file", f"{source.kind}/main/{source.episode}.png")
        if not isinstance(image_file, str) or not image_file.strip():
            raise ValueError("漫画 image_file 必须是本地图片路径字符串")
        return replace(source, image_path=(directory / image_file).resolve())

    def validate(self) -> None:
        if self.image_url or not self.notice.strip():
            raise ValueError(f"漫画 {self.kind}/{self.episode} 的替代页需要来源说明和本地图片")

    def read_image(self, font_path: Path | None) -> Image.Image:
        return decode_image(self, self.image_path.read_bytes())
# //// /读取具有来源说明的本地插画替代页 ////


# //// 按来源类型校验漫画登记 [@x380kkm 2026-09-20] ////
def read_source_records(path: Path) -> list[ComicSource]:
    records = json.loads(path.read_text(encoding="utf-8-sig"))
    if not isinstance(records, list) or not records:
        raise ValueError("漫画来源清单必须是非空数组")
    sources = []
    source_types = {"available": ComicSource, "unavailable": UnavailableComicSource,
                    "substitute": SubstituteComicSource}
    for record in records:
        source_type = source_types.get(record.get("availability", "available"))
        if source_type is None:
            raise ValueError("漫画 availability 必须是 available, unavailable 或 substitute")
        source = source_type.from_record(record, path.parent)
        if type(source.kind) is not int or source.kind not in (0, 1):
            raise ValueError("漫画 kind 必须是 0 或 1")
        if type(source.episode) is not int or source.episode <= 0 or not isinstance(source.title, str):
            raise ValueError("漫画必须具有正整数话数和字符串标题")
        if not all(isinstance(value, str) for value in (source.source_url, source.image_url, source.notice)):
            raise ValueError("漫画来源地址和说明必须是字符串")
        source.validate()
        sources.append(source)
    return sources


# //// 校验客户端按原始话数连续导航的目录 [@x380kkm 2026-09-20] ////
def read_sources(path: Path) -> list[ComicSource]:
    sources = read_source_records(path)
    for kind in {source.kind for source in sources}:
        episodes = sorted(source.episode for source in sources if source.kind == kind)
        missing = sorted(set(range(1, episodes[-1] + 1)) - set(episodes))
        if missing or len(set(episodes)) != len(episodes):
            raise ValueError(f"漫画 kind={kind} 话数缺失或重复, 缺失={missing}")
    return sorted(sources, key=lambda source: (source.kind, source.episode))


# //// 识别可继续使用的同源漫画成品 [@x380kkm 2026-09-20] ////
def existing_sources(output: Path, reuse_manifest: Path | None) -> dict[tuple[int, int], ComicSource]:
    manifests = [output / "index.json"]
    if reuse_manifest is not None:
        if not reuse_manifest.is_file():
            raise FileNotFoundError(reuse_manifest)
        manifests.append(reuse_manifest)
    return {(source.kind, source.episode): source
            for manifest in manifests if manifest.is_file()
            for source in read_source_records(manifest)}


# //// 定位正文和两种缩略图 [@x380kkm 2026-09-20] ////
def image_paths(output: Path, source: ComicSource) -> tuple[Path, Path, Path]:
    root = output / str(source.kind)
    return (
        root / "main" / f"{source.episode}.png",
        root / "thumbnail_l" / f"{source.episode}.jpg",
        root / "thumbnail_s" / f"{source.episode}.jpg",
    )


# //// 下载并解码单张真实漫画 [@x380kkm 2026-09-20] ////
def download_image(source: ComicSource) -> Image.Image:
    with urlopen(source.image_url, timeout=30) as response:
        data = response.read(32 * 1024 * 1024 + 1)
    return decode_image(source, data)


# //// 解码漫画图片并合成白色背景 [@x380kkm 2026-09-21] ////
def decode_image(source: ComicSource, data: bytes) -> Image.Image:
    if len(data) > 32 * 1024 * 1024:
        raise ValueError(f"漫画 {source.kind}/{source.episode} 原图超过 32 MiB")
    with Image.open(BytesIO(data)) as original:
        original.load()
        if original.format not in ("PNG", "JPEG", "WEBP"):
            raise ValueError(f"漫画 {source.kind}/{source.episode} 图片格式无效")
        if min(original.size) < 256:
            raise ValueError(f"漫画 {source.kind}/{source.episode} 原图尺寸不足")
        rgba = ImageOps.exif_transpose(original).convert("RGBA")
    background = Image.new("RGBA", rgba.size, "white")
    return Image.alpha_composite(background, rgba).convert("RGB")
# //// /解码漫画图片并合成白色背景 ////


# //// 渲染独立的缺页告知正文 [@x380kkm 2026-09-20] ////
def render_unavailable_page(source: ComicSource, font_path: Path | None) -> Image.Image:
    if font_path is None:
        raise ValueError("生成缺页说明需要通过 --font 指定中文字体")
    image = Image.new("RGB", (1080, 1600), "#f2f5f7")
    draw = ImageDraw.Draw(image)
    draw.rounded_rectangle((48, 48, 1032, 655), radius=28, fill="white", outline="#d9e1e7", width=2)
    label = f"第 {source.episode} {('话', '课')[source.kind]}"
    lines = (
        (label, 112, 64, "#263f50"),
        (source.notice, 242, 76, "#263f50"),
        ("缺页说明", 369, 46, "#607484"),
        ("本页保留原始话数.", 477, 38, "#607484"),
        ("可继续阅读前后章节.", 538, 38, "#607484"),
    )
    for text, top, size, color in lines:
        font = ImageFont.truetype(str(font_path), size)
        while draw.textlength(text, font=font) > 880 and size > 24:
            size -= 2
            font = ImageFont.truetype(str(font_path), size)
        draw.text((540, top), text, font=font, fill=color, anchor="mt")
    return image


# //// 保存完整正文和固定尺寸缩略图 [@x380kkm 2026-09-20] ////
def prepare_comic(output: Path, source: ComicSource, font_path: Path | None) -> None:
    original = source.read_image(font_path)
    main = original.copy()
    main.thumbnail((2048, 2048), Image.Resampling.LANCZOS)
    images = (
        (main, "PNG"),
        (ImageOps.fit(original, (984, 623), Image.Resampling.LANCZOS, centering=(0.5, 0.0)), "JPEG"),
        (ImageOps.fit(original, (298, 256), Image.Resampling.LANCZOS, centering=(0.5, 0.0)), "JPEG"),
    )
    for path, (image, image_format) in zip(image_paths(output, source), images):
        path.parent.mkdir(parents=True, exist_ok=True)
        temporary = path.with_suffix(path.suffix + ".tmp")
        options = {"quality": 90} if image_format == "JPEG" else {}
        image.save(temporary, format=image_format, **options)
        temporary.replace(path)


# //// 解码每个目录入口的正文和缩略图 [@x380kkm 2026-09-20] ////
def verify_images(sources: list[ComicSource], output: Path) -> int:
    verified = 0
    for source in sources:
        for path, expected_size in zip(image_paths(output, source), (None, (984, 623), (298, 256))):
            with Image.open(path) as image:
                image.load()
                expected_format = "PNG" if path.suffix == ".png" else "JPEG"
                if image.format != expected_format or (expected_size and image.size != expected_size):
                    raise ValueError(f"漫画图片格式或尺寸无效: {path}")
                if max(image.size) > 2048:
                    raise ValueError(f"漫画图片超过客户端尺寸范围: {path}")
            verified += 1
    return verified


# //// 序列化漫画来源与页面类型 [@x380kkm 2026-09-20] ////
def source_record(source: ComicSource) -> dict:
    return {name: getattr(source, name) for name, field in ComicSource.__dataclass_fields__.items()
            if getattr(source, name) != field.default}


# //// 准备全部漫画后发布客户端目录 [@x380kkm 2026-09-20] ////
def prepare_library(sources: list[ComicSource], output: Path, workers: int,
                    reuse_manifest: Path | None, font_path: Path | None) -> None:
    previous = existing_sources(output, reuse_manifest)
    pending = [source for source in sources
               if previous.get((source.kind, source.episode)) != source
               or not all(path.is_file() for path in image_paths(output, source))]
    with ThreadPoolExecutor(max_workers=workers) as executor:
        futures = {executor.submit(prepare_comic, output, source, font_path): source for source in pending}
        for completed, future in enumerate(as_completed(futures), 1):
            source = futures[future]
            try:
                future.result()
            except Exception as error:
                for other in futures:
                    other.cancel()
                raise RuntimeError(f"漫画 {source.kind}/{source.episode} 准备失败: {error}") from error
            if completed % 25 == 0 or completed == len(pending):
                print(f"漫画图片 {completed}/{len(pending)}", flush=True)
    verified = verify_images(sources, output)
    output.mkdir(parents=True, exist_ok=True)
    temporary = output / "index.json.tmp"
    temporary.write_text(json.dumps([source_record(source) for source in sources], ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    temporary.replace(output / "index.json")
    counts = {kind: sum(source.kind == kind for source in sources) for kind in {source.kind for source in sources}}
    downloaded = sum(bool(source.image_url) for source in pending)
    print(json.dumps({"output": str(output), "series": counts, "downloaded": downloaded,
                      "generated": len(pending) - downloaded, "reused": len(sources) - len(pending),
                      "images_verified": verified}, ensure_ascii=False))


# //// 读取漫画准备参数并生成目录 [@x380kkm 2026-09-20] ////
def main() -> None:
    parser = argparse.ArgumentParser(description="从真实漫画, 本地插画替代页和缺页说明的来源清单生成图片库")
    parser.add_argument("sources", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--reuse-manifest", type=Path, help="已准备成品对应的来源清单")
    parser.add_argument("--font", type=Path, help="缺页说明使用的中文字体")
    arguments = parser.parse_args()
    if not 1 <= arguments.workers <= 8:
        parser.error("workers 必须在 1 到 8 之间")
    prepare_library(read_sources(arguments.sources), arguments.output, arguments.workers,
                    arguments.reuse_manifest, arguments.font)


if __name__ == "__main__":
    main()
