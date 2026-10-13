// audience: internal
// # cn-gacha-campaign-test
// 此脚本使用独立数据库核对免费扭蛋活动的时间, 抽取类型和逐活动次数.

import assert from "node:assert/strict"
import { mkdtempSync, rmSync } from "node:fs"
import { createRequire } from "node:module"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { mock } from "node:test"
import { pack, unpack } from "msgpackr"

const require = createRequire(import.meta.url)
const temporaryRoot = mkdtempSync(join(tmpdir(), "starpoint-gacha-campaign-"))
const previousDatabasePath = process.env.DATABASE_PATH
process.env.DATABASE_PATH = join(temporaryRoot, "wdfp_data.db")
let database
let server

// //// 通过实际活动关系核对载入和免费抽取 [@x380kkm 2026-09-28] ////
try {
    require("ts-node/register")
    database = require("../../src/data/index.ts").default(0)
    const { getDefaultPlayerData, serializeGachaCampaign } = require("../../src/data/utils.ts")
    const {
        activateAccountPlayerSync,
        getPlayerGachaCampaignListSync,
        getPlayerGachaCampaignSync,
        getPlayerGachaInfoSync,
        insertAccount,
        insertPlayerSync,
        insertSessionWithToken,
    } = require("../../src/data/wdfpData.ts")
    const { SessionType } = require("../../src/data/types.ts")
    const assets = require("../../src/lib/assets.ts")
    const tutorial = require("../../src/lib/tutorial.ts")
    const { findAvailableGachaCampaign, projectActiveGachaCampaigns } = require("../../src/lib/gachaCampaign.ts")
    const { setServerTime, getServerTime } = require("../../src/utils.ts")
    const { default: gachaRoutes } = require("../../src/routes/api/gacha.ts")
    const { default: loadRoutes } = require("../../src/routes/cn/load.ts")
    const campaignList = assets.getGachaCampaignListSync()
    const campaign1011 = campaignList.find(campaign => campaign.campaignId === 1011)
    const campaign1012 = campaignList.find(campaign => campaign.campaignId === 1012)

    // //// 设置服务器时间 [@x380kkm 2026-09-28] ////
    function setTime(serverTime) {
        setServerTime(new Date(serverTime * 1000))
    }
    setTime(Date.parse("2023-08-18T04:00:00Z") / 1000)
    const account = await insertAccount({
        appId: "gacha-campaign-test",
        idpAlias: "gacha-campaign-test",
        idpCode: "device",
        idpId: "gacha-campaign-test",
        status: "active",
    })
    const playerId = insertPlayerSync(account.id, {
        ...getDefaultPlayerData(),
        lastLoginTime: new Date(getServerTime() * 1000),
    })
    activateAccountPlayerSync(account.id, playerId)
    const viewerId = 730_000 + account.id
    await insertSessionWithToken({
        token: String(viewerId),
        accountId: account.id,
        type: SessionType.VIEWER,
        expires: new Date(),
    })
    server = require("fastify")()
    server.addHook("onSend", (_request, reply, payload, done) => {
        done(null, reply.getHeader("content-type") === "application/x-msgpack" ? pack(payload) : payload)
    })
    await server.register(gachaRoutes, {
        prefix: "/gacha",
        drawGacha: (_gacha, amount) => new Map([[111001, amount]]),
    })
    await server.register(loadRoutes)
    mock.method(tutorial, "getCnTutorialGachaSync", assets.getGachaSync)

    // //// 调用免费扭蛋接口并解码响应 [@x380kkm 2026-09-28] ////
    async function draw(gachaId, type, expectedStatus) {
        const response = await server.inject({ method: "POST", url: "/gacha/exec", payload: {
            api_count: 1,
            payment_type: 4,
            number_of_exec: 1,
            viewer_id: viewerId,
            gacha_id: gachaId,
            type,
        } })
        assert.equal(response.statusCode, expectedStatus, response.body)
        return expectedStatus === 200 ? unpack(response.rawPayload).data : response.json()
    }

    // //// 读取玩家载入响应中的活动 [@x380kkm 2026-09-28] ////
    async function loadCampaigns() {
        const response = await server.inject({ method: "POST", url: "/load", payload: { viewer_id: viewerId } })
        assert.equal(response.statusCode, 200, response.body)
        return unpack(response.rawPayload).data.gacha_campaign_list
    }

    const available = await loadCampaigns()
    assert.deepEqual(available.filter(row => [1577, 1579].includes(row.gacha_id)), [
        { campaign_id: 1011, gacha_id: 1577, count: 1 },
        { campaign_id: 1011, gacha_id: 1579, count: 1 },
    ])
    assert.equal((await draw(1577, 8, 400)).message, "No active gacha campaign for this draw type.")
    assert.equal(getPlayerGachaCampaignSync(playerId, 1577, 1011), null)
    const single = await draw(1577, 11, 200)
    assert.equal(single.draw.length, 1)
    assert.deepEqual(single.gacha_campaign_list, [{ campaign_id: 1011, gacha_id: 1577, count: 0 }])
    assert.equal((await draw(1577, 11, 400)).message, "Already redeemed campaign for this period.")
    assert.equal(getPlayerGachaInfoSync(playerId, 1577).gachaExchangePoint, 1)
    const overlappingCatalog = mock.method(assets, "getGachaCampaignListSync", () => [
        campaign1011,
        { ...campaign1012, startTime: campaign1011.startTime },
    ])
    assert.equal((await draw(1577, 11, 400)).message, "Already redeemed campaign for this period.")
    assert.equal(getPlayerGachaCampaignSync(playerId, 1577, 1012), null)
    const visible = projectActiveGachaCampaigns(
        getPlayerGachaCampaignListSync(playerId).map(serializeGachaCampaign),
        getServerTime(),
        gachaId => gachaId === 1577,
    )
    assert.deepEqual(visible, [
        { campaign_id: 1011, gacha_id: 1577, count: 0 },
        { campaign_id: 1012, gacha_id: 1577, count: 1 },
    ])
    assert.equal(findAvailableGachaCampaign(1577, 1, visible).campaignId, 1012)
    assert.equal((await draw(1579, 7, 200)).draw.length, 1)
    assert.equal((await loadCampaigns()).find(row => row.gacha_id === 1577).count, 0)
    const followingCampaign = await draw(1577, 11, 200)
    assert.deepEqual(followingCampaign.gacha_campaign_list, [{ campaign_id: 1012, gacha_id: 1577, count: 0 }])
    overlappingCatalog.mock.restore()

    setTime(getServerTime() + 86_400)
    assert.equal((await loadCampaigns()).find(row => row.gacha_id === 1577).count, 1)
    assert.equal((await draw(1577, 11, 200)).draw.length, 1)
    setTime(campaign1012.startTime)
    const nextCampaign = await draw(1577, 11, 200)
    assert.deepEqual(nextCampaign.gacha_campaign_list, [{ campaign_id: 1012, gacha_id: 1577, count: 0 }])
    assert.equal(getPlayerGachaCampaignSync(playerId, 1577, 1011).count, 0)
    setTime(campaign1012.endTime + 1)
    assert.equal((await draw(1577, 11, 400)).message, "No active gacha campaign for this draw type.")

    const tenCampaign = campaignList.find(campaign => campaign.campaignId === 1)
    setTime(tenCampaign.startTime)
    assert.equal((await draw(28, 11, 400)).message, "No active gacha campaign for this draw type.")
    const ten = await draw(28, 8, 200)
    assert.equal(ten.draw.length, 10)
    assert.deepEqual(ten.gacha_campaign_list, [{ campaign_id: 1, gacha_id: 28, count: 0 }])
    assert.equal((await draw(28, 8, 400)).message, "Already redeemed campaign for this period.")
    console.log("已验证实际活动 1011/1012 的连续窗口, 抽取类型, 逐活动次数, 每日刷新, 部分池耗尽时保留活动和全部可见池耗尽后的切换.")
} finally {
    mock.restoreAll()
    await server?.close()
    database?.close()
    if (previousDatabasePath === undefined) delete process.env.DATABASE_PATH
    else process.env.DATABASE_PATH = previousDatabasePath
    rmSync(temporaryRoot, { recursive: true, force: true })
}
// //// /通过实际活动关系核对载入和免费抽取 ////
