// audience: internal
// # cn-daily-challenge-points-test
// 此脚本使用独立 SQLite 存档验证 CN 载入, 可移植导出和日切中的挑战活动引用.
// 运行依赖仓库的 TypeScript 编译输出.

import assert from "node:assert/strict"
import { mkdtempSync, rmSync } from "node:fs"
import { createRequire } from "node:module"
import { tmpdir } from "node:os"
import { dirname, join, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import Fastify from "fastify"
import { pack, unpack } from "msgpackr"

const repositoryRoot = fileURLToPath(new URL("../..", import.meta.url))
const temporaryParent = resolve(tmpdir())
const temporaryRoot = mkdtempSync(join(temporaryParent, "starpoint-cn-challenges-"))
process.env.DATABASE_PATH = join(temporaryRoot, "wdfp_data.db")
process.env.MANAGEMENT_STATE_FILE = join(temporaryRoot, "management-state.json")
let database
let app

// //// 执行隔离存档验证并关闭数据库 [@x380kkm 2026-09-28] ////
try {
    const require = createRequire(import.meta.url)
    const data = require(join(repositoryRoot, "out/data/wdfpData.js"))
    const serializers = require(join(repositoryRoot, "out/data/utils.js"))
    const snapshots = require(join(repositoryRoot, "out/data/portablePlayerSnapshots.js"))
    const { normalizeCnDailyChallengePoints } = require(join(repositoryRoot, "out/lib/cnDailyChallengePoints.js"))
    const { SessionType } = require(join(repositoryRoot, "out/data/types.js"))
    const loadRoute = require(join(repositoryRoot, "out/routes/cn/load.js"))
    database = require(join(repositoryRoot, "out/data/index.js")).default(0)
    app = Fastify()
    app.addHook("onSend", (_request, reply, payload, done) => {
        done(null, reply.getHeader("content-type") === "application/x-msgpack" ? pack(payload) : payload)
    })
    await app.register(loadRoute.default)

    // //// 核对重复次数和活动父表关系 [@x380kkm 2026-09-28] ////
    function verifyChallengeReferences() {
        const records = [
            { id: 251, point: 1, campaign_list: [{ campaign_id: 2023013102, additional_point: 2 }] },
            { id: 251, point: 5, campaign_list: [{ campaign_id: 2023013101, additional_point: 8 }] },
            { id: 1, point: 2, campaign_list: [{ campaign_id: 2023013101, additional_point: 2 }] },
            { id: 1, point: 3, campaign_list: [{ campaign_id: 2023013101, additional_point: 4 }] },
            { id: 10008, point: 1, campaign_list: [] },
        ]
        assert.deepEqual(normalizeCnDailyChallengePoints(records), [
            { id: 251, point: 5, campaign_list: [] },
            { id: 1, point: 3, campaign_list: [{ campaign_id: 2023013101, additional_point: 4 }] },
            { id: 10008, point: 1, campaign_list: [] },
        ])
        assert.equal(records[0].campaign_list[0].campaign_id, 2023013102)
    }

    // //// 通过 CN 载入读取挑战次数 [@x380kkm 2026-09-28] ////
    async function loadChallenges(viewerId) {
        const response = await app.inject({ method: "POST", url: "/load", payload: { viewer_id: viewerId } })
        assert.equal(response.statusCode, 200, response.body)
        return unpack(response.rawPayload).data.user_daily_challenge_point_list
    }

    // //// 核对旧导入在 CN 载入后的存储和导出 [@x380kkm 2026-09-28] ////
    async function verifyImportedChallenges() {
        const account = await data.insertAccount({
            appId: "wf_cn", idpAlias: "wf_cn:challenge", idpCode: "leiting",
            idpId: "cn:challenge", status: "normal",
        })
        const player = data.insertDefaultPlayerSync(account.id)
        const viewerId = 123456789
        await data.insertSessionWithToken({
            token: String(viewerId), accountId: account.id, type: SessionType.VIEWER, expires: new Date(),
        })
        const initial = serializers.getClientSerializedData(player.id, { viewerId })
        assert.deepEqual(initial.user_daily_challenge_point_list.find(entry => entry.id === 251), {
            id: 251, point: 2, campaign_list: [],
        })
        const legacy = structuredClone(initial)
        legacy.user_daily_challenge_point_list = [
            { id: 1, point: 3, campaign_list: [{ campaign_id: 2023013101, additional_point: 4 }] },
            { id: 251, point: 5, campaign_list: [
                { campaign_id: 2023013102, additional_point: 2 },
                { campaign_id: 2023013101, additional_point: 8 },
            ] },
        ]
        legacy.imported_field = { value: 12 }
        const imported = serializers.deserializePlayerData(player.id, legacy)
        assert.notEqual(imported, null)
        data.replacePlayerDataSync(imported)
        serializers.capturePortablePlayerSnapshotSync(player.id, legacy)
        const generic = serializers.getClientSerializedData(player.id, { viewerId })
        assert.deepEqual(generic.user_daily_challenge_point_list.find(entry => entry.id === 251)
            .campaign_list.map(campaign => campaign.campaign_id), [2023013101, 2023013102])

        const expected = [
            { id: 1, point: 3, campaign_list: [{ campaign_id: 2023013101, additional_point: 4 }] },
            { id: 251, point: 5, campaign_list: [] },
        ]
        assert.deepEqual(await loadChallenges(viewerId), expected)
        assert.deepEqual(data.getPlayerDailyChallengePointListSync(player.id), [
            { id: 1, point: 3, campaignList: [{ campaignId: 2023013101, additionalPoint: 4 }] },
            { id: 251, point: 5, campaignList: [] },
        ])
        const stored = snapshots.getPortablePlayerSnapshotSync(player.id)
        assert.deepEqual(stored.source.user_daily_challenge_point_list, expected)
        assert.deepEqual(stored.baseline.user_daily_challenge_point_list, expected)
        const exported = serializers.getPortableSerializedData(player.id, { viewerId })
        assert.deepEqual(exported.user_daily_challenge_point_list, expected)
        assert.deepEqual(exported.imported_field, legacy.imported_field)
        assert.deepEqual(await loadChallenges(viewerId), expected)

        stored.source.user_daily_challenge_point_list = legacy.user_daily_challenge_point_list
        snapshots.setPortablePlayerSnapshotSync(player.id, stored)
        assert.deepEqual(serializers.getPortableSerializedData(player.id, { viewerId }).user_daily_challenge_point_list,
            legacy.user_daily_challenge_point_list)
        assert.deepEqual(await loadChallenges(viewerId), expected)
        assert.deepEqual(serializers.getPortableSerializedData(player.id, { viewerId }).user_daily_challenge_point_list, expected)

        data.updatePlayerSync({ id: player.id, lastLoginTime: new Date("2020-01-01T00:00:00Z") })
        assert.deepEqual(await loadChallenges(viewerId), expected)
        const reimported = serializers.deserializePlayerData(player.id, exported)
        assert.notEqual(reimported, null)
        data.replacePlayerDataSync(reimported)
        serializers.capturePortablePlayerSnapshotSync(player.id, exported)
        assert.deepEqual(await loadChallenges(viewerId), expected)
    }

    verifyChallengeReferences()
    await verifyImportedChallenges()
    process.stdout.write("CN 挑战次数引用, 旧导入, 导出和日切验证完成.\n")
} finally {
    await app?.close()
    database?.close()
    if (dirname(resolve(temporaryRoot)) !== temporaryParent) throw new Error("Temporary directory is outside its parent")
    rmSync(temporaryRoot, { recursive: true, force: true })
}
// //// /执行隔离存档验证并关闭数据库 ////
