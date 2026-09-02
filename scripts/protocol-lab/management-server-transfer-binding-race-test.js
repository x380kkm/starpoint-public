// audience: internal
// # management-server-transfer-binding-race-test
//
// 该脚本在目标 PUT 暂停期间改写来源槽, 验证同步不会错误报告成功.
// 响应断流会进入持久化退避, local_wins 会刷新 open 冲突的双侧 ETag.

const assert = require("node:assert/strict")
const http = require("node:http")
const Database = require("better-sqlite3")
const {
    createBranchPackage,
} = require("./management-server-transfer-binding-helpers")

const targetInstanceId = "f".repeat(32)
const targetPlayerId = 1
const targetShellId = "race-target-shell"
const targetToken = `spt_slot_${"r".repeat(43)}`

function readJsonRequest(request) {
    return new Promise((resolve, reject) => {
        const chunks = []
        let totalBytes = 0
        request.on("data", (chunk) => {
            totalBytes += chunk.length
            if (totalBytes > 8 * 1024 * 1024) {
                reject(new Error("Race target request is too large."))
                request.destroy()
                return
            }
            chunks.push(chunk)
        })
        request.once("end", () => {
            try {
                resolve(JSON.parse(Buffer.concat(chunks).toString("utf8")))
            } catch (error) {
                reject(error)
            }
        })
        request.once("error", reject)
    })
}

function setIdentityHeaders(response, etag) {
    response.setHeader("content-type", "application/json; charset=utf-8")
    response.setHeader("etag", `"${etag}"`)
    response.setHeader("x-starpoint-instance-id", targetInstanceId)
    response.setHeader("x-starpoint-shell-id", targetShellId)
    response.setHeader("x-starpoint-slot-id", String(targetPlayerId))
}

// //// 启动可暂停一次 PUT 的目标槽服务 [@x380kkm 2026-08-04] ////
async function startDelayedTransferTarget(initialPackage) {
    let portablePackage = initialPackage
    let pendingDelay = null
    let nextPutResponseEtag = null
    let breakNextGetBody = false
    const server = http.createServer((request, response) => {
        void (async () => {
            if (
                request.url !== `/transfer/v1/slots/${targetPlayerId}`
                || request.headers.authorization !== `Bearer ${targetToken}`
            ) {
                response.writeHead(401).end(JSON.stringify({ error: "unauthorized" }))
                return
            }
            if (request.method === "GET") {
                setIdentityHeaders(response, portablePackage.payloadSha256)
                const serialized = JSON.stringify(portablePackage)
                if (breakNextGetBody) {
                    breakNextGetBody = false
                    response.setHeader("content-length", Buffer.byteLength(serialized))
                    response.flushHeaders()
                    response.write(serialized.slice(0, Math.max(1, Math.floor(serialized.length / 2))))
                    setImmediate(() => response.destroy())
                    return
                }
                response.end(serialized)
                return
            }
            if (request.method !== "PUT") {
                response.writeHead(405).end()
                return
            }
            if (request.headers["if-match"] !== `"${portablePackage.payloadSha256}"`) {
                response.writeHead(409).end(JSON.stringify({ error: "revision_conflict" }))
                return
            }
            const uploaded = await readJsonRequest(request)
            const delay = pendingDelay
            pendingDelay = null
            if (delay !== null) {
                delay.markStarted()
                await delay.releasePromise
            }
            portablePackage = uploaded
            const responseEtag = nextPutResponseEtag ?? portablePackage.payloadSha256
            nextPutResponseEtag = null
            setIdentityHeaders(response, responseEtag)
            response.end(JSON.stringify({ etag: responseEtag }))
        })().catch((error) => {
            if (!response.headersSent) response.writeHead(500)
            response.end(JSON.stringify({ error: error.message }))
        })
    })
    await new Promise((resolve, reject) => {
        server.once("error", reject)
        server.listen(0, "127.0.0.1", resolve)
    })
    const address = server.address()
    assert.notEqual(address, null)
    assert.equal(typeof address, "object")
    return {
        baseUrl: `http://127.0.0.1:${address.port}/transfer/v1`,
        delayNextPut() {
            assert.equal(pendingDelay, null)
            let markStarted
            let release
            const started = new Promise((resolve) => {
                markStarted = resolve
            })
            const releasePromise = new Promise((resolve) => {
                release = resolve
            })
            pendingDelay = { markStarted, release, releasePromise }
            return { started, release }
        },
        forgeNextPutEtag(etag) {
            nextPutResponseEtag = etag
        },
        breakNextGetResponse() {
            breakNextGetBody = true
        },
        getPackage: () => portablePackage,
        async close() {
            pendingDelay?.release()
            pendingDelay = null
            await new Promise((resolve, reject) => {
                server.close((error) => error === undefined ? resolve() : reject(error))
                server.closeAllConnections()
            })
        },
    }
}
// //// /启动可暂停一次 PUT 的目标槽服务 ////

function bindingPath(sourcePlayerId, bindingId) {
    return `/manage/api/saves/${sourcePlayerId}/transfer-bindings/${bindingId}`
}

// //// 验证上传和 local_wins 的来源并发保护 [@x380kkm 2026-08-04] ////
async function verifyServerTransferBindingRaces(input) {
    const target = await startDelayedTransferTarget(input.initialPackage)
    try {
        const sourceSlot = await input.importNodeSlot(input.sourceInstance, input.initialPackage)
        const sourceTransfer = await input.issueNodeTransferAccess(
            input.sourceInstance,
            sourceSlot.playerId,
            "Server binding race source",
        )
        const sourceInitial = await input.exportTransferSlot(sourceTransfer)
        const created = await input.requestInstance(input.sourceInstance, {
            method: "POST",
            path: `/manage/api/saves/${sourceSlot.playerId}/transfer-bindings`,
            payload: {
                targetBaseUrl: target.baseUrl,
                targetInstanceId,
                targetPlayerId,
                targetToken,
                uploadMode: "manual",
                pullMode: "manual",
                conflictPolicy: "ask",
                intervalSeconds: 900,
                enabled: true,
            },
            expectedStatus: 201,
        })
        const path = bindingPath(sourceSlot.playerId, created.body.bindingId)
        const baseline = await input.requestInstance(input.sourceInstance, {
            method: "POST",
            path: `${path}/sync`,
            payload: { direction: "auto" },
            expectedStatus: 200,
        })
        assert.equal(baseline.body.action, "unchanged")

        const failureStartedAt = Date.now()
        target.breakNextGetResponse()
        const brokenResponse = await input.requestInstance(input.sourceInstance, {
            method: "POST",
            path: `${path}/sync`,
            payload: { direction: "auto" },
            expectedStatus: 503,
        })
        assert.equal(brokenResponse.body.error, "transfer_target_unavailable")
        const failedBinding = await input.requestInstance(input.sourceInstance, {
            method: "GET",
            path,
            expectedStatus: 200,
        })
        assert.equal(failedBinding.body.lastError, "transfer_target_unavailable")
        assert.equal(Date.parse(failedBinding.body.nextRunAt) >= failureStartedAt + 4_000, true)
        const recoveredBodyRead = await input.requestInstance(input.sourceInstance, {
            method: "POST",
            path: `${path}/sync`,
            payload: { direction: "auto" },
            expectedStatus: 200,
        })
        assert.equal(recoveredBodyRead.body.action, "unchanged")

        const sourceBranchPackage = createBranchPackage(
            sourceInitial.body,
            "Server race source branch",
        )
        const sourceBranch = await input.overwriteTransferSlot(
            sourceTransfer,
            sourceInitial.etag,
            sourceBranchPackage,
        )
        const uploadDelay = target.delayNextPut()
        const pendingSync = input.requestInstance(input.sourceInstance, {
            method: "POST",
            path: `${path}/sync`,
            payload: { direction: "upload" },
            expectedStatus: 409,
        })
        await uploadDelay.started
        const sourceDatabase = new Database(input.sourceInstance.databasePath)
        sourceDatabase.pragma("busy_timeout = 5000")
        assert.throws(
            () => sourceDatabase.prepare("DELETE FROM players WHERE id = ?").run(sourceSlot.playerId),
            /server transfer binding blocks player deletion/,
        )
        assert.notEqual(
            sourceDatabase.prepare("SELECT 1 FROM players WHERE id = ?").get(sourceSlot.playerId),
            undefined,
        )
        sourceDatabase.close()
        const lateSourcePackage = createBranchPackage(
            sourceBranchPackage,
            "Server race late source",
        )
        const lateSource = await input.overwriteTransferSlot(
            sourceTransfer,
            sourceBranch.etag,
            lateSourcePackage,
        )
        uploadDelay.release()
        const conflicted = await pendingSync
        assert.equal(conflicted.body.error, "transfer_conflict")
        assert.equal(conflicted.body.conflict.sourceEtag, lateSource.body.etag)
        assert.equal(conflicted.body.conflict.targetEtag, sourceBranch.body.etag)
        assert.equal(target.getPackage().payloadSha256, sourceBranchPackage.payloadSha256)

        const preResolutionPackage = createBranchPackage(
            lateSourcePackage,
            "Server race pre-resolution source",
        )
        const preResolutionSource = await input.overwriteTransferSlot(
            sourceTransfer,
            lateSource.etag,
            preResolutionPackage,
        )
        const staleResolution = await input.requestInstance(input.sourceInstance, {
            method: "POST",
            path: `${path}/conflicts/${conflicted.body.conflict.conflictId}/resolve`,
            payload: { resolution: "local_wins" },
            expectedStatus: 409,
        })
        assert.equal(staleResolution.body.error, "conflict_changed")
        const refreshedConflict = await input.requestInstance(input.sourceInstance, {
            method: "GET",
            path: `${path}/conflicts`,
            expectedStatus: 200,
        })
        assert.equal(refreshedConflict.body[0].sourceEtag, preResolutionSource.body.etag)
        assert.equal(refreshedConflict.body[0].targetEtag, sourceBranch.body.etag)

        const resolutionDelay = target.delayNextPut()
        const pendingResolution = input.requestInstance(input.sourceInstance, {
            method: "POST",
            path: `${path}/conflicts/${conflicted.body.conflict.conflictId}/resolve`,
            payload: { resolution: "local_wins" },
            expectedStatus: 409,
        })
        await resolutionDelay.started
        const newestSourcePackage = createBranchPackage(
            preResolutionPackage,
            "Server race newest source",
        )
        const newestSource = await input.overwriteTransferSlot(
            sourceTransfer,
            preResolutionSource.etag,
            newestSourcePackage,
        )
        resolutionDelay.release()
        const rejectedResolution = await pendingResolution
        assert.equal(rejectedResolution.body.error, "conflict_changed")
        const conflicts = await input.requestInstance(input.sourceInstance, {
            method: "GET",
            path: `${path}/conflicts`,
            expectedStatus: 200,
        })
        assert.equal(conflicts.body[0].status, "open")
        assert.equal(conflicts.body[0].sourceEtag, newestSource.body.etag)
        assert.equal(conflicts.body[0].targetEtag, preResolutionSource.body.etag)
        assert.equal(target.getPackage().payloadSha256, preResolutionPackage.payloadSha256)

        const resolved = await input.requestInstance(input.sourceInstance, {
            method: "POST",
            path: `${path}/conflicts/${conflicted.body.conflict.conflictId}/resolve`,
            payload: { resolution: "local_wins" },
            expectedStatus: 200,
        })
        assert.equal(resolved.body.conflict.status, "resolved_local_wins")
        assert.equal(target.getPackage().payloadSha256, newestSourcePackage.payloadSha256)

        const sourceAfterResolve = await input.exportTransferSlot(sourceTransfer)
        const invalidResponsePackage = createBranchPackage(
            sourceAfterResolve.body,
            "Server race invalid response",
        )
        await input.overwriteTransferSlot(
            sourceTransfer,
            sourceAfterResolve.etag,
            invalidResponsePackage,
        )
        target.forgeNextPutEtag("0".repeat(64))
        const invalidResponse = await input.requestInstance(input.sourceInstance, {
            method: "POST",
            path: `${path}/sync`,
            payload: { direction: "upload" },
            expectedStatus: 502,
        })
        assert.equal(invalidResponse.body.error, "transfer_target_invalid_response")
        assert.equal(target.getPackage().payloadSha256, invalidResponsePackage.payloadSha256)
        const recovered = await input.requestInstance(input.sourceInstance, {
            method: "POST",
            path: `${path}/sync`,
            payload: { direction: "auto" },
            expectedStatus: 200,
        })
        assert.equal(recovered.body.action, "unchanged")

        const removed = await input.requestInstance(input.sourceInstance, {
            method: "DELETE",
            path,
            expectedStatus: 200,
        })
        assert.equal(removed.body.deleted, true)
    } finally {
        await target.close()
    }
}
// //// /验证上传和 local_wins 的来源并发保护 ////

module.exports = { verifyServerTransferBindingRaces }
