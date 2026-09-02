# audience: internal
# # export-cn-gacha-raw-odds-signatures
#
# 此脚本从 CN CDN 原始 odds orderedmap 生成每个卡池的内容签名.
# 每个签名保留 prize type 和三个 rarity slot 的完整 raw rows, 并忽略 odds map 名称差异.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import tempfile
from pathlib import Path
from typing import Any

from cn_gacha_banner_assets import EntityCatalog, GachaBannerError
from generate_cn_gacha_banners import (
    _load_master_assets,
    _pool_metadata,
    _read_odds_maps,
)


# //// 生成规范化 raw odds 内容签名 [@x380kkm 2026-08-24] ////
def _canonical_json(value: object) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")


def _raw_odds_groups(
    odds_ids: tuple[str, ...], odds_maps: dict[str, dict[str, Any]]
) -> list[list[list[str]]]:
    groups: list[list[list[str]]] = []
    for odds_id in odds_ids:
        odds_map = odds_maps.get(odds_id)
        if odds_map is None:
            raise GachaBannerError(
                "missing_raw_odds", "raw gacha odds map is unavailable", odds_id=odds_id
            )
        rows = odds_map.get(odds_id)
        if not isinstance(rows, dict):
            raise GachaBannerError(
                "invalid_raw_odds", "raw gacha odds map is missing its named row group", odds_id=odds_id
            )
        normalized_rows: list[list[str]] = []
        for row in rows.values():
            if not isinstance(row, list) or not all(isinstance(value, str) for value in row):
                raise GachaBannerError(
                    "invalid_raw_odds", "raw gacha odds row is invalid", odds_id=odds_id
                )
            normalized_rows.append(list(row))
        groups.append(sorted(normalized_rows))
    return groups


def build_raw_odds_signatures(
    source_cdn_root: Path, manifest_path: Path | None
) -> dict[str, object]:
    entity_catalog = EntityCatalog.load(source_cdn_root, manifest_path)
    gacha_master, _, _ = _load_master_assets(source_cdn_root, entity_catalog)
    pool_metadata = {
        int(pool_id): _pool_metadata(int(pool_id), gacha_master)
        for pool_id in gacha_master
    }
    odds_ids = {
        odds_id
        for _, _, pool_odds_ids in pool_metadata.values()
        for odds_id in pool_odds_ids
    }
    odds_maps, gaps = _read_odds_maps(source_cdn_root, entity_catalog, odds_ids)
    if gaps:
        raise GachaBannerError(
            "incomplete_raw_odds", "raw gacha odds resources are incomplete", gaps=gaps
        )

    signatures: dict[str, str] = {}
    canonical_pools: dict[str, object] = {}
    for pool_id, (_, prize_type, pool_odds_ids) in sorted(pool_metadata.items()):
        content = {
            "prizeType": prize_type,
            "oddsGroups": _raw_odds_groups(pool_odds_ids, odds_maps),
        }
        canonical_pools[str(pool_id)] = content
        signatures[str(pool_id)] = hashlib.sha256(_canonical_json(content)).hexdigest()
    return {
        "sourceRegion": "cn",
        "sourceSha256": hashlib.sha256(_canonical_json(canonical_pools)).hexdigest(),
        "poolCount": len(signatures),
        "signatureAlgorithm": "sha256 canonical JSON of prizeType and ordered rarity-slot raw rows",
        "signatures": signatures,
    }


# //// /生成规范化 raw odds 内容签名 ////


# //// 原子写入签名资产 [@x380kkm 2026-08-24] ////
def _write_json(path: Path, value: object) -> None:
    data = (json.dumps(value, ensure_ascii=False, indent=2) + "\n").encode("utf-8")
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_name = tempfile.mkstemp(
        prefix=f".{path.name}.", suffix=".tmp", dir=path.parent
    )
    temporary_path = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(data)
        os.replace(temporary_path, path)
    finally:
        temporary_path.unlink(missing_ok=True)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source-cdn-root", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--manifest", type=Path)
    arguments = parser.parse_args()
    source_cdn_root = arguments.source_cdn_root.resolve(strict=True)
    output_path = arguments.output.resolve()
    manifest_path = arguments.manifest.resolve(strict=True) if arguments.manifest else None
    asset = build_raw_odds_signatures(source_cdn_root, manifest_path)
    _write_json(output_path, asset)
    print(
        json.dumps(
            {
                "output": str(output_path),
                "poolCount": asset["poolCount"],
                "sourceSha256": asset["sourceSha256"],
            },
            separators=(",", ":"),
        )
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except GachaBannerError as error:
        failure = {"code": error.code, "message": str(error), "details": error.details}
        print(
            json.dumps(failure, ensure_ascii=False, separators=(",", ":")),
            file=sys.stderr,
        )
        raise SystemExit(1) from None


# //// /原子写入签名资产 ////
