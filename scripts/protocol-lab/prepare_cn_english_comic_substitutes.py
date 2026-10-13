# audience: internal
# # prepare-cn-english-comic-substitutes
# 此脚本按来源清单的阅读顺序拼接英文游戏截图, 并复用漫画准备工具生成正文与缩略图.
# 输出目录包含完整漫画索引和指定章节图片, 其余章节沿用已有图片.
# 原截图中的标题和文字保留, 来源和章节映射写入准备记录.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

from __future__ import annotations

import argparse
import importlib.util
import json
import sys
from pathlib import Path

from PIL import Image


# //// 载入现有漫画准备接口 [@x380kkm 2026-09-21] ////
def load_comic_preparation():
    path = Path(__file__).with_name("prepare-cn-comics.py")
    spec = importlib.util.spec_from_file_location("cn_comic_preparation", path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


# //// 原子写入漫画来源与索引 [@x380kkm 2026-09-21] ////
def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    temporary.replace(path)


# //// 按清单顺序纵向拼接原截图 [@x380kkm 2026-09-21] ////
def join_original_parts(comic, source_directory):
    parts = []
    for record in sorted(comic["source_parts"], key=lambda item: item["display_order"]):
        path = Path(record["file"])
        if not path.is_absolute():
            path = source_directory / path
        with Image.open(path) as image:
            image.load()
            parts.append(image.convert("RGB"))
    if not parts or len({image.width for image in parts}) != 1:
        raise ValueError("漫画截图需要相同宽度与明确的阅读顺序")
    joined = Image.new("RGB", (parts[0].width, sum(image.height for image in parts)))
    top = 0
    for image in parts:
        joined.paste(image, (0, top))
        top += image.height
    target = comic["target"]
    output = source_directory / f"cn-{target['kind']}-{target['episode']}-source.png"
    temporary = output.with_suffix(".png.tmp")
    joined.save(temporary, format="PNG")
    temporary.replace(output)
    return output, list(joined.size)


# //// 从原作标题和本地图片构建来源记录 [@x380kkm 2026-09-21] ////
def substitute_record(comic, image_path):
    return {
        **comic["target"],
        "title": comic["original_title"],
        "availability": "substitute",
        "source_url": comic["source_url"],
        "notice": "全球版英文游戏截图社区存档, 章节对应关系由两侧漫画画面推定.",
        "image_file": str(image_path),
    }


# //// 准备指定章节并保留完整目录 [@x380kkm 2026-09-21] ////
def prepare_substitutes(prepared_path, catalog_path, output):
    preparation = load_comic_preparation()
    preparation.read_sources(catalog_path)
    records = json.loads(catalog_path.read_text(encoding="utf-8-sig"))
    manifest = json.loads(prepared_path.read_text(encoding="utf-8-sig"))
    catalog_keys = {(record["kind"], record["episode"]) for record in records}
    replacements = {}
    assembled = []
    for comic in manifest["comics"]:
        key = (comic["target"]["kind"], comic["target"]["episode"])
        if key not in catalog_keys or key in replacements:
            raise ValueError(f"漫画替换目标需要在原目录中唯一存在: {key}")
        image_path, size = join_original_parts(comic, prepared_path.parent)
        replacements[key] = substitute_record(comic, image_path)
        assembled.append({**comic, "joined_original": str(image_path), "joined_size": size})
    input_records = [replacements.get((record["kind"], record["episode"]), record) for record in records]
    source_catalog = output / "catalog.json"
    write_json(source_catalog, input_records)
    sources = [source for source in preparation.read_sources(source_catalog)
               if (source.kind, source.episode) in replacements]
    library = output / "library"
    for source in sources:
        preparation.prepare_comic(library, source, None)
    verified = preparation.verify_images(sources, library)
    published_replacements = {(source.kind, source.episode): preparation.source_record(source) for source in sources}
    index_records = [published_replacements.get((record["kind"], record["episode"]), record) for record in records]
    write_json(library / "index.json", index_records)
    publish_files = ["index.json"] + [str(path.relative_to(library)).replace("\\", "/")
                                    for source in sources for path in preparation.image_paths(library, source)]
    report = {"source_catalog": str(catalog_path), "catalog_input": str(source_catalog),
              "library": str(library), "catalog_entries": len(records), "changed_comics": len(sources),
              "prepared_images": verified, "publish_files": publish_files, "comics": assembled}
    write_json(output / "prepared.json", report)
    print(json.dumps({key: report[key] for key in ("library", "catalog_entries", "changed_comics", "prepared_images", "publish_files")}))


# //// 解析本地漫画准备路径 [@x380kkm 2026-09-21] ////
def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("prepared", type=Path)
    parser.add_argument("catalog", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    prepare_substitutes(args.prepared.resolve(), args.catalog.resolve(), args.output.resolve())


if __name__ == "__main__":
    main()
