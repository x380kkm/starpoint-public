// audience: internal
// # test-cn-http-metadata-observer
// 此测试验证观察器关闭, 隐私字段隔离和日志故障隔离.

import assert from "node:assert/strict"
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from "node:fs"
import os from "node:os"
import path from "node:path"
import { createRequire } from "node:module"
import Fastify from "fastify"

const require = createRequire(import.meta.url)
const { installCnHttpMetadataObserver } = require("../../out/control/cnHttpMetadataObserver.js")

// //// 验证观察器关闭, 隐私字段隔离和日志故障隔离 [@x380kkm 2026-08-04] ////
const temporaryDirectory = mkdtempSync(path.join(os.tmpdir(), "starpoint-cn-http-metadata-"))
const outputPath = path.join(temporaryDirectory, "http-metadata.jsonl")
const server = Fastify({ logger: false })

try {
    const disabledOutputPath = path.join(temporaryDirectory, "disabled.jsonl")
    const disabledServer = Fastify({ logger: false })
    installCnHttpMetadataObserver(disabledServer, undefined)
    disabledServer.post("/api/index.php/load", async (_, reply) => reply.status(200).send())
    const disabledResponse = await disabledServer.inject({ method: "POST", url: "/api/index.php/load" })
    assert.equal(disabledResponse.statusCode, 200)
    assert.equal(existsSync(disabledOutputPath), false)
    await disabledServer.close()

    installCnHttpMetadataObserver(server, outputPath)
    server.post("/api/index.php/single_battle_quest/start", async (_, reply) => {
        reply.header("content-type", "application/x-msgpack; charset=binary")
        return reply.status(200).send(Buffer.from([0x80]))
    })
    server.post("/api/index.php/single_battle_quest/finish", async (_, reply) => {
        reply.header("content-type", "application/x-msgpack")
        return reply.status(409).send(Buffer.from([0x80]))
    })
    server.post("/outside", async (_, reply) => reply.status(204).send())

    await server.inject({
        method: "POST",
        url: "/api/index.php/single_battle_quest/start?access_token=secret-query",
        headers: { authorization: "private-token" },
        payload: { viewer_id: 123, token: "private-token" },
    })
    await server.inject({
        method: "POST",
        url: "/api/index.php/single_battle_quest/finish",
        payload: { viewer_id: 123 },
    })
    await server.inject({
        method: "POST",
        url: "/outside?access_token=outside-secret",
        payload: { token: "outside-secret" },
    })
    await server.inject({
        method: "POST",
        url: "/api/index.php/unknown/private-viewer-456?access_token=unknown-secret",
    })
    await server.close()

    const rawLog = readFileSync(outputPath, "utf8")
    const records = rawLog.trim().split("\n").map((line) => JSON.parse(line))
    assert.deepEqual(records.map(({ observedAtUtc, ...record }) => record), [
        {
            method: "POST",
            path: "/api/index.php/single_battle_quest/start",
            status: 200,
            contentType: "application/x-msgpack",
        },
        {
            method: "POST",
            path: "/api/index.php/single_battle_quest/finish",
            status: 409,
            contentType: "application/x-msgpack",
        },
    ])
    for (const record of records) assert.match(record.observedAtUtc, /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/)
    assert.doesNotMatch(
        rawLog,
        /secret-query|private-token|viewer_id|authorization|outside-secret|unknown-secret|private-viewer|123|456/,
    )

    const failureDirectory = path.join(temporaryDirectory, "runtime-failure")
    const failureOutputPath = path.join(failureDirectory, "http-metadata.jsonl")
    mkdirSync(failureDirectory)
    const failureServer = Fastify({ logger: false })
    installCnHttpMetadataObserver(failureServer, failureOutputPath)
    failureServer.post("/api/index.php/load", async (_, reply) => reply.status(200).send())
    rmSync(failureDirectory, { recursive: true, force: true })
    const firstFailureResponse = await failureServer.inject({ method: "POST", url: "/api/index.php/load" })
    const disabledAfterFailureResponse = await failureServer.inject({ method: "POST", url: "/api/index.php/load" })
    assert.equal(firstFailureResponse.statusCode, 200)
    assert.equal(disabledAfterFailureResponse.statusCode, 200)
    await failureServer.close()

    const initializationFailureServer = Fastify({ logger: false })
    installCnHttpMetadataObserver(
        initializationFailureServer,
        path.join(temporaryDirectory, "missing-parent", "http-metadata.jsonl"),
    )
    initializationFailureServer.post("/api/index.php/load", async (_, reply) => reply.status(200).send())
    const initializationFailureResponse = await initializationFailureServer.inject({
        method: "POST",
        url: "/api/index.php/load",
    })
    assert.equal(initializationFailureResponse.statusCode, 200)
    await initializationFailureServer.close()

    const throwingLoggerServer = {
        log: {
            warn() {
                throw new Error("logger failure")
            },
        },
        addHook() {
            throw new Error("observer hook must not be installed after initialization failure")
        },
    }
    assert.doesNotThrow(() => {
        installCnHttpMetadataObserver(
            throwingLoggerServer,
            path.join(temporaryDirectory, "missing-logger-parent", "http-metadata.jsonl"),
        )
    })

    console.log("CN HTTP 元数据观察器测试通过.")
} finally {
    await server.close().catch(() => {})
    rmSync(temporaryDirectory, { recursive: true, force: true })
}
// //// /验证观察器关闭, 隐私字段隔离和日志故障隔离 ////
