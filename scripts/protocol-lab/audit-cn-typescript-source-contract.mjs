// audience: internal
// # cn-typescript-source-contract-audit
// 此脚本从 TypeScript CN HTTP 源码枚举路由, 成功与错误响应和状态变化, 再与个人服务 Rust 实现比较.

import { existsSync, readFileSync } from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import ts from "typescript"
import {
    analyzeJavaScriptContract,
    normalizeRelativePath,
    parseRustRoutes,
} from "./audit-cn-reference-route-coverage.mjs"

const SCRIPT_ROOT = path.dirname(fileURLToPath(import.meta.url))
const REPOSITORY_ROOT = path.resolve(SCRIPT_ROOT, "../..")
const CN_PATH_PREFIXES = ["/api/index.php", "/latest/api/index.php", "/shijtswy/version/"]
const MUTATION_HELPER_PATTERN = /^(?:abort|add|apply|award|claim|collect|consume|continue|create|delete|deliver|disband|edit|exchange|finish|grant|inject|insert|learn|receive|recover|remove|reset|reward|save|sell|set|spend|start|update|upgrade)(?:_|[A-Z]|$)/

// //// 解释可运行参考与离线策略确认的源码差异 [@x380kkm 2026-08-24] ////
const RESOLVED_ROUTE_DIFFERENCES = new Map([
    ["POST /api/index.php/asset/get_path", {
        kind: "reference-supersedes-source",
        successFields: ["contentType"],
        missingErrorStatuses: [],
        stateStatus: "local-extension",
        reason: "TypeScript 源码为 JSON, 可运行 iOS 参考服务和动态差分为 MessagePack.",
    }],
    ["POST /api/index.php/channels/channel_leiting_pay/query_unfinish_order", {
        kind: "local-compatibility",
        successFields: [],
        missingErrorStatuses: [400],
        stateStatus: "matched",
        reason: "离线支付查询接受稳定本地身份并返回空订单, 避免原生支付 SDK 进入错误流程.",
    }],
    ["POST /api/index.php/ex_boost/select", {
        kind: "reference-supersedes-source",
        successFields: ["contentType"],
        missingErrorStatuses: [],
        stateStatus: "matched",
        reason: "TypeScript 源码为 MessagePack, 可运行 iOS 参考服务和动态差分为 JSON.",
    }],
    ["POST /api/index.php/multi_battle_quest/disband_room", {
        kind: "local-compatibility",
        successFields: [],
        missingErrorStatuses: [404],
        stateStatus: "matched",
        reason: "重复解散按幂等成功处理, 支持断线和切后台后的本地恢复.",
    }],
    ["POST /api/index.php/multi_battle_quest/prepare", {
        kind: "local-compatibility",
        successFields: [],
        missingErrorStatuses: [404],
        stateStatus: "local-extension",
        reason: "本地房间准备保留重入状态并使用可恢复错误, 支持 AI 队友和断线恢复.",
    }],
    ["POST /api/index.php/multi_battle_quest/start", {
        kind: "local-compatibility",
        successFields: [],
        missingErrorStatuses: [404],
        stateStatus: "matched",
        reason: "本地多人启动使用可恢复错误状态, 避免临时房间状态丢失导致流程中断.",
    }],
    ["POST /api/index.php/multi_battle_quest/summon", {
        kind: "local-compatibility",
        successFields: [],
        missingErrorStatuses: [404],
        stateStatus: "local-extension",
        reason: "召集接口可创建本地 AI 队友并保持可重入, 房间缺失不进入严格远端 404.",
    }],
    ["POST /api/index.php/single_battle_quest/abort", {
        kind: "local-compatibility",
        successFields: ["contentType"],
        missingErrorStatuses: [409],
        stateStatus: "matched",
        reason: "单人中断按幂等 JSON 成功处理, 支持熄屏和重复中断后的本地恢复.",
    }],
])
// //// /解释可运行参考与离线策略确认的源码差异 ////

function readOptionalOption(args, name) {
    const index = args.indexOf(name)
    if (index < 0) return null
    const value = args[index + 1]
    if (!value || value.startsWith("--")) throw new Error(`missing option value: ${name}`)
    return value
}

function hasOption(args, name) {
    return args.includes(name)
}

function isCnPath(routePath) {
    return CN_PATH_PREFIXES.some((prefix) => routePath.startsWith(prefix))
}

function canonicalCnPath(routePath) {
    const latestPrefix = "/latest/api/index.php"
    return routePath.startsWith(latestPrefix)
        ? routePath.slice("/latest".length)
        : routePath
}

function joinRoutePath(prefix, routePath) {
    if (!prefix) return routePath
    return `${prefix.replace(/\/+$/, "")}/${routePath.replace(/^\/+/, "")}`
}

function propertyName(node) {
    if (ts.isIdentifier(node) || ts.isStringLiteral(node) || ts.isNumericLiteral(node)) {
        return node.text
    }
    return null
}

function isFunctionNode(node) {
    return ts.isArrowFunction(node) ||
        ts.isFunctionDeclaration(node) ||
        ts.isFunctionExpression(node) ||
        ts.isMethodDeclaration(node)
}

// //// 解析 TypeScript 路由模块 [@x380kkm 2026-08-24] ////
const moduleCache = new Map()

function stringConstants(sourceFile) {
    const constants = new Map()
    for (const statement of sourceFile.statements) {
        if (!ts.isVariableStatement(statement)) continue
        for (const declaration of statement.declarationList.declarations) {
            if (!ts.isIdentifier(declaration.name) || !declaration.initializer) continue
            if (ts.isStringLiteral(declaration.initializer) ||
                ts.isNoSubstitutionTemplateLiteral(declaration.initializer)) {
                constants.set(declaration.name.text, declaration.initializer.text)
            }
        }
    }
    return constants
}

function evaluateString(node, constants) {
    if (!node) return null
    if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) return node.text
    if (ts.isIdentifier(node)) return constants.get(node.text) ?? null
    if (ts.isBinaryExpression(node) && node.operatorToken.kind === ts.SyntaxKind.PlusToken) {
        const left = evaluateString(node.left, constants)
        const right = evaluateString(node.right, constants)
        return left === null || right === null ? null : `${left}${right}`
    }
    if (ts.isTemplateExpression(node)) {
        let value = node.head.text
        for (const span of node.templateSpans) {
            const expression = evaluateString(span.expression, constants)
            if (expression === null) return null
            value += expression + span.literal.text
        }
        return value
    }
    return null
}

function importedBindings(sourceFile) {
    const imports = new Map()
    for (const statement of sourceFile.statements) {
        if (!ts.isImportDeclaration(statement) ||
            !ts.isStringLiteral(statement.moduleSpecifier) ||
            !statement.importClause) continue
        const specifier = statement.moduleSpecifier.text
        if (statement.importClause.name) {
            imports.set(statement.importClause.name.text, { importedName: "default", specifier })
        }
        const bindings = statement.importClause.namedBindings
        if (!bindings || !ts.isNamedImports(bindings)) continue
        for (const element of bindings.elements) {
            imports.set(element.name.text, {
                importedName: element.propertyName?.text ?? element.name.text,
                specifier,
            })
        }
    }
    return imports
}

function reexportedBindings(sourceFile) {
    const exports = new Map()
    for (const statement of sourceFile.statements) {
        if (!ts.isExportDeclaration(statement) ||
            !statement.moduleSpecifier ||
            !ts.isStringLiteral(statement.moduleSpecifier) ||
            !statement.exportClause ||
            !ts.isNamedExports(statement.exportClause)) continue
        for (const element of statement.exportClause.elements) {
            exports.set(element.name.text, {
                importedName: element.propertyName?.text ?? element.name.text,
                specifier: statement.moduleSpecifier.text,
            })
        }
    }
    return exports
}

function functionDeclarations(sourceFile) {
    const functions = new Map()
    let defaultExport = null
    for (const statement of sourceFile.statements) {
        if (ts.isFunctionDeclaration(statement) && statement.name) {
            functions.set(statement.name.text, statement)
            if (statement.modifiers?.some((modifier) =>
                modifier.kind === ts.SyntaxKind.DefaultKeyword)) {
                defaultExport = statement.name.text
            }
        }
        if (ts.isVariableStatement(statement)) {
            for (const declaration of statement.declarationList.declarations) {
                if (!ts.isIdentifier(declaration.name) || !declaration.initializer ||
                    !isFunctionNode(declaration.initializer)) continue
                functions.set(declaration.name.text, declaration.initializer)
            }
        }
        if (ts.isExportAssignment(statement) && ts.isIdentifier(statement.expression)) {
            defaultExport = statement.expression.text
        }
    }
    return { functions, defaultExport }
}

function loadModule(filePath) {
    const resolvedPath = path.resolve(filePath)
    const cached = moduleCache.get(resolvedPath)
    if (cached) return cached
    const source = readFileSync(resolvedPath, "utf8")
    const sourceFile = ts.createSourceFile(
        resolvedPath,
        source,
        ts.ScriptTarget.Latest,
        true,
        ts.ScriptKind.TS,
    )
    const declarations = functionDeclarations(sourceFile)
    const module = {
        filePath: resolvedPath,
        source,
        sourceFile,
        constants: stringConstants(sourceFile),
        imports: importedBindings(sourceFile),
        reexports: reexportedBindings(sourceFile),
        functions: declarations.functions,
        defaultExport: declarations.defaultExport,
    }
    moduleCache.set(resolvedPath, module)
    return module
}

function resolveTypeScriptModule(importingFile, specifier) {
    if (!specifier.startsWith(".")) return null
    const moduleBase = path.resolve(path.dirname(importingFile), specifier)
    const candidates = [`${moduleBase}.ts`, path.join(moduleBase, "index.ts")]
    return candidates.find((candidate) => existsSync(candidate)) ?? null
}

function resolveFunctionBinding(module, importedName, visited = new Set()) {
    const visitKey = `${module.filePath}:${importedName}`
    if (visited.has(visitKey)) return null
    visited.add(visitKey)
    const functionName = importedName === "default" ? module.defaultExport : importedName
    if (functionName && module.functions.has(functionName)) return { module, functionName }
    const binding = module.reexports.get(importedName) ?? module.imports.get(importedName)
    const modulePath = binding && resolveTypeScriptModule(module.filePath, binding.specifier)
    if (!modulePath) return null
    return resolveFunctionBinding(loadModule(modulePath), binding.importedName, visited)
}

function calledLocalFunctions(module, node) {
    const names = new Set()
    const visit = (child) => {
        if (ts.isCallExpression(child) && ts.isIdentifier(child.expression) &&
            module.functions.has(child.expression.text)) {
            names.add(child.expression.text)
        }
        ts.forEachChild(child, visit)
    }
    visit(node)
    return [...names]
}

function expandFunctionSource(module, name, visited = new Set()) {
    if (!name || visited.has(name)) return ""
    const declaration = module.functions.get(name)
    if (!declaration) return ""
    visited.add(name)
    const sources = [declaration.getText(module.sourceFile)]
    for (const helperName of calledLocalFunctions(module, declaration)) {
        const helperSource = expandFunctionSource(module, helperName, visited)
        if (helperSource) sources.push(helperSource)
    }
    return sources.join("\n")
}

function handlerSource(module, node) {
    if (!node) return ""
    if (ts.isIdentifier(node)) return expandFunctionSource(module, node.text)
    if (!isFunctionNode(node)) return node.getText(module.sourceFile)
    const sources = [node.getText(module.sourceFile)]
    const visited = new Set()
    for (const helperName of calledLocalFunctions(module, node)) {
        const helperSource = expandFunctionSource(module, helperName, visited)
        if (helperSource) sources.push(helperSource)
    }
    return sources.join("\n")
}
// //// /解析 TypeScript 路由模块 ////

// //// 从服务装配入口反向枚举 CN 路由 [@x380kkm 2026-08-24] ////
function routeCall(node) {
    if (!ts.isCallExpression(node) || !ts.isPropertyAccessExpression(node.expression)) return null
    const owner = node.expression.expression
    const method = node.expression.name.text
    if (!ts.isIdentifier(owner) || owner.text !== "fastify" ||
        !["get", "post", "put", "patch", "delete"].includes(method)) return null
    return { method: method.toUpperCase(), call: node }
}

function lineNumber(module, node) {
    return module.sourceFile.getLineAndCharacterOfPosition(node.getStart(module.sourceFile)).line + 1
}

function collectFunctionRoutes(module, functionName, prefix, repositoryRoot, visited) {
    const visitKey = `${module.filePath}:${functionName}:${prefix}`
    if (visited.has(visitKey)) return []
    visited.add(visitKey)
    const declaration = module.functions.get(functionName)
    if (!declaration?.body) return []
    const routes = []
    const visit = (node) => {
        if (node !== declaration.body && isFunctionNode(node)) return
        const route = routeCall(node)
        if (route) {
            const routePath = evaluateString(route.call.arguments[0], module.constants)
            if (routePath !== null) {
                const fullPath = canonicalCnPath(joinRoutePath(prefix, routePath))
                if (isCnPath(fullPath)) {
                    const source = handlerSource(module, route.call.arguments.at(-1))
                    routes.push({
                        method: route.method,
                        path: fullPath,
                        source: normalizeRelativePath(repositoryRoot, module.filePath),
                        line: lineNumber(module, node),
                        contract: analyzeJavaScriptContract(source),
                    })
                }
            }
            return
        }
        if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) &&
            node.arguments.some((argument) => ts.isIdentifier(argument) && argument.text === "fastify")) {
            const registrar = resolveFunctionBinding(module, node.expression.text)
            if (!registrar) {
                ts.forEachChild(node, visit)
                return
            }
            routes.push(...collectFunctionRoutes(
                registrar.module,
                registrar.functionName,
                prefix,
                repositoryRoot,
                visited,
            ))
            return
        }
        ts.forEachChild(node, visit)
    }
    visit(declaration.body)
    return routes
}

function registrationPrefix(node, constants) {
    const options = node.arguments[1]
    if (!options || !ts.isObjectLiteralExpression(options)) return ""
    for (const property of options.properties) {
        if (!ts.isPropertyAssignment(property) || propertyName(property.name) !== "prefix") continue
        return evaluateString(property.initializer, constants)
    }
    return ""
}

function collectTypeScriptRoutes(repositoryRoot) {
    const serverModule = loadModule(path.join(repositoryRoot, "src", "server.ts"))
    const routeRoots = [
        path.join(repositoryRoot, "src", "routes", "api") + path.sep,
        path.join(repositoryRoot, "src", "routes", "cn") + path.sep,
        path.join(repositoryRoot, "src", "multi") + path.sep,
    ]
    const routes = []
    const visitedRegistrars = new Set()
    const visitServer = (node) => {
        if (node !== serverModule.sourceFile && isFunctionNode(node)) return
        if (ts.isCallExpression(node) && ts.isPropertyAccessExpression(node.expression) &&
            ts.isIdentifier(node.expression.expression) &&
            node.expression.expression.text === "fastify" &&
            node.expression.name.text === "register" &&
            ts.isIdentifier(node.arguments[0])) {
            const binding = serverModule.imports.get(node.arguments[0].text)
            const modulePath = binding && resolveTypeScriptModule(serverModule.filePath, binding.specifier)
            if (modulePath && routeRoots.some((root) => modulePath.startsWith(root))) {
                const routeModule = loadModule(modulePath)
                const registrar = resolveFunctionBinding(routeModule, binding.importedName)
                const prefix = registrationPrefix(node, serverModule.constants)
                if (registrar && prefix !== null) {
                    routes.push(...collectFunctionRoutes(
                        registrar.module,
                        registrar.functionName,
                        prefix,
                        repositoryRoot,
                        visitedRegistrars,
                    ))
                }
            }
            return
        }
        const directRoute = routeCall(node)
        if (directRoute) {
            const routePath = evaluateString(directRoute.call.arguments[0], serverModule.constants)
            if (routePath && isCnPath(routePath)) {
                routes.push({
                    method: directRoute.method,
                    path: canonicalCnPath(routePath),
                    source: normalizeRelativePath(repositoryRoot, serverModule.filePath),
                    line: lineNumber(serverModule, node),
                    contract: analyzeJavaScriptContract(
                        handlerSource(serverModule, directRoute.call.arguments.at(-1)),
                    ),
                })
            }
            return
        }
        ts.forEachChild(node, visitServer)
    }
    visitServer(serverModule.sourceFile)
    const routeMap = new Map()
    for (const route of routes) {
        const key = `${route.method} ${route.path}`
        const existing = routeMap.get(key)
        if (!existing) {
            routeMap.set(key, route)
            continue
        }
        if (existing.source === route.source && existing.line === route.line) continue
        const existingIsDirectCnRoute = existing.source.startsWith("src/routes/cn/")
        const routeIsDirectCnRoute = route.source.startsWith("src/routes/cn/")
        if (routeIsDirectCnRoute && !existingIsDirectCnRoute) {
            routeMap.set(key, route)
            continue
        }
        if (existingIsDirectCnRoute && !routeIsDirectCnRoute) continue
        throw new Error(`duplicate TypeScript route: ${key}`)
    }
    return [...routeMap.values()].sort((left, right) =>
        left.method.localeCompare(right.method) || left.path.localeCompare(right.path))
}
// //// /从服务装配入口反向枚举 CN 路由 ////

// //// 比较成功, 错误和状态契约 [@x380kkm 2026-08-24] ////
function uniqueSorted(values) {
    return [...new Set(values)].sort()
}

function resolvedRouterSuccess(status, contentType) {
    return {
        status,
        contentType,
        envelope: [],
        dataShape: null,
        dataKeys: [],
        unresolved: ["envelope", "dataShape", "dataKeys"],
        confidence: "static",
    }
}

function applyRouterSuccessEvidence(repositoryRoot, rustRoutes) {
    for (const routePath of [
        "/shijtswy/version/client_release_android.dis",
        "/shijtswy/version/client_release_ios.dis",
    ]) {
        const route = rustRoutes.get(`GET ${routePath}`)
        const sourcePath = route && [...route.sources]
            .map((relativePath) => path.join(repositoryRoot, relativePath))
            .find((filePath) => existsSync(filePath))
        if (!route || !sourcePath) continue
        const source = readFileSync(sourcePath, "utf8")
        if (source.includes(routePath) && source.includes("HttpResponse::text")) {
            route.contract.success = resolvedRouterSuccess(200, "text/plain; charset=utf-8")
        }
    }

    const titlePath = "/api/index.php/assetintitle/version_info_in_title"
    const titleRoute = rustRoutes.get(`POST ${titlePath}`)
    const titleSourcePath = titleRoute && [...titleRoute.sources]
        .map((relativePath) => path.join(repositoryRoot, relativePath))
        .find((filePath) => existsSync(filePath))
    if (!titleRoute || !titleSourcePath) return
    const titleSource = readFileSync(titleSourcePath, "utf8")
    const handlerStart = titleSource.indexOf("fn version_info_in_title(")
    const handlerEnd = titleSource.indexOf("fn version_info_data(", handlerStart)
    if (handlerStart >= 0 && handlerEnd > handlerStart &&
        titleSource.slice(handlerStart, handlerEnd).includes("msgpack_response(")) {
        titleRoute.contract.success = resolvedRouterSuccess(200, "application/x-msgpack")
        titleRoute.contract.state = {
            readKeys: [],
            writeKeys: [],
            helpers: [],
            confidence: "static",
        }
    }
}

function compareSuccess(reference, rust) {
    const differences = []
    const unresolved = []
    for (const field of ["status", "contentType", "envelope", "dataShape", "dataKeys"]) {
        if (reference.unresolved.includes(field) || rust.unresolved.includes(field)) {
            unresolved.push(field)
        } else if (JSON.stringify(reference[field]) !== JSON.stringify(rust[field])) {
            differences.push({ field, reference: reference[field], rust: rust[field] })
        }
    }
    return {
        status: differences.length > 0 ? "mismatched" : unresolved.length > 0 ? "unresolved" : "matched",
        differences,
        unresolved,
    }
}

function compareErrors(reference, rust) {
    const expectedStatuses = uniqueSorted(reference.map((error) => error.status))
    const actualStatuses = uniqueSorted(rust.map((error) => error.status))
    const missingStatuses = expectedStatuses.filter((status) => !actualStatuses.includes(status))
    const extraStatuses = actualStatuses.filter((status) => !expectedStatuses.includes(status))
    const contentTypeDifferences = []
    for (const status of expectedStatuses.filter((value) => actualStatuses.includes(value))) {
        const expected = uniqueSorted(reference
            .filter((error) => error.status === status)
            .map((error) => error.contentType)
            .filter(Boolean))
        const actual = uniqueSorted(rust
            .filter((error) => error.status === status)
            .map((error) => error.contentType)
            .filter(Boolean))
        if (expected.length > 0 && actual.length > 0 &&
            !expected.some((value) => actual.includes(value))) {
            contentTypeDifferences.push({ status, reference: expected, rust: actual })
        }
    }
    const status = missingStatuses.length > 0 || contentTypeDifferences.length > 0
        ? actualStatuses.length === 0 ? "unresolved" : "mismatched"
        : extraStatuses.length > 0
            ? "unresolved"
            : "matched"
    return {
        status,
        expectedStatuses,
        actualStatuses,
        missingStatuses,
        extraStatuses,
        contentTypeDifferences,
    }
}

function mutationState(contract) {
    const mutationHelpers = contract.state.helpers.filter((helper) =>
        MUTATION_HELPER_PATTERN.test(helper) && !/(?:header|response)/i.test(helper))
    return {
        mutates: contract.state.writeKeys.length > 0 || mutationHelpers.length > 0,
        writeKeys: contract.state.writeKeys,
        mutationHelpers,
        confidence: contract.state.confidence,
    }
}

function compareState(referenceContract, rustContract) {
    const reference = mutationState(referenceContract)
    const rust = mutationState(rustContract)
    let status = "matched"
    if (reference.confidence === "unresolved" || rust.confidence === "unresolved") status = "unresolved"
    else if (reference.mutates && !rust.mutates) status = "mismatched"
    else if (!reference.mutates && rust.mutates) status = "local-extension"
    else if (reference.mutates && rust.mutates) status = "unresolved"
    return { status, reference, rust }
}

function sameValues(left, right) {
    return JSON.stringify(uniqueSorted(left)) === JSON.stringify(uniqueSorted(right))
}

function resolveRouteDifference(method, routePath, success, errors, state) {
    const resolution = RESOLVED_ROUTE_DIFFERENCES.get(`${method} ${routePath}`)
    if (!resolution || success.unresolved.length > 0 || errors.contentTypeDifferences.length > 0) {
        return null
    }
    const successFields = success.differences.map((difference) => difference.field)
    if (!sameValues(successFields, resolution.successFields) ||
        !sameValues(errors.missingStatuses, resolution.missingErrorStatuses) ||
        !sameValues(errors.extraStatuses, resolution.extraErrorStatuses ?? []) ||
        state.status !== resolution.stateStatus) {
        return null
    }
    return {
        kind: resolution.kind,
        reason: resolution.reason,
        verifiedDifference: {
            successFields: uniqueSorted(successFields),
            missingErrorStatuses: uniqueSorted(errors.missingStatuses),
            extraErrorStatuses: uniqueSorted(errors.extraStatuses),
            stateStatus: state.status,
        },
    }
}

function compareRoutes(referenceRoutes, rustRoutes) {
    return referenceRoutes.map((reference) => {
        const rust = rustRoutes.get(`${reference.method} ${reference.path}`)
        if (!rust) return { ...reference, status: "missing", rustSources: [] }
        const success = compareSuccess(reference.contract.success, rust.contract.success)
        const errors = compareErrors(reference.contract.errors, rust.contract.errors)
        const state = compareState(reference.contract, rust.contract)
        const statuses = [success.status, errors.status, state.status]
        const rawStatus = statuses.includes("mismatched") || statuses.includes("local-extension")
            ? "mismatched"
            : statuses.includes("unresolved")
                ? "unresolved"
                : "matched"
        const resolution = rawStatus === "mismatched"
            ? resolveRouteDifference(reference.method, reference.path, success, errors, state)
            : null
        const status = resolution ? "resolved-difference" : rawStatus
        return {
            method: reference.method,
            path: reference.path,
            source: reference.source,
            line: reference.line,
            rustSources: [...rust.sources].sort(),
            status,
            rawStatus,
            resolution,
            success,
            errors,
            state,
            referenceContract: reference.contract,
            rustContract: rust.contract,
        }
    })
}
// //// /比较成功, 错误和状态契约 ////

// //// 输出 TypeScript 源码反向核对结果 [@x380kkm 2026-08-24] ////
function main() {
    const args = process.argv.slice(2)
    const sourceRoot = path.resolve(readOptionalOption(args, "--source-root") ?? REPOSITORY_ROOT)
    const referenceRoutes = collectTypeScriptRoutes(sourceRoot)
    const rustRoutes = parseRustRoutes(REPOSITORY_ROOT)
    applyRouterSuccessEvidence(REPOSITORY_ROOT, rustRoutes)
    const compared = compareRoutes(referenceRoutes, rustRoutes)
    const missing = compared.filter((route) => route.status === "missing")
    const mismatched = compared.filter((route) => route.status === "mismatched")
    const unresolved = compared.filter((route) => route.status === "unresolved")
    const matched = compared.filter((route) => route.status === "matched")
    const resolvedDifferences = compared.filter((route) => route.status === "resolved-difference")
    const stateExtensions = compared.filter((route) => route.state?.status === "local-extension")
    const summary = {
        total: compared.length,
        matched: matched.length,
        missing: missing.length,
        mismatched: mismatched.length,
        unresolved: unresolved.length,
        resolvedDifferences: resolvedDifferences.length,
        stateExtensions: stateExtensions.length,
        successBranches: compared.filter((route) =>
            route.referenceContract?.success.status !== null).length,
        errorBranches: compared.reduce((count, route) =>
            count + (route.referenceContract?.errors.length ?? 0), 0),
        mutatingRoutes: compared.filter((route) => route.state?.reference.mutates).length,
    }
    const compactRoute = (route) => ({
        method: route.method,
        path: route.path,
        source: route.source,
        line: route.line,
        rustSources: route.rustSources,
        success: route.success,
        errors: route.errors,
        state: route.state,
        resolution: route.resolution,
    })
    const output = hasOption(args, "--summary")
        ? {
            sourceRoot,
            summary,
            missing: missing.map(compactRoute),
            mismatched: mismatched.map(compactRoute),
            unresolved: unresolved.map(compactRoute),
            resolved_differences: resolvedDifferences.map(compactRoute),
        }
        : { sourceRoot, summary, routes: compared }
    process.stdout.write(`${JSON.stringify(output, null, 2)}\n`)
    if (!hasOption(args, "--report-only") &&
        (missing.length > 0 || mismatched.length > 0)) process.exitCode = 1
}

main()
// //// /输出 TypeScript 源码反向核对结果 ////
