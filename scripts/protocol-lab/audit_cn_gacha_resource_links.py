# audience: internal
# # audit-cn-gacha-resource-links
#
# 此脚本从 CDN 当前 gacha 与 feature master 反查客户端必需的图片, 动画和详情文档引用.
# feature 表以卡池 ID 为根, 与客户端 GachaLogic.getFeatureImageValues 的关联一致.
#
# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

from __future__ import annotations

import argparse
import json
from pathlib import Path

from cn_gacha_banner_assets import EntityCatalog, decode_ordered_map, read_logical_assets


MASTER_PATHS = {
    "gacha": "master/gacha/gacha.orderedmap",
    "feature": "master/gacha/gacha_feature_content.orderedmap",
}


# //// 读取当前 master 的客户端字节 [@x380kkm 2026-09-20] ////
def read_masters(root: Path) -> dict:
    catalog = EntityCatalog.load(root)
    records = {logical: catalog.find(logical) for logical in MASTER_PATHS.values()}
    if any(record is None for record in records.values()):
        raise ValueError("gacha resource master is absent from EntityLists")
    assets, missing = read_logical_assets(root, records)
    if missing:
        raise ValueError(f"gacha resource master is absent from archives: {missing}")
    return {name: decode_ordered_map(assets[logical]) for name, logical in MASTER_PATHS.items()}


def add_reference(references: dict, logical: str, owner: str, evidence: str) -> None:
    item = references.setdefault(logical, {"logicalPath": logical, "owners": set(), "evidence": set()})
    item["owners"].add(owner)
    item["evidence"].add(evidence)


def with_suffix(value: str, suffix: str) -> str:
    return value if value.endswith(suffix) else value + suffix
# //// /读取当前 master 的客户端字节 ////


# //// 核对每个卡池的特辑关联并提取文件引用 [@x380kkm 2026-09-20] ////
def audit_masters(masters: dict) -> dict:
    gacha, features = masters["gacha"], masters["feature"]
    references = {}
    missing_features = []
    invalid_rows = []
    feature_counts = {"movie": 0, "image": 0, "skill_preview": 0}
    for pool_id, row in gacha.items():
        owner = f"gacha:{pool_id}"
        if not isinstance(row, list) or len(row) < 47:
            invalid_rows.append({"poolId": pool_id, "kind": "invalid_gacha_row"})
            continue
        add_reference(references, with_suffix(row[3], ".png"), owner, "gacha:field:3")
        add_reference(references, with_suffix(row[12], ".html.deflate"), owner, "gacha:field:12")
        for field in (17, 18, 40):
            if row[field] and row[field] != "(None)":
                add_reference(references, f"gacha/{row[field]}.gacha.amf3.deflate", owner, f"gacha:field:{field}")
        group = features.get(pool_id)
        if not isinstance(group, dict) or not group:
            missing_features.append({"poolId": pool_id, "name": row[1], "banner": row[3],
                                     "pageKind": row[4], "start": row[29], "end": row[30]})
            continue
        for key, feature in group.items():
            if not isinstance(feature, list) or len(feature) < 9 or feature[0] not in {"0", "1", "2"}:
                invalid_rows.append({"poolId": pool_id, "featureKey": key, "kind": "invalid_feature_row"})
                continue
            evidence = f"gacha_feature_content:{pool_id}/{key}"
            if feature[0] == "0":
                feature_counts["movie"] += 1
                add_reference(references, with_suffix(feature[2], ".movie.amf3.deflate"), owner, evidence)
                sprite_sheet = feature[2].rsplit("/", 1)[0] + "/sprite_sheet"
                add_reference(references, sprite_sheet + ".atlas.amf3.deflate", owner, evidence + ":sprite_sheet")
                add_reference(references, sprite_sheet + ".png", owner, evidence + ":sprite_sheet")
            elif feature[0] == "1":
                feature_counts["image"] += 1
                add_reference(references, with_suffix(feature[1], ".png"), owner, evidence)
            else:
                feature_counts["skill_preview"] += 1
    return {"scope": {"gachaCount": len(gacha), "featurePoolCount": len(features), "featureKinds": feature_counts},
            "missingFeatureGroups": missing_features, "invalidRows": invalid_rows,
            "references": [{**entry, "owners": sorted(entry["owners"]), "evidence": sorted(entry["evidence"])}
                           for entry in references.values()]}


def audit(root: Path) -> dict:
    return audit_masters(read_masters(root))
# //// /核对每个卡池的特辑关联并提取文件引用 ////


# //// 输出特辑契约和资源引用 [@x380kkm 2026-09-20] ////
def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cdn-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    report = audit(args.cdn_root.resolve(strict=True))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"report": str(args.output), **report["scope"],
                      "missingFeatureGroups": report["missingFeatureGroups"], "invalidRows": report["invalidRows"]}, ensure_ascii=False))
    return 1 if report["missingFeatureGroups"] or report["invalidRows"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
# //// /输出特辑契约和资源引用 ////
