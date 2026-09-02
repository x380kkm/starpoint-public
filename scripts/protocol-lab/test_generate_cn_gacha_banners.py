# audience: internal
# # test-generate-cn-gacha-banners
# 此测试验证卡池 banner 生成保留富活动目录中的每日图片、时间和资源身份.

# /// script
# requires-python = ">=3.12"
# dependencies = ["Pillow"]
# ///

from __future__ import annotations

import io
import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from cn_gacha_banner_assets import EntityCatalog, EntityRecord, encode_entity_digest
from generate_cn_gacha_banners import (
    _discover_rich_catalog_path,
    _materialize_rich_catalog_assets,
    _merge_rich_catalog_fields,
    _require_rich_candidate_identity,
    _to_activity_catalog_manifest,
)
from PIL import Image


def png_bytes(color: tuple[int, int, int]) -> bytes:
    output = io.BytesIO()
    Image.new("RGB", (640, 360), color).save(output, format="PNG")
    return output.getvalue()


def daily_activity(index: int, candidates: list[dict[str, object]]) -> dict[str, object]:
    prefix = "daily-exp-mana" if index == 0 else "daily-week"
    identifier = 1 if index == 0 else index
    activity = {
        "activity_id": f"{prefix}:{identifier}",
        "name": f"Daily {index}",
        "kind": "daily",
        "tags": ["CN", "Daily"],
        "description": "",
        "image_candidates": candidates,
    }
    if index not in (0, 19):
        activity["default_start_at_ms"] = 1_483_214_400_000 + index
        activity["default_end_at_ms"] = 1_514_750_400_000 + index
    return activity


def candidate(
    index: int, data: bytes, source_type: str
) -> tuple[dict[str, object], EntityRecord]:
    source_hash = f"{index + 1:040x}"
    source_entry = f"production/upload/{source_hash[:2]}/{source_hash[2:]}"
    return (
        {
            "source_hash": source_hash,
            "source_type": source_type,
            "logical_path": f"quest/daily/{index}.png",
            "source_entry": source_entry,
            "source_version": "1.4.0",
            "source_byte_length": len(data),
            "source_digest": encode_entity_digest(data),
            "source_asset_kind": "common",
            "evidence": f"master:daily:field:{index}",
        },
        EntityRecord(source_entry, len(data), encode_entity_digest(data), "common"),
    )
# //// 验证 922 项目录保留每日 41 个资源候选 [@x380kkm 2026-08-28] ////
class RichActivityCatalogTests(unittest.TestCase):
    def test_rejects_a_rich_candidate_without_identity(self) -> None:
        with self.assertRaisesRegex(
            RuntimeError, "requires key or source_hash"
        ):
            _require_rich_candidate_identity(
                "daily-exp-mana:1", {"source_entry": "production/upload/00/" + "0" * 38}
            )

    def test_rejects_a_rich_candidate_without_entity_path(self) -> None:
        with self.assertRaisesRegex(RuntimeError, "requires source_entry"):
            _merge_rich_catalog_fields(
                {
                    "activities": [
                        {
                            "activity_id": "daily-exp-mana:1",
                            "kind": "daily",
                            "image_candidates": [],
                        }
                    ]
                },
                {
                    "activities": [
                        {
                            "activity_id": "daily-exp-mana:1",
                            "kind": "daily",
                            "image_candidates": [
                                {
                                    "source_hash": "0" * 40,
                                    "source_type": "activity_banner",
                                }
                            ],
                        }
                    ]
                },
            )

    def test_reports_a_projected_candidate_without_entity_path(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            catalog = {
                "activities": [
                    {
                        "activity_id": "event:1",
                        "kind": "event",
                        "image_candidates": [{"key": "0" * 40 + ".png"}],
                    }
                ]
            }
            requested, materialized, gaps = _materialize_rich_catalog_assets(
                catalog,
                root,
                root / "output",
                EntityCatalog(root / "PathFile.csv", {}),
            )
            self.assertEqual(0, requested)
            self.assertEqual(0, materialized)
            self.assertEqual("missing_catalog_entity_path", gaps[0]["kind"])

    def test_tracked_cn_catalog_source_contains_daily_resources(self) -> None:
        repository_root = Path(__file__).resolve().parents[2]
        source_path = repository_root / "assets" / "cn-activity-catalog-source.json"
        self.assertTrue(source_path.is_file())
        catalog = json.loads(source_path.read_text(encoding="utf-8"))
        activities = catalog["activities"]
        gacha = [activity for activity in activities if activity["kind"] == "gacha"]
        daily = [
            activity
            for activity in activities
            if str(activity["activity_id"]).startswith(
                ("daily-exp-mana:", "daily-week:")
            )
        ]
        self.assertEqual(922, len(activities))
        self.assertEqual(483, len(gacha))
        self.assertTrue(all(activity.get("banner_key") for activity in gacha))
        self.assertTrue(
            all("banner:generated" in activity.get("tags", []) for activity in gacha)
        )
        self.assertEqual(20, len(daily))
        self.assertEqual(41, sum(len(activity["image_candidates"]) for activity in daily))

    def test_package_catalog_initializer_replaces_the_minimal_bundle_catalog(self) -> None:
        repository_root = Path(__file__).resolve().parents[2]
        package_script = (
            repository_root
            / "scripts"
            / "protocol-lab"
            / "package-ios-cn-personal-service.ps1"
        )
        with tempfile.TemporaryDirectory() as temporary_directory:
            bundle = Path(temporary_directory)
            (bundle / "activity-catalog.json").write_text(
                json.dumps({"activities": [{"activity_id": "raid:1", "kind": "raid"}]}),
                encoding="utf-8",
            )
            helper_script = f"""
$ErrorActionPreference = 'Stop'
$scriptPath = '{package_script.as_posix()}'
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($scriptPath, [ref]$tokens, [ref]$errors)
if ($errors.Count -gt 0) {{ throw $errors[0].Message }}
$definitions = @($ast.FindAll({{ param($node)
    $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
    $node.Name -in @('Find-CnRichActivityCatalog', 'Initialize-CnActivityCatalog')
}}, $true) | Sort-Object {{ $_.Extent.StartOffset }} | ForEach-Object {{ $_.Extent.Text }}) -join "`n"
. ([scriptblock]::Create($definitions))
Initialize-CnActivityCatalog -RepositoryRoot '{repository_root.as_posix()}' -CnCdnBundlePath '{bundle.as_posix()}' | ConvertTo-Json -Compress
"""
            result = subprocess.run(
                ["pwsh", "-NoLogo", "-NoProfile", "-NonInteractive", "-Command", helper_script],
                capture_output=True,
                text=True,
                check=True,
            )
            report = json.loads(result.stdout)
            catalog = json.loads((bundle / "activity-catalog.json").read_text(encoding="utf-8"))
            activities = catalog["activities"]
            gacha = [activity for activity in activities if activity["kind"] == "gacha"]
            self.assertEqual(
                repository_root / "assets" / "cn-activity-catalog-source.json",
                Path(report["source"]),
            )
            self.assertEqual(922, report["activity_count"])
            self.assertEqual(483, report["gacha_activity_count"])
            self.assertEqual(20, report["daily_activity_count"])
            self.assertEqual(922, len(activities))
            self.assertEqual(483, len(gacha))
            self.assertTrue(all(activity.get("banner_key") for activity in gacha))
            self.assertTrue(
                all("banner:generated" in activity.get("tags", []) for activity in gacha)
            )

    def test_discovers_adjacent_activity_catalog_source(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            catalog_path = root / "activity-catalog.json"
            source_path = root / "activity-catalog-source.json"
            catalog_path.write_text("{}", encoding="utf-8")
            source_path.write_text("{}", encoding="utf-8")
            self.assertEqual(
                source_path.resolve(),
                _discover_rich_catalog_path(catalog_path, None),
            )

    def test_preserves_daily_catalog_fields_and_reachable_assets(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            source_root = root / "source"
            output_root = root / "output"
            source_root.mkdir()
            output_root.mkdir()

            assets: dict[str, bytes] = {}
            records: dict[str, EntityRecord] = {}
            rich_daily: list[dict[str, object]] = []
            candidate_index = 0
            for activity_index in range(20):
                candidate_count = 3 if activity_index == 0 else 2
                activity_candidates: list[dict[str, object]] = []
                for activity_candidate_index in range(candidate_count):
                    data = png_bytes(
                        (
                            20 + activity_index,
                            40 + activity_candidate_index,
                            80 + candidate_index,
                        )
                    )
                    source_type = (
                        "activity_banner"
                        if activity_candidate_index == 0
                        else "quest_cover"
                    )
                    definition, record = candidate(candidate_index, data, source_type)
                    key = f"catalog-candidate/{definition['source_hash']}.png"
                    assets[key] = data
                    records[record.entry_path] = record
                    activity_candidates.append(definition)
                    candidate_index += 1
                rich_daily.append(daily_activity(activity_index, activity_candidates))

            rich_catalog = {
                "format_version": 1,
                "region": "cn",
                "client_version": "1.8.4",
                "asset_version": "1.4.60",
                "activities": [
                    *rich_daily,
                    *(
                        {
                            "activity_id": f"gacha:{index + 1}",
                            "name": f"Gacha {index + 1}",
                            "kind": "gacha",
                            "tags": ["CN"],
                            "description": "",
                            "image_candidates": [],
                        }
                        for index in range(584)
                    ),
                    *(
                        {
                            "activity_id": f"event:{index + 1}",
                            "name": f"Event {index + 1}",
                            "kind": "event",
                            "tags": ["CN"],
                            "description": "",
                            "image_candidates": [],
                        }
                        for index in range(419)
                    ),
                ],
            }
            catalog = {
                **rich_catalog,
                "activities": [
                    {
                        **activity,
                        "image_candidates": [],
                    }
                    for activity in rich_catalog["activities"]
                    if not (
                        str(activity["activity_id"]).startswith("gacha:")
                        and int(str(activity["activity_id"]).removeprefix("gacha:"))
                        > 483
                    )
                ],
            }

            enriched_activities, enriched_candidates = _merge_rich_catalog_fields(
                catalog, rich_catalog
            )
            entity_catalog = EntityCatalog(root / "PathFile.csv", records)
            with patch(
                "generate_cn_gacha_banners.read_logical_assets",
                return_value=(assets, []),
            ):
                requested, materialized, gaps = _materialize_rich_catalog_assets(
                    catalog, source_root, output_root, entity_catalog
                )
            manifest = _to_activity_catalog_manifest(
                catalog, (output_root / "activity-banners",)
            )

            activities = manifest["activities"]
            daily = [
                activity
                for activity in activities
                if str(activity["activity_id"]).startswith(
                    ("daily-exp-mana:", "daily-week:")
                )
            ]
            self.assertEqual(922, len(activities))
            self.assertEqual(483, sum(activity["kind"] == "gacha" for activity in activities))
            self.assertEqual(20, len(daily))
            self.assertEqual(20, enriched_activities)
            self.assertEqual(41, enriched_candidates)
            self.assertEqual(41, requested)
            self.assertEqual(41, materialized)
            self.assertEqual([], gaps)
            self.assertEqual(41, sum(len(activity["image_candidates"]) for activity in daily))
            self.assertTrue(all(activity["image_candidates"] for activity in daily))
            self.assertEqual(1_483_214_400_001, daily[1]["default_start_at_ms"])
            self.assertEqual(1_514_750_400_001, daily[1]["default_end_at_ms"])
            self.assertNotIn("default_start_at_ms", daily[0])
            for activity in daily:
                for image in activity["image_candidates"]:
                    self.assertTrue(
                        (output_root / "activity-banners" / image["key"]).is_file()
                    )

    def test_power_shell_coverage_helper_checks_final_catalog_shape(self) -> None:
        repository_root = Path(__file__).resolve().parents[2]
        package_script = (
            repository_root
            / "scripts"
            / "protocol-lab"
            / "package-ios-cn-personal-service.ps1"
        )
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            bundle = root / "bundle"
            entities = bundle / "entities"
            banners = bundle / "activity-banners"
            entities.mkdir(parents=True)
            banners.mkdir()

            daily: list[dict[str, object]] = []
            manifest_lines: list[str] = []
            for index in range(20):
                candidates: list[dict[str, object]] = []
                count = 3 if index == 0 else 2
                for offset in range(count):
                    candidate_index = index * 2 + offset
                    if index == 0:
                        candidate_index = offset
                    source_hash = f"{candidate_index + 1:040x}"
                    source_entry = (
                        f"production/upload/{source_hash[:2]}/{source_hash[2:]}"
                    )
                    candidates.append(
                        {
                            "key": f"{source_hash}.png",
                            "source_hash": source_hash,
                            "source_entry": source_entry,
                        }
                    )
                    (banners / f"{source_hash}.png").write_bytes(b"png")
                    manifest_lines.append(
                        f"{source_entry},1.4.0,3,{'A' * 43},common"
                    )
                daily.append(
                    {
                        "activity_id": (
                            "daily-exp-mana:1"
                            if index == 0
                            else f"daily-week:{index}"
                        ),
                        "kind": "daily",
                        "image_candidates": candidates,
                    }
                )
            activities = [
                *daily,
                *(
                    {"activity_id": f"gacha:{index}", "kind": "gacha"}
                    for index in range(483)
                ),
                *(
                    {"activity_id": f"event:{index}", "kind": "event"}
                    for index in range(419)
                ),
            ]
            rich_path = root / "rich.json"
            (bundle / "activity-catalog.json").write_text(
                json.dumps({"activities": activities}), encoding="utf-8"
            )
            rich_path.write_text(
                json.dumps({"activities": daily}), encoding="utf-8"
            )
            for manifest_name in ("PathFile.csv", "10939-ios_medium.csv"):
                (entities / manifest_name).write_text(
                    "\n".join(manifest_lines) + "\n", encoding="utf-8"
                )

            helper_script = f"""
$ErrorActionPreference = 'Stop'
$scriptPath = '{package_script.as_posix()}'
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($scriptPath, [ref]$tokens, [ref]$errors)
if ($errors.Count -gt 0) {{ throw $errors[0].Message }}
$definitions = @($ast.FindAll({{ param($node)
    $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
    $node.Name -in @('Get-OptionalPropertyValue', 'Assert-CnDailyActivityCoverage')
}}, $true) | Sort-Object {{ $_.Extent.StartOffset }} | ForEach-Object {{ $_.Extent.Text }}) -join "`n"
. ([scriptblock]::Create($definitions))
$coverage = Assert-CnDailyActivityCoverage -CnCdnBundlePath '{bundle.as_posix()}' -RichCatalogPath '{rich_path.as_posix()}'
$coverage | ConvertTo-Json -Compress
"""
            result = subprocess.run(
                ["pwsh", "-NoLogo", "-NoProfile", "-NonInteractive", "-Command", helper_script],
                capture_output=True,
                text=True,
                check=True,
            )
            coverage = json.loads(result.stdout)
            self.assertEqual(922, coverage["activity_count"])
            self.assertEqual(483, coverage["gacha_activity_count"])
            self.assertEqual(20, coverage["daily_activity_count"])
            self.assertEqual(41, coverage["daily_candidate_count"])

            activities.pop()
            (bundle / "activity-catalog.json").write_text(
                json.dumps({"activities": activities}), encoding="utf-8"
            )
            failed = subprocess.run(
                ["pwsh", "-NoLogo", "-NoProfile", "-NonInteractive", "-Command", helper_script],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertNotEqual(0, failed.returncode)
            self.assertIn("922 activities and 483 gacha", failed.stderr + failed.stdout)


# //// /验证 922 项目录保留每日 41 个资源候选 ////


if __name__ == "__main__":
    unittest.main()
