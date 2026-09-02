// audience: internal
// # cn-reference-differential-lib
//
// 该模块解码 JSON, Base64 MessagePack 和原始 MessagePack 响应, 并比较传输契约, 封套, 数据和状态变化.
// 规范化保留值类型和同值关系, 同时折叠 viewer_id, 运行时间, 会话标识和服务基址.

import { isDeepStrictEqual } from "node:util"
import { unpack } from "msgpackr"

const MAX_REPORTED_DIFFERENCES = 100
const DYNAMIC_ID_KEYS = new Set([
    "viewer_id", "viewerId", "session_id", "sessionId", "request_id", "requestId",
    "trace_id", "traceId", "device_token", "deviceToken", "access_token", "accessToken",
    "refresh_token", "refreshToken", "login_token", "loginToken", "associate_token", "associateToken",
    "keychain", "account_name", "accountName", "roleName", "uuid",
])
const TIME_KEYS = new Set([
    "server_time", "serverTime", "current_time", "currentTime", "request_time", "requestTime",
    "join_time", "joinTime", "update_time", "updateTime", "created_at", "createdAt",
    "updated_at", "updatedAt", "create_time", "createTime", "login_time", "loginTime", "last_login_time", "lastLoginTime",
    "stamina_heal_time", "staminaHealTime", "exp_pooled_time", "expPooledTime", "createDate",
    "aggregated_time", "aggregatedTime", "start_time", "startTime", "host_entry_time", "hostEntryTime",
    "servertime", "timestamp",
])

// //// 读取响应和规范化配置中的字段路径 [@x380kkm 2026-08-23] ////
function parseFieldPath(fieldPath) {
    const source = fieldPath.replace(/^\$\.?/, "")
    if (source.length === 0) return []
    const tokens = []
    const matcher = /(?:^|\.)([^.[\]]+)|\[(\d+|"(?:[^"\\]|\\.)*")\]/g
    let matchedLength = 0
    for (const match of source.matchAll(matcher)) {
        if (match.index !== matchedLength) throw new Error(`invalid field path ${fieldPath}`)
        const token = match[1] ?? match[2]
        tokens.push(token.startsWith("\"") ? JSON.parse(token) : /^\d+$/.test(token) ? Number(token) : token)
        matchedLength = match.index + match[0].length
    }
    if (matchedLength !== source.length) throw new Error(`invalid field path ${fieldPath}`)
    return tokens
}

export function readDifferentialField(value, fieldPath) {
    let current = value
    for (const token of parseFieldPath(fieldPath)) {
        if (current === null || typeof current !== "object" || !(token in current)) {
            throw new Error(`field reference ${fieldPath} was not found`)
        }
        current = current[token]
    }
    return current
}

export function collectDifferentialVariables(value, variables) {
    if (Array.isArray(value)) {
        for (const entry of value) collectDifferentialVariables(entry, variables)
        return
    }
    if (value === null || typeof value !== "object") return
    for (const [key, entry] of Object.entries(value)) {
        if (DYNAMIC_ID_KEYS.has(key) && entry !== null && typeof entry !== "object") variables[key] = entry
        collectDifferentialVariables(entry, variables)
    }
}
// //// /读取响应和规范化配置中的字段路径 ////

// //// 解码 JSON 和两种 MessagePack 传输 [@x380kkm 2026-08-23] ////
function decodeStrictBase64(body) {
    const source = body.toString("ascii")
    if (source.length === 0 || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(source)) {
        throw new TypeError("body is not canonical Base64")
    }
    const decoded = Buffer.from(source, "base64")
    if (decoded.toString("base64") !== source) throw new TypeError("body is not canonical Base64")
    return decoded
}

function decodeWith(decoder, encoding, body) {
    try {
        return { resolved: true, encoding, value: decoder(body) }
    } catch (error) {
        return { resolved: false, error: `${encoding}: ${error.message}` }
    }
}

function isJsonContentType(contentType) {
    return contentType === "application/json" || contentType.endsWith("+json")
}

function isMessagePackContentType(contentType) {
    return ["application/x-msgpack", "application/msgpack", "application/vnd.msgpack"].includes(contentType)
}

function isTextContentType(contentType) {
    return contentType.startsWith("text/") || ["application/xml", "application/javascript"].includes(contentType)
}

export function decodeDifferentialResponse(response, encodingHint = "auto") {
    if (!response.ok) return { resolved: false, error: `${response.error.name}: ${response.error.message}` }
    if (response.body.length === 0) return { resolved: true, encoding: "empty", value: null }
    const decoders = {
        json: [(body) => JSON.parse(body.toString("utf8")), "json"],
        "base64-messagepack": [(body) => unpack(decodeStrictBase64(body)), "base64-messagepack"],
        messagepack: [(body) => unpack(decodeStrictBase64(body)), "base64-messagepack"],
        "raw-messagepack": [(body) => unpack(body), "raw-messagepack"],
        msgpack: [(body) => unpack(body), "raw-messagepack"],
        text: [(body) => body.toString("utf8"), "text"],
    }
    const hint = String(encodingHint).toLowerCase()
    if (hint !== "auto") {
        const selected = decoders[hint]
        if (selected === undefined) return { resolved: false, error: `unsupported response encoding ${hint}` }
        return decodeWith(selected[0], selected[1], response.body)
    }
    if (isJsonContentType(response.contentType)) return decodeWith(...decoders.json, response.body)
    if (isMessagePackContentType(response.contentType)) {
        const base64 = decodeWith(...decoders["base64-messagepack"], response.body)
        return base64.resolved ? base64 : decodeWith(...decoders["raw-messagepack"], response.body)
    }
    if (isTextContentType(response.contentType)) return { resolved: true, encoding: "text", value: response.body.toString("utf8") }
    const text = response.body.toString("utf8").trimStart()
    if (text.startsWith("{") || text.startsWith("[")) {
        const json = decodeWith(...decoders.json, response.body)
        if (json.resolved) return json
    }
    const base64 = decodeWith(...decoders["base64-messagepack"], response.body)
    if (base64.resolved) return base64
    const raw = decodeWith(...decoders["raw-messagepack"], response.body)
    if (raw.resolved) return raw
    return { resolved: false, error: `${base64.error}; ${raw.error}` }
}
// //// /解码 JSON 和两种 MessagePack 传输 ////

// //// 将响应值转换为稳定的可比较结构 [@x380kkm 2026-08-23] ////
function comparableType(value) {
    if (value === null) return "null"
    if (Array.isArray(value)) return "array"
    return typeof value === "object" ? "object" : typeof value
}

function toComparable(value) {
    if (typeof value === "bigint") return { $bigint: value.toString() }
    if (Buffer.isBuffer(value) || value instanceof Uint8Array) return { $binary: Buffer.from(value).toString("base64") }
    if (value instanceof Date) return { $date: value.toISOString() }
    if (value instanceof Map) return Object.fromEntries([...value.entries()].map(([key, entry]) => [String(key), toComparable(entry)]))
    if (Array.isArray(value)) return value.map(toComparable)
    if (value === null || typeof value !== "object") return value
    return Object.fromEntries(Object.entries(value).map(([key, entry]) => [key, toComparable(entry)]))
}

function transformAtPath(value, fieldPath, transform) {
    const tokens = parseFieldPath(fieldPath)
    const visit = (current, index) => {
        if (index === tokens.length) return transform(current)
        if (current === null || typeof current !== "object") return current
        const token = tokens[index]
        if (token === "*") {
            for (const key of Object.keys(current)) current[key] = visit(current[key], index + 1)
            return current
        }
        if (!(token in current)) return current
        current[token] = visit(current[token], index + 1)
        return current
    }
    return visit(value, 0)
}

function removeIgnoredAtPath(value, fieldPath) {
    const tokens = parseFieldPath(fieldPath)
    if (tokens.length === 0) return { $comparisonIgnored: true }
    const visit = (current, index) => {
        if (current === null || typeof current !== "object") return current
        const token = tokens[index]
        const isLeaf = index === tokens.length - 1
        const removeOrReplace = (key) => {
            if (Array.isArray(current)) current[key] = { $comparisonIgnored: true }
            else delete current[key]
        }
        if (token === "*") {
            for (const key of Object.keys(current)) {
                if (isLeaf) removeOrReplace(key)
                else current[key] = visit(current[key], index + 1)
            }
            return current
        }
        if (!(token in current)) return current
        if (isLeaf) removeOrReplace(token)
        else current[token] = visit(current[token], index + 1)
        return current
    }
    return visit(value, 0)
}

function projectText(value, configuration) {
    if (configuration === undefined || typeof value !== "string") return value
    const stripFirstLines = configuration.stripFirstLines ?? 0
    const text = value.split(/\r?\n/).slice(stripFirstLines).join("\n")
    return configuration.parseJson === true ? JSON.parse(text) : text
}

function projectMapValues(value, fieldPath) {
    return transformAtPath(value, fieldPath, (record) => {
        if (record === null || typeof record !== "object" || Array.isArray(record)) {
            throw new TypeError(`${fieldPath} must contain an object record`)
        }
        return Object.values(record).sort((left, right) => JSON.stringify(left).localeCompare(JSON.stringify(right), "en"))
    })
}

function readFilterField(value, fieldPath) {
    return fieldPath === undefined || fieldPath === "$" ? value : readDifferentialField(value, fieldPath)
}

function matchesFilterCondition(value, condition) {
    let candidate
    try {
        candidate = readFilterField(value, condition.field ?? condition.path)
    } catch {
        return false
    }
    if (condition.equals !== undefined && !isDeepStrictEqual(candidate, condition.equals)) return false
    if (condition.in !== undefined && !condition.in.some((entry) => isDeepStrictEqual(candidate, entry))) return false
    if (condition.includes !== undefined && !String(candidate).includes(String(condition.includes))) return false
    if (condition.includesAny !== undefined
        && !condition.includesAny.some((entry) => String(candidate).includes(String(entry)))) return false
    if (condition.matches !== undefined && !new RegExp(condition.matches).test(String(candidate))) return false
    return true
}

function projectArrayFilter(value, filter) {
    return transformAtPath(value, filter.path, (entries) => {
        if (!Array.isArray(entries)) throw new TypeError(`${filter.path} must contain an array`)
        const conditions = Array.isArray(filter.where) ? filter.where : [filter.where ?? filter]
        return entries.filter((entry) => conditions.every((condition) => matchesFilterCondition(entry, condition)))
    })
}

function projectComparable(value, comparison) {
    let projected = structuredClone(toComparable(value))
    projected = projectText(projected, comparison.text)
    for (const fieldPath of comparison.ignorePaths ?? []) {
        projected = removeIgnoredAtPath(projected, fieldPath)
    }
    for (const fieldPath of [...(comparison.mapValues ?? []), ...(comparison.recordValues ?? [])]) {
        projected = projectMapValues(projected, typeof fieldPath === "string" ? fieldPath : fieldPath.path)
    }
    for (const filter of comparison.arrayFilters ?? []) projected = projectArrayFilter(projected, filter)
    return projected
}

function selectValuePaths(value, fieldPaths) {
    return Object.fromEntries(fieldPaths.map((fieldPath) => {
        try {
            return [fieldPath, readDifferentialField(value, fieldPath)]
        } catch {
            return [fieldPath, { $missing: true }]
        }
    }))
}

function parsePattern(pattern) {
    if (pattern.startsWith("/")) {
        return pattern.slice(1).split("/").map((token) => token.replaceAll("~1", "/").replaceAll("~0", "~"))
    }
    return parseFieldPath(pattern).map(String)
}

function segmentsMatch(pattern, valuePath, patternIndex = 0, pathIndex = 0) {
    if (patternIndex === pattern.length) return pathIndex === valuePath.length
    if (pattern[patternIndex] === "**") {
        return segmentsMatch(pattern, valuePath, patternIndex + 1, pathIndex)
            || (pathIndex < valuePath.length && segmentsMatch(pattern, valuePath, patternIndex, pathIndex + 1))
    }
    return pathIndex < valuePath.length
        && (pattern[patternIndex] === "*" || pattern[patternIndex] === String(valuePath[pathIndex]))
        && segmentsMatch(pattern, valuePath, patternIndex + 1, pathIndex + 1)
}

function matchesAnyPath(patterns, valuePath) {
    return patterns.some((pattern) => segmentsMatch(parsePattern(pattern), valuePath))
}

export function mergeDifferentialNormalization(defaults, route) {
    return {
        ...(defaults ?? {}),
        ...(route ?? {}),
        keys: [...(defaults?.keys ?? []), ...(route?.keys ?? [])],
        dynamicIdKeys: [...(defaults?.dynamicIdKeys ?? []), ...(route?.dynamicIdKeys ?? [])],
        timeKeys: [...(defaults?.timeKeys ?? []), ...(route?.timeKeys ?? [])],
        paths: [...(defaults?.paths ?? []), ...(route?.paths ?? [])],
        timePaths: [...(defaults?.timePaths ?? []), ...(route?.timePaths ?? [])],
        ignorePaths: [...(defaults?.ignorePaths ?? []), ...(route?.ignorePaths ?? [])],
        zeroBaselinePaths: [...(defaults?.zeroBaselinePaths ?? []), ...(route?.zeroBaselinePaths ?? [])],
    }
}

function createNormalizer(configuration, baseUrl) {
    const dynamicKeys = new Set([...(configuration.defaults === false ? [] : DYNAMIC_ID_KEYS), ...configuration.keys, ...configuration.dynamicIdKeys])
    const timeKeys = new Set([...(configuration.defaults === false ? [] : TIME_KEYS), ...configuration.timeKeys])
    const mappings = new Map()
    const endpoint = new URL(baseUrl)
    const normalizeScalar = (value, family, includeIndex) => {
        const type = comparableType(value)
        if (!includeIndex) return { $normalized: family, type }
        if (!mappings.has(family)) mappings.set(family, new Map())
        const values = mappings.get(family)
        const identity = `${type}:${JSON.stringify(value)}`
        if (!values.has(identity)) values.set(identity, values.size + 1)
        return { $normalized: family, type, index: values.get(identity) }
    }
    const visit = (input, valuePath = []) => {
        const value = toComparable(input)
        const key = valuePath.length > 0 ? String(valuePath.at(-1)) : ""
        if (matchesAnyPath(configuration.ignorePaths, valuePath)) return normalizeScalar(value, "ignored", false)
        if (matchesAnyPath(configuration.timePaths, valuePath) || timeKeys.has(key)) return normalizeScalar(value, `time:${key}`, false)
        if (matchesAnyPath(configuration.paths, valuePath) || dynamicKeys.has(key)) return normalizeScalar(value, `dynamic:${key}`, true)
        if (typeof value === "string") {
            return value
                .replaceAll(endpoint.origin, "<base-url>")
                .replaceAll(baseUrl, "<base-url>")
                .replaceAll(endpoint.host, "<base-host>")
        }
        if (Array.isArray(value)) return value.map((entry, index) => visit(entry, [...valuePath, index]))
        if (value !== null && typeof value === "object") {
            return Object.fromEntries(Object.entries(value).map(([name, entry]) => [name, visit(entry, [...valuePath, name])]))
        }
        return value
    }
    return visit
}
// //// /将响应值转换为稳定的可比较结构 ////

// //// 比较封套, 数据形状和规范化值 [@x380kkm 2026-08-23] ////
function splitEnvelope(value) {
    if (value !== null && typeof value === "object" && !Array.isArray(value) && Object.hasOwn(value, "data")) {
        return { envelope: Object.fromEntries(Object.entries(value).filter(([key]) => key !== "data")), data: value.data }
    }
    return { envelope: null, data: value }
}

function describePayload(value) {
    const split = splitEnvelope(toComparable(value))
    return {
        rootType: comparableType(value),
        envelopeKeys: split.envelope === null ? [] : Object.keys(split.envelope).sort(),
        dataType: comparableType(split.data),
        dataKeys: split.data !== null && typeof split.data === "object" && !Array.isArray(split.data)
            ? Object.keys(split.data).sort()
            : [],
        dataLength: Array.isArray(split.data) ? split.data.length : null,
    }
}

function collectShapeDifferences(reference, rust, valuePath = "$", output = []) {
    const referenceType = comparableType(reference)
    const rustType = comparableType(rust)
    if (referenceType !== rustType) {
        output.push({ path: valuePath, reference: referenceType, rust: rustType })
        return output
    }
    if (referenceType === "array") {
        for (let index = 0; index < Math.min(reference.length, rust.length); index += 1) {
            collectShapeDifferences(reference[index], rust[index], `${valuePath}[${index}]`, output)
        }
    } else if (referenceType === "object") {
        const referenceKeys = Object.keys(reference).sort()
        const rustKeys = Object.keys(rust).sort()
        if (!isDeepStrictEqual(referenceKeys, rustKeys)) output.push({ path: valuePath, referenceKeys, rustKeys })
        for (const key of referenceKeys.filter((entry) => Object.hasOwn(rust, entry))) {
            collectShapeDifferences(reference[key], rust[key], `${valuePath}.${key}`, output)
        }
    }
    return output
}

function collectValueDifferences(reference, rust, valuePath = "$", output = [], limit = Infinity) {
    if (output.length >= limit || isDeepStrictEqual(reference, rust)) return output
    const referenceType = comparableType(reference)
    const rustType = comparableType(rust)
    if (referenceType !== rustType) {
        output.push({ path: valuePath, reference, rust })
        return output
    }
    if (referenceType === "array") {
        if (reference.length !== rust.length) output.push({ path: `${valuePath}.length`, reference: reference.length, rust: rust.length })
        for (let index = 0; index < Math.min(reference.length, rust.length) && output.length < limit; index += 1) {
            collectValueDifferences(reference[index], rust[index], `${valuePath}[${index}]`, output, limit)
        }
    } else if (referenceType === "object") {
        const keys = [...new Set([...Object.keys(reference), ...Object.keys(rust)])].sort()
        for (const key of keys) {
            if (output.length >= limit) break
            if (!Object.hasOwn(reference, key) || !Object.hasOwn(rust, key)) {
                output.push({ path: `${valuePath}.${key}`, reference: reference[key], rust: rust[key] })
            } else {
                collectValueDifferences(reference[key], rust[key], `${valuePath}.${key}`, output, limit)
            }
        }
    } else {
        output.push({ path: valuePath, reference, rust })
    }
    return output
}

function addDifference(differences, field, entries) {
    if (entries.length === 0) return
    differences.push({ field, differences: entries.slice(0, MAX_REPORTED_DIFFERENCES), truncated: entries.length > MAX_REPORTED_DIFFERENCES })
}

function summarizeResponse(response, decoded) {
    if (!response.ok) return { transport: "failed", error: response.error }
    return {
        status: response.status,
        contentType: response.contentType,
        location: response.location,
        bodyBytes: response.body.length,
        encoding: decoded.encoding ?? null,
        decoded: decoded.resolved,
        decodeError: decoded.resolved ? null : decoded.error,
        payload: decoded.resolved ? describePayload(decoded.value) : null,
    }
}

export function compareDifferentialResponses(
    referenceResponse,
    rustResponse,
    referenceDecoded,
    rustDecoded,
    normalization = {},
    baseUrls = {},
    comparison = {},
) {
    const differences = []
    const unresolvedReasons = []
    if (!referenceResponse.ok) unresolvedReasons.push(`reference transport: ${referenceDecoded.error}`)
    if (!rustResponse.ok) unresolvedReasons.push(`rust transport: ${rustDecoded.error}`)
    if (referenceResponse.ok && rustResponse.ok) {
        if (referenceResponse.status !== rustResponse.status) {
            differences.push({ field: "status", reference: referenceResponse.status, rust: rustResponse.status })
        }
        if (referenceResponse.contentType !== rustResponse.contentType) {
            differences.push({ field: "contentType", reference: referenceResponse.contentType, rust: rustResponse.contentType })
        }
        const referenceLocation = referenceResponse.location
            ?.replaceAll(new URL(baseUrls.reference ?? "http://reference.invalid").host, "<base-host>") ?? null
        const rustLocation = rustResponse.location
            ?.replaceAll(new URL(baseUrls.rust ?? "http://rust.invalid").host, "<base-host>") ?? null
        if (referenceLocation !== rustLocation) differences.push({ field: "location", reference: referenceLocation, rust: rustLocation })
    }
    if (!referenceDecoded.resolved) unresolvedReasons.push(`reference decode: ${referenceDecoded.error}`)
    if (!rustDecoded.resolved) unresolvedReasons.push(`rust decode: ${rustDecoded.error}`)
    if (referenceDecoded.resolved && rustDecoded.resolved) {
        if (referenceDecoded.encoding !== rustDecoded.encoding) {
            differences.push({ field: "transportEncoding", reference: referenceDecoded.encoding, rust: rustDecoded.encoding })
        }
        try {
            const referenceComparable = projectComparable(referenceDecoded.value, comparison)
            const rustComparable = projectComparable(rustDecoded.value, comparison)
            const referenceSplit = splitEnvelope(referenceComparable)
            const rustSplit = splitEnvelope(rustComparable)
            addDifference(differences, "envelopeShape", collectShapeDifferences(referenceSplit.envelope, rustSplit.envelope))
            const referenceShape = comparison.valuePaths?.length > 0
                ? selectValuePaths(referenceComparable, comparison.valuePaths)
                : referenceSplit.data
            const rustShape = comparison.valuePaths?.length > 0
                ? selectValuePaths(rustComparable, comparison.valuePaths)
                : rustSplit.data
            addDifference(differences, "dataShape", collectShapeDifferences(referenceShape, rustShape))
            const configuration = mergeDifferentialNormalization({}, normalization)
            const normalizedReferenceRoot = createNormalizer(
                configuration,
                baseUrls.reference ?? "http://reference.invalid",
            )(referenceComparable)
            const normalizedRustRoot = createNormalizer(
                configuration,
                baseUrls.rust ?? "http://rust.invalid",
            )(rustComparable)
            const normalizedReference = splitEnvelope(normalizedReferenceRoot)
            const normalizedRust = splitEnvelope(normalizedRustRoot)
            addDifference(differences, "envelopeValue", collectValueDifferences(normalizedReference.envelope, normalizedRust.envelope))
            const referenceData = comparison.valuePaths?.length > 0
                ? selectValuePaths(normalizedReferenceRoot, comparison.valuePaths)
                : normalizedReference.data
            const rustData = comparison.valuePaths?.length > 0
                ? selectValuePaths(normalizedRustRoot, comparison.valuePaths)
                : normalizedRust.data
            addDifference(differences, "dataValue", collectValueDifferences(referenceData, rustData))
        } catch (error) {
            unresolvedReasons.push(`comparison projection: ${error.message}`)
        }
    }
    return {
        status: differences.length > 0 ? "mismatched" : unresolvedReasons.length > 0 ? "unresolved" : "matched",
        differences,
        unresolvedReasons,
        reference: summarizeResponse(referenceResponse, referenceDecoded),
        rust: summarizeResponse(rustResponse, rustDecoded),
    }
}
// //// /比较封套, 数据形状和规范化值 ////

// //// 比较状态探针的前后变化 [@x380kkm 2026-08-23] ////
function changesBetween(before, after, normalization, baseUrl, comparison) {
    const configuration = mergeDifferentialNormalization({}, normalization)
    const normalizer = createNormalizer(configuration, baseUrl)
    const projectState = (value) => {
        const projected = projectComparable(value, comparison)
        return comparison.valuePaths?.length > 0
            ? selectValuePaths(projected, comparison.valuePaths)
            : projected
    }
    const comparableBefore = projectState(before)
    const comparableAfter = projectState(after)
    return collectValueDifferences(normalizer(comparableBefore), normalizer(comparableAfter))
        .filter((change) => !matchesAnyPath(configuration.ignorePaths, parseFieldPath(change.path)))
        .map((change) => {
            const path = parseFieldPath(change.path)
            const zeroBaseline = matchesAnyPath(configuration.zeroBaselinePaths, path)
            const reference = zeroBaseline && change.reference === undefined && typeof change.rust === "number"
                ? 0
                : change.reference
            const rust = zeroBaseline && change.rust === undefined && typeof change.reference === "number"
                ? 0
                : change.rust
            if (typeof reference === "number" && typeof rust === "number"
                && Number.isFinite(reference) && Number.isFinite(rust)) {
                return { path: change.path, delta: rust - reference }
            }
            return { path: change.path, after: change.rust }
        })
}

export function compareDifferentialStateTransition(before, after, normalization, baseUrls, comparison = {}) {
    if (!before.referenceDecoded.resolved || !before.rustDecoded.resolved
        || !after.referenceDecoded.resolved || !after.rustDecoded.resolved) {
        return { status: "unresolved", unresolvedReasons: ["state probe response could not be decoded"] }
    }
    const referenceChanges = changesBetween(
        before.referenceDecoded.value, after.referenceDecoded.value, normalization, baseUrls.reference, comparison,
    )
    const rustChanges = changesBetween(
        before.rustDecoded.value, after.rustDecoded.value, normalization, baseUrls.rust, comparison,
    )
    if (isDeepStrictEqual(referenceChanges, rustChanges)) return { status: "matched", referenceChanges, rustChanges }
    return {
        status: "mismatched",
        differences: collectValueDifferences(referenceChanges, rustChanges).slice(0, MAX_REPORTED_DIFFERENCES),
        referenceChanges: referenceChanges.slice(0, MAX_REPORTED_DIFFERENCES),
        rustChanges: rustChanges.slice(0, MAX_REPORTED_DIFFERENCES),
        truncated: referenceChanges.length > MAX_REPORTED_DIFFERENCES || rustChanges.length > MAX_REPORTED_DIFFERENCES,
    }
}
// //// /比较状态探针的前后变化 ////
