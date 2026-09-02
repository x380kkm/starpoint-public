// audience: internal
// # cn-evidence
//
// 该模块验证 CN 动态实验只保留安全的路由元数据和运行摘要.

import { readFileSync } from "node:fs"

const HTTP_METHODS = new Set(["GET", "POST", "PUT", "PATCH", "DELETE"])
const ALLOWED_SUMMARY_KEYS = new Set([
    "schemaVersion",
    "run",
    "client",
    "target",
    "emulator",
    "gpu",
    "serverPort",
    "sessionPort",
    "versionRequest",
    "versionResponse",
    "loginRoutes",
    "responseStatuses",
    "nextUi",
    "rawDataStoredElsewhere",
])
const SENSITIVE_KEY_PATTERN = /(?:token|secret|password|credential|authorization|cookie|device|viewer|payload|requestbody|responsebody|pcap|save|account|sessionid)/i
const SENSITIVE_VALUE_PATTERN = /(?:authorization\s*[:=]|bearer\s+|(?:token|secret|password|device_id|viewer_id|session_id)\s*[:=]|\bpcap\b|\.pcap\b)/i
const CN_PATH_PATTERN = /^(?:\/api\/index\.php\/|\/shijtswy\/|\/patch\/cn\/)/

function fail(message) {
    throw new Error(`invalid CN evidence: ${message}`)
}

function assertSafeKey(key, context) {
    if (SENSITIVE_KEY_PATTERN.test(key)) fail(`${context} contains a sensitive key`)
}

function assertSafeValue(value, context) {
    if (typeof value === "string" && SENSITIVE_VALUE_PATTERN.test(value)) {
        fail(`${context} contains a sensitive value`)
    }
}

function assertSafeTree(value, context = "root") {
    if (Array.isArray(value)) {
        value.forEach((item, index) => assertSafeTree(item, `${context}[${index}]`))
        return
    }
    if (value === null || typeof value !== "object") {
        assertSafeValue(value, context)
        return
    }
    for (const [key, child] of Object.entries(value)) {
        assertSafeKey(key, context)
        assertSafeTree(child, `${context}.${key}`)
    }
}

// //// 验证 CN 路由元数据行 [@x380kkm 2026-08-13] ////
export function validateCnMetadataRecord(record, context = "metadata") {
    if (record === null || typeof record !== "object" || Array.isArray(record)) {
        fail(`${context} must be an object`)
    }
    const keys = Object.keys(record).sort()
    const expectedKeys = ["contentType", "method", "observedAtUtc", "path", "status"]
    if (JSON.stringify(keys) !== JSON.stringify(expectedKeys)) {
        fail(`${context} has an unexpected shape`)
    }
    if (typeof record.observedAtUtc !== "string" || Number.isNaN(Date.parse(record.observedAtUtc))) {
        fail(`${context}.observedAtUtc is invalid`)
    }
    if (typeof record.method !== "string" || !HTTP_METHODS.has(record.method)) {
        fail(`${context}.method is invalid`)
    }
    if (typeof record.path !== "string" || !CN_PATH_PATTERN.test(record.path) || /[?#\s]/.test(record.path)) {
        fail(`${context}.path is invalid`)
    }
    if (!Number.isInteger(record.status) || record.status < 100 || record.status > 599) {
        fail(`${context}.status is invalid`)
    }
    if (record.contentType !== null && typeof record.contentType !== "string") {
        fail(`${context}.contentType is invalid`)
    }
    assertSafeTree(record, context)
    return record
}
// //// /验证 CN 路由元数据行 ////

// //// 验证 CN 运行摘要 [@x380kkm 2026-08-13] ////
export function validateCnRunSummary(summary) {
    if (summary === null || typeof summary !== "object" || Array.isArray(summary)) {
        fail("summary must be an object")
    }
    for (const key of Object.keys(summary)) {
        if (!ALLOWED_SUMMARY_KEYS.has(key)) fail(`summary.${key} is not allowed`)
    }
    if (summary.schemaVersion !== 1) fail("summary.schemaVersion must be 1")
    for (const key of ["run", "client", "target", "emulator", "gpu", "versionRequest", "versionResponse", "responseStatuses", "nextUi"]) {
        if (typeof summary[key] !== "string" || summary[key].length === 0) fail(`summary.${key} is required`)
    }
    if (!/^protocol-lab:[A-Za-z0-9][A-Za-z0-9-]*$/.test(summary.run)) fail("summary.run is invalid")
    if (!/^cn-(?:android|ios)-/.test(summary.client)) fail("summary.client is invalid")
    for (const key of ["serverPort", "sessionPort"]) {
        if (!Number.isInteger(summary[key]) || summary[key] < 1 || summary[key] > 65535) fail(`summary.${key} is invalid`)
    }
    if (!Array.isArray(summary.loginRoutes) || summary.loginRoutes.length === 0) fail("summary.loginRoutes is required")
    for (const [index, route] of summary.loginRoutes.entries()) validateEvidenceRoute(route, `summary.loginRoutes[${index}]`)
    if (summary.rawDataStoredElsewhere !== false) fail("summary.rawDataStoredElsewhere must be false")
    assertSafeTree(summary, "summary")
    return summary
}
// //// /验证 CN 运行摘要 ////

// //// 验证账本中的安全方法和路径 [@x380kkm 2026-08-13] ////
export function validateEvidenceRoute(route, context = "route") {
    if (typeof route !== "string") fail(`${context} must be a string`)
    const match = /^(GET|POST|PUT|PATCH|DELETE) (\/[^\s?#]*)$/.exec(route)
    if (!match || !CN_PATH_PATTERN.test(match[2])) fail(`${context} is invalid`)
    assertSafeValue(route, context)
    return route
}
// //// /验证账本中的安全方法和路径 ////

// //// 读取并验证安全证据文件 [@x380kkm 2026-08-13] ////
export function readAndValidateCnEvidence(summaryPath, metadataPath, expectedRoutes = []) {
    const summary = validateCnRunSummary(JSON.parse(readFileSync(summaryPath, "utf8")))
    const lines = readFileSync(metadataPath, "utf8").split(/\r?\n/).filter((line) => line.length > 0)
    const records = lines.map((line, index) => {
        let record
        try {
            record = JSON.parse(line)
        } catch {
            fail(`metadata line ${index + 1} is not JSON`)
        }
        return validateCnMetadataRecord(record, `metadata line ${index + 1}`)
    })
    const observedRoutes = new Set(records.map((record) => `${record.method} ${record.path}`))
    for (const route of expectedRoutes) {
        validateEvidenceRoute(route, "expected route")
        if (!observedRoutes.has(route)) fail(`expected route was not observed: ${route}`)
    }
    return { summary, records, observedRoutes }
}
// //// /读取并验证安全证据文件 ////
