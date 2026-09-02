// audience: internal
// # portable-save-roundtrip-test
//
// 该脚本让 Node 生成跨运行时存档包, 由 Rust 解析并重新序列化, 再由 Node 验证.
// 临时文件只用于本次测试, 不保存账号、凭据或客户端资源.

import assert from "node:assert/strict"
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join, resolve } from "node:path"
import { spawnSync } from "node:child_process"
import { fileURLToPath } from "node:url"
import {
    calculatePortableGameDataSha256,
    parseStarpointSavePackage,
} from "../../out/games/starpoint/portableSave.js"

const root = resolve(fileURLToPath(new URL("../..", import.meta.url)))
const manifestPath = join(root, "core", "personal-service", "Cargo.toml")

function runRustProbe(packageValue, expectRejected = false) {
    const temporaryRoot = mkdtempSync(join(tmpdir(), "starpoint-save-roundtrip-"))
    const inputPath = join(temporaryRoot, "input.json")
    const outputPath = join(temporaryRoot, "output.json")
    writeFileSync(inputPath, JSON.stringify(packageValue), "utf8")
    const result = spawnSync(
        "cargo",
        [
            "+1.78.0",
            "test",
            "--lib",
            "--locked",
            "--manifest-path",
            manifestPath,
            "portable_save::tests::portable_save_json_roundtrip_probe",
            "--",
            "--ignored",
            "--exact",
            "--nocapture",
        ],
        {
            cwd: root,
            env: {
                ...process.env,
                STARPOINT_PORTABLE_SAVE_ROUNDTRIP_INPUT: inputPath,
                STARPOINT_PORTABLE_SAVE_ROUNDTRIP_OUTPUT: outputPath,
                STARPOINT_PORTABLE_SAVE_ROUNDTRIP_EXPECT_REJECTED: expectRejected ? "1" : "0",
            },
            encoding: "utf8",
            maxBuffer: 16 * 1024 * 1024,
            timeout: 600_000,
        },
    )
    try {
        if (expectRejected) {
            assert.equal(result.status, 0, result.stderr)
            assert.deepEqual(JSON.parse(readFileSync(outputPath, "utf8")), { accepted: false })
            return null
        }
        assert.equal(result.status, 0, result.stderr)
        return JSON.parse(readFileSync(outputPath, "utf8"))
    } finally {
        rmSync(temporaryRoot, { recursive: true, force: true })
    }
}

const identityBearingData = JSON.parse(`{
    "__proto__":{"portable_marker":true},
    "account_id":1,
    "data_headers":{"viewer_id":2},
    "device_id":3,
    "user_character_list":{},
    "user_info":{"name":"跨运行时","rate":0.25},
    "user_tutorial":{"viewer_id":4,"tutorial_step":7},
    "viewer_id":5,
    "nested":[{"bond_token":9,"session_id":"source-session"}]
}`)
const legacyPackage = {
    format: "starpoint-save-package",
    version: 1,
    game: "starpoint",
    region: "cn",
    createdAt: "2026-08-13T00:00:00.000Z",
    source: {
        instanceKind: "remote",
        slotId: "slot-source",
        slotName: "Source",
        revisionId: "revision-source",
    },
    sourceClient: { platform: "android", version: "1.8.1" },
    payloadSha256: calculatePortableGameDataSha256(identityBearingData),
    data: identityBearingData,
}
const roundtripped = runRustProbe(JSON.parse(JSON.stringify(legacyPackage)))
assert.notEqual(roundtripped, null)
const expectedPortableData = JSON.parse(`{
    "__proto__":{"portable_marker":true},
    "user_character_list":{},
    "user_info":{"name":"跨运行时","rate":0.25},
    "user_tutorial":{"tutorial_step":7},
    "nested":[{"bond_token":9}]
}`)
assert.deepEqual(roundtripped.data, expectedPortableData)
assert.notEqual(parseStarpointSavePackage(roundtripped), null)
assert.equal(roundtripped.payloadSha256, calculatePortableGameDataSha256(roundtripped.data))

const tampered = structuredClone(legacyPackage)
tampered.data.user_info.name = "被篡改"
runRustProbe(tampered, true)
assert.equal(parseStarpointSavePackage(tampered), null)

process.stdout.write("Portable save Node/Rust roundtrip test passed.\n")
