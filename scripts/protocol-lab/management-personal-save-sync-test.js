// audience: internal
// # management-personal-save-sync-test
//
// 此脚本辅助管理 API 测试启动 Rust 个人服务并验证真实 loopback 密文存档同步.
// 调用方提供已监听的管理服务和临时数据库. 此脚本终止自己启动的个人服务进程.

const assert = require("node:assert/strict")
const path = require("node:path")
const Database = require("better-sqlite3")
const {
    requestJson,
    signupCn,
    startPersonalService,
    stopChildProcess,
} = require("./loopback-test-services")

// //// 通过真实 Node 和 Rust 进程同步密文存档 [@x380kkm 2026-07-23] ////
async function verifyPersonalServiceSaveSync(app, root, accessStore, playerCredentials) {
    const address = app.server.address()
    assert.notEqual(address, null)
    assert.equal(typeof address, "object")
    assert.equal(address.address, "127.0.0.1")

    const serviceRoot = path.join(root, "personal-service-save-sync")
    const personalService = await startPersonalService(serviceRoot)
    try {
        const managementToken = personalService.managementToken
        await signupCn(personalService.baseUrl, 91001, "management-personal-save-sync-test")
        const initialState = await requestPersonalService(
            personalService.port,
            managementToken,
            "GET",
            "/v1/local-saves",
            undefined,
            200,
        )
        assert.equal(initialState.slots.length, 1)
        assert.equal(initialState.devices.length, 1)
        const sourceSlotId = initialState.slots[0].id
        assert.equal(initialState.devices[0].active_slot_id, sourceSlotId)
        const sourceExport = await requestPersonalService(
            personalService.port,
            managementToken,
            "GET",
            `/v1/local-saves/${sourceSlotId}/export`,
            undefined,
            200,
        )
        assert.equal(Object.hasOwn(sourceExport.data, "associate_token"), false)
        assert.equal(Object.hasOwn(sourceExport.data.user_tutorial, "viewer_id"), false)

        const target = await requestPersonalService(
            personalService.port,
            managementToken,
            "POST",
            "/v1/save-sync-targets",
            {
                name: "Node management save server",
                scheme: "http",
                host: "127.0.0.1",
                port: address.port,
                username: playerCredentials.username,
                password: playerCredentials.password,
            },
            201,
        )
        assert.equal(target.has_credentials, true)
        assert.equal(Object.hasOwn(target, "password"), false)

        const objectId = "rust-personal-device"
        const uploaded = await requestPersonalService(
            personalService.port,
            managementToken,
            "POST",
            `/v1/local-saves/${sourceSlotId}/sync/upload`,
            { target_id: target.id, object_id: objectId },
            200,
        )
        assert.equal(uploaded.uploaded, true)
        assert.equal(uploaded.etag.length, 64)
        verifyStoredCiphertext(accessStore.databasePath, objectId, sourceExport.data)

        const downloaded = await requestPersonalService(
            personalService.port,
            managementToken,
            "POST",
            "/v1/local-saves/sync/download",
            {
                target_id: target.id,
                object_id: objectId,
                name: "Downloaded from Node management server",
            },
            201,
        )
        assert.equal(downloaded.downloaded, true)
        assert.notEqual(downloaded.slot.id, sourceSlotId)
        const downloadedExport = await requestPersonalService(
            personalService.port,
            managementToken,
            "GET",
            `/v1/local-saves/${downloaded.slot.id}/export`,
            undefined,
            200,
        )
        assert.deepEqual(downloadedExport.data, sourceExport.data)

        const finalState = await requestPersonalService(
            personalService.port,
            managementToken,
            "GET",
            "/v1/local-saves",
            undefined,
            200,
        )
        assert.equal(finalState.slots.length, 2)
        assert.equal(finalState.devices[0].active_slot_id, sourceSlotId)
    } finally {
        await stopChildProcess(personalService.process, "Personal service probe")
    }
}

async function requestPersonalService(port, token, method, requestPath, payload, expectedStatus) {
    const response = await requestJson(`http://127.0.0.1:${port}`, {
        method,
        path: requestPath,
        token,
        payload,
        expectedStatus,
    })
    return response.body
}

function verifyStoredCiphertext(databasePath, objectId, sourceData) {
    const database = new Database(databasePath, { readonly: true, fileMustExist: true })
    let row
    try {
        row = database
            .prepare("SELECT envelope_json FROM management_encrypted_saves WHERE object_id = ?")
            .get(objectId)
    } finally {
        database.close()
    }
    assert.notEqual(row, undefined)
    assert.equal(typeof row.envelope_json, "string")
    const envelope = JSON.parse(row.envelope_json)
    assert.deepEqual(Object.keys(envelope).sort(), [
        "algorithm",
        "ciphertext",
        "format",
        "keyId",
        "nonce",
        "version",
    ])
    assert.equal(envelope.format, "starpoint-encrypted-save")
    assert.equal(envelope.algorithm, "AES-256-GCM")
    assert.equal(typeof envelope.ciphertext, "string")
    assert.ok(envelope.ciphertext.length > 32)
    const plaintext = JSON.stringify(sourceData)
    assert.notEqual(row.envelope_json, plaintext)
    assert.doesNotMatch(row.envelope_json, /user_info|viewer_id|account_id/)
}

module.exports = { verifyPersonalServiceSaveSync }
// //// /通过真实 Node 和 Rust 进程同步密文存档 ////
