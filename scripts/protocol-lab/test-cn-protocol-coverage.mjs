// audience: internal
// # cn-protocol-coverage-tests
//
// 该脚本验证 CN 协议账本的客户端版本, 处理器, 证据等级, 动态证据路由和 P0 完整性.

import assert from "node:assert/strict"
import { existsSync, readFileSync } from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { validateEvidenceRoute } from "./cn-evidence.mjs"

const SCRIPT_ROOT = path.dirname(fileURLToPath(import.meta.url))
const REPOSITORY_ROOT = path.resolve(SCRIPT_ROOT, "../..")
const COVERAGE_PATH = path.join(SCRIPT_ROOT, "cn-protocol-coverage.json")
const CLIENT_IDS = ["cn-android-1.8.1", "cn-ios-1.8.4"]
const REQUIRED_ENTRY_IDS = [
    "cn.version.android",
    "cn.version.ios",
    "cn.asset.entity_list",
    "cn.auth.leiting_login",
    "cn.auth.leiting_antiaddiction_login",
    "cn.account.signup",
    "cn.account.load",
    "cn.tutorial.update_step",
    "cn.story.finish_with_skip",
    "cn.single_battle.start",
    "cn.single_battle.finish",
    "cn.gacha.exec",
    "cn.shop.recover_stamina",
    "cn.mail.receive",
    "instance.shell.slots",
    "instance.save.portable_transfer",
]
const VALID_EVIDENCE_LEVELS = new Set(["none", "not-applicable", "static", "dynamic"])
const VALID_IMPLEMENTATION_STATES = new Set(["missing", "partial", "covered", "not-applicable"])
const VALID_AUTHORITIES = new Set(["gateway", "personal-service", "server", "unsupported"])
const VALID_STATUSES = new Set([
    "not-implemented",
    "structural-blocker",
    "partial",
    "ready-for-dynamic",
    "verified",
])

// //// 保持动态证据引用和已捕获路由一致 [@x380kkm 2026-07-27] ////
const DYNAMIC_EVIDENCE_ROUTES = new Map([
    [
        "protocol-lab:android-cn-cold-start-replay-20260813T105011Z",
        new Set([
            "GET /shijtswy/version/client_release_android.dis",
            "POST /api/index.php/channels/channel_leiting/leiting_login",
            "POST /api/index.php/channels/channel_leiting/leiting_antiaddiction_login",
            "POST /api/index.php/tool/signup",
            "POST /api/index.php/load",
            "POST /api/index.php/asset/get_path",
            "POST /api/index.php/asset/version_info",
        ]),
    ],
    [
        "protocol-lab:android-cn-m4-isolated-service-cold-start-20260813T141113Z",
        new Set([
            "POST /api/index.php/channels/channel_leiting/leiting_login",
            "POST /api/index.php/channels/channel_leiting/leiting_antiaddiction_login",
            "POST /api/index.php/tool/signup",
            "POST /api/index.php/load",
            "POST /api/index.php/channels/channel_leiting_pay/query_unfinish_order",
        ]),
    ],
    [
        "protocol-lab:android-cn-strict-paired-restart-20260818",
        new Set([
            "POST /api/index.php/channels/channel_leiting/leiting_login",
            "POST /api/index.php/channels/channel_leiting/leiting_antiaddiction_login",
            "POST /api/index.php/tool/signup",
            "POST /api/index.php/load",
            "POST /api/index.php/channels/channel_leiting_pay/query_unfinish_order",
        ]),
    ],
    [
        "protocol-lab:android-cn-title-login-gate-paired-20260813T170142Z-pcap",
        new Set([
            "GET /shijtswy/version/client_release_android.dis",
            "POST /api/index.php/channels/channel_leiting/leiting_login",
            "POST /api/index.php/channels/channel_leiting/leiting_antiaddiction_login",
            "POST /api/index.php/tool/signup",
            "POST /api/index.php/load",
            "POST /api/index.php/asset/get_path",
            "POST /api/index.php/asset/version_info",
        ]),
    ],
    [
        "protocol-lab:android-cn-version-query-and-login-cdn-20260813T063945Z-pcap",
        new Set([
            "GET /shijtswy/version/client_release_android.dis",
            "POST /api/index.php/channels/channel_leiting/leiting_login",
            "POST /api/index.php/channels/channel_leiting/leiting_antiaddiction_login",
            "POST /api/index.php/tool/signup",
            "POST /api/index.php/load",
            "POST /api/index.php/asset/get_path",
            "POST /api/index.php/asset/version_info",
        ]),
    ],
    [
        "protocol-lab:android-cn-version-query-dynamic-20260813T054914Z-pcap",
        new Set(["GET /shijtswy/version/client_release_android.dis"]),
    ],
    [
        "protocol-lab:android-cn-personal-service-cold-start-pcap",
        new Set([
            "POST /api/index.php/channels/channel_leiting/leiting_login",
            "POST /api/index.php/channels/channel_leiting/leiting_antiaddiction_login",
            "POST /api/index.php/tool/signup",
            "POST /api/index.php/load",
            "POST /api/index.php/channels/channel_leiting_pay/query_unfinish_order",
            "POST /api/index.php/tutorial/update_step",
            "POST /api/index.php/channels/channel_leiting/leiting_update",
        ]),
    ],
    [
        "protocol-lab:android-cn-cold-start-http-metadata",
        new Set([
            "POST /api/index.php/tool/signup",
            "POST /api/index.php/load",
            "POST /api/index.php/tutorial/update_step",
        ]),
    ],
    [
        "protocol-lab:android-cn-entity-list-http-metadata",
        new Set([
            "GET /patch/cn/EntityLists/10939-android_medium.csv",
            "GET /patch/cn/entities/10939-android_medium.csv",
        ]),
    ],
    [
        "protocol-lab:android-cn-method-preserving-login-chain-pcap",
        new Set([
            "POST /api/index.php/channels/channel_leiting/leiting_login",
            "POST /api/index.php/channels/channel_leiting/leiting_antiaddiction_login",
            "POST /api/index.php/tool/signup",
            "POST /api/index.php/load",
        ]),
    ],
    [
        "protocol-lab:android-cn-main-quest-stamina-http-metadata",
        new Set([
            "POST /api/index.php/story_quest/finish_with_skip",
            "POST /api/index.php/shop/recover_stamina",
            "POST /api/index.php/single_battle_quest/start",
            "POST /api/index.php/single_battle_quest/finish",
        ]),
    ],
    [
        "protocol-lab:android-cn-story-skip-success",
        new Set([
            "POST /api/index.php/story_quest/finish",
            "POST /api/index.php/story_quest/finish_with_skip",
            "POST /api/index.php/load",
        ]),
    ],
    [
        "protocol-lab:android-cn-dynamic-20260814T105910Z-metadata",
        new Set([
            "POST /api/index.php/asset/get_path",
            "POST /api/index.php/asset/version_info",
            "POST /api/index.php/channels/channel_leiting_pay/query_unfinish_order",
            "POST /api/index.php/channels/channel_leiting/leiting_antiaddiction_login",
            "POST /api/index.php/channels/channel_leiting/leiting_login",
            "POST /api/index.php/channels/channel_leiting/leiting_update",
            "POST /api/index.php/gacha/exec",
            "POST /api/index.php/load",
            "POST /api/index.php/mail/index",
            "POST /api/index.php/option/update",
            "POST /api/index.php/party/edit",
            "POST /api/index.php/shop/recover_stamina",
            "POST /api/index.php/single_battle_quest/finish",
            "POST /api/index.php/single_battle_quest/play_continue",
            "POST /api/index.php/single_battle_quest/start",
            "POST /api/index.php/story_quest/finish",
            "POST /api/index.php/story_quest/finish_with_skip",
            "POST /api/index.php/tool/custom_notify",
            "POST /api/index.php/tool/signup",
            "POST /api/index.php/tutorial/finish_trigger",
            "POST /api/index.php/tutorial/update_step",
        ]),
    ],
    [
        "protocol-lab:android-cn-mail-time-sentinel-20260817T020739Z",
        new Set([
            "POST /api/index.php/mail/index",
            "POST /api/index.php/mail/receive_all",
        ]),
    ],
])
// //// /保持动态证据引用和已捕获路由一致 ////

// //// 载入并验证协议账本顶层结构 [@x380kkm 2026-07-27] ////
const coverage = JSON.parse(readFileSync(COVERAGE_PATH, "utf8"))
assert.equal(coverage.schemaVersion, 1)
assert.ok(Array.isArray(coverage.clients))
assert.ok(Array.isArray(coverage.entries))

const clientsById = new Map(coverage.clients.map((client) => [client.id, client]))
assert.deepEqual([...clientsById.keys()].sort(), [...CLIENT_IDS].sort())
for (const client of clientsById.values()) {
    assert.match(client.sha256, /^[a-f0-9]{64}$/)
    assert.ok(typeof client.version === "string" && client.version.length > 0)
    assert.ok(["android", "ios"].includes(client.platform))
}
// //// /载入并验证协议账本顶层结构 ////

// //// 验证每条路由的实现位置和证据声明 [@x380kkm 2026-07-27] ////
const entriesById = new Map()
for (const entry of coverage.entries) {
    assert.ok(typeof entry.id === "string" && entry.id.length > 0)
    assert.equal(entriesById.has(entry.id), false, `duplicate coverage entry: ${entry.id}`)
    entriesById.set(entry.id, entry)
    assert.ok(["P0", "P1", "P2"].includes(entry.priority), `invalid priority: ${entry.id}`)
    assert.ok(["http", "tcp", "management"].includes(entry.transport), `invalid transport: ${entry.id}`)
    assert.ok(VALID_STATUSES.has(entry.status), `invalid status: ${entry.id}`)
    assert.ok(typeof entry.nextEvidence === "string" && entry.nextEvidence.length > 0)
    assert.ok(VALID_AUTHORITIES.has(entry.authority.local), `invalid local authority: ${entry.id}`)
    assert.ok(VALID_AUTHORITIES.has(entry.authority.remote), `invalid remote authority: ${entry.id}`)

    for (const runtime of ["server", "personalService"]) {
        const implementation = entry.implementation[runtime]
        assert.ok(VALID_IMPLEMENTATION_STATES.has(implementation.status), `invalid ${runtime} status: ${entry.id}`)
        assert.ok(Array.isArray(implementation.tests), `missing ${runtime} tests: ${entry.id}`)
        if (implementation.handler !== null) {
            assert.equal(existsSync(path.join(REPOSITORY_ROOT, implementation.handler)), true, `missing handler: ${implementation.handler}`)
        }
        for (const testPath of implementation.tests) {
            assert.equal(existsSync(path.join(REPOSITORY_ROOT, testPath)), true, `missing test: ${testPath}`)
        }
    }

    for (const clientId of CLIENT_IDS) {
        const evidence = entry.clientEvidence[clientId]
        assert.ok(evidence !== undefined, `missing ${clientId} evidence: ${entry.id}`)
        assert.ok(VALID_EVIDENCE_LEVELS.has(evidence.level), `invalid evidence level: ${entry.id}`)
        if (evidence.level === "static" || evidence.level === "dynamic") {
            assert.ok(typeof evidence.reference === "string" && evidence.reference.length > 0, `missing evidence reference: ${entry.id}`)
        }
        if (evidence.level === "dynamic") {
            const evidenceRoutes = DYNAMIC_EVIDENCE_ROUTES.get(evidence.reference)
            assert.ok(evidenceRoutes !== undefined, `unmapped dynamic evidence: ${entry.id}`)
            const requestRoute = `${entry.request.method} ${entry.request.path}`
            validateEvidenceRoute(requestRoute, `${entry.id} request route`)
            for (const evidenceRoute of evidenceRoutes) validateEvidenceRoute(evidenceRoute, `${entry.id} evidence route`)
            assert.ok(evidenceRoutes.has(requestRoute), `dynamic evidence does not capture route: ${entry.id}`)
        }
    }

    if (entry.status === "verified") {
        const applicableEvidence = CLIENT_IDS
            .map((clientId) => entry.clientEvidence[clientId].level)
            .filter((level) => level !== "not-applicable")
        assert.ok(applicableEvidence.length > 0, `verified entry has no applicable client: ${entry.id}`)
        assert.ok(applicableEvidence.every((level) => level === "dynamic"), `verified entry lacks dynamic evidence: ${entry.id}`)
    }
}
// //// /验证每条路由的实现位置和证据声明 ////

// //// 保持离线 P0 垂直流程在账本中完整可见 [@x380kkm 2026-07-27] ////
for (const entryId of REQUIRED_ENTRY_IDS) {
    assert.equal(entriesById.has(entryId), true, `required coverage entry is missing: ${entryId}`)
    assert.equal(entriesById.get(entryId).priority, "P0", `required entry is not P0: ${entryId}`)
}

// //// 固定版本和 EntityLists 的实现位置 [@x380kkm 2026-08-07] ////
function assertCoveredImplementation(entryId, target, handler, testPath) {
    const implementation = entriesById.get(entryId).implementation[target]
    assert.equal(implementation.status, "covered")
    assert.equal(implementation.handler, handler)
    assert.ok(implementation.tests.includes(testPath))
}

assertCoveredImplementation(
    "cn.version.android",
    "personalService",
    "core/personal-service/src/http.rs",
    "core/personal-service/tests/lifecycle.rs",
)
assertCoveredImplementation(
    "cn.version.ios",
    "personalService",
    "core/personal-service/src/http.rs",
    "core/personal-service/tests/lifecycle.rs",
)
assertCoveredImplementation(
    "cn.asset.entity_list",
    "server",
    "src/server.ts",
    "scripts/protocol-lab/test-cn-server.js",
)
assertCoveredImplementation(
    "cn.asset.entity_list",
    "personalService",
    "core/personal-service/src/cn_asset_files.rs",
    "core/personal-service/tests/cn_asset_files.rs",
)
// //// /固定版本和 EntityLists 的实现位置 ////

assert.equal(entriesById.get("cn.version.android").clientEvidence["cn-android-1.8.1"].level, "dynamic")
assert.match(entriesById.get("cn.version.android").nextEvidence, /完整 CN CDN/)
assert.match(entriesById.get("cn.version.android").nextEvidence, /leiting_login/)
assert.equal(
    entriesById.get("cn.version.android").clientEvidence["cn-android-1.8.1"].reference,
    "protocol-lab:android-cn-title-login-gate-paired-20260813T170142Z-pcap",
)
assert.match(entriesById.get("cn.version.android").nextEvidence, /一次触摸/)
assert.match(entriesById.get("cn.version.android").nextEvidence, /不把 Android 证据扩展到 iOS, gacha, mail 或战斗/)
for (const entryId of [
    "cn.auth.leiting_login",
    "cn.auth.leiting_antiaddiction_login",
    "cn.account.signup",
    "cn.account.load",
]) {
    assert.equal(
        entriesById.get(entryId).clientEvidence["cn-android-1.8.1"].reference,
        "protocol-lab:android-cn-strict-paired-restart-20260818",
    )
}
assert.match(entriesById.get("cn.asset.entity_list").nextEvidence, /显式 CN CDN 根/)
assert.match(entriesById.get("cn.asset.entity_list").nextEvidence, /没有出现 EntityLists GET 或 404/)
assert.match(entriesById.get("cn.asset.entity_list").nextEvidence, /49 个 archive GET/)
assert.match(entriesById.get("cn.auth.leiting_login").nextEvidence, /冷启动和进程重启/)
assert.match(entriesById.get("cn.account.signup").nextEvidence, /仍只有 1 个 account/)
assert.match(entriesById.get("cn.account.load").nextEvidence, /教程选择/)
const staminaRecovery = entriesById.get("cn.shop.recover_stamina").implementation.personalService
assert.equal(staminaRecovery.status, "covered")
assert.equal(staminaRecovery.handler, "core/personal-service/src/cn_shop.rs")
assert.ok(staminaRecovery.tests.includes("core/personal-service/tests/cn_shop.rs"))
const storySkip = entriesById.get("cn.story.finish_with_skip")
assert.equal(storySkip.implementation.server.handler, "src/routes/api/storyQuest.ts")
assert.equal(storySkip.implementation.personalService.handler, "core/personal-service/src/cn_story.rs")
assert.equal(storySkip.clientEvidence["cn-android-1.8.1"].reference, "protocol-lab:android-cn-story-skip-success")
const storySkipEvidenceRoutes = DYNAMIC_EVIDENCE_ROUTES.get("protocol-lab:android-cn-story-skip-success")
assert.ok(storySkipEvidenceRoutes.has("POST /api/index.php/story_quest/finish"))
assert.ok(storySkipEvidenceRoutes.has("POST /api/index.php/story_quest/finish_with_skip"))
assert.ok(storySkipEvidenceRoutes.has("POST /api/index.php/load"))
assert.match(storySkip.nextEvidence, /finish_with_skip 均返回 200 application\/x-msgpack/)
assert.match(storySkip.nextEvidence, /星导石从 1495 增加到 1510/)
assert.match(storySkip.nextEvidence, /free_mana 保持 1029/)
assert.match(storySkip.nextEvidence, /1001003 只有 1 条完成记录/)
assert.match(storySkip.nextEvidence, /冷启动登录链和 load 均返回 200/)
assert.match(storySkip.nextEvidence, /推进到 2-1/)

const mailReceive = entriesById.get("cn.mail.receive")
assert.equal(mailReceive.request.path, "/api/index.php/mail/receive_all")
assert.equal(mailReceive.clientEvidence["cn-android-1.8.1"].level, "dynamic")
assert.equal(
    mailReceive.clientEvidence["cn-android-1.8.1"].reference,
    "protocol-lab:android-cn-mail-time-sentinel-20260817T020739Z",
)
assert.match(mailReceive.nextEvidence, /0000-00-00 00:00:00/)
assert.match(mailReceive.nextEvidence, /mail\/receive_all 均返回 200 application\/x-msgpack/)
assert.match(mailReceive.nextEvidence, /客户端重启后邮件计数和奖励仍保持/)

const summary = coverage.entries.reduce((counts, entry) => {
    counts[entry.status] = (counts[entry.status] ?? 0) + 1
    return counts
}, {})
process.stdout.write(`CN protocol coverage validated: ${JSON.stringify(summary)}\n`)
// //// /保持离线 P0 垂直流程在账本中完整可见 ////
