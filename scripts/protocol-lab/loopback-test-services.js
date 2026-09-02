// audience: internal
// # loopback-test-services
//
// 该模块启动 Node 完整服务器和 Rust 个人服务的短时 loopback 进程.
// 调用方负责在 finally 中停止每个已启动进程.

const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")
const { spawn } = require("node:child_process")
const { createServer } = require("node:net")
const { pack, unpack } = require("msgpackr")

const repositoryRoot = path.resolve(__dirname, "..", "..")
const startupScript = path.join(repositoryRoot, "out", "start.js")
const requestTimeoutMs = 10_000
const processStartTimeoutMs = 30_000
const processStopTimeoutMs = 5_000

// //// 启动隔离的 Node 和 Rust 测试服务 [@x380kkm 2026-08-03] ////
async function startNodeServer(root, managementToken) {
    assert.ok(fs.existsSync(startupScript), "Run npm run build before starting a Node test server.")
    fs.mkdirSync(root, { recursive: true })
    const cdnDirectory = path.join(root, ".cdn")
    fs.mkdirSync(cdnDirectory, { recursive: true })
    const port = await reservePort()
    const sessionPort = await reservePort()
    const databasePath = path.join(root, "starpoint-cn.sqlite")
    const child = spawn(process.execPath, [startupScript], {
        cwd: root,
        env: {
            ...process.env,
            LISTEN_HOST: "127.0.0.1",
            LISTEN_PORT: String(port),
            SESSION_HOST: "127.0.0.1",
            SESSION_PUBLIC_HOST: "127.0.0.1",
            SESSION_PORT: String(sessionPort),
            CDN_DIR: cdnDirectory,
            DATABASE_PATH: databasePath,
            MANAGEMENT_STATE_FILE: path.join(root, ".management", "state.json"),
            MANAGEMENT_ACCESS_DATABASE_PATH: path.join(root, ".management", "control.db"),
            MANAGEMENT_ADMIN_TOKEN: managementToken,
            CN_MSGPACK_RESPONSE_ENCODING: "",
            ENABLE_LEGACY_WEB_ADMIN: "0",
        },
        stdio: ["ignore", "ignore", "pipe"],
        windowsHide: true,
    })
    let stderr = ""
    child.stderr.setEncoding("utf8")
    child.stderr.on("data", (chunk) => {
        stderr = `${stderr}${chunk}`.slice(-4096)
    })
    const baseUrl = `http://127.0.0.1:${port}`
    try {
        await waitForHealth(child, baseUrl, () => stderr)
        return { process: child, baseUrl, databasePath, managementToken }
    } catch (error) {
        await stopChildProcess(child, "Node test server")
        throw error
    }
}

async function startPersonalService(root) {
    const executableName = process.platform === "win32" ? "personal-service-probe.exe" : "personal-service-probe"
    const executablePath = process.env.PERSONAL_SERVICE_PROBE === undefined
        ? path.join(repositoryRoot, "core", "personal-service", "target", "debug", executableName)
        : path.resolve(process.env.PERSONAL_SERVICE_PROBE)
    assert.ok(fs.existsSync(executablePath), `Personal service probe is missing: ${executablePath}`)
    fs.mkdirSync(root, { recursive: true })
    const child = spawn(executablePath, [root, "--report-management-token"], {
        stdio: ["ignore", "pipe", "pipe"],
        windowsHide: true,
    })
    let stderr = ""
    child.stderr.setEncoding("utf8")
    child.stderr.on("data", (chunk) => {
        stderr = `${stderr}${chunk}`.slice(-4096)
    })
    try {
        const startup = await readProbeStartup(child, () => stderr)
        return {
            process: child,
            baseUrl: `http://127.0.0.1:${startup.port}`,
            ...startup,
        }
    } catch (error) {
        await stopChildProcess(child, "Personal service probe")
        throw error
    }
}
// //// /启动隔离的 Node 和 Rust 测试服务 ////

// //// 发送管理请求和 CN 客户端请求 [@x380kkm 2026-08-03] ////
async function requestJson(baseUrl, request) {
    const headers = { ...request.headers }
    if (request.token !== undefined) headers.authorization = `Bearer ${request.token}`
    if (request.payload !== undefined) headers["content-type"] = "application/json"
    const response = await fetch(`${baseUrl}${request.path}`, {
        method: request.method,
        headers,
        body: request.payload === undefined ? undefined : JSON.stringify(request.payload),
        signal: AbortSignal.timeout(requestTimeoutMs),
    })
    const text = await response.text()
    assert.equal(
        response.status,
        request.expectedStatus,
        `${request.method} ${request.path} returned ${response.status}.`,
    )
    return {
        body: text === "" ? null : JSON.parse(text),
        etag: response.headers.get("etag"),
        instanceId: response.headers.get("x-starpoint-instance-id"),
        shellId: response.headers.get("x-starpoint-shell-id"),
        slotId: response.headers.get("x-starpoint-slot-id"),
    }
}

async function signupCn(baseUrl, deviceId, udid) {
    const response = await fetch(`${baseUrl}/api/index.php/tool/signup`, {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded", udid },
        body: Buffer.from(pack({
            device_id: deviceId,
            channelNo: "leiting",
            media: "none",
            androidId: "",
            oaid: "",
            mac: "",
            terminInfo: "",
            osVer: "",
            storage_directory_path: "/data/user/0/com.leiting.wf",
        })).toString("base64"),
        signal: AbortSignal.timeout(requestTimeoutMs),
    })
    const text = await response.text()
    assert.equal(response.status, 200, `CN signup returned ${response.status}.`)
    const body = unpack(Buffer.from(text, "base64"))
    assert.equal(body.data_headers.result_code, 1)
    return body
}

async function loadCn(baseUrl, viewerId, deviceId) {
    const response = await fetch(`${baseUrl}/api/index.php/load`, {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded" },
        body: Buffer.from(pack({
            device_id: deviceId,
            device_token: "",
            keychain: viewerId,
            graphics_device_name: "Three instance test",
            platform_os_version: "Android 15",
            storage_directory_path: "/data/user/0/com.leiting.wf",
            viewer_id: viewerId,
        })).toString("base64"),
        signal: AbortSignal.timeout(requestTimeoutMs),
    })
    const text = await response.text()
    assert.equal(response.status, 200, `CN load returned ${response.status}.`)
    return unpack(Buffer.from(text, "base64"))
}
// //// /发送管理请求和 CN 客户端请求 ////

// //// 等待服务状态并停止子进程 [@x380kkm 2026-08-03] ////
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

async function waitForHealth(child, baseUrl, readStderr) {
    const deadline = Date.now() + processStartTimeoutMs
    while (Date.now() < deadline) {
        if (hasChildExited(child)) throw new Error(`Node test server exited early. ${readStderr()}`)
        try {
            const response = await fetch(`${baseUrl}/healthz`, {
                signal: AbortSignal.timeout(requestTimeoutMs),
            })
            if (response.status === 200) return
        } catch { }
        await new Promise((resolve) => setTimeout(resolve, 100))
    }
    throw new Error(`Node test server did not become healthy. ${readStderr()}`)
}

function readProbeStartup(child, readStderr) {
    return new Promise((resolve, reject) => {
        let output = ""
        let settled = false
        const timer = setTimeout(
            () => finish(new Error(`Personal service probe did not start. ${readStderr()}`)),
            processStartTimeoutMs,
        )
        const finish = (error, startup) => {
            if (settled) return
            settled = true
            clearTimeout(timer)
            child.stdout.off("data", onData)
            child.off("error", onError)
            child.off("exit", onExit)
            if (error === undefined) resolve(startup)
            else reject(error)
        }
        const onData = (chunk) => {
            output += chunk
            const lineEnd = output.indexOf("\n")
            if (lineEnd < 0) return
            let startup
            try {
                startup = JSON.parse(output.slice(0, lineEnd))
            } catch {
                finish(new Error("Personal service probe returned invalid startup JSON."))
                return
            }
            if (
                !Number.isInteger(startup.port)
                || startup.port <= 0
                || startup.port > 65535
                || typeof startup.managementToken !== "string"
                || startup.managementToken.length !== 43
            ) {
                finish(new Error("Personal service probe returned invalid startup data."))
                return
            }
            finish(undefined, startup)
        }
        const onError = (error) => finish(error)
        const onExit = (code) => finish(new Error(`Personal service probe exited with ${code}. ${readStderr()}`))
        child.stdout.setEncoding("utf8")
        child.stdout.on("data", onData)
        child.once("error", onError)
        child.once("exit", onExit)
    })
}

async function stopChildProcess(child, label) {
    if (hasChildExited(child)) return
    child.kill()
    if (await waitForExit(child, processStopTimeoutMs)) return
    child.kill("SIGKILL")
    assert.equal(await waitForExit(child, processStopTimeoutMs), true, `${label} did not stop`)
}

function waitForExit(child, timeoutMs) {
    return new Promise((resolve) => {
        if (hasChildExited(child)) {
            resolve(true)
            return
        }
        const timer = setTimeout(() => {
            child.off("exit", onExit)
            resolve(false)
        }, timeoutMs)
        const onExit = () => {
            clearTimeout(timer)
            resolve(true)
        }
        child.once("exit", onExit)
    })
}

function hasChildExited(child) {
    return child.exitCode !== null || child.signalCode !== null
}
// //// /等待服务状态并停止子进程 ////

module.exports = {
    loadCn,
    requestJson,
    signupCn,
    startNodeServer,
    startPersonalService,
    stopChildProcess,
}
