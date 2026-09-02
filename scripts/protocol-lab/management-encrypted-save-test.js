// audience: internal
// # management-encrypted-save-test
// 此模块验证管理服务器按用户保存真实 AES-256-GCM 密文且无法读取存档明文.

const assert = require("node:assert/strict")
const { createCipheriv, createDecipheriv, randomBytes } = require("node:crypto")
const Database = require("better-sqlite3")

// //// 创建并解密测试用加密存档封装 [@x380kkm 2026-07-23] ////
function createEnvelope(plaintext) {
    const key = randomBytes(32)
    const nonce = randomBytes(12)
    const cipher = createCipheriv("aes-256-gcm", key, nonce)
    const ciphertext = Buffer.concat([
        cipher.update(plaintext, "utf8"),
        cipher.final(),
        cipher.getAuthTag(),
    ])
    return {
        key,
        envelope: {
            format: "starpoint-encrypted-save",
            version: 1,
            algorithm: "AES-256-GCM",
            keyId: randomBytes(12).toString("base64url"),
            nonce: nonce.toString("base64url"),
            ciphertext: ciphertext.toString("base64url"),
        },
    }
}

function decryptEnvelope(envelope, key) {
    const encrypted = Buffer.from(envelope.ciphertext, "base64url")
    const ciphertext = encrypted.subarray(0, -16)
    const authenticationTag = encrypted.subarray(-16)
    const decipher = createDecipheriv("aes-256-gcm", key, Buffer.from(envelope.nonce, "base64url"))
    decipher.setAuthTag(authenticationTag)
    return Buffer.concat([decipher.update(ciphertext), decipher.final()]).toString("utf8")
}
// //// /创建并解密测试用加密存档封装 ////

// //// 验证用户密文存档上传, 隔离, 下载和删除 [@x380kkm 2026-07-23] ////
async function verifyEncryptedSaveStorage(app, accessStore, adminCookie, playerCookie, adminToken) {
    const plaintext = "private-save-marker: Imported Player, 12345 mana"
    const { envelope } = createEnvelope(plaintext)
    const objectId = "primary-device"

    const invalidEnvelope = await app.inject({
        method: "PUT",
        url: `/manage/api/encrypted-saves/${objectId}`,
        headers: { cookie: playerCookie, "if-none-match": "*" },
        payload: { format: "starpoint-encrypted-save", data: plaintext },
    })
    assert.equal(invalidEnvelope.statusCode, 400)

    const invalidObjectId = await app.inject({
        method: "PUT",
        url: "/manage/api/encrypted-saves/bad.id",
        headers: { cookie: playerCookie },
        payload: envelope,
    })
    assert.equal(invalidObjectId.statusCode, 400)

    const bearerUpload = await app.inject({
        method: "PUT",
        url: `/manage/api/encrypted-saves/${objectId}`,
        headers: { authorization: `Bearer ${adminToken}` },
        payload: envelope,
    })
    assert.equal(bearerUpload.statusCode, 403)

    const blindUpload = await app.inject({
        method: "PUT",
        url: `/manage/api/encrypted-saves/${objectId}`,
        headers: { cookie: playerCookie },
        payload: envelope,
    })
    assert.equal(blindUpload.statusCode, 428)

    const uploaded = await app.inject({
        method: "PUT",
        url: `/manage/api/encrypted-saves/${objectId}`,
        headers: { cookie: playerCookie, "if-none-match": "*" },
        payload: envelope,
    })
    assert.equal(uploaded.statusCode, 201)
    assert.equal(uploaded.json().objectId, objectId)
    assert.equal(uploaded.json().sha256.length, 64)
    assert.equal(uploaded.headers.etag, `"${uploaded.json().sha256}"`)

    const staleUpload = await app.inject({
        method: "PUT",
        url: `/manage/api/encrypted-saves/${objectId}`,
        headers: { cookie: playerCookie, "if-match": `"${"0".repeat(64)}"` },
        payload: envelope,
    })
    assert.equal(staleUpload.statusCode, 412)

    const replacementPlaintext = `${plaintext}; newer snapshot`
    const replacement = createEnvelope(replacementPlaintext)
    const replaced = await app.inject({
        method: "PUT",
        url: `/manage/api/encrypted-saves/${objectId}`,
        headers: { cookie: playerCookie, "if-match": uploaded.headers.etag },
        payload: replacement.envelope,
    })
    assert.equal(replaced.statusCode, 200)
    assert.notEqual(replaced.headers.etag, uploaded.headers.etag)

    const listed = await app.inject({
        method: "GET",
        url: "/manage/api/encrypted-saves",
        headers: { cookie: playerCookie },
    })
    assert.deepEqual(listed.json().saves.map((save) => save.objectId), [objectId])

    const isolated = await app.inject({
        method: "GET",
        url: `/manage/api/encrypted-saves/${objectId}`,
        headers: { cookie: adminCookie },
    })
    assert.equal(isolated.statusCode, 404)

    const downloaded = await app.inject({
        method: "GET",
        url: `/manage/api/encrypted-saves/${objectId}`,
        headers: { cookie: playerCookie },
    })
    assert.equal(downloaded.statusCode, 200)
    assert.match(downloaded.headers["content-disposition"], /primary-device\.starpoint-save\.json/)
    assert.match(downloaded.headers["content-type"], /^application\/vnd\.starpoint\.encrypted-save\+json/)
    assert.equal(downloaded.headers.etag, replaced.headers.etag)
    assert.deepEqual(downloaded.json(), replacement.envelope)
    assert.equal(decryptEnvelope(downloaded.json(), replacement.key), replacementPlaintext)

    const database = new Database(accessStore.databasePath, { readonly: true })
    const stored = database.prepare(`
        SELECT envelope_json FROM management_encrypted_saves WHERE object_id = ?
    `).get(objectId)
    database.close()
    assert.equal(typeof stored.envelope_json, "string")
    assert.doesNotMatch(stored.envelope_json, /private-save-marker|Imported Player|12345 mana/)

    const deleted = await app.inject({
        method: "DELETE",
        url: `/manage/api/encrypted-saves/${objectId}`,
        headers: { cookie: playerCookie },
    })
    assert.deepEqual(deleted.json(), { deleted: true, objectId })
    const missing = await app.inject({
        method: "GET",
        url: `/manage/api/encrypted-saves/${objectId}`,
        headers: { cookie: playerCookie },
    })
    assert.equal(missing.statusCode, 404)
}
// //// /验证用户密文存档上传, 隔离, 下载和删除 ////

module.exports = { verifyEncryptedSaveStorage }
