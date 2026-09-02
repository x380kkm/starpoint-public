// audience: internal
// # test-audit-cn-shop-reward-closure
//
// 该脚本核对商店审计器区分客户端目录外条目与服务端结构断裂.

import assert from "node:assert/strict"
import fs from "node:fs"
import os from "node:os"
import path from "node:path"

import {
    auditShopRewardClosure,
    reportHasStructuralIssues,
} from "./audit-cn-shop-reward-closure.mjs"

// //// 构造独立的商店与参考目录 [@x380kkm 2026-08-28] ////
function writeJson(filePath, value) {
    fs.mkdirSync(path.dirname(filePath), { recursive: true })
    fs.writeFileSync(filePath, `${JSON.stringify(value, null, 2)}\n`, "utf8")
}

function writeAuditDirectories(root) {
    const assetRoot = path.join(root, "assets")
    const referenceAssetRoot = path.join(root, "reference-assets")
    writeJson(path.join(assetRoot, "treasure_shop.json"), {
        2001: { costs: [], rewards: [{ type: 1, count: 10 }], userCost: { type: 1, amount: 5 } },
    })
    writeJson(path.join(assetRoot, "general_shop.json"), {
        8001: { costs: [], rewards: [{ type: 0, id: 10, count: 1 }] },
    })
    writeJson(path.join(assetRoot, "star_grain_shop.json"), {
        9001: { costs: [{ id: 20, amount: 1 }], rewards: [{ type: 2, count: 100 }] },
    })
    writeJson(path.join(assetRoot, "equipment_enhancement_shop.json"), {
        10001: { costs: [{ id: 10, amount: 1 }], rewards: [], equipmentId: 500 },
    })
    writeJson(path.join(assetRoot, "event_item_shop.json"), {
        0: { 1: { 4001: { costs: [{ id: 10, amount: 1 }], rewards: [{ type: 3, id: 100, count: 1 }] } } },
    })
    writeJson(path.join(assetRoot, "event_item_shop_id_map.json"), {
        4001: { eventType: 0, eventId: 1 },
    })
    writeJson(path.join(assetRoot, "boss_coin_shop.json"), {
        1: { 7000: { costs: [{ id: 20, amount: 1 }], rewards: [{ type: 4, id: 500, count: 1 }] } },
    })
    writeJson(path.join(assetRoot, "boss_coin_shop_item_category_map.json"), { 7000: 1 })
    writeJson(path.join(assetRoot, "cdn_shop_master_whitelists.json"), {
        2: [2001],
        4: [4001],
        7: [7000],
        8: [8001],
        9: [9001, 9999],
        10: [10001],
    })
    writeJson(path.join(referenceAssetRoot, "item_ids.json"), [10, 20])
    writeJson(path.join(referenceAssetRoot, "equipment_ids.json"), [500])
    writeJson(path.join(referenceAssetRoot, "character.json"), { 100: {} })
    writeJson(path.join(referenceAssetRoot, "cdndata", "boss_coin_shop.json"), {
        7000: [["1", "", "", "", "", "", "", "7000", "", "", "", "", "", "", "", "", "(None)", "20", "1", "(None)", "", "(None)", "", "(None)", "", "2020-01-01 00:00:00", "(None)", "1", "", "", "", "(None)", "4", "500", "1"]],
    })
    return { assetRoot, referenceAssetRoot }
}
// //// /构造独立的商店与参考目录 ////

// //// 核对目录外条目与引用断裂的分类 [@x380kkm 2026-08-28] ////
const temporaryRoot = fs.mkdtempSync(path.join(os.tmpdir(), "starpoint-shop-audit-"))
try {
    const roots = writeAuditDirectories(temporaryRoot)
    const report = auditShopRewardClosure(roots)
    assert.equal(reportHasStructuralIssues(report), false)
    assert.deepEqual(report.summary, {
        structural_issue_count: 0,
        catalog_gap_count: 1,
        missing_server_catalog_entry_count: 0,
        client_master_only_entry_count: 1,
        server_catalog_only_count: 0,
        whitelist_item_count: 7,
        resolvable_whitelist_item_count: 6,
        audited_catalog_item_count: 6,
    })
    assert.equal(report.reference_catalogs.equipment_count, 1)
    assert.deepEqual(report.catalog_gap_groups.map((group) => [group.kind, group.shop_item_ids]), [
        ["client_master_only_entry", [9999]],
    ])

    writeJson(path.join(roots.assetRoot, "boss_coin_shop.json"), {})
    writeJson(path.join(roots.assetRoot, "boss_coin_shop_item_category_map.json"), {})
    writeJson(path.join(roots.assetRoot, "general_shop.json"), {
        8001: { costs: [], rewards: [{ type: 4, id: 999, count: 1 }] },
    })
    const brokenReport = auditShopRewardClosure(roots)
    assert.equal(reportHasStructuralIssues(brokenReport), true)
    assert.equal(brokenReport.structural_issue_counts.missing_server_catalog_entry, 1)
    assert.equal(brokenReport.structural_issue_counts.invalid_reward_reference, 1)
} finally {
    fs.rmSync(temporaryRoot, { recursive: true, force: true })
}

process.stdout.write("CN shop reward closure audit classification verified\n")
// //// /核对目录外条目与引用断裂的分类 ////
