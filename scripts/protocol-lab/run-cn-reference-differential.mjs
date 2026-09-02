// audience: internal
// # run-cn-reference-differential
//
// 该命令向参考 CN 服务和 Rust 个人服务重放同一份有序语料, 并输出逐路由动态差分报告.
// matched 表示传输和规范化值一致, local-extension 表示精确匹配已声明的本地增强, mismatched 表示已证明差异, unresolved 表示请求或响应缺少可比较证据.

import assert from "node:assert/strict"
import { mkdirSync, readFileSync, writeFileSync } from "node:fs"
import { createServer } from "node:http"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { pack, unpack } from "msgpackr"
import {
    collectDifferentialVariables,
    compareDifferentialResponses,
    compareDifferentialStateTransition,
    decodeDifferentialResponse,
    mergeDifferentialNormalization,
    readDifferentialField,
} from "./cn-reference-differential-lib.mjs"

const DEFAULT_TIMEOUT_MS = 30_000
const USAGE = `Usage:
  node scripts/protocol-lab/run-cn-reference-differential.mjs \\
    --reference-base-url <url> --rust-base-url <url> --corpus <file> [--report <file>] [--timeout-ms <ms>] [--account-seed <n>]
  node scripts/protocol-lab/run-cn-reference-differential.mjs --check-corpus --corpus <file>
  node scripts/protocol-lab/run-cn-reference-differential.mjs --self-test
`

class DifferentialInputError extends Error {}

// //// 解析差分命令行和语料 [@x380kkm 2026-08-23] ////
function normalizeBaseUrl(value, option) {
    let parsed
    try {
        parsed = new URL(value)
    } catch {
        throw new DifferentialInputError(`${option} must be an absolute HTTP URL`)
    }
    if (!["http:", "https:"].includes(parsed.protocol)) throw new DifferentialInputError(`${option} must use HTTP or HTTPS`)
    parsed.hash = ""
    parsed.search = ""
    return parsed.toString().replace(/\/$/, "")
}

function parseArguments(args) {
    const options = { timeoutMs: DEFAULT_TIMEOUT_MS }
    for (let index = 0; index < args.length; index += 1) {
        const argument = args[index]
        if (["--self-test", "--check-corpus", "--help"].includes(argument)) {
            options[argument.slice(2).replace("-", "")] = true
            continue
        }
        if (!argument.startsWith("--")) throw new DifferentialInputError(`unexpected argument ${argument}`)
        const value = args[index + 1]
        if (value === undefined || value.startsWith("--")) throw new DifferentialInputError(`${argument} requires a value`)
        index += 1
        if (["--reference-base-url", "--reference"].includes(argument)) options.referenceBaseUrl = normalizeBaseUrl(value, argument)
        else if (["--rust-base-url", "--rust"].includes(argument)) options.rustBaseUrl = normalizeBaseUrl(value, argument)
        else if (argument === "--corpus") options.corpusPath = value
        else if (["--report", "--output"].includes(argument)) {
            if (options.reportPath !== undefined) throw new DifferentialInputError("report path was provided more than once")
            options.reportPath = value
        } else if (argument === "--timeout-ms") {
            options.timeoutMs = Number(value)
            if (!Number.isInteger(options.timeoutMs) || options.timeoutMs <= 0) throw new DifferentialInputError("--timeout-ms must be a positive integer")
        } else if (argument === "--account-seed") {
            options.accountSeed = Number(value)
            if (!Number.isSafeInteger(options.accountSeed) || options.accountSeed < 0) {
                throw new DifferentialInputError("--account-seed must be a non-negative safe integer")
            }
        } else throw new DifferentialInputError(`unknown option ${argument}`)
    }
    if (options.selftest || options.help) return options
    if (options.checkcorpus) {
        if (options.corpusPath === undefined) throw new DifferentialInputError("--corpus is required")
        return options
    }
    for (const [field, option] of [["referenceBaseUrl", "--reference-base-url"], ["rustBaseUrl", "--rust-base-url"], ["corpusPath", "--corpus"]]) {
        if (options[field] === undefined) throw new DifferentialInputError(`${option} is required`)
    }
    return options
}

function requireObject(value, label) {
    if (value === null || typeof value !== "object" || Array.isArray(value)) throw new DifferentialInputError(`${label} must be an object`)
    return value
}

function validateStringArray(value, label) {
    if (!Array.isArray(value) || value.some((entry) => typeof entry !== "string" || entry.length === 0)) {
        throw new DifferentialInputError(`${label} must be an array of non-empty paths`)
    }
}

function validateComparison(value, label) {
    if (value === undefined) return
    const comparison = requireObject(value, label)
    if (comparison.text !== undefined) {
        const text = requireObject(comparison.text, `${label}.text`)
        if (text.stripFirstLines !== undefined
            && (!Number.isSafeInteger(text.stripFirstLines) || text.stripFirstLines < 0)) {
            throw new DifferentialInputError(`${label}.text.stripFirstLines must be a non-negative integer`)
        }
        if (text.parseJson !== undefined && typeof text.parseJson !== "boolean") {
            throw new DifferentialInputError(`${label}.text.parseJson must be boolean`)
        }
    }
    for (const field of ["ignorePaths", "mapValues", "recordValues", "valuePaths"]) {
        if (comparison[field] !== undefined) validateStringArray(comparison[field], `${label}.${field}`)
    }
    if (comparison.arrayFilters !== undefined) {
        if (!Array.isArray(comparison.arrayFilters)) {
            throw new DifferentialInputError(`${label}.arrayFilters must be an array`)
        }
        for (const [index, rawFilter] of comparison.arrayFilters.entries()) {
            const filterLabel = `${label}.arrayFilters[${index}]`
            const filter = requireObject(rawFilter, filterLabel)
            if (typeof filter.path !== "string" || filter.path.length === 0) {
                throw new DifferentialInputError(`${filterLabel}.path must be a non-empty string`)
            }
            const conditions = Array.isArray(filter.where) ? filter.where : [filter.where ?? filter]
            for (const [conditionIndex, rawCondition] of conditions.entries()) {
                const conditionLabel = `${filterLabel}.where[${conditionIndex}]`
                const condition = requireObject(rawCondition, conditionLabel)
                const conditionPath = condition.field ?? condition.path
                if (typeof conditionPath !== "string" || conditionPath.length === 0) {
                    throw new DifferentialInputError(`${conditionLabel}.field must be a non-empty string`)
                }
                if (condition.in !== undefined && !Array.isArray(condition.in)) {
                    throw new DifferentialInputError(`${conditionLabel}.in must be an array`)
                }
                if (condition.includesAny !== undefined && !Array.isArray(condition.includesAny)) {
                    throw new DifferentialInputError(`${conditionLabel}.includesAny must be an array`)
                }
                if (condition.matches !== undefined) {
                    if (typeof condition.matches !== "string") {
                        throw new DifferentialInputError(`${conditionLabel}.matches must be a string`)
                    }
                    try {
                        new RegExp(condition.matches)
                    } catch {
                        throw new DifferentialInputError(`${conditionLabel}.matches must be a valid regular expression`)
                    }
                }
            }
        }
    }
}

function mergeComparison(defaults, request) {
    return {
        ...(defaults ?? {}),
        ...(request ?? {}),
        text: defaults?.text === undefined && request?.text === undefined
            ? undefined
            : { ...(defaults?.text ?? {}), ...(request?.text ?? {}) },
        ignorePaths: [...(defaults?.ignorePaths ?? []), ...(request?.ignorePaths ?? [])],
        mapValues: [...(defaults?.mapValues ?? []), ...(request?.mapValues ?? [])],
        recordValues: [...(defaults?.recordValues ?? []), ...(request?.recordValues ?? [])],
        arrayFilters: [...(defaults?.arrayFilters ?? []), ...(request?.arrayFilters ?? [])],
        valuePaths: [...(defaults?.valuePaths ?? []), ...(request?.valuePaths ?? [])],
    }
}

function validateRequestEncoding(request, label) {
    if (request.body !== undefined && request.encoding === undefined && request.bodyBase64 === undefined) {
        throw new DifferentialInputError(`${label}.encoding is required when body is present`)
    }
    const supported = ["none", "json", "messagepack", "base64-messagepack", "msgpack-base64", "raw-messagepack", "msgpack", "form", "base64", "text"]
    if (request.encoding !== undefined && !supported.includes(String(request.encoding).toLowerCase())) {
        throw new DifferentialInputError(`${label}.encoding is unsupported`)
    }
    if (request.stateExpectation !== undefined && !["changed", "unchanged"].includes(request.stateExpectation)) {
        throw new DifferentialInputError(`${label}.stateExpectation must be changed or unchanged`)
    }
    if (request.expectsReferenceStateChange !== undefined && typeof request.expectsReferenceStateChange !== "boolean") {
        throw new DifferentialInputError(`${label}.expectsReferenceStateChange must be boolean`)
    }
    if (request.stateIgnorePaths !== undefined && (!Array.isArray(request.stateIgnorePaths)
        || request.stateIgnorePaths.some((valuePath) => typeof valuePath !== "string"))) {
        throw new DifferentialInputError(`${label}.stateIgnorePaths must be an array of paths`)
    }
    if (request.stateComparison !== undefined
        && !["exact", "change-presence"].includes(request.stateComparison)) {
        throw new DifferentialInputError(`${label}.stateComparison must be exact or change-presence`)
    }
    validateComparison(request.stateProjection, `${label}.stateProjection`)
    if (request.timeoutMs !== undefined && (!Number.isInteger(request.timeoutMs) || request.timeoutMs <= 0)) {
        throw new DifferentialInputError(`${label}.timeoutMs must be a positive integer`)
    }
    validateComparison(request.comparison, `${label}.comparison`)
    return request
}

function validateLocalExtension(value, label) {
    if (value === undefined) return
    const extension = requireObject(value, label)
    if (typeof extension.reason !== "string" || extension.reason.length === 0) {
        throw new DifferentialInputError(`${label}.reason must be a non-empty string`)
    }
    for (const side of ["reference", "rust"]) {
        const expected = requireObject(extension[side], `${label}.${side}`)
        if (!Number.isInteger(expected.status)) {
            throw new DifferentialInputError(`${label}.${side}.status must be an integer`)
        }
        if (typeof expected.contentType !== "string" || expected.contentType.length === 0) {
            throw new DifferentialInputError(`${label}.${side}.contentType must be a non-empty string`)
        }
    }
}

function normalizeRequestList(value, label) {
    if (value === undefined) return []
    return (Array.isArray(value) ? value : [value]).map((entry, index) => {
        const entryLabel = `${label}[${index}]`
        return validateRequestEncoding(requireObject(entry, entryLabel), entryLabel)
    })
}

function normalizeSideRequests(value, label) {
    if (value === undefined) return []
    return (Array.isArray(value) ? value : [value]).map((entry, index) => {
        const entryLabel = `${label}[${index}]`
        const sideRequest = requireObject(entry, entryLabel)
        const id = sideRequest.id ?? `${label}-${index + 1}`
        if (typeof id !== "string" || id.length === 0) {
            throw new DifferentialInputError(`${entryLabel}.id must be a non-empty string`)
        }
        const normalizeSide = (side) => {
            if (sideRequest[side] === undefined) return null
            const request = requireObject(sideRequest[side], `${entryLabel}.${side}`)
            if (typeof request.path !== "string") {
                throw new DifferentialInputError(`${entryLabel}.${side}.path must be a string`)
            }
            if (request.expectedLocationIncludes !== undefined
                && (typeof request.expectedLocationIncludes !== "string" || request.expectedLocationIncludes.length === 0)) {
                throw new DifferentialInputError(`${entryLabel}.${side}.expectedLocationIncludes must be a non-empty string`)
            }
            return { ...validateRequestEncoding(request, `${entryLabel}.${side}`), id }
        }
        const reference = normalizeSide("reference")
        const rust = normalizeSide("rust")
        if (reference === null && rust === null) {
            throw new DifferentialInputError(`${entryLabel} requires reference or rust`)
        }
        return { ...sideRequest, id, reference, rust }
    })
}

function parseCorpus(value) {
    const document = Array.isArray(value) ? { cases: value } : requireObject(value, "corpus")
    if (!Array.isArray(document.cases)) throw new DifferentialInputError("corpus.cases must be an array")
    const ids = new Set()
    const cases = document.cases.map((entry, index) => {
        const route = requireObject(entry, `cases[${index}]`)
        if (typeof route.method !== "string" || typeof route.path !== "string") throw new DifferentialInputError(`cases[${index}] requires string method and path`)
        const id = route.id ?? `${route.method.toUpperCase()} ${route.path} #${index + 1}`
        if (typeof id !== "string" || id.length === 0) throw new DifferentialInputError(`cases[${index}].id must be a string`)
        if (ids.has(id)) throw new DifferentialInputError(`duplicate case id ${id}`)
        ids.add(id)
        validateRequestEncoding(route, `cases[${index}]`)
        for (const side of ["reference", "rust"]) {
            if (route[side] === undefined) continue
            const override = requireObject(route[side], `cases[${index}].${side}`)
            validateRequestEncoding({ ...route, ...override }, `cases[${index}].${side}`)
        }
        if (route.dependsOn !== undefined && (!Array.isArray(route.dependsOn)
            || route.dependsOn.some((dependency) => typeof dependency !== "string"))) {
            throw new DifferentialInputError(`cases[${index}].dependsOn must be an array of case ids`)
        }
        if (route.isolationGroup !== undefined && typeof route.isolationGroup !== "string") {
            throw new DifferentialInputError(`cases[${index}].isolationGroup must be a string`)
        }
        if (route.stateExpectation !== undefined && !["changed", "unchanged"].includes(route.stateExpectation)) {
            throw new DifferentialInputError(`cases[${index}].stateExpectation must be changed or unchanged`)
        }
        if (route.expectsReferenceStateChange !== undefined && typeof route.expectsReferenceStateChange !== "boolean") {
            throw new DifferentialInputError(`cases[${index}].expectsReferenceStateChange must be boolean`)
        }
        if (route.stateIgnorePaths !== undefined && (!Array.isArray(route.stateIgnorePaths)
            || route.stateIgnorePaths.some((valuePath) => typeof valuePath !== "string"))) {
            throw new DifferentialInputError(`cases[${index}].stateIgnorePaths must be an array of paths`)
        }
        validateLocalExtension(route.localExtension, `cases[${index}].localExtension`)
        const probes = route.probes === undefined ? {} : requireObject(route.probes, `${id}.probes`)
        const normalized = {
            ...route,
            id,
            method: route.method.toUpperCase(),
            prerequisites: normalizeRequestList(route.prerequisites, `${id}.prerequisites`),
            sideRequests: normalizeSideRequests(route.sideRequests, `${id}.sideRequests`),
            dependsOn: route.dependsOn ?? [],
            probes: {
                before: normalizeRequestList(probes.before, `${id}.probes.before`),
                after: normalizeRequestList(probes.after, `${id}.probes.after`),
                state: normalizeRequestList(probes.state, `${id}.probes.state`),
            },
        }
        const routeExpectation = normalized.stateExpectation !== undefined
            || normalized.expectsReferenceStateChange === true
            || normalized.branch?.requiresStateChange === true
        if (!routeExpectation && normalized.probes.state.some((probe) => probe.stateExpectation === undefined
            && probe.expectsReferenceStateChange !== true)) {
            throw new DifferentialInputError(`${id} requires an explicit state expectation`)
        }
        return normalized
    })
    const defaults = document.defaults === undefined ? {} : requireObject(document.defaults, "corpus.defaults")
    validateComparison(defaults.comparison, "corpus.defaults.comparison")
    return {
        version: document.version ?? 1,
        defaults,
        variables: document.variables === undefined ? {} : requireObject(document.variables, "corpus.variables"),
        cases,
    }
}

function readCorpus(corpusPath) {
    try {
        return parseCorpus(JSON.parse(readFileSync(corpusPath, "utf8")))
    } catch (error) {
        if (error instanceof DifferentialInputError) throw error
        throw new DifferentialInputError(`could not read corpus: ${error.message}`)
    }
}
// //// /解析差分命令行和语料 ////

// //// 解析跨 case 动态引用 [@x380kkm 2026-08-23] ////
function resolveCaseReference(reference, runtime) {
    const caseIds = [...runtime.responses.keys()].sort((left, right) => right.length - left.length)
    const caseId = caseIds.find((candidate) => reference === candidate || reference.startsWith(`${candidate}.`))
    if (caseId !== undefined) {
        const fieldPath = reference.slice(caseId.length).replace(/^\./, "")
        return fieldPath.length === 0 ? runtime.responses.get(caseId) : readDifferentialField(runtime.responses.get(caseId), fieldPath)
    }
    if (reference.startsWith("variables.")) return readDifferentialField(runtime.variables, reference.slice(10))
    if (Object.hasOwn(runtime.variables, reference)) return runtime.variables[reference]
    throw new DifferentialInputError(`case reference ${reference} was not found`)
}

function resolveRuntimeValue(value, runtime) {
    if (typeof value === "string") {
        const exact = value.match(/^\$\{([^}]+)\}$/)
        if (exact) return resolveCaseReference(exact[1], runtime)
        return value.replaceAll(/\$\{([^}]+)\}/g, (_, reference) => {
            const resolved = resolveCaseReference(reference, runtime)
            if (["string", "number", "boolean", "bigint"].includes(typeof resolved)) return String(resolved)
            throw new DifferentialInputError(`template ${reference} must resolve to a scalar`)
        })
    }
    if (Array.isArray(value)) return value.map((entry) => resolveRuntimeValue(entry, runtime))
    if (value === null || typeof value !== "object") return value
    if (Object.keys(value).length === 1 && typeof value.$ref === "string") return resolveCaseReference(value.$ref, runtime)
    if (Object.keys(value).length === 1 && value.$collect !== undefined) {
        const collection = requireObject(value.$collect, "$collect")
        if (typeof collection.from !== "string" || typeof collection.field !== "string") {
            throw new DifferentialInputError("$collect requires string from and field values")
        }
        const entries = resolveCaseReference(collection.from, runtime)
        if (!Array.isArray(entries)) throw new DifferentialInputError("$collect source must resolve to an array")
        return entries.map((entry) => readDifferentialField(entry, collection.field))
    }
    return Object.fromEntries(Object.entries(value).map(([key, entry]) => [key, resolveRuntimeValue(entry, runtime)]))
}

function recordResponse(caseSpec, decoded, runtime) {
    if (!decoded.resolved) return []
    runtime.responses.set(caseSpec.id, decoded.value)
    collectDifferentialVariables(decoded.value, runtime.variables)
    const issues = []
    for (const [name, fieldPath] of Object.entries(caseSpec.capture ?? {})) {
        try {
            runtime.variables[name] = readDifferentialField(decoded.value, String(fieldPath))
        } catch (error) {
            issues.push(error.message)
        }
    }
    return issues
}
// //// /解析跨 case 动态引用 ////

// //// 编码并发送同形 HTTP 请求 [@x380kkm 2026-08-23] ////
function setDefaultHeader(headers, name, value) {
    if (!Object.keys(headers).some((key) => key.toLowerCase() === name.toLowerCase())) headers[name] = value
}

function encodeRequestBody(spec, headers, runtime) {
    if (spec.bodyBase64 !== undefined) return Buffer.from(String(resolveRuntimeValue(spec.bodyBase64, runtime)), "base64")
    const resolvedBody = resolveRuntimeValue(spec.body, runtime)
    const body = resolvedBody !== null && typeof resolvedBody === "object" && !Array.isArray(resolvedBody)
        && resolvedBody.device_id === runtime.originalDeviceId
        ? { ...resolvedBody, device_id: runtime.variables.device_id }
        : resolvedBody
    if (body !== undefined && spec.encoding === undefined) {
        throw new DifferentialInputError("request body requires explicit encoding")
    }
    const encoding = String(spec.encoding ?? "none").toLowerCase()
    if (encoding === "none") return undefined
    if (encoding === "json") {
        setDefaultHeader(headers, "content-type", "application/json")
        return JSON.stringify(body)
    }
    if (["messagepack", "base64-messagepack", "msgpack-base64"].includes(encoding)) {
        setDefaultHeader(headers, "content-type", "application/x-www-form-urlencoded")
        return Buffer.from(pack(body ?? {})).toString("base64")
    }
    if (["raw-messagepack", "msgpack"].includes(encoding)) {
        setDefaultHeader(headers, "content-type", "application/x-msgpack")
        return Buffer.from(pack(body ?? {}))
    }
    if (encoding === "form") {
        setDefaultHeader(headers, "content-type", "application/x-www-form-urlencoded")
        if (typeof body === "string") return body
        return new URLSearchParams(Object.entries(body ?? {}).map(([key, entry]) => [key, String(entry)])).toString()
    }
    if (encoding === "base64") return Buffer.from(String(body ?? ""), "base64")
    if (encoding === "text") return String(body ?? "")
    throw new DifferentialInputError(`unsupported request encoding ${encoding}`)
}

function appendQuery(requestPath, query) {
    const target = new URL(requestPath, "http://differential.invalid")
    for (const [name, rawValue] of Object.entries(query)) {
        for (const value of Array.isArray(rawValue) ? rawValue : [rawValue]) target.searchParams.append(name, value === null ? "" : String(value))
    }
    return `${target.pathname}${target.search}`
}

function prepareRequest(spec, defaults, runtime) {
    const resolvedPath = resolveRuntimeValue(spec.path, runtime)
    if (typeof resolvedPath !== "string") throw new DifferentialInputError("request path must resolve to a string")
    const query = resolveRuntimeValue({ ...(defaults.query ?? {}), ...(spec.query ?? {}) }, runtime)
    const headers = resolveRuntimeValue({ ...(defaults.headers ?? {}), ...(spec.headers ?? {}) }, runtime)
    for (const [name, value] of Object.entries(headers)) headers[name] = String(value)
    return {
        method: String(spec.method ?? defaults.method ?? "GET").toUpperCase(),
        path: appendQuery(resolvedPath, query),
        headers,
        body: encodeRequestBody({ ...defaults, ...spec }, headers, runtime),
        responseEncoding: spec.responseEncoding ?? defaults.responseEncoding ?? "auto",
    }
}

async function sendRequest(baseUrl, request, timeoutMs) {
    const controller = new AbortController()
    const timeout = setTimeout(() => controller.abort(), timeoutMs)
    try {
        const response = await fetch(new URL(request.path, `${baseUrl}/`), {
            method: request.method, headers: request.headers, body: request.body, redirect: "manual", signal: controller.signal,
        })
        return {
            ok: true,
            status: response.status,
            contentType: (response.headers.get("content-type") ?? "").split(";", 1)[0].trim().toLowerCase(),
            location: response.headers.get("location"),
            body: Buffer.from(await response.arrayBuffer()),
        }
    } catch (error) {
        return { ok: false, error: { name: error.name, message: error.message } }
    } finally {
        clearTimeout(timeout)
    }
}
// //// /编码并发送同形 HTTP 请求 ////

// //// 执行前置请求和状态探针 [@x380kkm 2026-08-23] ////
async function runRequestPair(spec, execution) {
    let referenceRequest
    let rustRequest
    try {
        const referenceSpec = spec.reference === undefined
            ? spec
            : { ...spec, ...spec.reference, reference: undefined, rust: undefined }
        const rustSpec = spec.rust === undefined
            ? spec
            : { ...spec, ...spec.rust, reference: undefined, rust: undefined }
        referenceRequest = prepareRequest(referenceSpec, execution.defaults, execution.referenceRuntime)
        rustRequest = prepareRequest(rustSpec, execution.defaults, execution.rustRuntime)
    } catch (error) {
        return {
            public: { status: "unresolved", differences: [], unresolvedReasons: [error.message], reference: null, rust: null },
            referenceDecoded: { resolved: false, error: error.message }, rustDecoded: { resolved: false, error: error.message },
        }
    }
    const [referenceResponse, rustResponse] = await Promise.all([
        sendRequest(execution.referenceBaseUrl, referenceRequest, spec.timeoutMs ?? execution.timeoutMs),
        sendRequest(execution.rustBaseUrl, rustRequest, spec.timeoutMs ?? execution.timeoutMs),
    ])
    const referenceDecoded = decodeDifferentialResponse(referenceResponse, referenceRequest.responseEncoding)
    const rustDecoded = decodeDifferentialResponse(rustResponse, rustRequest.responseEncoding)
    const comparison = compareDifferentialResponses(
        referenceResponse,
        rustResponse,
        referenceDecoded,
        rustDecoded,
        execution.normalization,
        { reference: execution.referenceBaseUrl, rust: execution.rustBaseUrl },
        mergeComparison(execution.defaults.comparison, spec.comparison),
    )
    const expectedStatus = spec.branch?.status ?? spec.expectedStatus
    if (expectedStatus !== undefined && referenceResponse.ok && referenceResponse.status !== expectedStatus) {
        comparison.unresolvedReasons.push(`reference returned ${referenceResponse.status}; corpus branch expects ${expectedStatus}`)
        if (comparison.status === "matched") comparison.status = "unresolved"
    }
    return {
        public: {
            request: { method: referenceRequest.method, path: spec.path },
            ...comparison,
        },
        referenceDecoded,
        rustDecoded,
    }
}

async function runSideRequest(spec, baseUrl, runtime, execution) {
    let prepared
    try {
        prepared = prepareRequest(spec, execution.defaults, runtime)
    } catch (error) {
        return {
            status: "unresolved",
            request: { method: spec.method ?? null, path: spec.path ?? null },
            unresolvedReasons: [error.message],
            captureIssues: [],
            response: null,
        }
    }
    const response = await sendRequest(baseUrl, prepared, spec.timeoutMs ?? execution.timeoutMs)
    if (!response.ok) {
        return {
            status: "unresolved",
            request: { method: prepared.method, path: spec.path },
            unresolvedReasons: [`transport: ${response.error.name}: ${response.error.message}`],
            captureIssues: [],
            response: null,
        }
    }
    const decoded = decodeDifferentialResponse(response, prepared.responseEncoding)
    const expectedStatus = spec.branch?.status ?? spec.expectedStatus
    const statusMatched = expectedStatus === undefined
        ? response.status >= 200 && response.status < 300
        : response.status === expectedStatus
    const captureIssues = decoded.resolved && typeof spec.id === "string"
        ? recordResponse(spec, decoded, runtime)
        : []
    const unresolvedReasons = []
    if (!statusMatched) {
        unresolvedReasons.push(expectedStatus === undefined
            ? `returned non-success status ${response.status}`
            : `returned ${response.status}; expected ${expectedStatus}`)
    }
    if (spec.expectedLocationIncludes !== undefined
        && !response.location?.includes(spec.expectedLocationIncludes)) {
        unresolvedReasons.push(`redirect location does not include ${spec.expectedLocationIncludes}`)
    }
    if (!decoded.resolved && (spec.capture !== undefined || spec.requireDecodedResponse === true)) {
        unresolvedReasons.push(`decode: ${decoded.error}`)
    }
    return {
        status: unresolvedReasons.length === 0 && captureIssues.length === 0 ? "matched" : "unresolved",
        request: { method: prepared.method, path: spec.path },
        expectedStatus: expectedStatus ?? null,
        unresolvedReasons,
        captureIssues,
        response: {
            status: response.status,
            contentType: response.contentType,
            location: response.location,
            encoding: decoded.resolved ? decoded.encoding : null,
            decoded: decoded.resolved,
            decodeError: decoded.resolved ? null : decoded.error,
        },
    }
}

async function runSideRequests(requests, execution) {
    const results = []
    for (const request of requests) {
        const referenceSpec = request.reference === null
            ? null
            : { ...request, ...request.reference, reference: undefined, rust: undefined }
        const rustSpec = request.rust === null
            ? null
            : { ...request, ...request.rust, reference: undefined, rust: undefined }
        const [reference, rust] = await Promise.all([
            referenceSpec === null
                ? null
                : runSideRequest(referenceSpec, execution.referenceBaseUrl, execution.referenceRuntime, execution),
            rustSpec === null
                ? null
                : runSideRequest(rustSpec, execution.rustBaseUrl, execution.rustRuntime, execution),
        ])
        const compared = [reference, rust].filter((entry) => entry !== null)
        results.push({
            id: request.id,
            status: compared.every((entry) => entry.status === "matched") ? "matched" : "unresolved",
            reference,
            rust,
        })
    }
    return results
}

async function runRequestList(requests, execution, recordResponses = true) {
    const results = []
    for (const request of requests) {
        const result = await runRequestPair(request, execution)
        if (recordResponses && typeof request.id === "string" && request.id.length > 0) {
            result.public.captureIssues = [
                ...recordResponse(request, result.referenceDecoded, execution.referenceRuntime).map((message) => `reference capture: ${message}`),
                ...recordResponse(request, result.rustDecoded, execution.rustRuntime).map((message) => `rust capture: ${message}`),
            ]
        }
        results.push(result)
    }
    return results
}

function aggregateStatus(entries) {
    const statuses = entries.filter(Boolean).map((entry) => entry.status)
    if (statuses.includes("mismatched")) return "mismatched"
    if (statuses.includes("unresolved")) return "unresolved"
    return "matched"
}

function matchesLocalExtension(route, primary, compared) {
    const extension = route.localExtension
    if (extension === undefined || primary.status !== "mismatched") return false
    if (compared.some((entry) => entry !== primary && entry?.status !== "matched")) return false
    const sidesMatch = ["reference", "rust"].every((side) => {
        const expected = extension[side]
        const actual = primary[side]
        return actual?.status === expected.status && actual?.contentType === expected.contentType
    })
    if (!sidesMatch) return false
    const expectedFields = [
        extension.reference.status === extension.rust.status ? null : "status",
        extension.reference.contentType === extension.rust.contentType ? null : "contentType",
    ].filter(Boolean).sort()
    const actualFields = (primary.differences ?? []).map((difference) => difference.field).sort()
    return actualFields.length === expectedFields.length &&
        actualFields.every((field, index) => field === expectedFields[index])
}

function createSkippedCase(route, reason) {
    return {
        id: route.id,
        method: route.method,
        path: route.path,
        status: "unresolved",
        dependencySatisfied: false,
        unresolvedReasons: [reason],
        branch: route.branch ?? null,
    }
}

function applyStateExpectation(transition, expectation) {
    if (expectation === undefined || transition.status === "unresolved") return { ...transition, expectation: expectation ?? null }
    const referenceChanged = transition.referenceChanges.length > 0
    const satisfied = expectation === "changed" ? referenceChanged : !referenceChanged
    if (satisfied) return { ...transition, expectation, referenceChanged }
    return {
        ...transition,
        status: "unresolved",
        expectation,
        referenceChanged,
        unresolvedReasons: [`reference state was ${referenceChanged ? "changed" : "unchanged"}; expected ${expectation}`],
    }
}

function applyStateComparison(transition, mode) {
    if (mode !== "change-presence" || transition.status === "unresolved") return transition
    const referenceChanged = transition.referenceChanges.length > 0
    const rustChanged = transition.rustChanges.length > 0
    return {
        ...transition,
        status: referenceChanged === rustChanged ? "matched" : "mismatched",
        differences: referenceChanged === rustChanged ? [] : [{
            path: "$.changed",
            reference: referenceChanged,
            rust: rustChanged,
        }],
        comparison: mode,
        referenceChanged,
        rustChanged,
    }
}

function canSatisfyDependencies(route, primary, captureIssues, state) {
    if (route.branch?.kind === "error" || captureIssues.length > 0) return false
    const expectedStatus = route.branch?.status ?? route.expectedStatus
    if (expectedStatus === undefined) return false
    if (primary.reference?.status !== expectedStatus || primary.rust?.status !== expectedStatus) return false
    if (primary.reference.decoded !== true || primary.rust.decoded !== true) return false
    return state.every((probe) => probe.transition.status !== "unresolved")
}

async function runCase(route, execution, completedIds) {
    if (!Array.isArray(route.dependsOn)) return createSkippedCase(route, "dependsOn must be an array")
    const missing = route.dependsOn.filter((caseId) => !completedIds.has(caseId))
    if (missing.length > 0) return createSkippedCase(route, `dependencies were not completed: ${missing.join(", ")}`)
    if (route.skip === true || route.branch?.executable === false || route.branch?.kind === "unresolved") {
        return createSkippedCase(route, route.branch?.reason ?? route.reason ?? "case is marked as non-executable")
    }
    execution.normalization = mergeDifferentialNormalization(execution.defaults.normalize, route.normalize)
    const sideRequests = await runSideRequests(route.sideRequests, execution)
    const prerequisites = await runRequestList(route.prerequisites, execution)
    const before = await runRequestList(route.probes.before, execution)
    const stateWarmup = await runRequestList(route.probes.state, execution, false)
    const stateBefore = await runRequestList(route.probes.state, execution)
    const primary = await runRequestPair(route, execution)
    const captureIssues = [
        ...recordResponse(route, primary.referenceDecoded, execution.referenceRuntime).map((message) => `reference capture: ${message}`),
        ...recordResponse(route, primary.rustDecoded, execution.rustRuntime).map((message) => `rust capture: ${message}`),
    ]
    const after = await runRequestList(route.probes.after, execution)
    const stateAfter = await runRequestList(route.probes.state, execution)
    const state = stateBefore.map((entry, index) => {
        const probe = route.probes.state[index]
        const expectation = probe.stateExpectation
            ?? (probe.expectsReferenceStateChange === true ? "changed" : undefined)
            ?? route.stateExpectation
            ?? (route.expectsReferenceStateChange === true || route.branch?.requiresStateChange === true ? "changed" : undefined)
        const stateNormalization = mergeDifferentialNormalization(execution.normalization, {
            ignorePaths: [...(route.stateIgnorePaths ?? []), ...(probe.stateIgnorePaths ?? [])],
        })
        const transition = compareDifferentialStateTransition(
            entry,
            stateAfter[index],
            stateNormalization,
            { reference: execution.referenceBaseUrl, rust: execution.rustBaseUrl },
            mergeComparison(route.stateProjection, probe.stateProjection),
        )
        const expectedTransition = applyStateExpectation(transition, expectation)
        const comparedTransition = applyStateComparison(
            expectedTransition,
            probe.stateComparison ?? route.stateComparison ?? "exact",
        )
        return { before: entry.public, after: stateAfter[index].public, transition: comparedTransition }
    })
    const compared = [
        ...sideRequests,
        ...prerequisites.filter((entry) => entry.public.status === "unresolved").map((entry) => entry.public),
        ...before.map((entry) => entry.public), primary.public,
        ...after.map((entry) => entry.public), ...state.map((entry) => entry.transition),
    ]
    compared.push(...stateWarmup.filter((entry) => entry.public.status === "unresolved").map((entry) => entry.public))
    if (captureIssues.length > 0) compared.push({ status: "unresolved" })
    const missingRequiredStateProbe = (route.expectsReferenceStateChange === true || route.branch?.requiresStateChange === true)
        && state.length === 0
    if (missingRequiredStateProbe) compared.push({ status: "unresolved" })
    const aggregate = aggregateStatus(compared)
    const localExtension = aggregate === "mismatched" && matchesLocalExtension(route, primary.public, compared)
        ? route.localExtension
        : null
    const status = localExtension === null ? aggregate : "local-extension"
    const sideRequestsSatisfied = sideRequests.every((entry) => entry.status === "matched")
    return {
        id: route.id, method: route.method, path: route.path, status, branch: route.branch ?? null,
        dependencySatisfied: sideRequestsSatisfied && !missingRequiredStateProbe
            && canSatisfyDependencies(route, primary.public, captureIssues, state),
        dependsOn: route.dependsOn, captureIssues, sideRequests, localExtension,
        prerequisites: prerequisites.map((entry) => entry.public), primary: primary.public,
        probes: { before: before.map((entry) => entry.public), after: after.map((entry) => entry.public), state },
    }
}

function orderCases(cases, initiallyCompleted = []) {
    const orderedCases = []
    const remainingCases = [...cases]
    const scheduledIds = new Set(initiallyCompleted)
    while (remainingCases.length > 0) {
        const nextIndex = remainingCases.findIndex((route) => Array.isArray(route.dependsOn)
            && route.dependsOn.every((caseId) => scheduledIds.has(caseId)))
        if (nextIndex < 0) {
            orderedCases.push(...remainingCases)
            break
        }
        const [route] = remainingCases.splice(nextIndex, 1)
        orderedCases.push(route)
        scheduledIds.add(route.id)
    }
    return orderedCases
}

function deriveIsolationGroups(cases, bootstrapId) {
    const routes = cases.filter((route) => route.id !== bootstrapId)
    const routeIds = new Set(routes.map((route) => route.id))
    const links = new Map(routes.map((route) => [route.id, new Set()]))
    for (const route of routes) {
        for (const dependency of route.dependsOn) {
            if (dependency === bootstrapId || !routeIds.has(dependency)) continue
            links.get(route.id).add(dependency)
            links.get(dependency).add(route.id)
        }
    }
    const explicitGroups = new Map()
    for (const route of routes.filter((entry) => typeof entry.isolationGroup === "string")) {
        if (!explicitGroups.has(route.isolationGroup)) explicitGroups.set(route.isolationGroup, [])
        explicitGroups.get(route.isolationGroup).push(route.id)
    }
    for (const group of explicitGroups.values()) {
        for (const routeId of group.slice(1)) {
            links.get(group[0]).add(routeId)
            links.get(routeId).add(group[0])
        }
    }
    const routeById = new Map(routes.map((route) => [route.id, route]))
    const visited = new Set()
    const groups = []
    for (const route of routes) {
        if (visited.has(route.id)) continue
        const pending = [route.id]
        const groupIds = []
        visited.add(route.id)
        while (pending.length > 0) {
            const routeId = pending.shift()
            groupIds.push(routeId)
            for (const linkedId of links.get(routeId)) {
                if (visited.has(linkedId)) continue
                visited.add(linkedId)
                pending.push(linkedId)
            }
        }
        groups.push(routes.filter((entry) => groupIds.includes(entry.id)).map((entry) => routeById.get(entry.id)))
    }
    return groups
}

function createExecution(options, corpus, deviceId) {
    const variables = structuredClone(corpus.variables)
    variables.device_id = deviceId
    variables.play_id = `${corpus.variables.play_id ?? "cn-reference-differential-play"}-${deviceId}`
    return {
        referenceBaseUrl: options.referenceBaseUrl,
        rustBaseUrl: options.rustBaseUrl,
        timeoutMs: options.timeoutMs,
        defaults: corpus.defaults,
        normalization: mergeDifferentialNormalization(corpus.defaults.normalize, {}),
        referenceRuntime: { responses: new Map(), variables: structuredClone(variables), originalDeviceId: corpus.variables.device_id },
        rustRuntime: { responses: new Map(), variables: structuredClone(variables), originalDeviceId: corpus.variables.device_id },
    }
}

function getDeviceIdForGroup(corpus, accountSeed, groupIndex) {
    const base = Number(corpus.variables.device_id ?? 1_000_000)
    const deviceId = base + accountSeed * 1_000 + groupIndex
    if (!Number.isSafeInteger(deviceId)) throw new DifferentialInputError("isolated account device_id exceeds the safe integer range")
    return deviceId
}

async function bootstrapAccount(bootstrapCase, deviceId, execution) {
    const request = structuredClone(bootstrapCase)
    request.body = { ...(request.body ?? {}), device_id: deviceId }
    if (request.headers !== undefined && Object.hasOwn(request.headers, "udid")) request.headers.udid = `differential-${deviceId}`
    const result = await runRequestPair(request, execution)
    const captureIssues = [
        ...recordResponse(bootstrapCase, result.referenceDecoded, execution.referenceRuntime).map((message) => `reference capture: ${message}`),
        ...recordResponse(bootstrapCase, result.rustDecoded, execution.rustRuntime).map((message) => `rust capture: ${message}`),
    ]
    return { ...result, captureIssues }
}

async function runDifferential(options, corpus) {
    const bootstrapCase = corpus.cases.find((route) => route.id === "tool-signup")
        ?? corpus.cases.find((route) => route.path === "/api/index.php/tool/signup")
    if (bootstrapCase === undefined) throw new DifferentialInputError("corpus requires a tool signup case for account isolation")
    const accountSeed = options.accountSeed ?? Date.now() % 1_000_000_000
    const groups = deriveIsolationGroups(corpus.cases, bootstrapCase.id)
    const results = new Map()

    const bootstrapExecution = createExecution(options, corpus, getDeviceIdForGroup(corpus, accountSeed, 0))
    const bootstrapResult = await runCase(bootstrapCase, bootstrapExecution, new Set())
    bootstrapResult.isolationGroup = bootstrapCase.id
    results.set(bootstrapCase.id, bootstrapResult)

    for (let groupIndex = 0; groupIndex < groups.length; groupIndex += 1) {
        const deviceId = getDeviceIdForGroup(corpus, accountSeed, groupIndex + 1)
        const execution = createExecution(options, corpus, deviceId)
        const setup = await bootstrapAccount(bootstrapCase, deviceId, execution)
        const setupSatisfied = setup.public.status === "matched" && setup.captureIssues.length === 0
        const completedIds = new Set(setupSatisfied ? [bootstrapCase.id] : [])
        const group = orderCases(groups[groupIndex], completedIds)
        for (const route of group) {
            const result = await runCase(route, execution, completedIds)
            result.isolationGroup = route.isolationGroup ?? group[0].id
            result.accountSetup = { ...setup.public, captureIssues: setup.captureIssues }
            if (!setupSatisfied && result.status === "matched") {
                result.status = "unresolved"
                result.dependencySatisfied = false
            }
            results.set(route.id, result)
            if (result.dependencySatisfied) completedIds.add(route.id)
        }
    }
    const routes = corpus.cases.map((route) => results.get(route.id))
    return {
        version: 1, corpusVersion: corpus.version, referenceBaseUrl: options.referenceBaseUrl, rustBaseUrl: options.rustBaseUrl,
        accountIsolation: { seed: accountSeed, groups: groups.length + 1 },
        summary: {
            total: routes.length,
            matched: routes.filter((route) => route.status === "matched").length,
            localExtensions: routes.filter((route) => route.status === "local-extension").length,
            mismatched: routes.filter((route) => route.status === "mismatched").length,
            unresolved: routes.filter((route) => route.status === "unresolved").length,
        },
        routes,
    }
}
// //// /执行前置请求和状态探针 ////

// //// 自检双服务重放和动态引用 [@x380kkm 2026-08-23] ////
async function listenSelfTestServer(viewerId, serverTime) {
    let counter = 0
    const server = createServer(async (request, response) => {
        const target = new URL(request.url, "http://127.0.0.1")
        const chunks = []
        for await (const chunk of request) chunks.push(chunk)
        if (target.pathname === "/signup") {
            const signup = unpack(Buffer.from(Buffer.concat(chunks).toString("ascii"), "base64"))
            assert.equal(Number.isSafeInteger(signup.device_id), true)
            response.writeHead(200, { "content-type": "application/x-msgpack" })
            response.end(Buffer.from(pack({ data_headers: { result_code: 1, viewer_id: viewerId, server_time: serverTime }, data: { newAccount: 1 } })).toString("base64"))
        } else if (target.pathname === "/comic") {
            response.writeHead(200, { "content-type": "application/json" })
            response.end(JSON.stringify({ data: {
                viewer_id: Number(target.searchParams.get("viewer_id")),
                counter: Number(target.searchParams.get("counter")),
            } }))
        } else if (target.pathname === "/state") {
            response.writeHead(200, { "content-type": "application/json" })
            response.end(JSON.stringify({ data: { counter } }))
        } else if (target.pathname === "/increment") {
            counter += 1
            response.writeHead(200, { "content-type": "application/json" })
            response.end(JSON.stringify({ data: { counter } }))
        } else if (target.pathname === "/variant-increment") {
            counter += viewerId === 123 ? 1 : 2
            response.writeHead(200, { "content-type": "application/json" })
            response.end(JSON.stringify({ data: { ok: true } }))
        } else if (target.pathname === "/noop") {
            response.writeHead(200, { "content-type": "application/json" })
            response.end(JSON.stringify({ data: { ok: true } }))
        } else if (target.pathname === "/projection-text") {
            response.writeHead(200, { "content-type": "text/plain; charset=utf-8" })
            response.end(`${viewerId === 123 ? "reference" : "rust"} heading\n{"data":{"stable":1}}`)
        } else if (target.pathname === "/projection-map") {
            const records = viewerId === 123
                ? { referenceA: { id: 1 }, referenceB: { id: 2 } }
                : { rustA: { id: 1 }, rustB: { id: 2 } }
            response.writeHead(200, { "content-type": "application/json" })
            response.end(JSON.stringify({ data: { records } }))
        } else if (target.pathname === "/projection-filter") {
            response.writeHead(200, { "content-type": "application/json" })
            response.end(JSON.stringify({ data: { archive: [
                { device: 1, location: "common", size: 7 },
                { device: 2, location: viewerId === 123 ? "reference" : "rust", size: viewerId },
            ] } }))
        } else if (target.pathname === "/projection-values") {
            response.writeHead(200, { "content-type": "application/json" })
            response.end(JSON.stringify({ data: { stable: 1, serverSpecific: viewerId } }))
        } else if (target.pathname === "/projection-shape") {
            response.writeHead(200, { "content-type": "application/json" })
            response.end(JSON.stringify({ data: {
                stable: 1,
                policy: viewerId === 123 ? null : { completed: true },
            } }))
        } else if (target.pathname === "/local-extension") {
            response.writeHead(viewerId === 123 ? 404 : 200, {
                "content-type": viewerId === 123 ? "application/json" : "image/png",
            })
            response.end(viewerId === 123 ? JSON.stringify({ error: "missing" }) : "png")
        } else if (target.pathname === "/side-reference") {
            response.writeHead(302, { location: "/mail?ok=1" })
            response.end()
        } else if (target.pathname === "/side-rust") {
            response.writeHead(201, { "content-type": "application/json" })
            response.end(JSON.stringify({ data: { grant: viewerId } }))
        } else {
            response.writeHead(404, { "content-type": "application/json" })
            response.end(JSON.stringify({ error: "not found" }))
        }
    })
    await new Promise((resolve, reject) => {
        server.once("error", reject)
        server.listen(0, "127.0.0.1", resolve)
    })
    return {
        baseUrl: `http://127.0.0.1:${server.address().port}`,
        close: () => new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve())),
    }
}

async function runSelfTest() {
    const referenceServer = await listenSelfTestServer(123, 1_800_000_000)
    const rustServer = await listenSelfTestServer(987, 1_800_000_005)
    try {
        const report = await runDifferential({
            referenceBaseUrl: referenceServer.baseUrl,
            rustBaseUrl: rustServer.baseUrl,
            timeoutMs: 2_000,
            accountSeed: 1,
        }, parseCorpus({
            variables: { device_id: 1, play_id: "selftest" },
            cases: [
                { id: "tool-signup", method: "POST", path: "/signup", encoding: "messagepack", body: { device_id: 1 } },
                {
                    id: "comic",
                    method: "GET",
                    path: "/comic",
                    query: { viewer_id: { $ref: "tool-signup.data_headers.viewer_id" }, counter: "${counter}" },
                    prerequisites: [{
                        id: "setup-state",
                        method: "GET",
                        path: "/state",
                        encoding: "none",
                        capture: { counter: "$.data.counter" },
                    }],
                    dependsOn: ["tool-signup"],
                },
                {
                    id: "increment",
                    method: "POST",
                    path: "/increment",
                    encoding: "none",
                    expectsReferenceStateChange: true,
                    probes: { state: { method: "GET", path: "/state", encoding: "none" } },
                },
                {
                    id: "projection-text",
                    method: "GET",
                    path: "/projection-text",
                    comparison: { text: { stripFirstLines: 1, parseJson: true } },
                },
                {
                    id: "projection-map",
                    method: "GET",
                    path: "/projection-map",
                    comparison: { mapValues: ["$.data.records"] },
                },
                {
                    id: "projection-filter",
                    method: "GET",
                    path: "/projection-filter",
                    comparison: {
                        arrayFilters: [{ path: "$.data.archive", where: { field: "device", in: [1] } }],
                    },
                },
                {
                    id: "projection-values",
                    method: "GET",
                    path: "/projection-values",
                    comparison: { valuePaths: ["$.data.stable"] },
                },
                {
                    id: "state-change-presence",
                    method: "POST",
                    path: "/variant-increment",
                    encoding: "none",
                    stateExpectation: "changed",
                    stateComparison: "change-presence",
                    probes: { state: { method: "GET", path: "/state", encoding: "none" } },
                },
                {
                    id: "projection-shape",
                    method: "GET",
                    path: "/projection-shape",
                    comparison: { ignorePaths: ["$.data.policy"] },
                },
                {
                    id: "side-setup",
                    method: "POST",
                    path: "/noop",
                    encoding: "none",
                    sideRequests: [{
                        id: "grant",
                        reference: {
                            method: "POST", path: "/side-reference", encoding: "none",
                            expectedStatus: 302, expectedLocationIncludes: "/mail?ok=",
                        },
                        rust: { method: "POST", path: "/side-rust", encoding: "none", expectedStatus: 201 },
                    }],
                },
                {
                    id: "local-extension",
                    method: "GET",
                    path: "/local-extension",
                    localExtension: {
                        reason: "self-test extension",
                        reference: { status: 404, contentType: "application/json" },
                        rust: { status: 200, contentType: "image/png" },
                    },
                },
            ],
        }))
        assert.deepEqual(report.summary, {
            total: 11, matched: 10, localExtensions: 1, mismatched: 0, unresolved: 0,
        })
        assert.deepEqual(report.accountIsolation.groups, 11)
        assert.equal(report.routes.find((route) => route.id === "side-setup").sideRequests[0].status, "matched")
        assert.equal(report.routes.find((route) => route.id === "local-extension").status, "local-extension")
        const extensionCase = report.routes.find((route) => route.id === "local-extension")
        assert.equal(matchesLocalExtension(
            { localExtension: extensionCase.localExtension },
            {
                ...extensionCase.primary,
                differences: [...extensionCase.primary.differences, { field: "dataValue" }],
            },
            [],
        ), false)
        const projectedState = compareDifferentialStateTransition(
            {
                referenceDecoded: { resolved: true, value: { data: { stable: 1, reward: "reference-a" } } },
                rustDecoded: { resolved: true, value: { data: { stable: 1, reward: "rust-a" } } },
            },
            {
                referenceDecoded: { resolved: true, value: { data: { stable: 2, reward: "reference-b" } } },
                rustDecoded: { resolved: true, value: { data: { stable: 2, reward: "rust-b" } } },
            },
            {},
            { reference: referenceServer.baseUrl, rust: rustServer.baseUrl },
            { valuePaths: ["$.data.stable"] },
        )
        assert.equal(projectedState.status, "matched")
        const unresolvedReport = await runDifferential({
            referenceBaseUrl: referenceServer.baseUrl,
            rustBaseUrl: rustServer.baseUrl,
            timeoutMs: 2_000,
            accountSeed: 2,
        }, parseCorpus({
            variables: { device_id: 1, play_id: "selftest" },
            cases: [
                { id: "tool-signup", method: "POST", path: "/signup", encoding: "messagepack", body: { device_id: 1 } },
                {
                    id: "noop",
                    method: "POST",
                    path: "/noop",
                    encoding: "none",
                    expectsReferenceStateChange: true,
                    probes: { state: { method: "GET", path: "/state", encoding: "none" } },
                },
            ],
        }))
        assert.deepEqual(unresolvedReport.summary, {
            total: 2, matched: 1, localExtensions: 0, mismatched: 0, unresolved: 1,
        })
        assert.throws(() => parseCorpus({ cases: [{
            id: "invalid-comparison",
            method: "GET",
            path: "/projection-values",
            comparison: { valuePaths: "$.data.stable" },
        }] }), /valuePaths must be an array/)
        assert.throws(() => parseCorpus({ cases: [{
            id: "invalid-state-comparison",
            method: "GET",
            path: "/state",
            stateComparison: "loose",
        }] }), /stateComparison must be exact or change-presence/)
    } finally {
        await Promise.all([referenceServer.close(), rustServer.close()])
    }
    return {
        status: "matched",
        checks: [
            "json", "base64-messagepack", "raw-messagepack", "normalization", "query",
            "reference", "prerequisite-capture", "account-isolation", "state-probe", "state-expectation",
            "text-projection", "map-values", "array-filter", "value-paths", "comparison-schema",
            "state-change-presence",
            "side-specific-setup",
            "shape-ignore-path",
            "local-extension",
            "state-value-paths",
        ],
    }
}
// //// /自检双服务重放和动态引用 ////

// //// 写入动态差分报告并返回进程状态 [@x380kkm 2026-08-23] ////
async function main(args = process.argv.slice(2)) {
    const options = parseArguments(args)
    if (options.help) {
        process.stdout.write(USAGE)
        return
    }
    const corpus = options.corpusPath === undefined ? null : readCorpus(options.corpusPath)
    const report = options.selftest
        ? await runSelfTest()
        : options.checkcorpus
            ? { status: "matched", corpusVersion: corpus.version, routes: corpus.cases.length }
            : await runDifferential(options, corpus)
    const output = `${JSON.stringify(report, null, 2)}\n`
    let displayedReport = report
    if (options.reportPath !== undefined) {
        const reportPath = path.resolve(options.reportPath)
        mkdirSync(path.dirname(reportPath), { recursive: true })
        writeFileSync(reportPath, output, "utf8")
        if (!options.selftest && !options.checkcorpus) {
            displayedReport = {
                version: report.version,
                summary: report.summary,
                accountIsolation: report.accountIsolation,
                report: reportPath,
            }
        }
    }
    process.stdout.write(`${JSON.stringify(displayedReport, null, 2)}\n`)
    if (!options.selftest && !options.checkcorpus
        && (report.summary.mismatched > 0 || report.summary.unresolved > 0)) process.exitCode = 1
}

if (process.argv[1] !== undefined && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
    main().catch((error) => {
        const prefix = error instanceof DifferentialInputError ? "input" : "runtime"
        process.stderr.write(`${prefix} error: ${error.message}\n`)
        process.exitCode = 1
    })
}
// //// /写入动态差分报告并返回进程状态 ////
