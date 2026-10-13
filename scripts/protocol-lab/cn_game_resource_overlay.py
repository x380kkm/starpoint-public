# audience: internal
# # cn-game-resource-overlay
#
# 此模块验证生成卡池图片, 并把客户端哈希加入 bundle_png.filelist.
# iOS asset 目录和 Android bundle.zip 使用相同的 production/bundle 资源契约.

from __future__ import annotations

import json
import re
import struct
from pathlib import Path


BANNER_KEY_PATTERN = re.compile(r"^[0-9a-f]{40}\.png$")
STANDARD_PNG = b"\x89PNG\r\n\x1a\n"
CLIENT_PNG = b"\x89png\r\n\x1a\n"
BANNER_DIMENSIONS = (510, 180)
PNG_FILELIST_ENTRY = "production/bundle/ef/c788bb52ba31ccdfba3f3e02b3117cdf984cea"


# //// 验证管理页图片与游戏图片成对存在 [@x380kkm 2026-09-20] ////
def read_banner_dimensions(data: bytes, signature: bytes, path: Path) -> tuple[int, int]:
    if len(data) < 24 or data[:8] != signature or data[12:16] != b"IHDR":
        raise ValueError(f"CN 卡池 banner 格式无效: {path}.")
    return struct.unpack_from(">II", data, 16)


def validate_gacha_banner_assets(cdn_root: Path) -> list[Path]:
    catalog_path = cdn_root / "activity-catalog.json"
    if not catalog_path.is_file():
        return []
    catalog = json.loads(catalog_path.read_text(encoding="utf-8-sig"))
    activities = catalog.get("activities")
    if not isinstance(activities, list):
        raise ValueError("CN 活动目录缺少 activities 数组.")
    banners = {}
    for activity in activities:
        if activity.get("kind") != "gacha":
            continue
        key = activity.get("banner_key")
        if not isinstance(key, str) or not BANNER_KEY_PATTERN.fullmatch(key):
            raise ValueError("CN 卡池活动缺少有效 banner_key.")
        if not any(candidate.get("key") == key for candidate in activity.get("image_candidates", [])):
            raise ValueError(f"CN 卡池活动缺少 banner 候选: {key}.")
        banners[key] = banners.get(key, False) or "banner:generated" in activity.get("tags", [])
    result = []
    for key, generated in sorted(banners.items()):
        management_path = cdn_root / "activity-banners" / key
        if not management_path.is_file():
            raise ValueError(f"CN 卡池 banner 资源对不完整: {key}.")
        management = management_path.read_bytes()
        if read_banner_dimensions(management, STANDARD_PNG, management_path) != BANNER_DIMENSIONS:
            raise ValueError(f"CN 卡池 banner 尺寸必须为 510x180: {key}.")
        if not generated:
            continue
        identity = Path(key).stem
        game_path = cdn_root / "production/bundle" / identity[:2] / identity[2:]
        if not game_path.is_file():
            raise ValueError(f"CN 卡池 banner 资源对不完整: {key}.")
        game = game_path.read_bytes()
        if read_banner_dimensions(game, CLIENT_PNG, game_path) != BANNER_DIMENSIONS:
            raise ValueError(f"CN 卡池 banner 尺寸必须为 510x180: {key}.")
        if len(management) != len(game) or management[8:] != game[8:]:
            raise ValueError(f"CN 卡池 banner 管理页与游戏资源不一致: {key}.")
        result.append(game_path)
    return result
# //// /验证管理页图片与游戏图片成对存在 ////


# //// 保留原文件清单并登记生成图片 [@x380kkm 2026-09-20] ////
def merge_png_filelist(original: bytes, hashed_paths: list[str]) -> bytes:
    entries = original.decode("utf-8-sig").splitlines()
    known = set(entries)
    for path in sorted(hashed_paths):
        if not re.fullmatch(r"[a-f0-9]{2}/[a-f0-9]{38}", path):
            raise ValueError(f"bundle PNG identity is invalid: {path}")
        if path not in known:
            entries.append(path)
            known.add(path)
    return ("\n".join(entries) + "\n").encode("utf-8")
# //// /保留原文件清单并登记生成图片 ////
