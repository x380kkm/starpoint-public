// audience: internal
// # generate-cn-default-player-fixture
//
// 该脚本从 Node 行为基准生成确定性的 CN 默认玩家数据.
// 默认模式读取本仓库编译输出, --reference-base-url 模式读取已启动的参考服务.

import { createHash } from "node:crypto"
import fs from "node:fs"
import os from "node:os"
import path from "node:path"
import { createRequire } from "node:module"
import { fileURLToPath } from "node:url"
import { pack, unpack } from "msgpackr"

const FIXED_SERVER_TIME = 1_651_514_014
const FIXED_SERVER_DATE = "2022-05-02 16:33:34"

// //// 解析输出路径并规范默认玩家数据 [@x380kkm 2026-07-22] ////
function getOutputPath() {
    const outputIndex = process.argv.indexOf("--output")
    if (outputIndex === -1 || process.argv[outputIndex + 1] === undefined) {
        throw new Error("--output is required")
    }
    return path.resolve(process.argv[outputIndex + 1])
}

function getOption(name) {
    const index = process.argv.indexOf(name)
    return index === -1 ? undefined : process.argv[index + 1]
}

function normalizePlayerData(value, viewerId, fieldName = "") {
    if (Array.isArray(value)) return value.map((entry) => normalizePlayerData(entry, viewerId))
    if (value !== null && typeof value === "object") {
        return Object.fromEntries(
            Object.keys(value)
                .sort((left, right) => left.localeCompare(right, "en"))
                .map((key) => [key, normalizePlayerData(value[key], viewerId, key)]),
        )
    }
    if (typeof value === "number") {
        if (fieldName === "viewer_id" || value === viewerId) return 0
        if (value >= 1_500_000_000 && value <= 2_000_000_000) return FIXED_SERVER_TIME
    }
    if (typeof value === "string" && /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/.test(value)) {
        return FIXED_SERVER_DATE
    }
    return value
}

// //// 从已启动的参考服务读取默认玩家数据 [@x380kkm 2026-08-23] ////
async function loadReferencePlayerData(baseUrl) {
    const post = async (requestPath, body) => {
        const response = await fetch(new URL(requestPath, `${baseUrl.replace(/\/$/, "")}/`), {
            method: "POST",
            headers: { "content-type": "application/x-www-form-urlencoded" },
            body: Buffer.from(pack(body)).toString("base64"),
        })
        if (!response.ok) throw new Error(`reference ${requestPath} returned ${response.status}`)
        return unpack(Buffer.from(await response.text(), "base64"))
    }
    const signup = await post("/api/index.php/tool/signup", { device_id: 184_202_608_230_001 })
    const viewerId = signup?.data_headers?.viewer_id
    if (!Number.isSafeInteger(viewerId) || viewerId <= 0) {
        throw new Error("reference signup did not return a viewer ID")
    }
    const loadBody = { viewer_id: viewerId, keychain: viewerId }
    await post("/api/index.php/load", loadBody)
    const load = await post("/api/index.php/load", loadBody)
    if (load?.data === null || typeof load?.data !== "object" || Array.isArray(load.data)) {
        throw new Error("reference load did not return player data")
    }
    const normalized = normalizePlayerData(load.data, viewerId)
    delete normalized.unfinished_quest_list
    delete normalized.unfinished_multi_quest_list
    normalized.cn_crash_url = ""
    normalized.gacha_info_list = []
    return normalized
}
// //// /从已启动的参考服务读取默认玩家数据 ////

// //// 写入确定性的默认玩家数据 [@x380kkm 2026-08-23] ////
function writePlayerFixture(outputPath, normalized) {
    if (normalized.user_tutorial?.viewer_id !== 0) {
        throw new Error("viewer ID was not normalized")
    }
    if (Object.keys(normalized.user_character_list ?? {}).length === 0) {
        throw new Error("default character was not serialized")
    }
    const serialized = `${JSON.stringify(normalized)}\n`
    fs.mkdirSync(path.dirname(outputPath), { recursive: true })
    fs.writeFileSync(outputPath, serialized, "utf8")
    console.log(JSON.stringify({
        output: outputPath,
        bytes: Buffer.byteLength(serialized),
        sha256: createHash("sha256").update(serialized).digest("hex"),
    }))
}
// //// /写入确定性的默认玩家数据 ////
// //// /解析输出路径并规范默认玩家数据 ////

// //// 从 Node 数据层生成并校验默认玩家数据 [@x380kkm 2026-07-22] ////
async function main() {
    const outputPath = getOutputPath()
    const referenceBaseUrl = getOption("--reference-base-url")
    if (referenceBaseUrl !== undefined) {
        writePlayerFixture(outputPath, await loadReferencePlayerData(referenceBaseUrl))
        return
    }
    const scriptDirectory = path.dirname(fileURLToPath(import.meta.url))
    const repositoryRoot = path.resolve(scriptDirectory, "..", "..")
    const compiledDataPath = path.join(repositoryRoot, "out", "data", "wdfpData.js")
    if (!fs.existsSync(compiledDataPath)) throw new Error("Run npx tsc before generating the fixture.")

    const temporaryRoot = fs.mkdtempSync(path.join(os.tmpdir(), "starpoint-cn-fixture-"))
    process.env.DATABASE_PATH = path.join(temporaryRoot, "wdfp_data.db")
    process.env.MANAGEMENT_STATE_FILE = path.join(temporaryRoot, "management-state.json")
    const require = createRequire(import.meta.url)
    const data = require(path.join(repositoryRoot, "out", "data", "wdfpData.js"))
    const dataIndex = require(path.join(repositoryRoot, "out", "data", "index.js"))
    const serializers = require(path.join(repositoryRoot, "out", "data", "utils.js"))
    const loadRoute = require(path.join(repositoryRoot, "out", "routes", "cn", "load.js"))
    const serverUtils = require(path.join(repositoryRoot, "out", "utils.js"))

    try {
        const account = await data.insertAccount({
            appId: "wf_cn",
            idpAlias: "wf_cn:1:android",
            idpCode: "leiting",
            idpId: "cn:1",
            status: "normal",
        })
        data.insertDefaultPlayerSync(account.id)
        const player = data.getPlayerFromAccountIdSync(account.id)
        if (player === null) throw new Error("default player was not created")
        const now = serverUtils.getServerDate()
        data.dailyResetPlayerDataSync(player, now)
        data.collectPlayerDataPooledExpSync(player, now)
        const viewerId = 123_456_789
        const clientData = serializers.getClientSerializedData(player.id, { viewerId })
        if (clientData === null) throw new Error("default player data was not serialized")
        loadRoute.addCnLoadCompatibilityFields(clientData, "1.4.54")
        const normalized = normalizePlayerData(clientData, viewerId)
        normalized.gacha_info_list = []
        writePlayerFixture(outputPath, normalized)
    } finally {
        const database = dataIndex.default(0)
        database.pragma("wal_checkpoint(TRUNCATE)")
        database.close()
        fs.rmSync(temporaryRoot, { recursive: true, force: true })
    }
}
// //// /从 Node 数据层生成并校验默认玩家数据 ////

await main()
