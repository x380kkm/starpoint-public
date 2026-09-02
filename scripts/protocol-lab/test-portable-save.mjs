// audience: internal
// # portable-save-tests
//
// 该脚本验证 Node 存档包摘要与 Rust 固定向量一致, 并拒绝被替换的数据.

import assert from "node:assert/strict"
import {
    calculatePortableGameDataSha256,
    createStarpointSavePackage,
    parseStarpointSavePackage,
} from "../../out/games/starpoint/portableSave.js"
import {
    mergePortableExtrasIntoClientData,
    reconcilePortableGameData,
} from "../../out/games/starpoint/portablePlayerData.js"

// //// 验证跨运行时摘要, 格式和存档包篡改拒绝 [@x380kkm 2026-07-27] ////
const data = {
    user_character_list: {},
    user_info: { name: "Portable", rate: 0.25 },
}
assert.equal(
    calculatePortableGameDataSha256(data),
    "a1817b16f66c8d8d2880ac9634a0653a902dd48270dd9c4f9b3e95d6e8797337",
)
assert.equal(
    calculatePortableGameDataSha256({
        user_character_list: {},
        user_info: { numbers: [1, 1.0, -0, 1.25e-7, 1e-7, 1e-6] },
    }),
    "fb6110301d62dd2725bcb487dd1bc6ca4ef6f646294c614b4c450234691a2c01",
)

const savePackage = createStarpointSavePackage({
    data,
    createdAt: "2026-07-27T00:00:00.000Z",
    source: {
        instanceKind: "local",
        slotId: "1",
        slotName: "First",
        revisionId: null,
    },
})
assert.notEqual(parseStarpointSavePackage(savePackage), null)

assert.throws(
    () => createStarpointSavePackage({
        data,
        createdAt: "2026-07-27T00:00:00.000Z",
        source: {
            instanceKind: "local",
            slotId: "1",
            slotName: "First",
            revisionId: null,
            unexpected: true,
        },
    }),
    /source is invalid/,
)
assert.throws(
    () => createStarpointSavePackage({
        data,
        createdAt: "2026-07-27T00:00:00.000Z",
        source: {
            instanceKind: "local",
            slotId: "1",
            slotName: "First",
            revisionId: null,
        },
        sourceClient: { platform: "desktop", version: "1.0" },
    }),
    /client metadata is invalid/,
)

const prototypeKeyData = JSON.parse(
    '{"__proto__":{"portable_marker":true},"user_character_list":{},"user_info":{"name":"Prototype key"}}',
)
const prototypeKeyPackage = createStarpointSavePackage({
    data: prototypeKeyData,
    createdAt: "2026-08-03T00:00:00.000Z",
    source: {
        instanceKind: "local",
        slotId: null,
        slotName: null,
        revisionId: null,
    },
})
assert.equal(
    prototypeKeyPackage.payloadSha256,
    "44de641c5c74646295fcad9cd3896dd229691d7fbf6f0962ef1e06957d8a654a",
)
assert.equal(Object.hasOwn(prototypeKeyPackage.data, "__proto__"), true)
assert.deepEqual(prototypeKeyPackage.data.__proto__, { portable_marker: true })
const reparsedPrototypeKeyPackage = parseStarpointSavePackage(
    JSON.parse(JSON.stringify(prototypeKeyPackage)),
)
assert.notEqual(reparsedPrototypeKeyPackage, null)
assert.equal(Object.hasOwn(reparsedPrototypeKeyPackage.data, "__proto__"), true)
assert.equal(reparsedPrototypeKeyPackage.payloadSha256, prototypeKeyPackage.payloadSha256)

const identityBearingData = {
    account_id: 1,
    associate_token: "source-associate-token",
    block_list: [{ viewer_id: 2 }],
    data_headers: { viewer_id: 3 },
    device_id: 4,
    follow_info: [{ viewer_id: 5 }],
    follow_list: [{ viewer_id: 6 }],
    followed_count: 1,
    follower_list: [{ viewer_id: 7 }],
    friend_list: [{ viewer_id: 8 }],
    friends: [{ viewer_id: 9 }],
    keychain: "source-keychain",
    management_role: "admin",
    management_token: "source-management-token",
    permissions: ["manage"],
    player_id: 10,
    session: { id: "source-session" },
    session_id: "source-session-id",
    shell_credential: "source-shell-credential",
    shell_id: "source-shell-id",
    token: "source-token",
    transfer_token: "source-transfer-token",
    user_character_list: {},
    user_info: { name: "身份迁移", bond_token: 7 },
    user_tutorial: { viewer_id: 11, tutorial_step: 4 },
    viewer_id: 12,
    nested: [{ viewer_id: 13, bond_token: 9 }],
}
const identityPackage = createStarpointSavePackage({
    data: identityBearingData,
    createdAt: "2026-08-03T00:00:00.000Z",
    source: {
        instanceKind: "remote",
        slotId: "12",
        slotName: "Source",
        revisionId: "revision-source",
    },
})
assert.equal(
    identityPackage.payloadSha256,
    "536876fe1aee54176779e0dd84d5fde67c580764787de5dd3dad5e4a02686b65",
)
assert.deepEqual(identityPackage.data, {
    user_character_list: {},
    user_info: { name: "身份迁移", bond_token: 7 },
    user_tutorial: { tutorial_step: 4 },
    nested: [{ bond_token: 9 }],
})

const legacyIdentityPackage = structuredClone(identityPackage)
legacyIdentityPackage.data = structuredClone(identityBearingData)
legacyIdentityPackage.payloadSha256 = calculatePortableGameDataSha256(legacyIdentityPackage.data)
const migratedLegacyPackage = parseStarpointSavePackage(legacyIdentityPackage)
assert.notEqual(migratedLegacyPackage, null)
assert.deepEqual(migratedLegacyPackage.data, identityPackage.data)
assert.equal(migratedLegacyPackage.payloadSha256, identityPackage.payloadSha256)

const tamperedLegacyPackage = structuredClone(legacyIdentityPackage)
tamperedLegacyPackage.data.user_info.name = "Tampered legacy package"
assert.equal(parseStarpointSavePackage(tamperedLegacyPackage), null)

assert.equal(parseStarpointSavePackage({ ...savePackage, unexpected: true }), null)
assert.equal(parseStarpointSavePackage({ ...savePackage, createdAt: "not-a-time" }), null)
assert.equal(parseStarpointSavePackage({ ...savePackage, createdAt: "2026-02-30T00:00:00.000Z" }), null)
assert.equal(
    parseStarpointSavePackage({
        ...savePackage,
        source: { ...savePackage.source, unexpected: true },
    }),
    null,
)
assert.equal(
    parseStarpointSavePackage({
        ...savePackage,
        sourceClient: { platform: "desktop", version: "1.0" },
    }),
    null,
)
assert.throws(
    () => calculatePortableGameDataSha256({
        user_character_list: {},
        user_info: { value: 9_007_199_254_740_992 },
    }),
    /unsafe integer/,
)

const tampered = structuredClone(savePackage)
tampered.data.user_info.name = "Tampered"
assert.equal(parseStarpointSavePackage(tampered), null)

const sourceSnapshot = JSON.parse(`{
    "__proto__":{"portable_marker":true},
    "available_asset_version":"1.4.54",
    "raw_only":{"value":1},
    "user_info":{"name":"Source","source_only":7},
    "list":[{"source":true}]
}`)
const importBaseline = {
    available_asset_version: "2.1.125",
    user_info: { name: "Source" },
    list: [{ normalized: true }],
    serializer_added: true,
}
assert.deepEqual(
    reconcilePortableGameData(sourceSnapshot, importBaseline, structuredClone(importBaseline)),
    sourceSnapshot,
)
const changedCurrent = structuredClone(importBaseline)
changedCurrent.user_info.name = "Changed"
const reconciled = reconcilePortableGameData(sourceSnapshot, importBaseline, changedCurrent)
const expectedReconciled = structuredClone(sourceSnapshot)
expectedReconciled.user_info.name = "Changed"
assert.deepEqual(reconciled, expectedReconciled)
assert.equal(Object.hasOwn(reconciled, "__proto__"), true)
const clientData = mergePortableExtrasIntoClientData(sourceSnapshot, importBaseline, changedCurrent)
assert.equal(Object.hasOwn(clientData, "__proto__"), true)
assert.deepEqual(clientData, JSON.parse(`{
    "__proto__":{"portable_marker":true},
    "available_asset_version":"2.1.125",
    "raw_only":{"value":1},
    "user_info":{"name":"Changed","source_only":7},
    "list":[{"normalized":true}],
    "serializer_added":true
}`))
process.stdout.write("Portable save contract test passed.\n")
// //// /验证跨运行时摘要, 格式和存档包篡改拒绝 ////
