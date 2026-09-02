// audience: internal
// # reference-non-game-surface-audit-test
//
// 该测试对真实参考入口运行非游戏运行面审计, 并验证 SDK, CDN 清单, 固定补丁位点和失败判定.

import assert from "node:assert/strict"
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { buildReferenceNonGameSurfaceAudit } from "./audit-reference-non-game-surface.mjs"

const SCRIPT_ROOT = path.dirname(fileURLToPath(import.meta.url))
const localRoot = path.resolve(SCRIPT_ROOT, "../..")
const referenceRoot = path.resolve(localRoot, "../startpoint-cn-launcher")
const temporaryRoot = mkdtempSync(path.join(tmpdir(), "starpoint-reference-surface-"))
const cnCdnBundle = path.join(temporaryRoot, "StarpointCNCDN")

mkdirSync(path.join(cnCdnBundle, "archive-common-full"), { recursive: true })
mkdirSync(path.join(cnCdnBundle, "entities"), { recursive: true })
mkdirSync(path.join(cnCdnBundle, "management-assets", "item-icons"), { recursive: true })
writeFileSync(path.join(cnCdnBundle, "path"), JSON.stringify({
    info: {
        client_asset_version: "1.4.54",
        target_asset_version: "1.4.54",
        eventual_target_asset_version: "1.4.54",
        is_initial: true,
        latest_maj_first_version: "1.4.0",
    },
    full: { version: "1.4.0", archive: [] },
    diff: [],
    asset_version_hash: "",
}), "utf8")
writeFileSync(path.join(cnCdnBundle, "activity-catalog.json"), '{"activities":[]}', "utf8")
writeFileSync(path.join(cnCdnBundle, "entities", "10939-android_medium.csv"), "", "utf8")
writeFileSync(path.join(cnCdnBundle, "entities", "10939-ios_medium.csv"), "", "utf8")

// //// 验证参考入口的完整非游戏运行面 [@x380kkm 2026-08-24] ////
try {
    const report = buildReferenceNonGameSurfaceAudit({ localRoot, referenceRoot, cnCdnBundle })

    assert.equal(report.status, "passed")
    assert.equal(report.summary.failureCount, 0)
    assert.equal(report.summary.referenceManagementRouteCount, 49)
    assert.equal(report.summary.referenceStaticMountCount, 2)
    assert.equal(report.summary.referenceIosSdkRouteCount, 22)
    assert.equal(report.summary.referenceFixedIosPatchSiteCount, 22)
    assert.equal(report.summary.referenceIosPatchRoutineCount, 8)
    assert.equal(report.staticSurface.files.localCnCdnBundle.exists, true)
    assert.equal(report.staticSurface.files.localCnCdnBundle.manifest.status, "valid")
    assert.ok(report.management.routes.every((route) => route.reference.anchor.match(/:\d+$/)))
    assert.ok(report.ios.fixedPatches.every((patch) => patch.status === "matched"))
    assert.ok(report.ios.facts.some((fact) => fact.status === "explicit-local-extension"))

    const missingBundleReport = buildReferenceNonGameSurfaceAudit({
        localRoot,
        referenceRoot,
        cnCdnBundle: path.join(temporaryRoot, "missing"),
        explicitBundle: true,
    })
    assert.equal(missingBundleReport.status, "failed")
    assert.ok(missingBundleReport.summary.missing > 0)
} finally {
    rmSync(temporaryRoot, { recursive: true, force: true })
}
// //// /验证参考入口的完整非游戏运行面 ////

process.stdout.write("reference non-game surface audit test passed\n")
