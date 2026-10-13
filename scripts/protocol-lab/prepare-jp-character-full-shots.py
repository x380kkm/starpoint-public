# audience: internal
# # prepare-jp-character-full-shots
# 此入口把本地日服原图映射到当前角色目录, 并输出保留平台专有资源的派生 CDN.
# --sources-root 指向 jp-character-full-shot-sources.json 所列原图集合的共同父目录.
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

from __future__ import annotations

import argparse
import importlib.util
import json
import sys
import tempfile
from pathlib import Path

from cn_character_portrait_sources import (
    collect_portrait_sources, prepare_portrait_sources, prepare_preview_sources,
)


# //// 按当前平台 CDN 生成日服角色立绘 [@x380kkm 2026-09-27] ////
def prepare_portraits(cdn_root: Path, sources_root: Path, config_path: Path, output_root: Path):
    module_path = Path(__file__).with_name("stage-cn-character-full-shots.py")
    spec = importlib.util.spec_from_file_location("stage_cn_character_full_shots", module_path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    selected, coverage = collect_portrait_sources(cdn_root, sources_root, config_path)
    output_root.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="jp-portrait-sources-", dir=output_root.parent) as temporary:
        inputs = Path(temporary)
        mappings = prepare_portrait_sources(selected, inputs)
        prepare_preview_sources(selected, sources_root, config_path, inputs)
        manifest = module.stage_portrait_cdn(cdn_root, inputs / "source", mappings, output_root)
    manifest["character_references"] = coverage
    module._write_json_atomic(output_root / "starpoint-character-full-shots.json", manifest)
    return {"output_root": str(output_root.resolve()), "coverage": manifest["coverage"],
            "character_references": coverage,
            "illustration_preview_count": len(manifest["illustration_previews"])}
# //// /按当前平台 CDN 生成日服角色立绘 ////


# //// 读取当前资源底稿和本地日服原图位置 [@x380kkm 2026-09-27] ////
def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cdn-root", type=Path, required=True)
    parser.add_argument("--sources-root", type=Path, required=True)
    parser.add_argument("--output-root", type=Path, required=True)
    parser.add_argument("--source-config", type=Path,
                        default=Path(__file__).with_name("jp-character-full-shot-sources.json"))
    args = parser.parse_args()
    result = prepare_portraits(args.cdn_root, args.sources_root, args.source_config, args.output_root)
    print(json.dumps(result, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
# //// /读取当前资源底稿和本地日服原图位置 ////
