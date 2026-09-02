// audience: internal
// # cn-evidence-tests
//
// 该脚本验证 CN 动态实验摘要和 HTTP 元数据不会携带敏感字段.

import assert from "node:assert/strict"
import { mkdtempSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { readAndValidateCnEvidence, validateCnMetadataRecord, validateCnRunSummary } from "./cn-evidence.mjs"

const validSummary = {
    schemaVersion: 1,
    run: "protocol-lab:android-cn-m4-safe-metadata-test",
    client: "cn-android-1.8.1",
    target: "10.0.2.2:8001",
    emulator: "starpoint-cn-api35-x86_64",
    gpu: "swiftshader_indirect",
    serverPort: 8001,
    sessionPort: 8003,
    versionRequest: "GET /shijtswy/version/client_release_android.dis",
    versionResponse: "200 text/plain",
    loginRoutes: ["POST /api/index.php/tool/signup", "POST /api/index.php/load"],
    responseStatuses: "all 200",
    nextUi: "download method selection",
    rawDataStoredElsewhere: false,
}

const validRecords = [
    {
        observedAtUtc: "2026-08-13T00:00:00.000Z",
        method: "POST",
        path: "/api/index.php/tool/signup",
        status: 200,
        contentType: "application/x-msgpack",
    },
    {
        observedAtUtc: "2026-08-13T00:00:01.000Z",
        method: "POST",
        path: "/api/index.php/load",
        status: 200,
        contentType: "application/x-msgpack",
    },
]

// //// 验证合法 CN 证据文件和路由覆盖 [@x380kkm 2026-08-13] ////
const temporaryRoot = mkdtempSync(join(tmpdir(), "starpoint-cn-evidence-"))
try {
    const summaryPath = join(temporaryRoot, "safe-summary.json")
    const metadataPath = join(temporaryRoot, "http-metadata.jsonl")
    writeFileSync(summaryPath, JSON.stringify(validSummary), "utf8")
    writeFileSync(metadataPath, `${validRecords.map((record) => JSON.stringify(record)).join("\n")}\n`, "utf8")
    const result = readAndValidateCnEvidence(summaryPath, metadataPath, validSummary.loginRoutes)
    assert.equal(result.records.length, 2)
    assert.equal(result.observedRoutes.size, 2)
} finally {
    rmSync(temporaryRoot, { recursive: true, force: true })
}
// //// /验证合法 CN 证据文件和路由覆盖 ////

// //// 验证敏感字段和非法路径被拒绝 [@x380kkm 2026-08-13] ////
assert.throws(
    () => validateCnMetadataRecord({ ...validRecords[0], path: "/api/index.php/load?viewer_id=123" }),
    /invalid CN evidence/,
)
assert.throws(
    () => validateCnMetadataRecord({ ...validRecords[0], token: "should-not-be-recorded" }),
    /invalid CN evidence/,
)
assert.throws(
    () => validateCnRunSummary({ ...validSummary, rawDataStoredElsewhere: true }),
    /invalid CN evidence/,
)
assert.throws(
    () => validateCnRunSummary({ ...validSummary, managementToken: "should-not-be-recorded" }),
    /invalid CN evidence/,
)
assert.doesNotThrow(() => validateCnRunSummary({ ...validSummary, run: "protocol-lab:android-cn-cold-start-replay-20260813T105011Z" }))
// //// /验证敏感字段和非法路径被拒绝 ////

process.stdout.write("CN evidence safety validator passed.\n")
