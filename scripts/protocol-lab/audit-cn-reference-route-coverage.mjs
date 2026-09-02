// audience: internal
// # cn-reference-route-coverage
//
// 该脚本从显式 startpoint-cn 根解析游戏路由, SDK 入口, 中间件, 静态挂载和 TCP 行为.
// 游戏路由, SDK 入口, 中间件和 TCP 行为与个人服务 Rust 实现比较; 管理和静态表面进入同一份审计报告.
// 使用 --write-fixture 时生成 Rust 集成测试读取的同源游戏路由清单; 使用 --fail-on-unresolved 时将未解析契约作为失败条件.

import {
    existsSync,
    mkdirSync,
    readFileSync,
    readdirSync,
    writeFileSync,
} from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { buildMultiplayerDifferential } from "./audit-multiplayer-reference-differential.mjs"

const SCRIPT_ROOT = path.dirname(fileURLToPath(import.meta.url))
const REPOSITORY_ROOT = path.resolve(SCRIPT_ROOT, "../..")
const API_PLUGIN_SOURCE_PREFIX = "src/routes/api/"
const CN_TOOL_PLUGIN_SOURCE = "src/routes/cn/tool.ts"
const MULTI_REGISTER_SOURCE = "src/multi/http/register.ts"
const COMPILED_API_PLUGIN_PREFIX = "out/routes/api/"
const COMPILED_CN_PLUGIN_PREFIX = "out/routes/cn/"
const COMPILED_MULTI_ENTRY = "out/multi/index.js"
const COMPILED_WEB_ENTRY = "out/routes/web/index.js"
const COMPILED_WEB_API_ENTRY = "out/routes/web_api/index.js"
const COMPILED_SEEDS_ENTRY = "out/routes/web_api/seeds.js"

// //// 读取显式审计输入 [@x380kkm 2026-08-22] ////
function readRequiredOption(args, name) {
    const optionIndex = args.indexOf(name)
    const value = args[optionIndex + 1]
    if (optionIndex < 0 || !value || value.startsWith("--")) {
        throw new Error(`missing required option: ${name}`)
    }
    return value
}

function readOptionalOption(args, name) {
    const optionIndex = args.indexOf(name)
    if (optionIndex < 0) return null
    const value = args[optionIndex + 1]
    if (!value || value.startsWith("--")) {
        throw new Error(`missing option value: ${name}`)
    }
    return value
}

function hasOption(args, name) {
    return args.includes(name)
}
// //// /读取显式审计输入 ////

// //// 解析 TypeScript 模块绑定 [@x380kkm 2026-08-22] ////
function readSource(filePath) {
    if (!existsSync(filePath)) throw new Error(`missing source file: ${filePath}`)
    return readFileSync(filePath, "utf8")
}

function sourceWithoutComments(source) {
    return source
        .replace(/\/\*[\s\S]*?\*\//g, "")
        .replace(/^\s*\/\/.*$/gm, "")
}

function parseImports(source) {
    const imports = new Map()
    const defaultImportPattern =
        /^import\s+([A-Za-z_$][\w$]*)\s+from\s+["']([^"']+)["'];?/gm
    for (const match of source.matchAll(defaultImportPattern)) {
        imports.set(match[1], { importedName: "default", specifier: match[2] })
    }

    const namedImportPattern =
        /^import\s+\{([^}]+)\}\s+from\s+["']([^"']+)["'];?/gm
    for (const match of source.matchAll(namedImportPattern)) {
        for (const item of match[1].split(",")) {
            const names = item.trim().replace(/^type\s+/, "").split(/\s+as\s+/)
            if (!names[0]) continue
            imports.set(names[1] ?? names[0], {
                importedName: names[0],
                specifier: match[2],
            })
        }
    }
    return imports
}

function resolveTypeScriptModule(fromFile, specifier) {
    if (!specifier.startsWith(".")) {
        throw new Error(`external module cannot provide a route plugin: ${specifier}`)
    }
    const moduleBase = path.resolve(path.dirname(fromFile), specifier)
    const candidates = [`${moduleBase}.ts`, path.join(moduleBase, "index.ts")]
    const resolved = candidates.find((candidate) => existsSync(candidate))
    if (!resolved) throw new Error(`cannot resolve ${specifier} from ${fromFile}`)
    return resolved
}

function resolveExportSource(moduleFile, exportName, visited = new Set()) {
    if (exportName === "default") return moduleFile
    const visitKey = `${moduleFile}:${exportName}`
    if (visited.has(visitKey)) throw new Error(`cyclic TypeScript export: ${visitKey}`)
    visited.add(visitKey)

    const source = sourceWithoutComments(readSource(moduleFile))
    const exportPattern = /export\s+\{([^}]+)\}\s+from\s+["']([^"']+)["']/g
    for (const match of source.matchAll(exportPattern)) {
        for (const item of match[1].split(",")) {
            const names = item.trim().split(/\s+as\s+/)
            const importedName = names[0]
            const exportedName = names[1] ?? importedName
            if (exportedName !== exportName) continue
            const nextModule = resolveTypeScriptModule(moduleFile, match[2])
            return resolveExportSource(nextModule, importedName, visited)
        }
    }
    return moduleFile
}

function resolveImportedBinding(importingFile, imports, bindingName) {
    const imported = imports.get(bindingName)
    if (!imported) throw new Error(`missing import for registered plugin: ${bindingName}`)
    const moduleFile = resolveTypeScriptModule(importingFile, imported.specifier)
    return resolveExportSource(moduleFile, imported.importedName)
}
// //// /解析 TypeScript 模块绑定 ////

// //// 提取源码中可静态证明的契约字段 [@x380kkm 2026-08-23] ////
function escapeRegularExpression(value) {
    return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
}

function uniqueSorted(values) {
    return [...new Set(values)].sort()
}

function findClosingDelimiter(source, openIndex) {
    const pairs = new Map([["(", ")"], ["[", "]"], ["{", "}"]])
    const stack = []
    let quote = null
    let escaped = false
    for (let index = openIndex; index < source.length; index += 1) {
        const character = source[index]
        if (quote !== null) {
            if (escaped) escaped = false
            else if (character === "\\") escaped = true
            else if (character === quote) quote = null
            continue
        }
        if (character === '"' || character === "'" || character === "`") {
            quote = character
            continue
        }
        if (pairs.has(character)) {
            stack.push(pairs.get(character))
            continue
        }
        if (stack.at(-1) === character) {
            stack.pop()
            if (stack.length === 0) return index
        }
    }
    return -1
}

function splitTopLevel(source) {
    const parts = []
    let start = 0
    const stack = []
    let quote = null
    let escaped = false
    const pairs = new Map([["(", ")"], ["[", "]"], ["{", "}"]])
    for (let index = 0; index < source.length; index += 1) {
        const character = source[index]
        if (quote !== null) {
            if (escaped) escaped = false
            else if (character === "\\") escaped = true
            else if (character === quote) quote = null
            continue
        }
        if (character === '"' || character === "'" || character === "`") {
            quote = character
            continue
        }
        if (pairs.has(character)) stack.push(pairs.get(character))
        else if (stack.at(-1) === character) stack.pop()
        else if (character === "," && stack.length === 0) {
            parts.push(source.slice(start, index).trim())
            start = index + 1
        }
    }
    parts.push(source.slice(start).trim())
    return parts.filter(Boolean)
}

function findTopLevelColon(source) {
    const stack = []
    let quote = null
    let escaped = false
    const pairs = new Map([["(", ")"], ["[", "]"], ["{", "}"]])
    for (let index = 0; index < source.length; index += 1) {
        const character = source[index]
        if (quote !== null) {
            if (escaped) escaped = false
            else if (character === "\\") escaped = true
            else if (character === quote) quote = null
            continue
        }
        if (character === '"' || character === "'" || character === "`") {
            quote = character
            continue
        }
        if (pairs.has(character)) stack.push(pairs.get(character))
        else if (stack.at(-1) === character) stack.pop()
        else if (character === ":" && stack.length === 0) return index
    }
    return -1
}

function parseObjectProperties(expression) {
    const source = expression.trim()
    if (!source.startsWith("{")) return null
    const closeIndex = findClosingDelimiter(source, 0)
    if (closeIndex !== source.length - 1) return null
    const properties = new Map()
    for (const field of splitTopLevel(source.slice(1, -1))) {
        const colonIndex = findTopLevelColon(field)
        if (colonIndex < 0 || field.startsWith("...")) return null
        const rawKey = field.slice(0, colonIndex).trim()
        const keyMatch = rawKey.match(/^(?:["']([^"']+)["']|([A-Za-z_$][\w$]*))$/)
        if (!keyMatch) return null
        properties.set(keyMatch[1] ?? keyMatch[2], field.slice(colonIndex + 1).trim())
    }
    return properties
}

function unwrapJsonMacro(expression) {
    const source = expression.trim()
    const match = source.match(/^json!\s*\(/)
    if (!match) return source
    const openIndex = match[0].lastIndexOf("(")
    const closeIndex = findClosingDelimiter(source, openIndex)
    return closeIndex === source.length - 1 ? source.slice(openIndex + 1, closeIndex).trim() : source
}

function describeDataShape(expression) {
    if (!expression) return { dataShape: null, dataKeys: [], resolved: false }
    const source = unwrapJsonMacro(expression)
    const object = parseObjectProperties(source)
    if (object) {
        return { dataShape: "object", dataKeys: [...object.keys()].sort(), resolved: true }
    }
    if (source.startsWith("[") && findClosingDelimiter(source, 0) === source.length - 1) {
        return { dataShape: "array", dataKeys: [], resolved: true }
    }
    if (/^(?:null|true|false|-?\d+(?:\.\d+)?|["'][\s\S]*["'])$/.test(source)) {
        return { dataShape: "scalar", dataKeys: [], resolved: true }
    }
    return { dataShape: null, dataKeys: [], resolved: false }
}

function unresolvedSuccess() {
    return {
        status: null,
        contentType: null,
        envelope: [],
        dataShape: null,
        dataKeys: [],
        unresolved: ["status", "contentType", "envelope", "dataShape", "dataKeys"],
        confidence: "unresolved",
    }
}

function mergeSuccessCandidates(candidates) {
    if (candidates.length === 0) return unresolvedSuccess()
    const fields = ["status", "contentType", "envelope", "dataShape", "dataKeys"]
    const success = {}
    const unresolved = []
    for (const field of fields) {
        const values = candidates.map((candidate) => candidate[field])
        const serialized = values.map((value) => JSON.stringify(value))
        const explicitlyUnresolved = candidates.some((candidate) =>
            candidate.unresolved?.includes(field))
        if (explicitlyUnresolved || values.some((value) => value === null) ||
            new Set(serialized).size !== 1) {
            success[field] = Array.isArray(values[0]) ? [] : null
            unresolved.push(field)
        } else {
            success[field] = values[0]
        }
    }
    success.unresolved = unresolved
    success.confidence = unresolved.length === 0 ? "static" : "unresolved"
    return success
}

function mergeErrorCandidates(contracts) {
    const errors = contracts
        .flatMap((contract) => contract.errors ?? [])
        .map((error) => ({
            status: error.status,
            contentType: error.contentType,
            envelope: error.envelope ?? [],
            dataShape: error.dataShape ?? null,
            dataKeys: error.dataKeys ?? [],
        }))
    const byContract = new Map(errors.map((error) => [JSON.stringify(error), error]))
    return [...byContract.values()].sort((left, right) =>
        left.status - right.status ||
        String(left.contentType).localeCompare(String(right.contentType)))
}

function mergeContracts(contracts) {
    if (contracts.length === 0) {
        return {
            success: unresolvedSuccess(),
            errors: [],
            state: { readKeys: [], writeKeys: [], helpers: [], confidence: "unresolved" },
        }
    }
    const successContracts = contracts.map((contract) => contract.success)
    const concreteSuccessContracts = successContracts.filter((success) =>
        success.status !== null || success.contentType !== null || success.envelope.length > 0)
    const success = mergeSuccessCandidates(
        concreteSuccessContracts.length > 0 ? concreteSuccessContracts : successContracts,
    )
    const readKeys = uniqueSorted(contracts.flatMap((contract) => contract.state.readKeys))
    const writeKeys = uniqueSorted(contracts.flatMap((contract) => contract.state.writeKeys))
    const helpers = uniqueSorted(contracts.flatMap((contract) => contract.state.helpers))
    return {
        success,
        errors: mergeErrorCandidates(contracts),
        state: {
            readKeys,
            writeKeys,
            helpers,
            confidence: readKeys.length + writeKeys.length + helpers.length > 0
                ? "static"
                : "unresolved",
        },
    }
}
// //// /提取源码中可静态证明的契约字段 ////

// //// 提取 Fastify 路由契约 [@x380kkm 2026-08-23] ////
function extractJavaScriptNamedHandler(source, name) {
    const definitionPattern = new RegExp(`\\bconst\\s+${escapeRegularExpression(name)}\\s*=`)
    const definition = definitionPattern.exec(source)
    if (!definition) return null
    const tail = source.slice(definition.index)
    const generatorIndex = tail.search(/function\s*\*?\s*\([^)]*\)\s*\{/)
    const arrowIndex = tail.search(/=>\s*\{/)
    const bodyOffset = generatorIndex >= 0 ? generatorIndex : arrowIndex
    if (bodyOffset < 0) return null
    const openIndex = source.indexOf("{", definition.index + bodyOffset)
    const closeIndex = findClosingDelimiter(source, openIndex)
    return closeIndex < 0 ? null : source.slice(definition.index, closeIndex + 1)
}

function extractJavaScriptHelpers(source) {
    const helpers = []
    const compiledHelperPattern = /\(0,\s*[A-Za-z_$][\w$]*\.([A-Za-z_$][\w$]*)\)\s*\(/g
    for (const match of source.matchAll(compiledHelperPattern)) helpers.push(match[1])
    const directHelperPattern = /(?:^|[^.A-Za-z0-9_$])([A-Za-z_$][\w$]*)\s*\(/g
    const ignored = new Set([
        "catch", "for", "function", "if", "new", "return", "switch", "throw", "while",
    ])
    for (const match of source.matchAll(directHelperPattern)) {
        if (!ignored.has(match[1])) helpers.push(match[1])
    }
    return uniqueSorted(helpers)
}

function extractJavaScriptState(source) {
    const readKeys = []
    const writeKeys = []
    const propertyPattern = /\b(?:player|clientData|activeQuest)\.([A-Za-z_$][\w$]*)\b/g
    for (const match of source.matchAll(propertyPattern)) readKeys.push(match[1])
    const assignmentPattern = /\b(?:player|clientData|activeQuest)\.([A-Za-z_$][\w$]*)\s*=/g
    for (const match of source.matchAll(assignmentPattern)) writeKeys.push(match[1])
    return {
        readKeys: uniqueSorted(readKeys.filter((key) => !writeKeys.includes(key))),
        writeKeys: uniqueSorted(writeKeys),
        helpers: extractJavaScriptHelpers(source),
        confidence: "static",
    }
}

function extractJavaScriptContentType(source) {
    const values = []
    const headerPattern = /reply\s*\.\s*header\(\s*["']content-type["']\s*,\s*["']([^"']+)["']\s*\)/gi
    for (const match of source.matchAll(headerPattern)) values.push(match[1])
    const typePattern = /reply\s*\.\s*type\(\s*["']([^"']+)["']\s*\)/g
    for (const match of source.matchAll(typePattern)) values.push(match[1])
    const unique = uniqueSorted(values)
    return unique.length === 1 ? unique[0] : null
}

function describeJavaScriptResponse(source, openIndex, status, contentType) {
    const closeIndex = findClosingDelimiter(source, openIndex)
    if (closeIndex < 0) return null
    const response = parseObjectProperties(source.slice(openIndex + 1, closeIndex))
    const inferredContentType = status >= 400 ? "application/json" : contentType
    if (!response) {
        return {
            status,
            contentType: inferredContentType,
            envelope: null,
            dataShape: null,
            dataKeys: null,
        }
    }
    const data = describeDataShape(response.get("data"))
    return {
        status,
        contentType: inferredContentType,
        envelope: [...response.keys()].sort(),
        dataShape: data.resolved ? data.dataShape : null,
        dataKeys: data.resolved ? data.dataKeys : null,
    }
}

function analyzeJavaScriptContract(source) {
    if (!source) return mergeContracts([])
    const contentType = extractJavaScriptContentType(source)
    const candidates = []
    const errors = []
    const responsePattern =
        /reply(?:\s*\.\s*[A-Za-z_$][\w$]*\([^)]*\))*\s*\.\s*send\s*\(/g
    for (const match of source.matchAll(responsePattern)) {
        const openIndex = match.index + match[0].lastIndexOf("(")
        const statusMatch = match[0].match(/\.\s*(?:status|code)\(\s*(\d{3})\s*\)/)
        const response = describeJavaScriptResponse(
            source,
            openIndex,
            statusMatch ? Number(statusMatch[1]) : 200,
            contentType,
        )
        if (!response) continue
        if (response.status >= 200 && response.status < 300) candidates.push(response)
        else errors.push(response)
    }
    if (candidates.length === 0) {
        const statusMatch = source.match(/reply\s*\.\s*(?:status|code)\(\s*(2\d\d)\s*\)/)
        if (statusMatch && /\breturn\s+(?:\{|[A-Za-z_$])/.test(source)) {
            candidates.push({
                status: Number(statusMatch[1]),
                contentType,
                envelope: null,
                dataShape: null,
                dataKeys: null,
            })
        }
    }
    return {
        success: mergeSuccessCandidates(candidates),
        errors: mergeErrorCandidates([{ errors }]),
        state: extractJavaScriptState(source),
    }
}

function extractJavaScriptRouteSource(source, routeMatch, nextRouteIndex) {
    const afterRoute = source.slice(routeMatch.index + routeMatch[0].length, nextRouteIndex)
    const namedHandler = afterRoute.match(/^\s*,\s*([A-Za-z_$][\w$]*)\s*\)/)
    if (namedHandler) return extractJavaScriptNamedHandler(source, namedHandler[1])
    return source.slice(routeMatch.index, nextRouteIndex)
}
// //// /提取 Fastify 路由契约 ////

// //// 解析编译后的 CommonJS 模块绑定 [@x380kkm 2026-08-23] ////
function parseCommonJsImports(source) {
    const imports = new Map()
    const defaultPattern = /^const\s+([A-Za-z_$][\w$]*)\s*=\s*__importDefault\(require\(["']([^"']+)["']\)\);?/gm
    for (const match of source.matchAll(defaultPattern)) imports.set(match[1], match[2])

    const directPattern = /^const\s+([A-Za-z_$][\w$]*)\s*=\s*require\(["']([^"']+)["']\);?/gm
    for (const match of source.matchAll(directPattern)) imports.set(match[1], match[2])
    return imports
}

function resolveJavaScriptModule(fromFile, specifier) {
    if (!specifier.startsWith(".")) {
        throw new Error(`external module cannot provide a route plugin: ${specifier}`)
    }
    const moduleBase = path.resolve(path.dirname(fromFile), specifier)
    const candidates = [`${moduleBase}.js`, path.join(moduleBase, "index.js")]
    const resolved = candidates.find((candidate) => existsSync(candidate))
    if (!resolved) throw new Error(`cannot resolve ${specifier} from ${fromFile}`)
    return resolved
}

function parseCompiledRegistrations(serverSource, constants) {
    const registrations = []
    const expression = String.raw`(?:\`(?:\\.|[^\`])*\`|"(?:\\.|[^"])*"|'(?:\\.|[^'])*'|[A-Za-z_$][\w$]*)`
    const registrationPattern = new RegExp(
        String.raw`fastify\.register\(\s*([A-Za-z_$][\w$]*)(?:\.([A-Za-z_$][\w$]*))?\s*,\s*\{\s*prefix\s*:\s*(${expression})\s*\}\s*\)`,
        "g",
    )
    for (const match of serverSource.matchAll(registrationPattern)) {
        registrations.push({
            bindingName: match[1],
            exportName: match[2] ?? "default",
            prefix: evaluatePrefix(match[3], constants),
        })
    }
    const unprefixedPattern = /fastify\.register\(\s*([A-Za-z_$][\w$]*)(?:\.([A-Za-z_$][\w$]*))?\s*\)/g
    for (const match of serverSource.matchAll(unprefixedPattern)) {
        registrations.push({
            bindingName: match[1],
            exportName: match[2] ?? "default",
            prefix: "",
        })
    }
    return registrations
}

function collectCompiledRouteFiles(moduleFile, visited = new Set()) {
    if (visited.has(moduleFile)) return []
    visited.add(moduleFile)
    const source = sourceWithoutComments(readSource(moduleFile))
    if (/fastify\s*\.\s*(?:get|post|put|patch|delete)\(/.test(source)) return [moduleFile]

    const routeFiles = []
    for (const specifier of parseCommonJsImports(source).values()) {
        if (!specifier.startsWith(".")) continue
        const importedFile = resolveJavaScriptModule(moduleFile, specifier)
        routeFiles.push(...collectCompiledRouteFiles(importedFile, visited))
    }
    return routeFiles
}

function collectCompiledRegisteredRoutes(referenceRoot, moduleFile, prefix, visited = new Set()) {
    const visitKey = `${moduleFile}\0${prefix}`
    if (visited.has(visitKey)) return []
    visited.add(visitKey)
    const source = sourceWithoutComments(readSource(moduleFile))
    const routes = extractFastifyRoutes(referenceRoot, moduleFile, prefix)
    const imports = parseCommonJsImports(source)
    const registrations = parseCompiledRegistrations(source, parseStringConstants(source))
    for (const registration of registrations) {
        const specifier = imports.get(registration.bindingName)
        if (!specifier?.startsWith(".")) continue
        const childFile = resolveJavaScriptModule(moduleFile, specifier)
        let childPrefix = prefix
        if (registration.prefix !== "") {
            childPrefix = prefix === ""
                ? registration.prefix
                : joinRoutePath(prefix, registration.prefix)
        }
        routes.push(...collectCompiledRegisteredRoutes(
            referenceRoot,
            childFile,
            childPrefix,
            visited,
        ))
    }
    return routes
}
// //// /解析编译后的 CommonJS 模块绑定 ////

// //// 解析 cn-server 注册的服务表面 [@x380kkm 2026-08-22] ////
function normalizeRelativePath(root, filePath) {
    return path.relative(root, filePath).split(path.sep).join("/")
}

function parseStringConstants(source) {
    const constants = new Map()
    const constantPattern = /\bconst\s+([A-Za-z_$][\w$]*)\s*=\s*(["'])(.*?)\2\s*;/g
    for (const match of source.matchAll(constantPattern)) constants.set(match[1], match[3])
    return constants
}

function evaluatePrefix(expression, constants) {
    const value = expression.trim()
    if ((value.startsWith('"') && value.endsWith('"')) ||
        (value.startsWith("'") && value.endsWith("'"))) {
        return value.slice(1, -1)
    }
    if (value.startsWith("`") && value.endsWith("`")) {
        const expanded = value.slice(1, -1).replace(/\$\{([A-Za-z_$][\w$]*)\}/g, (_, name) => {
            if (!constants.has(name)) throw new Error(`unknown prefix constant: ${name}`)
            return constants.get(name)
        })
        if (expanded.includes("${")) throw new Error(`dynamic Fastify prefix: ${value}`)
        return expanded
    }
    if (!constants.has(value)) throw new Error(`unknown Fastify prefix expression: ${value}`)
    return constants.get(value)
}

function parseRegistrations(serverSource, constants) {
    const registrations = []
    const expression = String.raw`(?:\`(?:\\.|[^\`])*\`|"(?:\\.|[^"])*"|'(?:\\.|[^'])*'|[A-Za-z_$][\w$]*)`
    const registrationPattern = new RegExp(
        String.raw`fastify\.register\(\s*([A-Za-z_$][\w$]*)\s*,\s*\{\s*prefix\s*:\s*(${expression})\s*\}\s*\)`,
        "g",
    )
    for (const match of serverSource.matchAll(registrationPattern)) {
        registrations.push({ bindingName: match[1], prefix: evaluatePrefix(match[2], constants) })
    }
    return registrations
}

function parseComposedRouteFiles(registerFile) {
    const source = sourceWithoutComments(readSource(registerFile))
    const imports = parseImports(source)
    const files = []
    const registrationCallPattern = /\b([A-Za-z_$][\w$]*)\(\s*fastify\s*\)/g
    for (const match of source.matchAll(registrationCallPattern)) {
        files.push(resolveImportedBinding(registerFile, imports, match[1]))
    }
    return files
}

function joinRoutePath(prefix, routePath) {
    return `${prefix.replace(/\/+$/, "")}/${routePath.replace(/^\/+/, "")}`
}

function extractFastifyRoutes(referenceRoot, sourceFile, prefix) {
    const source = sourceWithoutComments(readSource(sourceFile))
    const constants = parseStringConstants(source)
    const routes = []
    const routePattern = /fastify\s*\.\s*(get|post|put|patch|delete)\(\s*(`(?:\\.|[^`])*`|"(?:\\.|[^"])*"|'(?:\\.|[^'])*')/g
    const matches = [...source.matchAll(routePattern)]
    for (const [index, match] of matches.entries()) {
        const routePath = evaluatePrefix(match[2], constants)
        const nextRouteIndex = matches[index + 1]?.index ?? source.length
        const handlerSource = extractJavaScriptRouteSource(source, match, nextRouteIndex)
        routes.push({
            method: match[1].toUpperCase(),
            path: prefix === "" ? routePath : joinRoutePath(prefix, routePath),
            source: normalizeRelativePath(referenceRoot, sourceFile),
            contract: analyzeJavaScriptContract(handlerSource),
        })
    }
    return routes
}

function parseJavaScriptStringArrays(source) {
    const arrays = new Map()
    const arrayPattern = /\bconst\s+([A-Za-z_$][\w$]*)\s*=\s*\[([\s\S]*?)\]\s*;/g
    for (const match of source.matchAll(arrayPattern)) {
        const values = [...match[2].matchAll(/(["'])((?:\\.|(?!\1)[\s\S])*)\1/g)]
            .map((value) => value[2])
        if (values.length > 0) arrays.set(match[1], values)
    }
    return arrays
}

function extractFastifyAllRoutes(referenceRoot, sourceFile, source) {
    const routes = []
    const sourceName = normalizeRelativePath(referenceRoot, sourceFile)
    const directPattern = /fastify\s*\.\s*all\(\s*(["'])(.*?)\1/g
    for (const match of source.matchAll(directPattern)) {
        routes.push({ method: "ALL", path: match[2], group: null, source: sourceName })
    }

    const arrays = parseJavaScriptStringArrays(source)
    const loopPattern = /for\s*\(\s*const\s+([A-Za-z_$][\w$]*)\s+of\s+([A-Za-z_$][\w$]*)\s*\)\s*(?:\{\s*)?fastify\s*\.\s*all\(\s*\1\s*,/g
    for (const match of source.matchAll(loopPattern)) {
        const paths = arrays.get(match[2])
        if (!paths) throw new Error(`missing fastify.all path array: ${match[2]}`)
        for (const routePath of paths) {
            routes.push({ method: "ALL", path: routePath, group: match[2], source: sourceName })
        }
    }

    const uniqueRoutes = new Map()
    for (const route of routes) uniqueRoutes.set(`${route.method} ${route.path}`, route)
    return [...uniqueRoutes.values()].sort((left, right) => left.path.localeCompare(right.path))
}

function extractJavaScriptFlatObject(source, name) {
    const definition = new RegExp(`\\bconst\\s+${escapeRegularExpression(name)}\\s*=`).exec(source)
    if (!definition) throw new Error(`missing JavaScript object: ${name}`)
    const openIndex = source.indexOf("{", definition.index + definition[0].length)
    if (openIndex < 0) throw new Error(`invalid JavaScript object: ${name}`)
    const closeIndex = findClosingDelimiter(source, openIndex)
    if (closeIndex < 0) throw new Error(`invalid JavaScript object: ${name}`)
    const properties = parseObjectProperties(source.slice(openIndex, closeIndex + 1))
    if (!properties) throw new Error(`dynamic JavaScript object: ${name}`)
    const constants = parseStringConstants(source)
    return Object.fromEntries([...properties].map(([key, expression]) => {
        const value = expression.trim()
        if ((value.startsWith('"') && value.endsWith('"')) ||
            (value.startsWith("'") && value.endsWith("'"))) {
            return [key, value.slice(1, -1)]
        }
        if (constants.has(value)) return [key, constants.get(value)]
        throw new Error(`dynamic JavaScript object field: ${name}.${key}`)
    }))
}

function parseMethodAgnosticContracts(source) {
    return {
        login: extractJavaScriptFlatObject(source, "iosLoginOK"),
        stub: extractJavaScriptFlatObject(source, "iosStatusOK"),
    }
}

function parseReferenceRoutes(referenceRoot) {
    const serverFile = path.join(referenceRoot, "src", "cn-server.ts")
    const serverSource = sourceWithoutComments(readSource(serverFile))
    const imports = parseImports(serverSource)
    const constants = parseStringConstants(serverSource)
    const registrations = parseRegistrations(serverSource, constants)
    const routes = []
    const sources = []

    for (const registration of registrations) {
        const pluginFile = resolveImportedBinding(
            serverFile,
            imports,
            registration.bindingName,
        )
        const relativePlugin = normalizeRelativePath(referenceRoot, pluginFile)
        const isApiPlugin = relativePlugin.startsWith(API_PLUGIN_SOURCE_PREFIX)
        const isCnToolPlugin = relativePlugin === CN_TOOL_PLUGIN_SOURCE
        const isMultiPlugin = relativePlugin === MULTI_REGISTER_SOURCE
        if (!isApiPlugin && !isCnToolPlugin && !isMultiPlugin) continue

        const routeFiles = isMultiPlugin ? parseComposedRouteFiles(pluginFile) : [pluginFile]
        for (const routeFile of routeFiles) {
            const relativeRouteFile = normalizeRelativePath(referenceRoot, routeFile)
            sources.push({
                binding: registration.bindingName,
                prefix: registration.prefix,
                source: relativeRouteFile,
            })
            routes.push(...extractFastifyRoutes(referenceRoot, routeFile, registration.prefix))
        }
    }

    const routeKeys = new Set()
    for (const route of routes) {
        const key = `${route.method} ${route.path}`
        if (routeKeys.has(key)) throw new Error(`duplicate reference route: ${key}`)
        routeKeys.add(key)
    }
    routes.sort((left, right) =>
        left.method.localeCompare(right.method) || left.path.localeCompare(right.path))
    sources.sort((left, right) => left.source.localeCompare(right.source))
    return {
        routes,
        serverFile,
        sources,
        auxiliary: {
            managementRoutes: [],
            methodAgnosticRoutes: extractFastifyAllRoutes(
                referenceRoot,
                serverFile,
                serverSource,
            ),
            methodAgnosticContracts: parseMethodAgnosticContracts(serverSource),
            middleware: null,
            staticMounts: [],
            tcp: null,
        },
    }
}

function parseCompiledStaticMounts(referenceRoot, serverFile, serverSource) {
    const mounts = []
    const registrationPattern = /fastify\s*\.\s*register\(static_\d+\.default\s*,\s*\{([\s\S]*?)\}\s*\)/g
    for (const match of serverSource.matchAll(registrationPattern)) {
        const prefix = match[1].match(/\bprefix\s*:\s*["']([^"']+)["']/)?.[1]
        if (!prefix) continue
        mounts.push({
            prefix,
            source: normalizeRelativePath(referenceRoot, serverFile),
        })
    }
    return mounts.sort((left, right) => left.prefix.localeCompare(right.prefix))
}

function extractFunctionDeclaration(source, name) {
    const definition = new RegExp(`\\bfunction\\s+${escapeRegularExpression(name)}\\s*\\(`).exec(source)
    if (!definition) throw new Error(`missing compiled function: ${name}`)
    const openIndex = source.indexOf("{", definition.index)
    if (openIndex < 0) throw new Error(`invalid compiled function: ${name}`)
    const closeIndex = findClosingDelimiter(source, openIndex)
    if (closeIndex < 0) throw new Error(`invalid compiled function: ${name}`)
    return source.slice(definition.index, closeIndex + 1)
}

function numericSwitchCases(source) {
    return [...new Set([...source.matchAll(/\bcase\s+(\d+)\s*:/g)]
        .map((match) => Number(match[1])))]
        .sort((left, right) => left - right)
}

function parseCompiledMiddleware(serverSource) {
    const onRequest = serverSource.includes('fastify.addHook("onRequest"')
    const onSend = serverSource.includes('fastify.addHook("onSend"')
    return {
        request: {
            crashRateLimit: onRequest &&
                /request\.url\s*===\s*["']\/crash["']/.test(serverSource) &&
                /reply\.status\(429\)/.test(serverSource),
        },
        response: {
            msgpackNumberNormalization: onSend &&
                /content-type["']\)\s*===\s*["']application\/x-msgpack/.test(serverSource) &&
                /fixUint32Tags/.test(serverSource),
        },
        body: {
            formUrlencoded: /addContentTypeParser\(["']application\/x-www-form-urlencoded/.test(
                serverSource,
            ),
            json: /addContentTypeParser\(["']application\/json/.test(serverSource),
            msgpackBase64Fallback: /unpack\)\(Buffer\.from\(body,\s*["']base64["']\)\)/.test(
                serverSource,
            ),
        },
        strictNotFound: /setNotFoundHandler/.test(serverSource) &&
            /reply\.status\(404\)/.test(serverSource),
    }
}

function parseCompiledTcpSurface(referenceRoot) {
    const tcpRoot = path.join(referenceRoot, "out", "multi", "tcp")
    const handshakeFile = path.join(tcpRoot, "handshake.js")
    const lobbyFile = path.join(tcpRoot, "lobby.js")
    const battleFile = path.join(tcpRoot, "battle.js")
    const handshakeSource = sourceWithoutComments(readSource(handshakeFile))
    const lobbySource = sourceWithoutComments(readSource(lobbyFile))
    const battleSource = sourceWithoutComments(readSource(battleFile))
    return {
        framing: "nul-delimited-json",
        handshakes: uniqueSorted([...handshakeSource.matchAll(/socklet\s*===\s*["']([^"']+)["']/g)]
            .map((match) => match[1])),
        lobby: {
            clientTags: numericSwitchCases(extractFunctionDeclaration(lobbySource, "handleMessage")),
            notifyTags: numericSwitchCases(extractFunctionDeclaration(lobbySource, "handleNotify")),
        },
        battle: {
            clientTags: numericSwitchCases(extractFunctionDeclaration(battleSource, "handleBattleMessage")),
            notifyTags: numericSwitchCases(extractFunctionDeclaration(battleSource, "handleBattleNotify")),
        },
        sources: [handshakeFile, lobbyFile, battleFile]
            .map((file) => normalizeRelativePath(referenceRoot, file)),
    }
}

function parseCompiledAuxiliarySurface(referenceRoot, serverFile, serverSource) {
    const managementRoutes = extractFastifyRoutes(referenceRoot, serverFile, "")
        .filter((route) => ["/debug", "/crash"].includes(route.path))
    for (const [relativeEntry, prefix] of [
        [COMPILED_WEB_ENTRY, ""],
        [COMPILED_WEB_API_ENTRY, "/api"],
        [COMPILED_SEEDS_ENTRY, "/api/seeds"],
    ]) {
        managementRoutes.push(...collectCompiledRegisteredRoutes(
            referenceRoot,
            path.join(referenceRoot, ...relativeEntry.split("/")),
            prefix,
        ))
    }
    const routeKeys = new Set()
    for (const route of managementRoutes) {
        const key = `${route.method} ${route.path}`
        if (routeKeys.has(key)) throw new Error(`duplicate reference management route: ${key}`)
        routeKeys.add(key)
    }
    managementRoutes.sort((left, right) =>
        left.method.localeCompare(right.method) || left.path.localeCompare(right.path))
    return {
        managementRoutes,
        methodAgnosticRoutes: extractFastifyAllRoutes(
            referenceRoot,
            serverFile,
            serverSource,
        ),
        methodAgnosticContracts: parseMethodAgnosticContracts(serverSource),
        middleware: parseCompiledMiddleware(serverSource),
        staticMounts: parseCompiledStaticMounts(referenceRoot, serverFile, serverSource),
        tcp: parseCompiledTcpSurface(referenceRoot),
    }
}

function parseCompiledReferenceRoutes(referenceRoot) {
    const serverFile = path.join(referenceRoot, "out", "cn-server.js")
    const serverSource = sourceWithoutComments(readSource(serverFile))
    const imports = parseCommonJsImports(serverSource)
    const constants = parseStringConstants(serverSource)
    const registrations = parseCompiledRegistrations(serverSource, constants)
    const routes = extractFastifyRoutes(referenceRoot, serverFile, "")
        .filter((route) => route.path.startsWith("/api/index.php/"))
    const sources = []

    for (const registration of registrations) {
        const specifier = imports.get(registration.bindingName)
        if (!specifier) continue
        const pluginFile = resolveJavaScriptModule(serverFile, specifier)
        const relativePlugin = normalizeRelativePath(referenceRoot, pluginFile)
        const isApiPlugin = relativePlugin.startsWith(COMPILED_API_PLUGIN_PREFIX)
        const isCnPlugin = relativePlugin.startsWith(COMPILED_CN_PLUGIN_PREFIX)
        const isMultiPlugin = relativePlugin === COMPILED_MULTI_ENTRY
        if (!isApiPlugin && !isCnPlugin && !isMultiPlugin) continue

        const routeEntry = isMultiPlugin
            ? path.join(path.dirname(pluginFile), "http", "register.js")
            : pluginFile
        const routeFiles = collectCompiledRouteFiles(routeEntry)
        for (const routeFile of routeFiles) {
            const relativeRouteFile = normalizeRelativePath(referenceRoot, routeFile)
            sources.push({
                binding: `${registration.bindingName}.${registration.exportName}`,
                prefix: registration.prefix,
                source: relativeRouteFile,
            })
            routes.push(...extractFastifyRoutes(referenceRoot, routeFile, registration.prefix))
        }
    }

    const routeKeys = new Set()
    for (const route of routes) {
        const key = `${route.method} ${route.path}`
        if (routeKeys.has(key)) throw new Error(`duplicate reference route: ${key}`)
        routeKeys.add(key)
    }
    routes.sort((left, right) =>
        left.method.localeCompare(right.method) || left.path.localeCompare(right.path))
    sources.sort((left, right) => left.source.localeCompare(right.source))
    return {
        routes,
        serverFile,
        sources,
        auxiliary: parseCompiledAuxiliarySurface(referenceRoot, serverFile, serverSource),
    }
}
// //// /解析 cn-server 注册的服务表面 ////

// //// 解析个人服务 Rust 路由字面量 [@x380kkm 2026-08-22] ////
function collectFiles(root, extension) {
    const files = []
    for (const entry of readdirSync(root, { withFileTypes: true })) {
        const entryPath = path.join(root, entry.name)
        if (entry.isDirectory()) files.push(...collectFiles(entryPath, extension))
        else if (entry.name.endsWith(extension)) files.push(entryPath)
    }
    return files
}

function inferRustMethod(source, literalIndex) {
    const immediateSource = source.slice(Math.max(0, literalIndex - 80), literalIndex)
    const tupleMethod = immediateSource.match(/\("(GET|POST)"\s*,\s*$/)
    if (tupleMethod) return tupleMethod[1]
    const precedingSource = source.slice(Math.max(0, literalIndex - 1200), literalIndex)
    const methodPattern = /request\.method\(\)\s*(?:==|!=)\s*"(GET|POST)"/g
    let method = "POST"
    for (const match of precedingSource.matchAll(methodPattern)) method = match[1]
    return method
}

// //// 提取 Rust 路由契约 [@x380kkm 2026-08-23] ////
function inferRustHandler(source, literalEndIndex) {
    const tail = source.slice(literalEndIndex, literalEndIndex + 240)
    const tupleHandler = tail.match(/^\s*,\s*([a-z_][A-Za-z0-9_]*)\s*,?\s*\)/)
    if (tupleHandler) return tupleHandler[1]
    const handler = tail.match(
        /^\s*(?:=>\s*)?(?:\{\s*)?(?:return\s+)?(?:Some\s*\(\s*)?([a-z_][A-Za-z0-9_]*)\s*\(/,
    )
    return handler?.[1] ?? null
}

function extractRustFunctionSource(source, name) {
    const functionPattern = new RegExp(`\\bfn\\s+${escapeRegularExpression(name)}\\s*(?:<[^>{}]*>)?\\s*\\(`)
    const definition = functionPattern.exec(source)
    if (!definition) return null
    const openIndex = source.indexOf("{", definition.index + definition[0].length)
    if (openIndex < 0) return null
    const closeIndex = findClosingDelimiter(source, openIndex)
    return closeIndex < 0 ? null : source.slice(definition.index, closeIndex + 1)
}

function extractRustMacroSource(source, name) {
    const macroPattern = new RegExp(`\\bmacro_rules!\\s*${escapeRegularExpression(name)}\\s*\\{`)
    const definition = macroPattern.exec(source)
    if (!definition) return null
    const openIndex = source.indexOf("{", definition.index)
    const closeIndex = findClosingDelimiter(source, openIndex)
    return closeIndex < 0 ? null : source.slice(definition.index, closeIndex + 1)
}

function inferEnclosingRustHandler(source, literalIndex) {
    const functionPattern = /\bfn\s+([a-z_][A-Za-z0-9_]*)\s*(?:<[^>{}]*>)?\s*\(/g
    let enclosing = null
    for (const match of source.matchAll(functionPattern)) {
        if (match.index > literalIndex) break
        const openIndex = source.indexOf("{", match.index + match[0].length)
        if (openIndex < 0 || openIndex > literalIndex) continue
        const closeIndex = findClosingDelimiter(source, openIndex)
        if (closeIndex >= literalIndex) enclosing = match[1]
    }
    return enclosing
}

function expandRustFunctionSource(source, name, visited = new Set()) {
    if (!name || visited.has(name)) return ""
    const functionSource = extractRustFunctionSource(source, name) ??
        extractRustMacroSource(source, name)
    if (!functionSource) return ""
    visited.add(name)
    const sources = [functionSource]
    const callPattern = /(?:^|[^.A-Za-z0-9_])([a-z_][A-Za-z0-9_]*)\s*!?\s*\(/g
    for (const match of functionSource.matchAll(callPattern)) {
        const helperSource = expandRustFunctionSource(source, match[1], visited)
        if (helperSource) sources.push(helperSource)
    }
    return sources.join("\n")
}

function extractRustState(source) {
    const helpers = []
    const helperPattern = /(?:^|[^.A-Za-z0-9_])([a-z_][A-Za-z0-9_]*)\s*(!)?\s*\(/g
    const ignored = new Set(["if", "for", "while", "match", "loop", "some", "ok", "err"])
    for (const match of source.matchAll(helperPattern)) {
        if (!ignored.has(match[1])) helpers.push(match[1])
    }
    const methodPattern = /\.\s*([a-z_][A-Za-z0-9_]*)\s*\(/g
    for (const match of source.matchAll(methodPattern)) helpers.push(match[1])

    const readKeys = []
    const literalReadPattern = /(?:require_object|require_array|owned_ids|root\s*\.\s*get)\s*\(\s*(?:root\s*,\s*)?"([^"]+)"/g
    for (const match of source.matchAll(literalReadPattern)) readKeys.push(match[1])

    const writeKeys = []
    const literalWritePattern = /\.\s*(?:insert|entry|get_mut)\(\s*"([^"]+)"/g
    for (const match of source.matchAll(literalWritePattern)) writeKeys.push(match[1])
    return {
        readKeys: uniqueSorted(readKeys),
        writeKeys: uniqueSorted(writeKeys),
        helpers: uniqueSorted(helpers),
        confidence: "static",
    }
}

function extractRustSuccessCandidates(source) {
    const candidates = []
    const callPattern = /\b(msgpack_response(?:_at)?|msgpack_result_code_response_at)\s*\(/g
    for (const match of source.matchAll(callPattern)) {
        const openIndex = match.index + match[0].lastIndexOf("(")
        const closeIndex = findClosingDelimiter(source, openIndex)
        if (closeIndex < 0) continue
        const argumentsList = splitTopLevel(source.slice(openIndex + 1, closeIndex))
        const dataExpression = match[1] === "msgpack_result_code_response_at"
            ? "{}"
            : argumentsList.at(-1)
        const data = describeDataShape(dataExpression)
        candidates.push({
            status: 200,
            contentType: "application/x-msgpack",
            envelope: ["data", "data_headers"],
            dataShape: data.resolved ? data.dataShape : null,
            dataKeys: data.resolved ? data.dataKeys : null,
        })
    }
    const jsonCallPattern = /\bjson_response_at\s*\(/g
    for (const match of source.matchAll(jsonCallPattern)) {
        const openIndex = match.index + match[0].lastIndexOf("(")
        const closeIndex = findClosingDelimiter(source, openIndex)
        if (closeIndex < 0) continue
        const argumentsList = splitTopLevel(source.slice(openIndex + 1, closeIndex))
        const data = describeDataShape(argumentsList.at(-1))
        candidates.push({
            status: 200,
            contentType: "application/json",
            envelope: ["data", "data_headers"],
            dataShape: data.resolved ? data.dataShape : null,
            dataKeys: data.resolved ? data.dataKeys : null,
        })
    }
    const directPattern = /HttpResponse::(json|bytes|text)\(\s*"(2\d\d)[^"]*"(?:\s*,\s*"([^"]+)")?/g
    for (const match of source.matchAll(directPattern)) {
        candidates.push({
            status: Number(match[2]),
            contentType: match[1] === "bytes"
                ? match[3] ?? null
                : match[1] === "json"
                    ? "application/json"
                    : "text/plain",
            envelope: null,
            dataShape: null,
            dataKeys: null,
        })
    }
    if (/HttpResponse::text\s*\(/.test(source)) {
        candidates.push({
            status: 200,
            contentType: "text/plain; charset=utf-8",
            envelope: null,
            dataShape: null,
            dataKeys: null,
        })
    }
    return candidates
}

function extractRustErrorCandidates(source) {
    const errors = []
    const literalStatusPattern = /\b(?:error_response|battle_error)\(\s*"([45]\d\d)[^"]*"/g
    for (const match of source.matchAll(literalStatusPattern)) {
        errors.push({
            status: Number(match[1]),
            contentType: "application/json",
            envelope: ["error"],
            dataShape: null,
            dataKeys: [],
        })
    }
    const directJsonPattern = /HttpResponse::json\(\s*"([45]\d\d)[^"]*"/g
    for (const match of source.matchAll(directJsonPattern)) {
        errors.push({
            status: Number(match[1]),
            contentType: "application/json",
            envelope: null,
            dataShape: null,
            dataKeys: [],
        })
    }
    if (/\b(?:bad_request|invalid_request_body)\s*\(/.test(source)) {
        errors.push({
            status: 400,
            contentType: "application/json",
            envelope: null,
            dataShape: null,
            dataKeys: [],
        })
    }
    if (/\bnot_found\s*\(/.test(source)) {
        errors.push({
            status: 404,
            contentType: "application/json",
            envelope: null,
            dataShape: null,
            dataKeys: [],
        })
    }
    if (/\?/.test(source)) {
        errors.push({
            status: 500,
            contentType: "application/json",
            envelope: ["error"],
            dataShape: null,
            dataKeys: [],
        })
    }
    return mergeErrorCandidates([{ errors }])
}

function analyzeRustContract(source, handlerName, fallbackSources = []) {
    if (!handlerName) return mergeContracts([])
    let handlerContainer = source
    let directHandlerSource = extractRustFunctionSource(handlerContainer, handlerName)
    if (!directHandlerSource) {
        const containers = fallbackSources.filter((candidate) =>
            extractRustFunctionSource(candidate, handlerName) !== null)
        if (containers.length !== 1) return mergeContracts([])
        handlerContainer = containers[0]
        directHandlerSource = extractRustFunctionSource(handlerContainer, handlerName)
    }
    const handlerSource = expandRustFunctionSource(handlerContainer, handlerName)
    if (!directHandlerSource || !handlerSource) return mergeContracts([])
    return {
        success: mergeSuccessCandidates(extractRustSuccessCandidates(handlerSource)),
        errors: extractRustErrorCandidates(handlerSource),
        state: extractRustState(directHandlerSource),
    }
}
// //// /提取 Rust 路由契约 ////

function parseRustPrefixedRoutes(source, sourceFile, repositoryRoot, fallbackSources) {
    const constants = new Map()
    const constantPattern = /\bconst\s+([A-Za-z_$][\w$]*)\s*:\s*&str\s*=\s*"([^"]+)"\s*;/g
    for (const match of source.matchAll(constantPattern)) constants.set(match[1], match[2])

    const routes = []
    const prefixUsePattern = /strip_prefix\(\s*([A-Za-z_$][\w$]*)\s*\)/g
    for (const prefixUse of source.matchAll(prefixUsePattern)) {
        const prefix = constants.get(prefixUse[1])
        if (!prefix?.endsWith("/")) continue
        const tail = source.slice(prefixUse.index)
        const matchOffset = tail.search(/\bmatch\s+[A-Za-z_$][\w$]*\s*\{/)
        if (matchOffset < 0) continue
        const matchSource = tail.slice(matchOffset, matchOffset + 6000)
        const fallbackOffset = matchSource.search(/^\s*_\s*=>/m)
        const armsSource = fallbackOffset < 0 ? matchSource : matchSource.slice(0, fallbackOffset)
        const armPattern = /((?:"[^"]+"\s*(?:\|\s*)?)+)\s*=>/g
        for (const arm of armsSource.matchAll(armPattern)) {
            const handler = inferRustHandler(armsSource, arm.index + arm[0].length)
            for (const label of arm[1].matchAll(/"([^"]+)"/g)) {
                routes.push({
                    method: inferRustMethod(source, prefixUse.index),
                    path: `${prefix}${label[1]}`,
                    source: normalizeRelativePath(repositoryRoot, sourceFile),
                    contract: analyzeRustContract(source, handler, fallbackSources),
                })
            }
        }
    }
    return routes
}

function parseRustRoutes(repositoryRoot) {
    const sourceRoot = path.join(repositoryRoot, "core", "personal-service", "src")
    const routes = new Map()
    const routePattern = /"(\/(?:api\/index\.php|shijtswy\/version)\/[^"\s]+)"/g
    const sourceEntries = collectFiles(sourceRoot, ".rs").map((sourceFile) => ({
        sourceFile,
        source: sourceWithoutComments(readSource(sourceFile)),
    }))
    const fallbackSources = sourceEntries.map((entry) => entry.source)
    for (const { sourceFile, source } of sourceEntries) {
        for (const match of source.matchAll(routePattern)) {
            if (match[1].endsWith("/")) continue
            const method = inferRustMethod(source, match.index)
            const key = `${method} ${match[1]}`
            const handler = inferRustHandler(source, match.index + match[0].length) ??
                inferEnclosingRustHandler(source, match.index)
            const route = routes.get(key) ?? {
                method,
                path: match[1],
                sources: new Set(),
                contracts: [],
            }
            route.sources.add(normalizeRelativePath(repositoryRoot, sourceFile))
            route.contracts.push(analyzeRustContract(source, handler, fallbackSources))
            routes.set(key, route)
        }
        for (const prefixedRoute of parseRustPrefixedRoutes(
            source,
            sourceFile,
            repositoryRoot,
            fallbackSources,
        )) {
            const key = `${prefixedRoute.method} ${prefixedRoute.path}`
            const route = routes.get(key) ?? {
                method: prefixedRoute.method,
                path: prefixedRoute.path,
                sources: new Set(),
                contracts: [],
            }
            route.sources.add(prefixedRoute.source)
            route.contracts.push(prefixedRoute.contract)
            routes.set(key, route)
        }
    }
    for (const route of routes.values()) route.contract = mergeContracts(route.contracts)
    return routes
}

function parseRustStringSlices(source) {
    const slices = new Map()
    const slicePattern = /\bconst\s+([A-Za-z_$][\w$]*)\s*:\s*&\[\s*&str\s*\]\s*=\s*&\[([\s\S]*?)\]\s*;/g
    for (const match of source.matchAll(slicePattern)) {
        const values = [...match[2].matchAll(/"((?:\\.|[^"])*)"/g)]
            .map((value) => value[1])
        if (values.length > 0) slices.set(match[1], values)
    }
    return slices
}

function parseRustStringConstant(source, name) {
    const definition = new RegExp(
        `\\bconst\\s+${escapeRegularExpression(name)}\\s*:\\s*&str\\s*=\\s*`,
    ).exec(source)
    if (!definition) throw new Error(`missing Rust string constant: ${name}`)
    const value = source.slice(definition.index + definition[0].length)
    const raw = /^r(#+)"([\s\S]*?)"\1\s*;/.exec(value)
    if (raw) return raw[2]
    const escaped = /^"((?:\\.|[^"])*)"\s*;/.exec(value)
    if (escaped) return JSON.parse(`"${escaped[1]}"`)
    throw new Error(`invalid Rust string constant: ${name}`)
}

function parseRustMethodAgnosticRoutes(repositoryRoot) {
    const sourceRoot = path.join(repositoryRoot, "core", "personal-service", "src")
    const routes = new Map()
    for (const sourceFile of collectFiles(sourceRoot, ".rs")) {
        const source = sourceWithoutComments(readSource(sourceFile))
        const slices = parseRustStringSlices(source)
        const conditionPattern = /\bif\s+([^{}]+)\s*\{/g
        for (const conditionMatch of source.matchAll(conditionPattern)) {
            const condition = conditionMatch[1]
            if (/\bmethod\b/.test(condition)) continue
            const openIndex = source.indexOf("{", conditionMatch.index + conditionMatch[0].length - 1)
            const closeIndex = findClosingDelimiter(source, openIndex)
            const branchSource = closeIndex < 0 ? "" : source.slice(openIndex + 1, closeIndex)
            const bodyConstant = branchSource.match(
                /HttpResponse::json\([\s\S]*?,\s*([A-Z][A-Z0-9_]*)\.to_owned\(\)/,
            )?.[1]
            const contract = bodyConstant
                ? JSON.parse(parseRustStringConstant(source, bodyConstant))
                : null
            for (const [sliceName, paths] of slices) {
                const predicate = new RegExp(
                    `\\b${escapeRegularExpression(sliceName)}\\.contains\\(\\s*&path\\s*\\)`,
                )
                if (!predicate.test(condition)) continue
                for (const routePath of paths) {
                    const route = routes.get(routePath) ?? {
                        method: "ALL",
                        path: routePath,
                        groups: new Set(),
                        sources: new Set(),
                        contract,
                    }
                    if (JSON.stringify(route.contract) !== JSON.stringify(contract)) {
                        throw new Error(`conflicting method-agnostic contract: ${routePath}`)
                    }
                    route.groups.add(sliceName)
                    route.sources.add(normalizeRelativePath(repositoryRoot, sourceFile))
                    routes.set(routePath, route)
                }
            }
        }
    }
    return new Map([...routes].map(([routePath, route]) => [routePath, {
        ...route,
        groups: [...route.groups].sort(),
        sources: [...route.sources].sort(),
    }]))
}
// //// /解析个人服务 Rust 路由字面量 ////

// //// 比较辅助协议表面 [@x380kkm 2026-08-24] ////
function parseRustMiddleware(repositoryRoot) {
    const sourceRoot = path.join(repositoryRoot, "core", "personal-service", "src")
    const httpSource = sourceWithoutComments(readSource(path.join(sourceRoot, "http.rs")))
    const cnSource = sourceWithoutComments(readSource(path.join(sourceRoot, "cn.rs")))
    const msgpackFile = path.join(sourceRoot, "cn_msgpack.rs")
    const msgpackSource = existsSync(msgpackFile)
        ? sourceWithoutComments(readSource(msgpackFile))
        : ""
    const allSources = collectFiles(sourceRoot, ".rs")
        .map((sourceFile) => sourceWithoutComments(readSource(sourceFile)))
        .join("\n")
    return {
        request: {
            crashRateLimit: /429 Too Many Requests/.test(httpSource) &&
                /(?:CRASH_PATH|["']\/crash["'])/.test(allSources),
        },
        response: {
            msgpackNumberNormalization: /normalize_client_msgpack_numbers\(\s*&packed\s*\)/.test(
                cnSource,
            ) && /fn\s+normalize_client_msgpack_numbers\b/.test(msgpackSource),
        },
        body: {
            formUrlencoded: /application\/x-www-form-urlencoded/.test(cnSource),
            json: /serde_json::from_(?:slice|str)/.test(allSources),
            msgpackBase64Fallback: /STANDARD\s*\.\s*decode\(/.test(cnSource) &&
                /rmp_serde::from_slice/.test(cnSource),
        },
        strictNotFound: /404 Not Found/.test(httpSource) &&
            /not_found/.test(httpSource),
    }
}

function compareMethodAgnosticRoutes(referenceRoutes, referenceContracts, rustRoutes) {
    const covered = []
    const missing = []
    const contractMismatches = []
    for (const route of referenceRoutes) {
        const rustRoute = rustRoutes.get(route.path)
        if (!rustRoute) {
            missing.push(route)
            continue
        }
        const referenceContract = route.group === "iosLoginPaths"
            ? referenceContracts.login
            : route.group === "iosStubPaths"
                ? referenceContracts.stub
                : null
        const contractMatched = JSON.stringify(referenceContract) === JSON.stringify(
            rustRoute.contract,
        )
        const { contract: _contract, ...rustEvidence } = rustRoute
        covered.push({ ...route, rust: rustEvidence, contractMatched })
        if (!contractMatched) {
            contractMismatches.push({
                method: route.method,
                path: route.path,
                group: route.group,
                rustGroups: rustRoute.groups,
            })
        }
    }
    const referencePaths = new Set(referenceRoutes.map((route) => route.path))
    const extra = [...rustRoutes.values()]
        .filter((route) => !referencePaths.has(route.path))
        .sort((left, right) => left.path.localeCompare(right.path))
    return { covered, missing, extra, contractMismatches }
}

function compareMiddleware(reference, rust) {
    const assertions = [
        {
            name: "request.crashRateLimit",
            expected: reference.request.crashRateLimit,
            actual: rust.request.crashRateLimit,
            required: false,
        },
        {
            name: "response.msgpackNumberNormalization",
            expected: reference.response.msgpackNumberNormalization,
            actual: rust.response.msgpackNumberNormalization,
            required: true,
        },
        {
            name: "body.formUrlencoded",
            expected: reference.body.formUrlencoded,
            actual: rust.body.formUrlencoded,
            required: true,
        },
        {
            name: "body.json",
            expected: reference.body.json,
            actual: rust.body.json,
            required: true,
        },
        {
            name: "body.msgpackBase64Fallback",
            expected: reference.body.msgpackBase64Fallback,
            actual: rust.body.msgpackBase64Fallback,
            required: true,
        },
        {
            name: "strictNotFound",
            expected: reference.strictNotFound,
            actual: rust.strictNotFound,
            required: true,
        },
    ].map((assertion) => ({
        ...assertion,
        matched: assertion.expected === assertion.actual,
    }))
    return {
        assertions,
        matched: assertions.filter((assertion) => assertion.matched),
        requiredMismatches: assertions.filter(
            (assertion) => assertion.required && !assertion.matched,
        ),
        policyDifferences: assertions.filter(
            (assertion) => !assertion.required && !assertion.matched,
        ),
        reference,
        rust,
    }
}
// //// /比较辅助协议表面 ////

// //// 输出参考路由覆盖差异 [@x380kkm 2026-08-22] ////
function writeFixture(fixturePath, routes) {
    mkdirSync(path.dirname(fixturePath), { recursive: true })
    const content = `${routes.map((route) => `${route.method} ${route.path}`).join("\n")}\n`
    writeFileSync(fixturePath, content, "utf8")
}

function compareRoutes(referenceRoutes, rustRoutes) {
    const referenceKeys = new Set(referenceRoutes.map((route) => `${route.method} ${route.path}`))
    const covered = []
    const missing = []
    for (const route of referenceRoutes) {
        const key = `${route.method} ${route.path}`
        const rustRoute = rustRoutes.get(key)
        if (rustRoute) {
            covered.push({
                ...route,
                rustSources: [...rustRoute.sources].sort(),
                rustContract: rustRoute.contract,
            })
        } else {
            missing.push(route)
        }
    }
    const extra = [...rustRoutes]
        .filter(([key]) => !referenceKeys.has(key))
        .map(([, route]) => ({
            method: route.method,
            path: route.path,
            sources: [...route.sources].sort(),
        }))
        .sort((left, right) =>
            left.method.localeCompare(right.method) || left.path.localeCompare(right.path))
    return { covered, missing, extra }
}

function compareContracts(coveredRoutes) {
    const routes = coveredRoutes.map((route) => {
        const differences = []
        const unresolvedFields = []
        for (const field of ["status", "contentType", "envelope", "dataShape", "dataKeys"]) {
            const referenceUnresolved = route.contract.success.unresolved.includes(field)
            const rustUnresolved = route.rustContract.success.unresolved.includes(field)
            if (referenceUnresolved || rustUnresolved) {
                unresolvedFields.push(field)
                continue
            }
            const referenceValue = route.contract.success[field]
            const rustValue = route.rustContract.success[field]
            if (JSON.stringify(referenceValue) !== JSON.stringify(rustValue)) {
                differences.push({ field, reference: referenceValue, rust: rustValue })
            }
        }
        const status = differences.length > 0
            ? "mismatched"
            : unresolvedFields.length > 0
                ? "unresolved"
                : "matched"
        return {
            method: route.method,
            path: route.path,
            source: route.source,
            rustSources: route.rustSources,
            status,
            differences,
            unresolvedFields,
            reference: route.contract,
            rust: route.rustContract,
        }
    })
    return {
        routes,
        matched: routes.filter((route) => route.status === "matched"),
        mismatched: routes.filter((route) => route.status === "mismatched"),
        unresolved: routes.filter((route) => route.status === "unresolved"),
    }
}

function main() {
    const args = process.argv.slice(2)
    const referenceRoot = path.resolve(readRequiredOption(args, "--reference-root"))
    const fixtureOption = readOptionalOption(args, "--write-fixture")
    const fixturePath = fixtureOption ? path.resolve(fixtureOption) : null
    const compiledServer = path.join(referenceRoot, "out", "cn-server.js")
    const reference = existsSync(compiledServer)
        ? parseCompiledReferenceRoutes(referenceRoot)
        : parseReferenceRoutes(referenceRoot)
    const rustRoutes = parseRustRoutes(REPOSITORY_ROOT)
    const rustMethodAgnosticRoutes = parseRustMethodAgnosticRoutes(REPOSITORY_ROOT)
    const rustMiddleware = parseRustMiddleware(REPOSITORY_ROOT)
    const comparison = compareRoutes(reference.routes, rustRoutes)
    const contracts = compareContracts(comparison.covered)
    const methodAgnostic = compareMethodAgnosticRoutes(
        reference.auxiliary.methodAgnosticRoutes,
        reference.auxiliary.methodAgnosticContracts,
        rustMethodAgnosticRoutes,
    )
    const multiplayer = reference.auxiliary.tcp
        ? buildMultiplayerDifferential(referenceRoot, REPOSITORY_ROOT)
        : null
    const middleware = reference.auxiliary.middleware
        ? compareMiddleware(reference.auxiliary.middleware, rustMiddleware)
        : null

    if (fixturePath) writeFixture(fixturePath, reference.routes)
    const report = {
        reference: {
            root: referenceRoot,
            server: normalizeRelativePath(referenceRoot, reference.serverFile),
            count: reference.routes.length,
            sources: reference.sources,
            auxiliary: {
                management: {
                    count: reference.auxiliary.managementRoutes.length,
                    routes: reference.auxiliary.managementRoutes,
                },
                methodAgnostic: {
                    count: reference.auxiliary.methodAgnosticRoutes.length,
                    routes: reference.auxiliary.methodAgnosticRoutes,
                },
                middleware: reference.auxiliary.middleware,
                static: {
                    count: reference.auxiliary.staticMounts.length,
                    mounts: reference.auxiliary.staticMounts,
                },
                tcp: reference.auxiliary.tcp,
            },
        },
        covered: { count: comparison.covered.length, routes: comparison.covered },
        missing: { count: comparison.missing.length, routes: comparison.missing },
        extra: { count: comparison.extra.length, routes: comparison.extra },
        contracts: {
            count: contracts.routes.length,
            matched: { count: contracts.matched.length },
            mismatched: { count: contracts.mismatched.length, routes: contracts.mismatched },
            unresolved: { count: contracts.unresolved.length, routes: contracts.unresolved },
            routes: contracts.routes,
        },
        auxiliary: {
            methodAgnostic: {
                covered: { count: methodAgnostic.covered.length, routes: methodAgnostic.covered },
                missing: { count: methodAgnostic.missing.length, routes: methodAgnostic.missing },
                extra: { count: methodAgnostic.extra.length, routes: methodAgnostic.extra },
                contractMismatches: {
                    count: methodAgnostic.contractMismatches.length,
                    routes: methodAgnostic.contractMismatches,
                },
            },
            middleware,
            multiplayer,
        },
        fixture: fixturePath,
    }
    const output = hasOption(args, "--routes-only")
        ? {
            covered: {
                count: report.covered.count,
                routes: report.covered.routes.map(({ method, path: routePath, source }) => ({
                    method,
                    path: routePath,
                    source,
                })),
            },
            missing: {
                count: report.missing.count,
                routes: report.missing.routes.map(({ method, path: routePath, source }) => ({
                    method,
                    path: routePath,
                    source,
                })),
            },
            extra: report.extra,
        }
        : hasOption(args, "--summary")
            ? {
            reference: {
                root: report.reference.root,
                server: report.reference.server,
                count: report.reference.count,
                auxiliary: {
                    management: {
                        count: report.reference.auxiliary.management.count,
                    },
                    methodAgnostic: {
                        count: report.reference.auxiliary.methodAgnostic.count,
                    },
                    middleware: report.reference.auxiliary.middleware,
                    static: report.reference.auxiliary.static,
                    tcp: report.reference.auxiliary.tcp,
                },
            },
            covered: { count: report.covered.count },
            missing: report.missing,
            extra: { count: report.extra.count },
            contracts: {
                count: report.contracts.count,
                matched: report.contracts.matched,
                mismatched: {
                    count: report.contracts.mismatched.count,
                    routes: report.contracts.mismatched.routes.map((route) => ({
                        method: route.method,
                        path: route.path,
                        differences: route.differences,
                        unresolvedFields: route.unresolvedFields,
                    })),
                },
                unresolved: {
                    count: report.contracts.unresolved.count,
                    routes: report.contracts.unresolved.routes.map((route) => ({
                        method: route.method,
                        path: route.path,
                        unresolvedFields: route.unresolvedFields,
                    })),
                },
            },
            auxiliary: {
                methodAgnostic: {
                    covered: { count: report.auxiliary.methodAgnostic.covered.count },
                    missing: report.auxiliary.methodAgnostic.missing,
                    extra: { count: report.auxiliary.methodAgnostic.extra.count },
                    contractMismatches: report.auxiliary.methodAgnostic.contractMismatches,
                },
                middleware: report.auxiliary.middleware && {
                    matched: { count: report.auxiliary.middleware.matched.length },
                    requiredMismatches: {
                        count: report.auxiliary.middleware.requiredMismatches.length,
                        assertions: report.auxiliary.middleware.requiredMismatches,
                    },
                    policyDifferences: {
                        count: report.auxiliary.middleware.policyDifferences.length,
                        assertions: report.auxiliary.middleware.policyDifferences,
                    },
                },
                multiplayer: report.auxiliary.multiplayer && {
                    summary: report.auxiliary.multiplayer.comparison.summary,
                    differences: report.auxiliary.multiplayer.comparison.differences,
                    extensions: report.auxiliary.multiplayer.comparison.extensions,
                },
            },
            fixture: report.fixture,
            }
            : report
    process.stdout.write(`${JSON.stringify(output, null, 2)}\n`)
    const failOnUnresolved = hasOption(args, "--fail-on-unresolved")
    const reportOnly = hasOption(args, "--report-only")
    if (!reportOnly && (comparison.missing.length > 0 || contracts.mismatched.length > 0 ||
        methodAgnostic.missing.length > 0 || methodAgnostic.contractMismatches.length > 0 ||
        (multiplayer?.comparison.differences.length ?? 0) > 0 ||
        (middleware?.requiredMismatches.length ?? 0) > 0 ||
        (failOnUnresolved && contracts.unresolved.length > 0))) {
        process.exitCode = 1
    }
}

export {
    analyzeJavaScriptContract,
    normalizeRelativePath,
    parseRustRoutes,
}

const isMainModule = process.argv[1] !== undefined &&
    path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
if (isMainModule) main()
// //// /输出参考路由覆盖差异 ////
