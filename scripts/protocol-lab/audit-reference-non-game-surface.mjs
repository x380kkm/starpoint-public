// audience: internal
// # reference-non-game-surface-audit
//
// 该脚本从 startpoint-cn 运行入口, patcher, iOS 二进制和 CDN 清单抽取非游戏契约, 并核对个人服务中的对应实现.
// 报告覆盖 SDK 响应类型, 静态覆盖映射, 资源摘要和 iOS 补丁例程. missing 或 different 使进程失败.

import {
    closeSync,
    existsSync,
    openSync,
    mkdirSync,
    readFileSync,
    readSync,
    readdirSync,
    statSync,
    writeFileSync,
} from "node:fs"
import { createHash } from "node:crypto"
import path from "node:path"
import { fileURLToPath, pathToFileURL } from "node:url"

const SCRIPT_ROOT = path.dirname(fileURLToPath(import.meta.url))
const DEFAULT_LOCAL_ROOT = path.resolve(SCRIPT_ROOT, "../..")
const DEFAULT_REFERENCE_ROOT = path.resolve(DEFAULT_LOCAL_ROOT, "../startpoint-cn-launcher")
const DEFAULT_CN_CDN_BUNDLE = path.resolve(
    DEFAULT_LOCAL_ROOT,
    "../starpoint/artifacts/ios-device-staging/jp-art-final/Payload/worldflipper.app/StarpointCNCDN",
)

const FAILURE_STATUSES = new Set(["missing", "different"])

const REFERENCE_ENTRY_FILE = "resources/server/out/cn-server.js"
const REFERENCE_MANAGEMENT_MODULES = new Set([
    "./routes/web",
    "./routes/web_api",
    "./routes/web_api/seeds",
])
const REFERENCE_DIAGNOSTIC_PATHS = new Set(["/debug", "/crash"])
const ARCHIVE_DIRECTORIES = new Set([
    "archive-common-full",
    "archive-medium-full",
    "archive-android-full",
    "archive-ios-full",
    "archive-common-diff",
    "archive-medium-diff",
    "archive-android-diff",
    "archive-ios-diff",
])
const IOS_ARCHIVE_DIRECTORIES = new Set([
    "archive-common-full",
    "archive-medium-full",
    "archive-ios-full",
    "archive-common-diff",
    "archive-medium-diff",
    "archive-ios-diff",
])
const ANDROID_ARCHIVE_DIRECTORIES = new Set([
    "archive-common-full",
    "archive-medium-full",
    "archive-android-full",
    "archive-common-diff",
    "archive-medium-diff",
    "archive-android-diff",
])
const REFERENCE_PATCH_ROUTINES = new Set([
    "patchBuffer",
    "patchBlock",
    "deguardBuffer",
    "patchFirstLoginTip",
    "patchLoginDialog",
    "patchWelcomeBanner",
    "patchAgreementDialogs",
    "patchBundleIdCheck",
])

const LOCAL_EVIDENCE = {
    managementPage: [{
        file: "core/personal-service/src/management_web.rs",
        needle: '"/manage" | "/manage/"',
        purpose: "本地合并式管理页.",
    }],
    mailCreate: [{
        file: "core/personal-service/src/cn_mail.rs",
        needle: 'request.path() == "/v1/mails" && request.method() == "POST"',
        purpose: "本地邮件创建 API.",
    }],
    virtualTime: [{
        file: "core/personal-service/src/virtual_time.rs",
        needle: 'const VIRTUAL_TIME_PATH: &str = "/v1/time";',
        purpose: "本地虚拟时间 API.",
    }],
    saveList: [{
        file: "core/personal-service/src/local_saves.rs",
        needle: 'const LOCAL_SAVES_PATH: &str = "/v1/local-saves";',
        purpose: "本地存档槽 API.",
    }],
    saveActivate: [{
        file: "core/personal-service/src/local_saves/routes.rs",
        needle: '("POST", [slot_id, "activate"])',
        purpose: "本地存档激活操作.",
    }],
    saveCopy: [{
        file: "core/personal-service/src/local_saves/routes.rs",
        needle: '("POST", [slot_id, "copy"])',
        purpose: "本地存档复制操作.",
    }],
    saveContext: [{
        file: "core/personal-service/src/local_saves/routes.rs",
        needle: '("GET", [slot_id, "context"])',
        purpose: "本地存档上下文读取操作.",
    }],
    saveImport: [{
        file: "core/personal-service/src/local_saves/routes.rs",
        needle: '("POST", ["import"])',
        purpose: "本地可移植存档导入操作.",
    }],
    saveExport: [{
        file: "core/personal-service/src/local_saves/routes.rs",
        needle: '("GET", [slot_id, "export"])',
        purpose: "本地可移植存档导出操作.",
    }],
    sdkDiagnostics: [{
        file: "core/personal-service/src/sdk_compat.rs",
        needle: 'if (path == DEBUG_PATH && matches!(method, "GET" | "POST"))',
        purpose: "本地 debug 诊断兼容.",
    }, {
        file: "core/personal-service/src/sdk_compat.rs",
        needle: '|| (path == CRASH_PATH && method == "POST")',
        purpose: "本地 crash 诊断兼容.",
    }],
}

const MANAGEMENT_POLICY_GROUPS = [
    {
        routes: ["GET /", "GET /mail", "GET /player", "GET /player/:playerId"],
        classification: "local-equivalent",
        localCapability: "合并式 /manage 管理页.",
        evidence: LOCAL_EVIDENCE.managementPage,
    },
    {
        routes: [
            "GET /seeds",
            "GET /api/seeds/stats",
            "GET /api/seeds/list",
            "POST /api/seeds/mode",
            "POST /api/seeds/tag",
            "POST /api/seeds/test-seed",
            "DELETE /api/seeds/test-seed",
        ],
        classification: "reference-exclusive-management-ui",
        localCapability: null,
        rationale: "参考端运行时维护抽卡动画 seed 诊断状态. 本地使用固定 seed 资源池, 该管理面不属于游戏客户端契约.",
    },
    {
        routes: ["POST /api/mail/send"],
        classification: "local-equivalent",
        localCapability: "POST /v1/mails.",
        evidence: LOCAL_EVIDENCE.mailCreate,
    },
    {
        routes: [
            "GET /api/server/currentTime",
            "GET /api/server/resetTime",
            "GET /api/server/time",
        ],
        classification: "local-equivalent",
        localCapability: "GET/PUT /v1/time.",
        evidence: LOCAL_EVIDENCE.virtualTime,
    },
    {
        routes: ["POST /api/server/selectAccount", "POST /api/server/activateSave"],
        classification: "local-equivalent",
        localCapability: "POST /v1/local-saves/{slot}/activate.",
        evidence: LOCAL_EVIDENCE.saveActivate,
    },
    {
        routes: ["POST /api/server/cloneSave"],
        classification: "local-equivalent",
        localCapability: "POST /v1/local-saves/{slot}/copy.",
        evidence: LOCAL_EVIDENCE.saveCopy,
    },
    {
        routes: ["GET /api/server/accounts"],
        classification: "local-equivalent",
        localCapability: "GET /v1/local-saves.",
        evidence: LOCAL_EVIDENCE.saveList,
    },
    {
        routes: ["GET /api/server/nameLookup"],
        classification: "local-equivalent",
        localCapability: "GET /v1/local-saves/{slot}/context.",
        evidence: LOCAL_EVIDENCE.saveContext,
    },
    {
        routes: ["POST /api/server/importSave"],
        classification: "local-equivalent",
        localCapability: "POST /v1/local-saves/import.",
        evidence: LOCAL_EVIDENCE.saveImport,
    },
    {
        routes: [
            "POST /api/server/newSave",
            "POST /api/server/deleteSave",
            "POST /api/server/deleteAccount",
            "POST /api/server/renameSave",
            "POST /api/server/device/rename",
        ],
        classification: "reference-exclusive-management-ui",
        localCapability: null,
        rationale: "参考端直接管理其账号与设备数据库. 本地以可移植存档槽和设备绑定模型提供管理能力, 这些写接口不是游戏客户端契约.",
    },
    {
        routes: ["GET /api/player"],
        classification: "local-equivalent",
        localCapability: "GET /v1/local-saves 与存档 context.",
        evidence: [...LOCAL_EVIDENCE.saveList, ...LOCAL_EVIDENCE.saveContext],
    },
    {
        routes: ["GET /api/player/save"],
        classification: "local-equivalent",
        localCapability: "GET /v1/local-saves/{slot}/export.",
        evidence: LOCAL_EVIDENCE.saveExport,
    },
    {
        routes: ["POST /api/player/save"],
        classification: "local-equivalent",
        localCapability: "POST /v1/local-saves/import.",
        evidence: LOCAL_EVIDENCE.saveImport,
    },
    {
        routes: [
            "PATCH /api/player/:id/field",
            "POST /api/player/:id/clear_ex_boost",
            "POST /api/player/:id/reset_parties",
            "POST /api/player/:id/clear_mail",
            "POST /api/player/:id/clear_receive_history",
            "POST /api/player/:id/character",
            "DELETE /api/player/:id/character/:code",
            "POST /api/player/:id/item",
            "DELETE /api/player/:id/item/:itemId",
            "DELETE /api/player/:id/quest_progress/:section/:quest_id",
            "DELETE /api/player/:id/quest_progress",
            "DELETE /api/player/:id/drawn_quest/:category/:quest_id",
            "DELETE /api/player/:id/drawn_quest",
            "POST /api/player/:id/reset_challenge",
            "DELETE /api/player/:id/mail",
            "POST /api/player/:id/daily_reset",
            "POST /api/player/:id/weekly_reset",
        ],
        classification: "reference-exclusive-management-ui",
        localCapability: null,
        rationale: "参考端提供原始玩家文档编辑器. 本地通过游戏协议, 邮件奖励和版本化存档恢复保持状态一致, 这些编辑器接口不属于游戏客户端契约.",
    },
    {
        routes: ["GET /debug", "POST /debug", "POST /crash"],
        classification: "local-equivalent",
        localCapability: "本地 SDK 诊断兼容路由.",
        evidence: LOCAL_EVIDENCE.sdkDiagnostics,
    },
]

const LOCAL_PATCH_NAMES = new Map([
    [0xB00C, "air_packaging_guard"],
    [0x8E6F0, "air_safe_fallthrough_guard_0008e6f0"],
    [0x256B30, "air_safe_fallthrough_guard_00256b30"],
    [0x312230, "air_bundle_identifier_resource_check"],
    [0x4932D4, "air_safe_fallthrough_guard_004932d4"],
    [0x4AC098, "air_safe_fallthrough_guard_004ac098"],
    [0x4AC578, "air_safe_fallthrough_guard_004ac578"],
    [0x4D089C, "air_safe_fallthrough_guard_004d089c"],
    [0x4DEF4C, "air_safe_fallthrough_guard_004def4c"],
    [0x520960, "air_safe_fallthrough_guard_00520960"],
    [0x533240, "air_safe_fallthrough_guard_00533240"],
    [0x533254, "air_safe_fallthrough_guard_00533254"],
    [0x60F15C, "gdpr_privacy_view"],
    [0x634890, "login_welcome_view_call"],
    [0x64B238, "lt_welcome_view"],
    [0x68E878, "login_token_presence"],
    [0x68E898, "login_token_length"],
    [0x68E8D8, "login_privacy_gate"],
    [0x698990, "leiting_privacy_view"],
    [0x6ADB14, "login_manager_welcome"],
    [0x6AE0DC, "first_login_tip"],
    [0x6C6CFC, "login_license_gate"],
])
const REFERENCE_SAFE_FALLTHROUGH_GUARDS = [
    0x8E6F0,
    0x256B30,
    0x4932D4,
    0x4AC098,
    0x4AC578,
    0x4D089C,
    0x4DEF4C,
    0x520960,
    0x533240,
    0x533254,
]

// //// 读取源码并生成 path:line 证据 [@x380kkm 2026-08-24] ////
function normalizePath(value) {
    return value.replaceAll("\\", "/")
}

function lineNumberAt(source, index) {
    if (index < 0) return null
    return source.slice(0, index).split("\n").length
}

function createSourceReader(root, repository) {
    const cache = new Map()
    return {
        root,
        repository,
        read(relativePath) {
            if (!cache.has(relativePath)) {
                const filePath = path.join(root, ...relativePath.split("/"))
                if (!existsSync(filePath)) throw new Error(`missing source: ${filePath}`)
                cache.set(relativePath, readFileSync(filePath, "utf8"))
            }
            return cache.get(relativePath)
        },
        evidence(relativePath, index, purpose) {
            return {
                repository,
                path: normalizePath(relativePath),
                line: lineNumberAt(this.read(relativePath), index),
                anchor: `${normalizePath(relativePath)}:${lineNumberAt(this.read(relativePath), index)}`,
                purpose,
            }
        },
    }
}

function locateEvidence(reader, specification) {
    const source = reader.read(specification.file)
    const index = specification.needle !== undefined
        ? source.indexOf(specification.needle)
        : source.search(specification.pattern)
    if (index < 0) return null
    return reader.evidence(specification.file, index, specification.purpose)
}
// //// /读取源码并生成 path:line 证据 ////

// //// 抽取并分类参考管理路由 [@x380kkm 2026-08-24] ////
function joinRoutePath(prefix, routePath) {
    if (routePath === "/") return prefix || "/"
    return `${prefix}${routePath}`
}

export function extractReferenceManagementRoutes(referenceReader) {
    return extractReferenceManagementSurface(referenceReader).routes
}

function resolveImportedModule(referenceReader, currentFile, request) {
    if (!request.startsWith(".")) return null
    const unresolved = normalizePath(path.join(path.dirname(currentFile), request))
    for (const candidate of [`${unresolved}.js`, `${unresolved}/index.js`]) {
        if (existsSync(path.join(referenceReader.root, ...candidate.split("/")))) return candidate
    }
    return null
}

function extractImports(source) {
    const imports = new Map()
    const pattern = /const\s+([A-Za-z0-9_]+)\s*=\s*__importDefault\(require\((["'])([^"']+)\2\)\);/g
    for (const match of source.matchAll(pattern)) imports.set(match[1], match[3])
    return imports
}

function extractRegistrations(source) {
    const registrations = []
    const pattern = /fastify\.register\(\s*([A-Za-z0-9_]+)\.default(?:\s*,\s*\{\s*prefix:\s*(["'])([^"']*)\2\s*\})?\s*\)/g
    for (const match of source.matchAll(pattern)) {
        registrations.push({ alias: match[1], prefix: match[3] ?? "", index: match.index })
    }
    return registrations
}

function extractDirectRoutes(referenceReader, file, prefix, allowedPaths = null) {
    const routes = []
    const routePattern = /fastify\.(get|post|put|delete|patch|all)\(\s*["']([^"']+)["']/g
    const source = referenceReader.read(file)
    for (const match of source.matchAll(routePattern)) {
        if (allowedPaths && !allowedPaths.has(match[2])) continue
        const method = match[1].toUpperCase()
        const routePath = joinRoutePath(prefix, match[2])
        routes.push({
            method,
            path: routePath,
            key: `${method} ${routePath}`,
            reference: referenceReader.evidence(
                file,
                match.index,
                "参考端注册的非游戏 HTTP 路由.",
            ),
        })
    }
    return routes
}

function extractReferenceManagementSurface(referenceReader, issues = []) {
    const entrySource = referenceReader.read(REFERENCE_ENTRY_FILE)
    const entryImports = extractImports(entrySource)
    const entryRegistrations = extractRegistrations(entrySource).filter((registration) =>
        REFERENCE_MANAGEMENT_MODULES.has(entryImports.get(registration.alias)))
    const missingEntryModules = [...REFERENCE_MANAGEMENT_MODULES].filter((request) =>
        !entryRegistrations.some((registration) => entryImports.get(registration.alias) === request))
    for (const request of missingEntryModules) issues.push({
        status: "missing",
        surface: "management-registration",
        id: request,
        detail: "参考 CN 入口没有注册管理模块.",
    })

    const routes = extractDirectRoutes(
        referenceReader,
        REFERENCE_ENTRY_FILE,
        "",
        REFERENCE_DIAGNOSTIC_PATHS,
    )
    const registrations = []
    const pending = entryRegistrations.map((registration) => ({
        parentFile: REFERENCE_ENTRY_FILE,
        registration,
        request: entryImports.get(registration.alias),
        parentPrefix: "",
    }))
    const visited = new Set()
    while (pending.length > 0) {
        const current = pending.pop()
        const file = resolveImportedModule(referenceReader, current.parentFile, current.request)
        const prefix = joinRoutePath(current.parentPrefix, current.registration.prefix || "/")
            .replace(/\/$/, "")
        const parentSource = referenceReader.read(current.parentFile)
        const registrationEvidence = referenceReader.evidence(
            current.parentFile,
            current.registration.index,
            "参考端管理模块注册链.",
        )
        if (!file) {
            issues.push({
                status: "missing",
                surface: "management-registration",
                id: `${current.parentFile}:${current.request}`,
                detail: "参考管理模块无法解析到源码文件.",
                reference: registrationEvidence,
            })
            continue
        }
        registrations.push({
            status: "matched",
            prefix: prefix || "/",
            module: file,
            evidence: registrationEvidence,
        })
        const visitKey = `${file}\0${prefix}`
        if (visited.has(visitKey)) continue
        visited.add(visitKey)
        routes.push(...extractDirectRoutes(referenceReader, file, prefix))

        const source = referenceReader.read(file)
        const imports = extractImports(source)
        for (const registration of extractRegistrations(source)) {
            const request = imports.get(registration.alias)
            if (!request) continue
            pending.push({ parentFile: file, registration, request, parentPrefix: prefix })
        }
    }
    return {
        registrations,
        routes: routes.sort((left, right) => left.key.localeCompare(right.key)),
    }
}

function managementPolicies() {
    const policies = new Map()
    for (const group of MANAGEMENT_POLICY_GROUPS) {
        for (const route of group.routes) {
            if (policies.has(route)) throw new Error(`duplicate management policy: ${route}`)
            policies.set(route, group)
        }
    }
    return policies
}

function auditManagementRoutes(routes, localReader, issues) {
    const policies = managementPolicies()
    const seen = new Set()
    const report = routes.map((route) => {
        seen.add(route.key)
        const policy = policies.get(route.key)
        if (!policy) {
            issues.push({
                status: "missing",
                surface: "management-route",
                id: route.key,
                detail: "参考路由没有显式本地分类.",
                reference: route.reference,
            })
            return { ...route, classification: "missing", localCapability: null, localEvidence: [] }
        }
        const localEvidence = (policy.evidence ?? []).map((specification) => locateEvidence(localReader, specification))
        const absent = localEvidence.some((item) => item === null)
        if (absent) issues.push({
            status: "missing",
            surface: "management-route",
            id: route.key,
            detail: "声明的本地对应能力缺少源码证据.",
            reference: route.reference,
        })
        return {
            ...route,
            classification: absent ? "missing" : policy.classification,
            localCapability: policy.localCapability,
            rationale: policy.rationale ?? null,
            localEvidence: localEvidence.filter(Boolean),
        }
    })
    for (const route of policies.keys()) {
        if (seen.has(route)) continue
        issues.push({
            status: "different",
            surface: "management-route",
            id: route,
            detail: "显式管理路由策略在参考入口中没有对应注册项.",
        })
    }
    return report
}
// //// /抽取并分类参考管理路由 ////

// //// 核对参考 iOS SDK 路由契约 [@x380kkm 2026-08-24] ////
function extractStringArray(source, name) {
    const match = new RegExp(`\\b(?:const|static)\\s+${name}[^=]*=\\s*&?\\[([\\s\\S]*?)\\];`).exec(source)
    if (!match) throw new Error(`missing string array: ${name}`)
    return [...match[1].matchAll(/(["'])(.*?)\1/g)].map((value) => value[2])
}

function extractStringConstant(source, name) {
    const raw = new RegExp(`\\bconst\\s+${name}[^=]*=\\s*r#"([\\s\\S]*?)"#;`).exec(source)
    if (raw) return raw[1]
    const quoted = new RegExp(`\\bconst\\s+${name}[^=]*=\\s*(["'])(.*?)\\1;`, "s").exec(source)
    if (quoted) return quoted[2]
    throw new Error(`missing string constant: ${name}`)
}

function extractJavaScriptFlatObject(source, name, constants) {
    const match = new RegExp(`\\bconst\\s+${name}\\s*=\\s*\\{([^}]*)\\};`).exec(source)
    if (!match) throw new Error(`missing JavaScript object: ${name}`)
    const result = {}
    for (const property of match[1].matchAll(/([A-Za-z_$][\w$]*)\s*:\s*(?:(["'])(.*?)\2|([A-Za-z_$][\w$]*))/g)) {
        const value = property[3] ?? constants.get(property[4])
        if (value === undefined) throw new Error(`dynamic JavaScript object field: ${name}.${property[1]}`)
        result[property[1]] = value
    }
    return result
}

function canonicalJson(value) {
    if (Array.isArray(value)) return value.map(canonicalJson)
    if (value === null || typeof value !== "object") return value
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonicalJson(value[key])]))
}

function auditIosSdkRoutes(referenceReader, localReader, issues) {
    const referenceFile = REFERENCE_ENTRY_FILE
    const localFile = "core/personal-service/src/sdk_compat.rs"
    const referenceSource = referenceReader.read(referenceFile)
    const localSource = localReader.read(localFile)
    const constants = new Map([
        ["IOS_SDK_LOGIN_BLOB", extractStringConstant(referenceSource, "IOS_SDK_LOGIN_BLOB")],
    ])
    const groups = [{
        id: "login",
        referenceArray: "iosLoginPaths",
        referenceBody: "iosLoginOK",
        localArray: "LEITING_LOGIN_PATHS",
        localBody: "LEITING_GUEST_LOGIN_BODY",
        localBranch: "if LEITING_LOGIN_PATHS.contains(&path)",
    }, {
        id: "stub",
        referenceArray: "iosStubPaths",
        referenceBody: "iosStatusOK",
        localArray: "LEITING_PHONE_CODE_PATHS",
        localBody: "LEITING_STATUS_BODY",
        localBranch: "if LEITING_PHONE_CODE_PATHS.contains(&path)",
    }]
    const routes = []
    const localPaths = new Set()
    for (const group of groups) {
        const referencePaths = extractStringArray(referenceSource, group.referenceArray)
        const expectedBody = extractJavaScriptFlatObject(
            referenceSource,
            group.referenceBody,
            constants,
        )
        const groupLocalPaths = extractStringArray(localSource, group.localArray)
        const actualBody = JSON.parse(extractStringConstant(localSource, group.localBody))
        const allMethods = referenceSource.includes("fastify.all(p") && localSource.includes(group.localBranch)
        for (const routePath of groupLocalPaths) localPaths.add(routePath)
        for (const routePath of referencePaths) {
            const present = groupLocalPaths.includes(routePath)
            const contractMatched = allMethods && JSON.stringify(canonicalJson(expectedBody)) === JSON.stringify(canonicalJson(actualBody))
            const status = !present ? "missing" : contractMatched ? "matched" : "different"
            const route = {
                group: group.id,
                method: "ALL",
                path: routePath,
                status,
                contract: {
                    status: 200,
                    contentType: "application/json",
                    bodyType: "object",
                    body: expectedBody,
                },
                reference: referenceReader.evidence(
                    referenceFile,
                    referenceSource.indexOf(`"${routePath}"`),
                    "参考 iOS SDK 路由及 JSON 正文.",
                ),
                local: present ? localReader.evidence(
                    localFile,
                    localSource.indexOf(`"${routePath}"`),
                    "本地 iOS SDK 路由及类型化 JSON 正文.",
                ) : null,
            }
            routes.push(route)
            if (FAILURE_STATUSES.has(status)) issues.push({
                status,
                surface: "ios-sdk-route",
                id: `ALL ${routePath}`,
                detail: present
                    ? "SDK 响应状态, 媒体类型或正文与参考实现不同."
                    : "本地缺少参考 iOS SDK 路由.",
                reference: route.reference,
                local: route.local,
            })
        }
    }
    const referencePaths = new Set(routes.map((route) => route.path))
    const extensions = [...localPaths]
        .filter((routePath) => !referencePaths.has(routePath))
        .sort()
        .map((routePath) => ({
            method: "ALL",
            path: routePath,
            status: "explicit-local-extension",
            local: localReader.evidence(
                localFile,
                localSource.indexOf(`"${routePath}"`),
                "本地 AOT 登录桥接补充路由.",
            ),
        }))
    return { routes, extensions }
}
// //// /核对参考 iOS SDK 路由契约 ////

// //// 核对 iOS 二进制 URL 与类型化兼容面 [@x380kkm 2026-08-24] ////
const URL_TOKEN_PATTERN = /https?:\/\/[A-Za-z0-9._~:@/?#%+\-=!$&'()*;]+/g

function decodeU30(buffer, start, end) {
    let value = 0
    let shift = 0
    for (let index = start; index < end; index += 1) {
        const byte = buffer[index]
        value |= (byte & 0x7f) << shift
        if ((byte & 0x80) === 0) return index + 1 === end ? value : null
        shift += 7
        if (shift > 28) return null
    }
    return null
}

function recoverBinaryHttpStrings(executablePath) {
    const buffer = readFileSync(executablePath)
    const strings = new Set()
    let offset = 0
    while ((offset = buffer.indexOf("http", offset, "ascii")) >= 0) {
        for (let prefixLength = 1; prefixLength <= 5 && offset >= prefixLength; prefixLength += 1) {
            const length = decodeU30(buffer, offset - prefixLength, offset)
            if (length === null || length < 7 || length > 4096 || offset + length > buffer.length) continue
            const value = buffer.subarray(offset, offset + length).toString("utf8")
            if (value.includes("http")) strings.add(value)
        }
        let left = offset - 1
        while (left >= 0 && buffer[left] !== 0 && offset - left <= 4096) left -= 1
        let right = offset
        while (right < buffer.length && buffer[right] !== 0 && right - offset <= 4096) right += 1
        if (left >= 0 && right < buffer.length && right - offset <= 4096) {
            const value = buffer.subarray(left + 1, right).toString("utf8")
            if (value.includes("http")) strings.add(value)
        }
        offset += 4
    }
    return strings
}

function extractBinaryUrls(executablePath) {
    const urls = new Set()
    const strings = recoverBinaryHttpStrings(executablePath)
    for (const value of strings) {
        for (const match of value.matchAll(URL_TOKEN_PATTERN)) {
            urls.add(match[0].replace(/[.;]+$/, ""))
        }
    }
    return { strings, urls: [...urls].sort() }
}

function splitUrl(url) {
    const match = /^https?:\/\/([^/]+)(\/.*)?$/.exec(url)
    if (!match) return null
    const hostPort = match[1].split("@").at(-1).toLowerCase()
    const host = hostPort.startsWith("[")
        ? hostPort.slice(1, hostPort.indexOf("]"))
        : hostPort.split(":", 1)[0]
    const rawPath = match[2] ?? "/"
    const fragmentIndex = rawPath.indexOf("#")
    const pathWithoutFragment = fragmentIndex >= 0 ? rawPath.slice(0, fragmentIndex) : rawPath
    return { host, rawPath, requestPath: pathWithoutFragment.split("?", 1)[0] || "/" }
}

function isCnSdkHost(host) {
    return host === "127.0.0.1" || host === "127.1" ||
        host === "leiting.com" || host.endsWith(".leiting.com") ||
        host === "roguelike.com" || host.endsWith(".roguelike.com") ||
        host === "cl2009.com" || host.endsWith(".cl2009.com") ||
        host === "sobot.com" || host.endsWith(".sobot.com") ||
        host === "sohu.com" || host.endsWith(".sohu.com")
}

function binaryContentType(relativePath) {
    const extension = path.extname(relativePath).toLowerCase()
    if (extension === ".json") return "application/json"
    if (extension === ".txt") return "text/plain"
    if (extension === ".csv") return "text/csv"
    if ([".html", ".htm"].includes(extension)) return "text/html"
    if (extension === ".png") return "image/png"
    if ([".jpg", ".jpeg"].includes(extension)) return "image/jpeg"
    if (extension === ".zip") return "application/zip"
    return "application/octet-stream"
}

function typedSdkContract(requestPath, sdkSource) {
    const noContentPaths = new Set(extractStringArray(sdkSource, "NO_CONTENT_REPORT_PATHS"))
    const emptyHtmlPaths = new Set(extractStringArray(sdkSource, "EMPTY_HTML_PATHS"))
    if (noContentPaths.has(requestPath) || requestPath === "/wf_crash/crash.php") {
        return { status: 204, contentType: null, bodyType: "empty" }
    }
    if (
        emptyHtmlPaths.has(requestPath) ||
        requestPath === "/" ||
        requestPath.startsWith("/terrace/") ||
        (requestPath.startsWith("/protocols/leiting/third/") && requestPath.endsWith("/annex.html"))
    ) return { status: 200, contentType: "text/html", bodyType: "empty" }
    if (requestPath === "/chat/common/res/83f5636f-51b7-48d6-9d63-40eba0963bda.png") {
        return { status: 200, contentType: "image/png", bodyType: "binary" }
    }
    if (requestPath === "/cityjson") {
        return { status: 200, contentType: "application/javascript", bodyType: "text" }
    }
    if (requestPath === "/ips138.asp") {
        return { status: 200, contentType: "text/html", bodyType: "text" }
    }
    if (["/myip", "/ip/getIpContry.do", "/api/skan/query_detail"].includes(requestPath)) {
        return { status: 200, contentType: "text/plain", bodyType: "text" }
    }
    if (requestPath.startsWith("/mobile/multilingual/ios/ios_") && requestPath.endsWith(".json")) {
        return { status: 200, contentType: "application/json", bodyType: "object" }
    }
    return { status: 200, contentType: "application/json", bodyType: "object" }
}

function classifyBinaryPath(pathValue, localReader, cnCdnBundle) {
    const requestPath = pathValue.split(/[?#]/, 1)[0] || "/"
    const sdkSource = localReader.read("core/personal-service/src/sdk_compat.rs")
    const emptyHtmlPaths = new Set(extractStringArray(sdkSource, "EMPTY_HTML_PATHS"))
    const httpSource = localReader.read("core/personal-service/src/http.rs")
    if (pathValue === "/") return {
        status: "runtime-authority-base",
        requestPath,
        contract: typedSdkContract(requestPath, sdkSource),
    }
    if (pathValue === "/api/") return { status: "runtime-prefix", requestPath, contract: null }
    if (pathValue === "/shijtswy/version/" && httpSource.includes("client_release_ios.dis")) {
        return { status: "runtime-prefix", requestPath, contract: { status: 200, contentType: "text/plain", bodyType: "text" } }
    }
    if (requestPath.startsWith("/terrace/") && sdkSource.includes('path.starts_with("/terrace/")')) {
        return { status: "typed-sdk-route", requestPath, contract: typedSdkContract(requestPath, sdkSource) }
    }
    if (requestPath.startsWith("/protocols/leiting/third/") && sdkSource.includes("/protocols/leiting/third/")) {
        return { status: "typed-sdk-route", requestPath, contract: typedSdkContract(requestPath, sdkSource) }
    }
    if (requestPath.startsWith("/mobile/multilingual/ios/ios_") && sdkSource.includes("SOBOT_LOCALIZATION_PREFIX")) {
        return { status: "typed-sdk-route", requestPath, contract: typedSdkContract(requestPath, sdkSource) }
    }
    if (pathValue.includes("%@")) {
        const familyCovered = emptyHtmlPaths.has(requestPath) || (
            requestPath === "/ips138.asp" && sdkSource.includes("IP138_PATH")
        ) || (
            pathValue.startsWith("/mobile/multilingual/ios/ios_") && sdkSource.includes("SOBOT_LOCALIZATION_PREFIX")
        ) || (
            pathValue.startsWith("/protocols/leiting/sensitive/part/") &&
            localReader.read("core/personal-service/src/cn_asset_files.rs").includes("GAME_SENSITIVE_VERSION_PATHS")
        ) || (
            pathValue.startsWith("/protocols/leiting/third/") && sdkSource.includes("/protocols/leiting/third/")
        ) || (
            pathValue.startsWith("/terrace/") && sdkSource.includes('path.starts_with("/terrace/")')
        )
        const contract = pathValue.startsWith("/protocols/leiting/sensitive/part/")
            ? { status: 302, contentType: "text/plain", bodyType: "empty" }
            : typedSdkContract(requestPath, sdkSource)
        return { status: familyCovered ? "runtime-template" : "missing", requestPath, contract }
    }
    const relativePath = decodeURIComponent(requestPath.replace(/^\/+/, ""))
    if (relativePath.length > 0 && existsSync(path.join(cnCdnBundle, ...relativePath.split("/")))) {
        return {
            status: "bundle-file",
            requestPath,
            contract: { status: 200, contentType: binaryContentType(relativePath), bodyType: "binary" },
        }
    }
    if (sdkSource.includes(`"${requestPath}"`)) return {
        status: "typed-sdk-route",
        requestPath,
        contract: typedSdkContract(requestPath, sdkSource),
    }
    return { status: "missing", requestPath, contract: null }
}

function auditIosBinarySurface(clientExecutable, localReader, cnCdnBundle, issues) {
    const extracted = extractBinaryUrls(clientExecutable)
    const grouped = new Map()
    for (const url of extracted.urls) {
        const parsed = splitUrl(url)
        if (!parsed || !isCnSdkHost(parsed.host)) continue
        const entry = grouped.get(parsed.rawPath) ?? { path: parsed.rawPath, urls: [] }
        entry.urls.push(url)
        grouped.set(parsed.rawPath, entry)
    }
    const paths = [...grouped.values()]
        .sort((left, right) => left.path.localeCompare(right.path))
        .map((entry) => ({
            ...entry,
            ...classifyBinaryPath(entry.path, localReader, cnCdnBundle),
        }))
    for (const entry of paths.filter((item) => item.status === "missing")) issues.push({
        status: "missing",
        surface: "ios-binary-url",
        id: entry.path,
        detail: "iOS 二进制 URL 没有对应的类型化 SDK, 静态文件或运行时模板处理.",
    })
    return {
        executable: clientExecutable,
        recoveredStringCount: extracted.strings.size,
        uniqueUrlCount: extracted.urls.length,
        relevantPathCount: paths.length,
        uncoveredPathCount: paths.filter((entry) => entry.status === "missing").length,
        statusCounts: statusCounts(paths),
        paths,
    }
}
// //// /核对 iOS 二进制 URL 与类型化兼容面 ////

// //// 核对静态挂载与本地资源层 [@x380kkm 2026-08-24] ////
function inspectDirectory(directoryPath) {
    if (!existsSync(directoryPath) || !statSync(directoryPath).isDirectory()) {
        return { path: directoryPath, exists: false, fileCount: 0, totalBytes: 0 }
    }
    let fileCount = 0
    let totalBytes = 0
    const pending = [directoryPath]
    while (pending.length > 0) {
        const current = pending.pop()
        for (const entry of readdirSync(current, { withFileTypes: true })) {
            const child = path.join(current, entry.name)
            if (entry.isDirectory()) pending.push(child)
            else if (entry.isFile()) {
                const metadata = statSync(child)
                fileCount += 1
                totalBytes += metadata.size
            }
        }
    }
    return { path: directoryPath, exists: true, fileCount, totalBytes }
}

function listDirectoryFiles(directoryPath) {
    if (!existsSync(directoryPath) || !statSync(directoryPath).isDirectory()) return []
    const files = []
    const pending = [directoryPath]
    while (pending.length > 0) {
        const current = pending.pop()
        for (const entry of readdirSync(current, { withFileTypes: true })) {
            const child = path.join(current, entry.name)
            if (entry.isDirectory()) pending.push(child)
            else if (entry.isFile()) {
                const metadata = statSync(child)
                files.push({
                    path: child,
                    relativePath: normalizePath(path.relative(directoryPath, child)),
                    size: metadata.size,
                })
            }
        }
    }
    return files.sort((left, right) => left.relativePath.localeCompare(right.relativePath))
}

function sha256File(filePath) {
    const descriptor = openSync(filePath, "r")
    const hash = createHash("sha256")
    const buffer = Buffer.allocUnsafe(4 * 1024 * 1024)
    try {
        let count = 0
        while ((count = readSync(descriptor, buffer, 0, buffer.length, null)) > 0) {
            hash.update(buffer.subarray(0, count))
        }
        return hash.digest("base64")
    } finally {
        closeSync(descriptor)
    }
}

function archiveRelativePath(location) {
    let pathname
    try {
        pathname = new URL(location).pathname
    } catch {
        pathname = normalizePath(location)
    }
    const parts = pathname.split("/").filter(Boolean)
    if (parts.length < 2) return null
    const directory = parts.at(-2)
    const fileName = parts.at(-1)
    if (!ARCHIVE_DIRECTORIES.has(directory) || !fileName.endsWith(".zip")) return null
    return `${directory}/${fileName}`
}

function isSafeRelativePath(relativePath) {
    return relativePath.split("/").every((component) => (
        component.length > 0 &&
        component !== "." &&
        component !== ".." &&
        !component.includes("\\") &&
        !component.includes(":") &&
        ![...component].some((character) => character.charCodeAt(0) < 32)
    ))
}

function inspectCdnBundle(bundlePath) {
    const directory = inspectDirectory(bundlePath)
    if (!directory.exists) return {
        ...directory,
        topLevels: [],
        routing: { rawFallbackFiles: 0, patchFiles: 0, overrideMappings: 0, unsafePaths: [] },
        manifest: { status: "missing", archiveCount: 0 },
    }
    const files = listDirectoryFiles(bundlePath)
    const fileByPath = new Map(files.map((file) => [file.relativePath, file]))
    const topLevelMap = new Map()
    for (const file of files) {
        const topLevel = file.relativePath.split("/", 1)[0]
        const summary = topLevelMap.get(topLevel) ?? { name: topLevel, fileCount: 0, totalBytes: 0 }
        summary.fileCount += 1
        summary.totalBytes += file.size
        topLevelMap.set(topLevel, summary)
    }
    const unsafePaths = files
        .filter((file) => !isSafeRelativePath(file.relativePath))
        .map((file) => file.relativePath)
    const patchFiles = files.filter((file) => {
        const [directoryName] = file.relativePath.split("/")
        return (ARCHIVE_DIRECTORIES.has(directoryName) && file.relativePath.endsWith(".zip")) ||
            (["entities", "EntityLists"].includes(directoryName) && file.relativePath.endsWith(".csv"))
    })
    const pathFile = fileByPath.get("path")
    if (!pathFile) return {
        ...directory,
        topLevels: [...topLevelMap.values()].sort((left, right) => left.name.localeCompare(right.name)),
        routing: {
            rawFallbackFiles: files.length - unsafePaths.length,
            patchFiles: patchFiles.length,
            overrideMappings: files.length - unsafePaths.length,
            unsafePaths,
        },
        manifest: { status: "missing", archiveCount: 0 },
    }

    let manifest
    try {
        manifest = JSON.parse(readFileSync(pathFile.path, "utf8"))
    } catch (error) {
        return {
            ...directory,
            topLevels: [...topLevelMap.values()].sort((left, right) => left.name.localeCompare(right.name)),
            routing: {
                rawFallbackFiles: files.length - unsafePaths.length,
                patchFiles: patchFiles.length,
                overrideMappings: files.length - unsafePaths.length,
                unsafePaths,
            },
            manifest: { status: "invalid", error: String(error), archiveCount: 0 },
        }
    }

    const fullArchives = Array.isArray(manifest.full?.archive) ? manifest.full.archive : null
    const diffGroups = Array.isArray(manifest.diff) ? manifest.diff : null
    const diffArchives = diffGroups?.every((group) => Array.isArray(group?.archive))
        ? diffGroups.flatMap((group) => group.archive)
        : null
    if (!fullArchives || !diffArchives) return {
        ...directory,
        topLevels: [...topLevelMap.values()].sort((left, right) => left.name.localeCompare(right.name)),
        routing: {
            rawFallbackFiles: files.length - unsafePaths.length,
            patchFiles: patchFiles.length,
            overrideMappings: files.length - unsafePaths.length,
            unsafePaths,
        },
        manifest: { status: "invalid", error: "path manifest archive groups are invalid", archiveCount: 0 },
    }

    const archiveEntries = [...fullArchives, ...diffArchives]
    const records = archiveEntries.map((entry) => ({
        entry,
        relativePath: archiveRelativePath(entry?.location),
    }))
    const invalidEntries = records.filter((record) => (
        record.relativePath === null ||
        !Number.isSafeInteger(record.entry?.size) ||
        record.entry.size < 0 ||
        typeof record.entry?.sha256 !== "string" ||
        Buffer.from(record.entry.sha256, "base64").length !== 32
    )).map((record) => record.entry?.location ?? null)
    const validRecords = records.filter((record) => !invalidEntries.includes(record.entry?.location ?? null))
    const missingArchives = []
    const sizeMismatches = []
    const digestMismatches = []
    for (const record of validRecords) {
        const file = fileByPath.get(record.relativePath)
        if (!file) {
            missingArchives.push(record.relativePath)
            continue
        }
        if (file.size !== record.entry.size) sizeMismatches.push(record.relativePath)
        else if (sha256File(file.path) !== record.entry.sha256) digestMismatches.push(record.relativePath)
    }
    const manifestPaths = new Set(validRecords.map((record) => record.relativePath))
    const unlistedArchives = files
        .filter((file) => ARCHIVE_DIRECTORIES.has(file.relativePath.split("/", 1)[0]))
        .filter((file) => file.relativePath.endsWith(".zip") && !manifestPaths.has(file.relativePath))
        .map((file) => file.relativePath)
    const platformCount = (directories) => validRecords
        .filter((record) => directories.has(record.relativePath.split("/", 1)[0])).length
    return {
        ...directory,
        topLevels: [...topLevelMap.values()].sort((left, right) => left.name.localeCompare(right.name)),
        routing: {
            rawFallbackFiles: files.length - unsafePaths.length,
            patchFiles: patchFiles.length,
            overrideMappings: files.length - unsafePaths.length,
            unsafePaths,
        },
        titleEntityLists: {
            android: fileByPath.has("entities/10939-android_medium.csv"),
            ios: fileByPath.has("entities/10939-ios_medium.csv"),
        },
        manifest: {
            status: "valid",
            archiveCount: validRecords.length,
            fullArchiveCount: fullArchives.length,
            diffArchiveCount: diffArchives.length,
            iosArchiveCount: platformCount(IOS_ARCHIVE_DIRECTORIES),
            androidArchiveCount: platformCount(ANDROID_ARCHIVE_DIRECTORIES),
            invalidEntries,
            missingArchives,
            sizeMismatches,
            digestMismatches,
            unlistedArchives,
        },
    }
}

function requireEvidence(reader, specification, surface, id, issues) {
    const evidence = locateEvidence(reader, specification)
    if (!evidence) issues.push({
        status: "missing",
        surface,
        id,
        detail: `缺少源码事实: ${specification.purpose}`,
    })
    return evidence
}

function auditStaticSurfaces(referenceReader, localReader, cnCdnBundle, explicitBundle, issues) {
    const referenceCdnEvidence = [
        { file: "resources/server/out/cn-server.js", needle: 'const CDN_BASE_URL = process.env.CDN_BASE_URL || `http://${cdnDisplayHost}:${cdnPort}/patch/cn`;', purpose: "参考客户端 CDN 基址为 /patch/cn." },
        { file: "resources/server/out/cn-server.js", needle: 'const cdnDir = process.env.CDN_DIR || ".cdn";', purpose: "参考 CDN 从 CDN_DIR 或 .cdn 读取." },
        { file: "resources/server/out/cn-server.js", needle: 'prefix: "/patch",', purpose: "参考端把 CDN 根挂载到 /patch." },
    ].map((specification) => requireEvidence(referenceReader, specification, "static-mount", "patch", issues)).filter(Boolean)
    const localCdnEvidence = [
        { file: "core/personal-service/src/cn_asset_files.rs", needle: 'const PATCH_PREFIX: &str = "/patch/cn/";', purpose: "本地客户端 CDN 前缀为 /patch/cn/." },
        { file: "core/personal-service/src/cn_asset_files.rs", needle: "read_static_asset(override_root, &relative_path, is_head)", purpose: "本地优先读取可写覆盖层." },
        { file: "core/personal-service/src/cn_asset_files.rs", needle: ".or_else(|| read_static_asset(asset_root, &relative_path, is_head))", purpose: "覆盖层未命中时读取包内 CDN." },
        { file: "core/personal-service/src/service.rs", needle: 'let cn_override_root = root_path.join("cdn").join("override");', purpose: "可写覆盖层位于 App 数据根的 cdn/override." },
        { file: "core/personal-service/src/cn_asset.rs", needle: "retain_platform_archives(&mut data, platform);", purpose: "path 清单按请求平台筛选归档." },
        { file: "core/personal-service/src/cn_asset.rs", needle: "ClientPlatform::Ios => IOS_TITLE_ENTITY_LIST_NAME", purpose: "iOS 标题页读取 iOS 实体清单." },
        { file: "scripts/protocol-lab/package_ios_personal_service.py", needle: 'CN_CDN_BUNDLE_ARCHIVE_PATH = "StarpointCNCDN"', purpose: "IPA 内资源目录为 StarpointCNCDN." },
        { file: "platforms/ios/PersonalServiceBootstrap/StarpointPersonalServiceBootstrap.m", needle: "return sourceURL.path;", purpose: "direct 模式直接把包内目录交给个人服务." },
    ].map((specification) => requireEvidence(localReader, specification, "static-mount", "patch", issues)).filter(Boolean)

    const referencePublicEvidence = [
        { file: "resources/server/out/cn-server.js", needle: 'root: path_1.default.join(__dirname, "..", "web", "public"),', purpose: "参考 /public 从 web/public 读取." },
        { file: "resources/server/out/cn-server.js", needle: 'prefix: "/public",', purpose: "参考管理静态资源前缀为 /public." },
    ].map((specification) => requireEvidence(referenceReader, specification, "static-mount", "public", issues)).filter(Boolean)
    const localPublicEvidence = [
        { file: "core/personal-service/src/management_web.rs", needle: 'const APP_JAVASCRIPT: &[u8] = include_bytes!("../web/management/app.js");', purpose: "本地把管理页脚本嵌入个人服务." },
        { file: "core/personal-service/src/management_web.rs", needle: '"/manage/app.js"', purpose: "本地管理静态资源使用 /manage 前缀." },
    ].map((specification) => requireEvidence(localReader, specification, "static-mount", "public", issues)).filter(Boolean)

    const referencePublicRoot = path.join(referenceReader.root, "resources", "server", "web", "public")
    if (!existsSync(referencePublicRoot)) issues.push({
        status: "missing",
        surface: "static-files",
        id: "reference-public-root",
        detail: `参考 web/public 目录不存在: ${referencePublicRoot}`,
    })
    const localManagementRoot = path.join(localReader.root, "core", "personal-service", "web", "management")
    if (!existsSync(localManagementRoot)) issues.push({
        status: "missing",
        surface: "static-files",
        id: "local-management-root",
        detail: `本地管理资源目录不存在: ${localManagementRoot}`,
    })

    const bundleInspection = inspectCdnBundle(cnCdnBundle)
    const requiredBundleEntries = [
        "path",
        "activity-catalog.json",
        "archive-common-full",
        "entities/10939-android_medium.csv",
        "entities/10939-ios_medium.csv",
        "management-assets/item-icons",
    ].map((relativePath) => ({
        relativePath,
        exists: bundleInspection.exists && existsSync(path.join(cnCdnBundle, ...relativePath.split("/"))),
    }))
    if (!bundleInspection.exists) issues.push({
        status: "missing",
        surface: "static-files",
        id: "cn-cdn-bundle",
        detail: `CN CDN 构建输入不存在: ${cnCdnBundle}`,
    })
    if (bundleInspection.exists) {
        for (const entry of requiredBundleEntries) {
            if (!entry.exists) issues.push({
                status: "missing",
                surface: "static-files",
                id: `cn-cdn-bundle:${entry.relativePath}`,
                detail: `CN CDN 包缺少构建与运行入口: ${entry.relativePath}`,
            })
        }
        if (bundleInspection.manifest.status !== "valid") issues.push({
            status: "missing",
            surface: "static-files",
            id: "cn-cdn-path-manifest",
            detail: `CN CDN path 清单不可用: ${bundleInspection.manifest.error ?? bundleInspection.manifest.status}`,
        })
        for (const [field, detail] of [
            ["invalidEntries", "path 清单包含无效归档记录"],
            ["missingArchives", "path 清单引用缺失归档"],
            ["sizeMismatches", "path 清单归档大小不匹配"],
            ["digestMismatches", "path 清单归档 SHA-256 不匹配"],
            ["unlistedArchives", "包内归档未进入 path 清单"],
        ]) {
            const entries = bundleInspection.manifest[field] ?? []
            if (entries.length > 0) issues.push({
                status: "different",
                surface: "static-files",
                id: `cn-cdn-path-manifest:${field}`,
                detail: `${detail}: ${entries.slice(0, 20).join(", ")}`,
            })
        }
        if ((bundleInspection.routing.unsafePaths ?? []).length > 0) issues.push({
            status: "different",
            surface: "static-files",
            id: "cn-cdn-unsafe-paths",
            detail: `CN CDN 包含无法映射到覆盖层的路径: ${bundleInspection.routing.unsafePaths.slice(0, 20).join(", ")}`,
        })
    }

    return {
        mounts: [{
            id: "patch",
            status: "explicit-local-extension",
            referencePrefix: "/patch, 客户端基址 /patch/cn.",
            localPrefix: "/patch/cn/.",
            referenceEvidence: referenceCdnEvidence,
            localEvidence: localCdnEvidence,
            relation: "本地直接以 CN 根提供相同 URL 空间, 并增加可写覆盖层与包内 CDN 回退.",
        }, {
            id: "public",
            status: "reference-exclusive-management-ui",
            referencePrefix: "/public.",
            localPrefix: "/manage.",
            referenceEvidence: referencePublicEvidence,
            localEvidence: localPublicEvidence,
            relation: "两者只服务各自管理界面, 不属于游戏客户端静态资源契约.",
        }],
        files: {
            referencePublic: inspectDirectory(referencePublicRoot),
            referenceDefaultCdn: inspectDirectory(path.join(referenceReader.root, "resources", "server", ".cdn")),
            localManagement: inspectDirectory(localManagementRoot),
            localCnCdnBundle: {
                ...bundleInspection,
                explicit: explicitBundle,
                requiredEntries: requiredBundleEntries,
                use: "由 build-ios-cn-candidate.ps1 传给 IPA 包装器并以 direct 模式读取.",
            },
        },
    }
}
// //// /核对静态挂载与本地资源层 ////

// //// 抽取参考二进制补丁位点 [@x380kkm 2026-08-24] ////
function wordBytes(value) {
    const bytes = Buffer.alloc(4)
    bytes.writeUInt32LE(Number.parseInt(value, 16))
    return bytes
}

function section(source, startNeedle, endNeedle = null) {
    const start = source.indexOf(startNeedle)
    if (start < 0) return null
    const end = endNeedle === null ? source.length : source.indexOf(endNeedle, start + startNeedle.length)
    if (end < 0) return null
    return { source: source.slice(start, end), start }
}

function addReferencePatchSite(sites, referenceReader, file, source, index, name, offset, sourceWords, targetWords) {
    sites.push({
        name,
        offset,
        sourceBytes: Buffer.concat(sourceWords.map(wordBytes)),
        targetBytes: Buffer.concat(targetWords.map(wordBytes)),
        reference: referenceReader.evidence(file, index, "参考 iOS 生产补丁位点."),
    })
}

export function extractReferencePatchSites(referenceReader, issues = []) {
    const file = "tools/patch-ipa.mjs"
    const source = referenceReader.read(file)
    const sites = []

    const guard = source.match(/readUInt32LE\((0x[0-9a-f]+)\) === (0x[0-9a-f]+)\) \{ buf\.writeUInt32LE\((0x[0-9a-f]+), \1\)/i)
    if (guard) addReferencePatchSite(sites, referenceReader, file, source, guard.index, "air_packaging_guard", Number.parseInt(guard[1], 16), [guard[2]], [guard[3]])
    else issues.push({ status: "missing", surface: "ios-patch", id: "air_packaging_guard", detail: "参考启动保护补丁无法抽取." })

    const allGuardDefault = source.match(/const GUARD_MODE = args\['guard-mode'\] \|\| 'all'/)
    const allGuardScanner = section(source, "function deguardBuffer", "function patchFirstLoginTip")
    if (allGuardDefault && allGuardScanner?.source.includes("safeFallthrough")) {
        for (const offset of REFERENCE_SAFE_FALLTHROUGH_GUARDS) {
            addReferencePatchSite(
                sites,
                referenceReader,
                file,
                source,
                allGuardDefault.index,
                LOCAL_PATCH_NAMES.get(offset),
                offset,
                ["0xb9000109"],
                ["0xd503201f"],
            )
        }
    } else issues.push({ status: "missing", surface: "ios-patch", id: "all_guard_scanner", detail: "参考默认全 guard 补丁无法抽取." })

    const firstLogin = section(source, "function patchFirstLoginTip", "function patchLoginDialog")
    const firstMatch = firstLogin?.source.match(/const OFF = (0x[0-9a-f]+);[\s\S]*?readUInt32LE\(OFF\) !== (0x[0-9a-f]+)[\s\S]*?writeUInt32LE\((0x[0-9a-f]+), OFF\);[\s\S]*?writeUInt32LE\((0x[0-9a-f]+), OFF \+ 4\)/i)
    if (firstMatch) addReferencePatchSite(sites, referenceReader, file, source, firstLogin.start + firstMatch.index, "first_login_tip", Number.parseInt(firstMatch[1], 16), [firstMatch[2]], [firstMatch[3], firstMatch[4]])
    else issues.push({ status: "missing", surface: "ios-patch", id: "first_login_tip", detail: "参考实名提示补丁无法抽取." })

    const login = section(source, "function patchLoginDialog", "function patchWelcomeBanner")
    const loginArray = login?.source.match(/const sites = \[([^\n]+)\]/)
    const loginNop = login?.source.match(/const NOP = (0x[0-9a-f]+)/i)
    if (loginArray && loginNop) {
        const pairs = [...loginArray[1].matchAll(/\[(0x[0-9a-f]+), (0x[0-9a-f]+)\]/gi)]
        for (const pair of pairs) {
            const offset = Number.parseInt(pair[1], 16)
            addReferencePatchSite(sites, referenceReader, file, source, login.start + loginArray.index + pair.index, LOCAL_PATCH_NAMES.get(offset), offset, [pair[2]], [loginNop[1]])
        }
    } else issues.push({ status: "missing", surface: "ios-patch", id: "login_dialog", detail: "参考登录分支补丁无法抽取." })

    const welcome = section(source, "function patchWelcomeBanner", "function patchAgreementDialogs")
    const welcomeSpecs = [
        ["MAIN", "login_manager_welcome", /const MAIN = (0x[0-9a-f]+);[\s\S]*?readUInt32LE\(MAIN\) === (0x[0-9a-f]+)[\s\S]*?writeUInt32LE\((0x[0-9a-f]+), MAIN\);[\s\S]*?writeUInt32LE\((0x[0-9a-f]+), MAIN \+ 4\)/i],
        ["CLS", "lt_welcome_view", /const CLS = (0x[0-9a-f]+);[\s\S]*?readUInt32LE\(CLS\) === (0x[0-9a-f]+)[\s\S]*?writeUInt32LE\((0x[0-9a-f]+), CLS\);[\s\S]*?writeUInt32LE\((0x[0-9a-f]+), CLS \+ 4\)/i],
        ["CALL", "login_welcome_view_call", /const CALL = (0x[0-9a-f]+);[\s\S]*?readUInt32LE\(CALL\) === (0x[0-9a-f]+)[\s\S]*?writeUInt32LE\((0x[0-9a-f]+), CALL\)/i],
    ]
    for (const [, name, pattern] of welcomeSpecs) {
        const match = welcome?.source.match(pattern)
        if (match) addReferencePatchSite(sites, referenceReader, file, source, welcome.start + match.index, name, Number.parseInt(match[1], 16), [match[2]], match.slice(3))
        else issues.push({ status: "missing", surface: "ios-patch", id: name, detail: `参考 ${name} 补丁无法抽取.` })
    }

    const agreements = section(source, "function patchAgreementDialogs", "const PATCH_AGREEMENT")
    const eula = agreements?.source.match(/const EULA = (0x[0-9a-f]+);[\s\S]*?readUInt32LE\(EULA\) === (0x[0-9a-f]+)\) \{ buf\.writeUInt32LE\((0x[0-9a-f]+), EULA\)/i)
    if (eula) addReferencePatchSite(sites, referenceReader, file, source, agreements.start + eula.index, "login_license_gate", Number.parseInt(eula[1], 16), [eula[2]], [eula[3]])
    else issues.push({ status: "missing", surface: "ios-patch", id: "login_license_gate", detail: "参考许可协议补丁无法抽取." })

    const privacy = agreements?.source.match(/for \(const off of \[([^\]]+)\]\)[\s\S]*?readUInt32LE\(off\) === (0x[0-9a-f]+)[\s\S]*?writeUInt32LE\((0x[0-9a-f]+), off\);[\s\S]*?writeUInt32LE\((0x[0-9a-f]+), off \+ 4\)/i)
    if (privacy) {
        for (const offsetText of privacy[1].match(/0x[0-9a-f]+/gi) ?? []) {
            const offset = Number.parseInt(offsetText, 16)
            addReferencePatchSite(sites, referenceReader, file, source, agreements.start + privacy.index, LOCAL_PATCH_NAMES.get(offset), offset, [privacy[2]], [privacy[3], privacy[4]])
        }
    } else issues.push({ status: "missing", surface: "ios-patch", id: "privacy_gates", detail: "参考隐私 gate 补丁无法抽取." })

    const bundleId = section(source, "function patchBundleIdCheck", "const MACHO_REL")
    const bundleIdMatch = bundleId?.source.match(/const OFF = (0x[0-9a-f]+);[\s\S]*?readUInt32LE\(OFF\) !== (0x[0-9a-f]+)[\s\S]*?writeUInt32LE\((0x[0-9a-f]+), OFF\);[\s\S]*?writeUInt32LE\((0x[0-9a-f]+), OFF \+ 4\);[\s\S]*?writeUInt32LE\((0x[0-9a-f]+), OFF \+ 8\);[\s\S]*?writeUInt32LE\((0x[0-9a-f]+), OFF \+ 12\)/i)
    if (bundleIdMatch) addReferencePatchSite(
        sites,
        referenceReader,
        file,
        source,
        bundleId.start + bundleIdMatch.index,
        "air_bundle_identifier_resource_check",
        Number.parseInt(bundleIdMatch[1], 16),
        [bundleIdMatch[2]],
        bundleIdMatch.slice(3),
    )
    else issues.push({ status: "missing", surface: "ios-patch", id: "bundle_identifier_resource_check", detail: "参考 bundle ID 资源检查补丁无法抽取." })

    return sites.sort((left, right) => left.offset - right.offset)
}

function auditReferencePatchRoutines(referenceReader, issues) {
    const file = "tools/patch-ipa.mjs"
    const source = referenceReader.read(file)
    const definitions = [...source.matchAll(/\bfunction\s+(patch[A-Za-z0-9_]+|deguardBuffer)\s*\(/g)]
    const routines = definitions.map((definition) => {
        const name = definition[1]
        const invocationPattern = new RegExp(`\\b${name}\\s*\\(`, "g")
        const invocationCount = [...source.matchAll(invocationPattern)].length - 1
        const status = !REFERENCE_PATCH_ROUTINES.has(name)
            ? "missing"
            : invocationCount < 2
                ? "different"
                : "matched"
        const routine = {
            name,
            invocationCount,
            status,
            reference: referenceReader.evidence(file, definition.index, "参考补丁例程及两种输入模式调用."),
        }
        if (FAILURE_STATUSES.has(status)) issues.push({
            status,
            surface: "ios-patch-routine",
            id: name,
            detail: status === "missing"
                ? "参考 patcher 出现未纳入审计模型的补丁例程."
                : "参考补丁例程没有同时进入 bin 和 IPA 处理路径.",
            reference: routine.reference,
        })
        return routine
    })
    const found = new Set(routines.map((routine) => routine.name))
    for (const name of REFERENCE_PATCH_ROUTINES) {
        if (found.has(name)) continue
        issues.push({
            status: "missing",
            surface: "ios-patch-routine",
            id: name,
            detail: "参考 patcher 缺少已建模的生产补丁例程.",
        })
    }
    const longjmpMatches = [...source.matchAll(/args\['crash-longjmp'\][\s\S]{0,160}?writeUInt32LE\(0xd4200000,\s*0x57d834c\)/g)]
    if (longjmpMatches.length !== 2) issues.push({
        status: "different",
        surface: "ios-patch-routine",
        id: "crash-longjmp",
        detail: "参考可选 longjmp 诊断补丁没有同时进入 bin 和 IPA 处理路径.",
    })
    return {
        routines,
        optionalInlinePatches: [{
            name: "crash-longjmp",
            invocationCount: longjmpMatches.length,
            status: longjmpMatches.length === 2 ? "reference-optional-tooling" : "different",
            reference: longjmpMatches[0]
                ? referenceReader.evidence(file, longjmpMatches[0].index, "参考可选 longjmp 诊断补丁.")
                : null,
        }],
    }
}

function parseLocalBinaryPatches(localReader) {
    const file = "scripts/protocol-lab/ios_cn_compatibility_patch.py"
    const source = localReader.read(file)
    const pattern = /BinaryPatch\(\s*"([^"]+)",\s*(0x[0-9A-Fa-f]+),\s*bytes\.fromhex\(\s*((?:"[0-9A-Fa-f]+"\s*)+)\),\s*bytes\.fromhex\(\s*((?:"[0-9A-Fa-f]+"\s*)+)\),\s*(\d+),\s*(\d+),\s*\)/g
    const patches = new Map()
    for (const match of source.matchAll(pattern)) {
        const sourceHex = [...match[3].matchAll(/"([0-9A-Fa-f]+)"/g)].map((part) => part[1]).join("")
        const targetHex = [...match[4].matchAll(/"([0-9A-Fa-f]+)"/g)].map((part) => part[1]).join("")
        const changeOffset = Number(match[5])
        const changeLength = Number(match[6])
        const offset = Number.parseInt(match[2], 16)
        patches.set(offset, {
            name: match[1],
            offset,
            sourceBytes: Buffer.from(sourceHex, "hex").subarray(changeOffset, changeOffset + changeLength),
            targetBytes: Buffer.from(targetHex, "hex").subarray(changeOffset, changeOffset + changeLength),
            local: localReader.evidence(file, match.index, "本地 iOS 固定位点兼容补丁."),
        })
    }
    return patches
}

function auditFixedPatchSites(referenceReader, localReader, issues) {
    const referenceSites = extractReferencePatchSites(referenceReader, issues)
    const localPatches = parseLocalBinaryPatches(localReader)
    return referenceSites.map((site) => {
        const local = localPatches.get(site.offset)
        let status = "matched"
        let detail = "位点, 原始指令前缀和目标指令一致."
        if (!local) {
            status = "missing"
            detail = "本地兼容补丁缺少参考位点."
        } else if (
            local.name !== site.name
            || local.sourceBytes.length < site.sourceBytes.length
            || !local.sourceBytes.subarray(0, site.sourceBytes.length).equals(site.sourceBytes)
            || !local.targetBytes.equals(site.targetBytes)
        ) {
            status = "different"
            detail = "本地位点名称, 原始指令或目标指令与参考补丁不同."
        }
        if (FAILURE_STATUSES.has(status)) issues.push({
            status,
            surface: "ios-patch",
            id: site.name,
            detail,
            reference: site.reference,
            local: local?.local ?? null,
        })
        return {
            name: site.name,
            offset: `0x${site.offset.toString(16)}`,
            status,
            detail,
            reference: site.reference,
            local: local?.local ?? null,
        }
    })
}
// //// /抽取参考二进制补丁位点 ////

// //// 核对地址改写, 包装和签名假设 [@x380kkm 2026-08-24] ////
function auditFact(readers, definition, issues) {
    const reference = (definition.reference ?? []).map((specification) => locateEvidence(readers.reference, specification))
    const local = (definition.local ?? []).map((specification) => locateEvidence(readers.local, specification))
    const hasMissingEvidence = reference.some((item) => item === null) || local.some((item) => item === null)
    const status = hasMissingEvidence ? "missing" : definition.status
    if (hasMissingEvidence) issues.push({
        status: "missing",
        surface: definition.surface,
        id: definition.id,
        detail: "对照事实缺少源码证据.",
    })
    return {
        id: definition.id,
        status,
        detail: definition.detail,
        reference: reference.filter(Boolean),
        local: local.filter(Boolean),
    }
}

function auditIosSurface(referenceReader, localReader, issues) {
    const readers = { reference: referenceReader, local: localReader }
    const fixedPatches = auditFixedPatchSites(referenceReader, localReader, issues)
    const patchRoutines = auditReferencePatchRoutines(referenceReader, issues)
    const facts = [
        {
            id: "endpoint-domain-coverage",
            surface: "ios-address-rewrite",
            status: "matched",
            detail: "参考 leiting, roguelike 和 cl2009 域名均由本地 AOT 或原生请求路由覆盖.",
            reference: [{ file: "tools/patch-ipa.mjs", needle: "leiting\\.com|roguelike\\.com|cl2009\\.com", purpose: "参考地址改写域名集合." }],
            local: [
                { file: "scripts/protocol-lab/ios_cn_aot_patch.py", needle: "(?:leiting\\.com|roguelike\\.com)", purpose: "本地 AOT 地址改写域名集合." },
                { file: "platforms/ios/PersonalServiceBootstrap/StarpointPersonalServiceBootstrap.m", needle: '[normalizedHost hasSuffix:@".cl2009.com"]', purpose: "本地原生登录请求覆盖 cl2009.com." },
            ],
        },
        {
            id: "same-length-authority-rewrite",
            surface: "ios-address-rewrite",
            status: "matched",
            detail: "两端都使用 userinfo 填充保持 authority 字节长度不变.",
            reference: [{ file: "tools/patch-ipa.mjs", needle: "const paddedAuth = deficit === 0 ? TARGET", purpose: "参考等长 authority 写入." }],
            local: [
                { file: "scripts/protocol-lab/ios_cn_aot_patch.py", needle: "def pad_authority_with_userinfo", purpose: "本地 authority 填充函数." },
                { file: "scripts/protocol-lab/ios_cn_aot_patch.py", needle: "def replace_equal_length", purpose: "本地等长写入边界." },
            ],
        },
        {
            id: "in-process-loopback-topology",
            surface: "ios-address-rewrite",
            status: "explicit-local-extension",
            detail: "参考目标由外部 HOST:PORT 提供. 本地固定路由到同进程 127.0.0.1:17171.",
            reference: [{ file: "tools/patch-ipa.mjs", needle: "const HOST = args.host, PORT = args.port, OUT = args.out;", purpose: "参考外部服务地址参数." }],
            local: [
                { file: "scripts/protocol-lab/ios_cn_aot_patch.py", needle: 'PERSONAL_SERVICE_AUTHORITY = "127.0.0.1:17171"', purpose: "本地固定个人服务 authority." },
                { file: "platforms/ios/PersonalServiceBootstrap/StarpointPersonalServiceBootstrap.m", needle: "static const uint16_t StarpointPersonalServicePort = 17171;", purpose: "原生个人服务固定端口." },
            ],
        },
        {
            id: "public-ip-probe",
            surface: "ios-address-rewrite",
            status: "explicit-local-extension",
            detail: "参考把 sohu 探测导向拒绝连接端口. 本地导向 /cityjson 并返回合法 JavaScript.",
            reference: [{ file: "tools/patch-ipa.mjs", needle: "const DEAD_HOST_PORT = '127.0.0.1:1';", purpose: "参考公网 IP 探测阻断目标." }],
            local: [
                { file: "scripts/protocol-lab/ios_cn_compatibility_patch.py", needle: 'b"http://127.0.0.1:17171/cityjson?ie=u8"', purpose: "本地公网 IP 探测目标." },
                { file: "core/personal-service/src/sdk_compat.rs", needle: 'const CITY_JSON_BODY: &str = r#"var returnCitySN', purpose: "本地 /cityjson 类型化正文." },
            ],
        },
        {
            id: "native-login-runtime-routing",
            surface: "ios-address-rewrite",
            status: "explicit-local-extension",
            detail: "本地原生层补获残留登录域名和 127.0.0.1:443 TLS 失败地址.",
            local: [
                { file: "platforms/ios/PersonalServiceBootstrap/StarpointPersonalServiceBootstrap.m", needle: "static BOOL isFailedLoopbackTlsURL", purpose: "本地残留 TLS 地址识别." },
                { file: "platforms/ios/PersonalServiceBootstrap/StarpointPersonalServiceBootstrap.m", needle: "static NSURL *routeCnLoginSdkURL", purpose: "本地 NSURLSession 登录请求路由." },
            ],
        },
        {
            id: "sobot-runtime-surface",
            surface: "ios-address-rewrite",
            status: "explicit-local-extension",
            detail: "本地额外把已观测 Sobot authority 路由到个人服务.",
            local: [{ file: "scripts/protocol-lab/ios_cn_aot_patch.py", needle: "EXPECTED_SOBOT_AUTHORITY_COUNTS", purpose: "本地 Sobot authority 完整集合." }],
        },
        {
            id: "fixed-sdk-url-cardinality",
            surface: "ios-address-rewrite",
            status: "explicit-local-extension",
            detail: "参考允许可选 host 白名单. 本地要求目标二进制恰有 148 个雷霆 SDK authority.",
            reference: [{ file: "tools/patch-ipa.mjs", needle: "const HOSTS = args.hosts", purpose: "参考可选 host 白名单." }],
            local: [{ file: "scripts/protocol-lab/ios_cn_aot_patch.py", needle: "EXPECTED_SDK_URL_COUNT = 148", purpose: "本地固定 SDK authority 数量." }],
        },
        {
            id: "agreement-defaults",
            surface: "ios-patch",
            status: "matched",
            detail: "参考许可协议与两处隐私 gate 默认启用. 本地兼容补丁集固定应用相同位点.",
            reference: [
                { file: "tools/patch-ipa.mjs", needle: "const PATCH_AGREEMENT = args['agreement'] !== 'false'", purpose: "参考许可协议默认启用." },
                { file: "tools/patch-ipa.mjs", needle: "const PATCH_PRIVACY = args['privacy'] !== 'false'", purpose: "参考隐私 gate 默认启用." },
            ],
            local: [{ file: "scripts/protocol-lab/ios_cn_compatibility_patch.py", needle: "for patch in CN_1_8_4_COMPATIBILITY_PATCHES:", purpose: "本地应用完整固定补丁集." }],
        },
        {
            id: "all-guard-scanner",
            surface: "ios-patch",
            status: "matched",
            detail: "参考默认处理全部安全落空的故意崩溃 guard. 本地以固定上下文应用同一组 iOS 1.8.4 位点.",
            reference: [{ file: "tools/patch-ipa.mjs", needle: "const GUARD_MODE = args['guard-mode'] || 'all'", purpose: "参考默认启用全 guard 扫描器." }],
            local: [
                { file: "scripts/protocol-lab/ios_cn_compatibility_patch.py", needle: '"air_safe_fallthrough_guard_0008e6f0"', purpose: "本地固定首个安全落空 guard." },
                { file: "scripts/protocol-lab/ios_cn_compatibility_patch.py", needle: '"air_safe_fallthrough_guard_00533254"', purpose: "本地固定末个安全落空 guard." },
            ],
        },
        {
            id: "longjmp-diagnostic",
            surface: "ios-patch",
            status: "reference-optional-tooling",
            detail: "参考 crash-longjmp 只把异常转换为诊断崩溃, 不属于生产兼容补丁.",
            reference: [{ file: "tools/patch-ipa.mjs", needle: "args['crash-longjmp']", purpose: "参考可选 longjmp 诊断补丁." }],
        },
        {
            id: "macho-discovery",
            surface: "ios-packaging",
            status: "explicit-local-extension",
            detail: "参考固定 Payload/worldflipper.app/worldflipper. 本地从 IPA 和 Info.plist 发现 App 与可执行文件.",
            reference: [{ file: "tools/patch-ipa.mjs", needle: "const MACHO_REL = 'Payload/worldflipper.app/worldflipper';", purpose: "参考固定 Mach-O 路径." }],
            local: [
                { file: "scripts/protocol-lab/package_ios_personal_service.py", needle: "app_root = find_app_root(source)", purpose: "本地发现 App 根目录." },
                { file: "scripts/protocol-lab/package_ios_personal_service.py", needle: 'executable_name = info.get("CFBundleExecutable")', purpose: "本地从 Info.plist 发现可执行文件." },
            ],
        },
        {
            id: "decrypted-arm64-input",
            surface: "ios-packaging",
            status: "matched",
            detail: "两端都要求可重签的解密 Mach-O. 本地额外拒绝非单 arm64 输入.",
            reference: [{ file: "tools/patch-ipa.mjs", needle: "cryptid=0 (decrypted dump)", purpose: "参考解密二进制假设." }],
            local: [
                { file: "scripts/protocol-lab/package_ios_personal_service.py", needle: 'if executable_analysis["fairplay_encrypted"]:', purpose: "本地拒绝 FairPlay 加密输入." },
                { file: "scripts/protocol-lab/package_ios_personal_service.py", needle: 'if len(executable_analysis["slices"]) != 1:', purpose: "本地要求单一架构 slice." },
            ],
        },
        {
            id: "zip-entry-attributes",
            surface: "ios-packaging",
            status: "matched",
            detail: "两端重写 Mach-O 时都保留原 ZIP 条目属性和执行权限.",
            reference: [{ file: "tools/patch-ipa.mjs", needle: "exe.externalAttr / exe.versionMadeBy are kept", purpose: "参考保留原 ZIP 条目属性." }],
            local: [{ file: "scripts/protocol-lab/package_ios_personal_service.py", needle: "output.writestr(entry, patched_executable)", purpose: "本地复用原 ZipInfo 写入 Mach-O." }],
        },
        {
            id: "embedded-personal-service",
            surface: "ios-packaging",
            status: "explicit-local-extension",
            detail: "本地向 Mach-O 注入个人服务 Framework, 使服务与游戏同进程运行.",
            local: [
                { file: "scripts/protocol-lab/package_ios_personal_service.py", needle: "patched_executable, patched_layout = inject_load_dylib", purpose: "本地注入 Framework load command." },
                { file: "scripts/protocol-lab/package_ios_personal_service.py", needle: "add_framework_files(output, framework_path", purpose: "本地把 Framework 写入 IPA." },
            ],
        },
        {
            id: "embedded-cn-cdn",
            surface: "ios-packaging",
            status: "explicit-local-extension",
            detail: "参考依赖外部服务器 .cdn. 本地构建要求非空 StarpointCNCDN 并以 direct 模式读取.",
            reference: [{ file: "resources/server/out/cn-server.js", needle: 'const cdnDir = process.env.CDN_DIR || ".cdn";', purpose: "参考外部 CDN 目录." }],
            local: [
                { file: "scripts/protocol-lab/build-ios-cn-candidate.ps1", needle: "[Parameter(Mandatory)][string]$CnCdnBundle", purpose: "本地候选构建要求 CN CDN 输入." },
                { file: "scripts/protocol-lab/package_ios_personal_service.py", needle: 'CN_CDN_BUNDLE_MODE_DIRECT = "direct"', purpose: "本地包内 CDN 使用 direct 模式." },
            ],
        },
        {
            id: "development-signing",
            surface: "ios-signing",
            status: "explicit-local-extension",
            detail: "参考把重签交给 Sideloadly 或 AltStore. 本地提供证书, profile, Team ID 与设备 UDID 校验后的 macOS codesign 流程.",
            reference: [{ file: "tools/patch-ipa.mjs", needle: "IPA MUST be re-signed", purpose: "参考要求重签." }],
            local: [
                { file: "scripts/protocol-lab/package_ios_personal_service.py", needle: '"requires_resigning": True', purpose: "本地候选报告声明需要重签." },
                { file: "platforms/ios/sign-device-ipa-with-task-keychain.sh", needle: 'raise SystemExit("profile does not contain the target UDID")', purpose: "本地签名前验证目标设备." },
                { file: "platforms/ios/sign-device-ipa.sh", needle: "codesign --verify --deep --strict --verbose=2", purpose: "本地验证完整 App 签名树." },
            ],
        },
    ].map((definition) => auditFact(readers, definition, issues))
    return { fixedPatches, patchRoutines, facts }
}
// //// /核对地址改写, 包装和签名假设 ////

// //// 生成非游戏运行面报告 [@x380kkm 2026-08-24] ////
function statusCounts(items) {
    const counts = {}
    for (const item of items) counts[item.status] = (counts[item.status] ?? 0) + 1
    return counts
}

export function buildReferenceNonGameSurfaceAudit({
    localRoot = DEFAULT_LOCAL_ROOT,
    referenceRoot = DEFAULT_REFERENCE_ROOT,
    cnCdnBundle = DEFAULT_CN_CDN_BUNDLE,
    clientExecutable = null,
    explicitBundle = false,
} = {}) {
    const localReader = createSourceReader(path.resolve(localRoot), "local")
    const referenceReader = createSourceReader(path.resolve(referenceRoot), "reference")
    const issues = []
    const managementSurface = extractReferenceManagementSurface(referenceReader, issues)
    const registrations = managementSurface.registrations
    const managementRoutes = auditManagementRoutes(managementSurface.routes, localReader, issues)
    const sdk = auditIosSdkRoutes(referenceReader, localReader, issues)
    const staticSurface = auditStaticSurfaces(
        referenceReader,
        localReader,
        path.resolve(cnCdnBundle),
        explicitBundle,
        issues,
    )
    const iosBinary = clientExecutable === null
        ? null
        : auditIosBinarySurface(path.resolve(clientExecutable), localReader, path.resolve(cnCdnBundle), issues)
    const ios = auditIosSurface(referenceReader, localReader, issues)
    const comparable = [
        ...managementRoutes.map((route) => ({ status: route.classification })),
        ...staticSurface.mounts,
        ...sdk.routes,
        ...sdk.extensions,
        ...(iosBinary?.paths ?? []),
        ...ios.fixedPatches,
        ...ios.patchRoutines.routines,
        ...ios.patchRoutines.optionalInlinePatches,
        ...ios.facts,
    ]
    const failureCount = issues.filter((issue) => FAILURE_STATUSES.has(issue.status)).length
    return {
        schemaVersion: 1,
        status: failureCount === 0 ? "passed" : "failed",
        roots: {
            local: localReader.root,
            reference: referenceReader.root,
            cnCdnBundle: path.resolve(cnCdnBundle),
            clientExecutable: clientExecutable === null ? null : path.resolve(clientExecutable),
        },
        summary: {
            referenceManagementRouteCount: managementRoutes.length,
            referenceManagementRegistrationCount: registrations.length,
            referenceStaticMountCount: staticSurface.mounts.length,
            referenceIosSdkRouteCount: sdk.routes.length,
            iosBinaryPathCount: iosBinary?.relevantPathCount ?? 0,
            iosBinaryUncoveredPathCount: iosBinary?.uncoveredPathCount ?? 0,
            referenceFixedIosPatchSiteCount: ios.fixedPatches.length,
            referenceIosPatchRoutineCount: ios.patchRoutines.routines.length,
            comparisonStatusCounts: statusCounts(comparable),
            missing: issues.filter((issue) => issue.status === "missing").length,
            different: issues.filter((issue) => issue.status === "different").length,
            failureCount,
        },
        management: { registrations, routes: managementRoutes },
        sdk,
        staticSurface,
        iosBinary,
        ios,
        issues,
    }
}
// //// /生成非游戏运行面报告 ////

// //// 解析命令行并写出报告 [@x380kkm 2026-08-24] ////
function parseArguments(args) {
    const options = {
        localRoot: DEFAULT_LOCAL_ROOT,
        referenceRoot: DEFAULT_REFERENCE_ROOT,
        cnCdnBundle: DEFAULT_CN_CDN_BUNDLE,
        clientExecutable: null,
        explicitBundle: false,
        report: null,
    }
    const names = new Map([
        ["--local-root", "localRoot"],
        ["--reference-root", "referenceRoot"],
        ["--cn-cdn-bundle", "cnCdnBundle"],
        ["--client-executable", "clientExecutable"],
        ["--report", "report"],
    ])
    for (let index = 0; index < args.length; index += 2) {
        const name = args[index]
        const property = names.get(name)
        const value = args[index + 1]
        if (!property) throw new Error(`unknown option: ${name}`)
        if (!value || value.startsWith("--")) throw new Error(`missing option value: ${name}`)
        options[property] = path.resolve(value)
        if (name === "--cn-cdn-bundle") options.explicitBundle = true
    }
    return options
}

function main() {
    const options = parseArguments(process.argv.slice(2))
    const report = buildReferenceNonGameSurfaceAudit(options)
    const serialized = `${JSON.stringify(report, null, 2)}\n`
    if (options.report) {
        mkdirSync(path.dirname(options.report), { recursive: true })
        writeFileSync(options.report, serialized, "utf8")
    }
    process.stdout.write(serialized)
    if (report.summary.failureCount > 0) process.exitCode = 1
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main()
// //// /解析命令行并写出报告 ////
