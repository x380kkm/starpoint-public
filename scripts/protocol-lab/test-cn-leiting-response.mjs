// audience: internal
// # test-cn-leiting-response
// 此测试只验证 CN 雷霆登录响应的默认头、最小头和传输头实验模式.

import assert from "node:assert/strict"
import { spawn } from "node:child_process"
import { createServer } from "node:net"
import os from "node:os"
import path from "node:path"
import { mkdtempSync, rmSync } from "node:fs"
import { fileURLToPath } from "node:url"
import { pack, unpack } from "msgpackr"

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..")
const startupScript = path.join(repositoryRoot, "out", "start.js")

//// 预留本地 HTTP 端口 [@x380kkm 2026-08-03] ////
function reservePort() {
    return new Promise((resolve, reject) => {
        const server = createServer()
        server.once("error", reject)
        server.listen(0, "127.0.0.1", () => {
            const address = server.address()
            const port = typeof address === "object" && address !== null ? address.port : null
            server.close((error) => error ? reject(error) : resolve(port))
        })
    })
}
//// /预留本地 HTTP 端口 ////

//// 等待 CN 服务健康 [@x380kkm 2026-08-03] ////
async function waitForHealth(child, baseUrl, readErrors) {
    const deadline = Date.now() + 10000
    while (Date.now() < deadline) {
        if (child.exitCode !== null) throw new Error(`CN 服务启动失败.\n${readErrors()}`)
        try {
            const response = await fetch(`${baseUrl}/healthz`)
            if (response.status === 200) {
                const body = await response.json()
                assert.equal(body.status, "ok")
                return
            }
        } catch { }
        await new Promise((resolve) => setTimeout(resolve, 100))
    }
    throw new Error(`CN 服务未在期限内健康.\n${readErrors()}`)
}
//// /等待 CN 服务健康 ////

//// 停止 CN 服务子进程 [@x380kkm 2026-08-03] ////
async function stopChild(child) {
    if (child.exitCode !== null) return
    const closed = new Promise((resolve) => child.once("close", resolve))
    child.kill()
    await Promise.race([closed, new Promise((resolve) => setTimeout(resolve, 5000))])
    if (child.exitCode === null) {
        child.kill("SIGKILL")
        await closed
    }
}
//// /停止 CN 服务子进程 ////

//// 请求 CN 雷霆登录响应 [@x380kkm 2026-08-03] ////
async function requestLeitingLogin(baseUrl) {
    const response = await fetch(`${baseUrl}/api/index.php/channels/channel_leiting/leiting_login`, {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded" },
        body: Buffer.from(pack({
            userId: "cn-response-probe",
            game: "wf",
            channelNo: "1001",
            token: "probe-token",
        })).toString("base64"),
    })
    assert.equal(response.status, 200)
    assert.equal(response.headers.get("content-type"), "application/x-msgpack")
    return {
        headers: response.headers,
        body: unpack(Buffer.from(await response.text(), "base64")),
    }
}
//// /请求 CN 雷霆登录响应 ////

//// 在隔离目录启动 CN 响应服务 [@x380kkm 2026-08-03] ////
async function runResponseMode(headerMode) {
    const root = mkdtempSync(path.join(os.tmpdir(), "starpoint-cn-leiting-response-"))
    const port = await reservePort()
    const errors = []
    const child = spawn(process.execPath, [startupScript], {
        cwd: root,
        env: {
            ...process.env,
            LISTEN_HOST: "127.0.0.1",
            LISTEN_PORT: String(port),
            SESSION_HOST: "127.0.0.1",
            SESSION_PUBLIC_HOST: "127.0.0.1",
            SESSION_PORT: String(await reservePort()),
            CDN_DIR: path.join(root, ".cdn"),
            DATABASE_PATH: path.join(root, "wdfp_data.db"),
            MANAGEMENT_STATE_FILE: path.join(root, ".management", "state.json"),
            MANAGEMENT_ADMIN_TOKEN: "cn-response-test-token",
            CN_MSGPACK_RESPONSE_ENCODING: "",
            CN_LEITING_LOGIN_HEADERS: headerMode ?? "",
        },
        stdio: ["ignore", "ignore", "pipe"],
    })
    child.stderr.on("data", (chunk) => errors.push(chunk.toString()))
    try {
        const baseUrl = `http://127.0.0.1:${port}`
        await waitForHealth(child, baseUrl, () => errors.join(""))
        return await requestLeitingLogin(baseUrl)
    } finally {
        await stopChild(child)
        rmSync(root, { recursive: true, force: true })
    }
}
//// /在隔离目录启动 CN 响应服务 ////

//// 验证默认和最小响应头 [@x380kkm 2026-08-03] ////
const defaultResponse = await runResponseMode(undefined)
assert.equal(defaultResponse.body.data_headers.result_code, 1)
assert.equal(typeof defaultResponse.body.data_headers.servertime, "number")
assert.equal(typeof defaultResponse.body.data_headers.viewer_id, "number")
assert.equal(defaultResponse.body.data.status, "success")

const minimalResponse = await runResponseMode("minimal")
assert.deepEqual(minimalResponse.body.data_headers, { result_code: 1 })
assert.equal(minimalResponse.body.data.status, "success")

const transportResponse = await runResponseMode("transport")
assert.equal(transportResponse.body.data_headers.result_code, 1)
assert.equal(transportResponse.headers.get("connection"), "keep-alive")
assert.equal(transportResponse.headers.get("x-result-code"), "1")
assert.equal(transportResponse.headers.get("param"), "probe-param")
assert.equal(transportResponse.body.data.status, "success")

const minimalTransportResponse = await runResponseMode("minimal-transport")
assert.deepEqual(minimalTransportResponse.body.data_headers, { result_code: 1 })
assert.equal(minimalTransportResponse.headers.get("connection"), "keep-alive")
assert.equal(minimalTransportResponse.headers.get("x-result-code"), "1")
assert.equal(minimalTransportResponse.headers.get("param"), "probe-param")
assert.equal(minimalTransportResponse.body.data.status, "success")
console.log("test-cn-leiting-response: PASS")
//// /验证默认和最小响应头 ////
