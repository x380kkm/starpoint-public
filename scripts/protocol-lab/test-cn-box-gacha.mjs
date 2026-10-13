// audience: internal
// # cn-box-gacha-test
// 此脚本使用实际箱池配置和独立数据库检查角色奖励, 特奖停抽和批量扣费.

import assert from "node:assert/strict"
import { mkdtempSync, readFileSync, rmSync } from "node:fs"
import { createRequire } from "node:module"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { mock } from "node:test"
import { pack, unpack } from "msgpackr"

const require = createRequire(import.meta.url)
const temporaryRoot = mkdtempSync(join(tmpdir(), "starpoint-box-character-"))
const previousDatabasePath = process.env.DATABASE_PATH
process.env.DATABASE_PATH = join(temporaryRoot, "wdfp_data.db")

// //// 使用独立数据库验证箱池奖励和批量抽取 [@x380kkm 2026-09-28] ////
let database
let server
try {
    require("ts-node/register")
    database = require("../../src/data/index.ts").default(0)
    const { getDefaultPlayerData } = require("../../src/data/utils.ts")
    const {
        getPlayerCharacterSync,
        getPlayerItemSync,
        getPlayerBoxGachaSync,
        givePlayerItemSync,
        insertAccount,
        insertPlayerSync,
        insertSessionWithToken,
        insertPlayerBoxGachaSync,
        insertPlayerBoxGachaDrawnRewardSync,
        updatePlayerSync,
        getPlayerSync,
        updatePlayerItemSync,
    } = require("../../src/data/wdfpData.ts")
    const {
        drawBoxGachaSync,
        rewardPlayerBoxGachaResultSync,
    } = require("../../src/lib/gacha.ts")
    const pools = JSON.parse(readFileSync(new URL("../../assets/box_reward.json", import.meta.url), "utf8"))
    const duplicateItems = new Map([[263002, 14014], [233001, 14008], [223001, 14005]])
    const account = await insertAccount({
        appId: "box-character-test",
        idpAlias: "box-character-test",
        idpCode: "device",
        idpId: "box-character-test",
        status: "active",
    })
    let checkedRewards = 0

    for (const [poolId, boxes] of Object.entries(pools)) {
        for (const [boxId, rewards] of Object.entries(boxes)) {
            for (const [rewardId, reward] of Object.entries(rewards)) {
                if (reward.type !== 6) continue
                const label = `${poolId}/${boxId}/${rewardId}`
                const drawnRewards = Object.entries(rewards)
                    .filter(([id]) => id !== rewardId)
                    .map(([id, entry]) => ({ id: Number(id), number: entry.available }))
                const draw = drawBoxGachaSync(rewards, drawnRewards, 1, true)
                assert.deepEqual([...draw.characters], [[reward.id, reward.count]], label)
                assert.deepEqual(draw.rewards, [{ id: Number(rewardId), number: 1 }], label)

                const playerId = insertPlayerSync(account.id, getDefaultPlayerData())
                const firstReward = rewardPlayerBoxGachaResultSync(playerId, draw)
                assert.deepEqual(firstReward.joined_character_id_list, [reward.id], label)
                assert.equal(firstReward.character_list[0].character_id, reward.id, label)
                assert.equal(getPlayerCharacterSync(playerId, reward.id).stack, 0, label)

                const duplicateReward = rewardPlayerBoxGachaResultSync(playerId, draw)
                const duplicateItemId = duplicateItems.get(reward.id)
                assert.ok(duplicateItemId, label)
                assert.deepEqual(duplicateReward.joined_character_id_list, [], label)
                assert.equal(duplicateReward.character_list[0].character_id, reward.id, label)
                assert.equal(duplicateReward.items[duplicateItemId], 1, label)
                assert.equal(getPlayerCharacterSync(playerId, reward.id).stack, 1, label)
                assert.equal(getPlayerItemSync(playerId, duplicateItemId), 1, label)
                checkedRewards += 1
            }
        }
    }
    assert.equal(checkedRewards, duplicateItems.size)
    console.log(`已验证 ${checkedRewards} 条实际箱池角色奖励的抽取, 首次入队和重复角色材料.`)

    // //// 通过箱池接口验证特奖停抽和实际数量扣费 [@x380kkm 2026-09-28] ////
    const { SessionType } = require("../../src/data/types.ts")
    const { getBoxGachaSync } = require("../../src/lib/assets.ts")
    const { default: boxGachaRoutes } = require("../../src/routes/api/boxGacha.ts")
    server = require("fastify")()
    server.addHook("onSend", (_request, reply, payload, done) => {
        done(null, reply.getHeader("content-type") === "application/x-msgpack" ? pack(payload) : payload)
    })
    await server.register(boxGachaRoutes)
    mock.method(require("node:crypto"), "randomInt", () => 0)

    // //// 创建使用独立账号和代币余额的箱池玩家 [@x380kkm 2026-09-28] ////
    async function createBoxPlayer(poolId, currency) {
        const account = await insertAccount({
            appId: "box-route-test",
            idpAlias: "box-route-test",
            idpCode: "device",
            idpId: `box-route-${poolId}`,
            status: "active",
        })
        const playerId = insertPlayerSync(account.id, getDefaultPlayerData())
        const viewerId = 700_000 + account.id
        await insertSessionWithToken({
            token: String(viewerId),
            accountId: account.id,
            type: SessionType.VIEWER,
            expires: new Date(),
        })
        const gacha = getBoxGachaSync(poolId)
        givePlayerItemSync(playerId, gacha.redeemItemId, currency)
        return { playerId, viewerId, poolId, gacha, nextApiCount: 1 }
    }

    // //// 调用箱池接口并解码奖励响应 [@x380kkm 2026-09-28] ////
    async function drawFromRoute(player, boxId, count, stopOnFeatured, apiCount = player.nextApiCount++) {
        const response = await server.inject({
            method: "POST",
            url: "/exec",
            payload: {
                viewer_id: player.viewerId,
                box_gacha_id: player.poolId,
                box_id: boxId,
                number: count,
                stop_on_featured_rewards: stopOnFeatured,
                api_count: apiCount,
            },
        })
        assert.equal(response.statusCode, 200, response.body)
        return unpack(response.rawPayload).data
    }

    // //// 核对实际抽数和接口与存档的代币余额 [@x380kkm 2026-09-28] ////
    function verifyDrawCharge(player, response, expectedDraws, expectedCurrency) {
        const actualDraws = response.drawn_reward_list.reduce((sum, reward) => sum + reward.number, 0)
        assert.equal(actualDraws, expectedDraws)
        assert.equal(response.item_list[player.gacha.redeemItemId], expectedCurrency)
        assert.equal(getPlayerItemSync(player.playerId, player.gacha.redeemItemId), expectedCurrency)
    }

    const featuredPlayer = await createBoxPlayer(11, 100)
    const featuredDraw = await drawFromRoute(featuredPlayer, 2, 10, true)
    verifyDrawCharge(featuredPlayer, featuredDraw, 3, 70)
    assert.deepEqual(featuredDraw.drawn_reward_list, [
        { reward_id: 1001102001, number: 1 },
        { reward_id: 1001102002, number: 2 },
    ])
    const ordinaryDraw = await drawFromRoute(featuredPlayer, 2, 4, true)
    verifyDrawCharge(featuredPlayer, ordinaryDraw, 4, 30)

    const resettablePlayer = await createBoxPlayer(15, 30)
    const resettableDraw = await drawFromRoute(resettablePlayer, 5, 3, true)
    verifyDrawCharge(resettablePlayer, resettableDraw, 3, 0)

    const bulkGacha = getBoxGachaSync(1)
    const bulkCount = bulkGacha.availableCounts[4]
    const bulkPlayer = await createBoxPlayer(1, bulkCount * bulkGacha.redeemItemCount)
    const bulkDraw = await drawFromRoute(bulkPlayer, 4, 9999, false)
    verifyDrawCharge(bulkPlayer, bulkDraw, bulkCount, 0)
    const exhaustedBox = getPlayerBoxGachaSync(bulkPlayer.playerId, 1, 4)
    assert.equal(exhaustedBox.remainingNumber, 0)
    assert.equal(exhaustedBox.isClosed, true)
    // //// 满仓尾箱把余量和回执共同提交并重放原响应 [@x380kkm 2026-09-28] ////
    const capped = await createBoxPlayer(1, 30)
    const tail = new Set(["1000101004", "1000101009", "1000101010"])
    insertPlayerBoxGachaSync(capped.playerId, 1, { boxId: 1, resetTimes: 0, remainingNumber: tail.size, isClosed: false })
    for (const [id, entry] of Object.entries(pools[1][1])) {
        insertPlayerBoxGachaDrawnRewardSync(capped.playerId, 1, 1, { id: Number(id), number: entry.available - Number(tail.has(id)) })
    }
    givePlayerItemSync(capped.playerId, 49, 9999 - (getPlayerItemSync(capped.playerId, 49) ?? 0))
    updatePlayerSync({ id: capped.playerId, paidMana: 1000, freeMana: 999998949, expPool: 1999999500 })
    const mailCount = () => database.prepare("SELECT COUNT(*) AS count FROM player_mails WHERE player_id=?").get(capped.playerId).count
    database.exec("CREATE TRIGGER reject_box_mail BEFORE INSERT ON player_mails BEGIN SELECT RAISE(ABORT,'mail unavailable'); END;")
    const failure = await server.inject({ method: "POST", url: "/exec", payload: {
        viewer_id: capped.viewerId, box_gacha_id: 1, box_id: 1, number: 9999, stop_on_featured_rewards: false, api_count: 1,
    } })
    assert.equal(failure.statusCode, 500)
    assert.equal(getPlayerItemSync(capped.playerId, 30101), 30)
    assert.equal(getPlayerBoxGachaSync(capped.playerId, 1, 1).remainingNumber, 3)
    assert.equal(getPlayerSync(capped.playerId).freeMana, 999998949)
    assert.equal(mailCount(), 0)
    database.exec("DROP TRIGGER reject_box_mail")
    const cappedResult = await drawFromRoute(capped, 1, 9999, false, 1)
    verifyDrawCharge(capped, cappedResult, 3, 0)
    assert.equal(cappedResult.item_list[49], 9999)
    assert.equal(cappedResult.user_info.free_mana, 999998999)
    assert.equal(cappedResult.user_info.exp_pool, 2000000000)
    assert.equal(cappedResult.mail_arrived, true)
    const mailed = JSON.parse(database.prepare("SELECT rewards_json FROM player_mails WHERE player_id=?").get(capped.playerId).rewards_json)
    assert.equal(mailed.itemList[49], 1)
    assert.equal(mailed.freeMana, 350)
    assert.equal(mailed.expPool, 1500)
    assert.deepEqual(await drawFromRoute(capped, 1, 9999, false, 1), cappedResult)
    assert.equal(mailCount(), 1)
    // //// 领取邮件时保留满仓邮件并允许同批其他邮件到账 [@x380kkm 2026-09-28] ////
    const { default: mailRoutes } = require("../../src/routes/api/mail.ts")
    await server.close()
    server = require("fastify")()
    server.addHook("onSend", (_request, reply, payload, done) => {
        done(null, reply.getHeader("content-type") === "application/x-msgpack" ? pack(payload) : payload)
    })
    await server.register(mailRoutes)
    const { createPlayerMailSync } = require("../../src/data/playerMails.ts")
    const blockedId = database.prepare("SELECT id FROM player_mails WHERE player_id=?").get(capped.playerId).id
    const blocked = await server.inject({ method: "POST", url: "/receive", payload: { viewer_id: capped.viewerId, mail_id: blockedId } })
    assert.equal(blocked.statusCode, 200)
    assert.equal(unpack(blocked.rawPayload).data_headers.result_code, 1)
    updatePlayerItemSync(capped.playerId, 48, 0)
    const other = createPlayerMailSync({ playerId: capped.playerId, title: "素材", body: "素材奖励.", sender: "系统", rewards: { itemList: { 48: 1 } } })
    const partial = unpack((await server.inject({ method: "POST", url: "/receive_all", payload: { viewer_id: capped.viewerId } })).rawPayload).data
    assert.deepEqual(partial.mail_ids, [other.id])
    assert.equal(partial.max_overed_mail_count, 0)
    assert.equal(database.prepare("SELECT received_at FROM player_mails WHERE id=?").get(blockedId).received_at, null)
    updatePlayerItemSync(capped.playerId, 49, 9998)
    updatePlayerSync({ id: capped.playerId, freeMana: 999998649, expPool: 1999998500 })
    const claimed = unpack((await server.inject({ method: "POST", url: "/receive_all", payload: { viewer_id: capped.viewerId } })).rawPayload).data
    assert.deepEqual(claimed.mail_ids, [blockedId])
    assert.equal(claimed.item_list[49], 9999)
    assert.equal(claimed.user_info.free_mana, 999998999)
    assert.equal(claimed.user_info.exp_pool, 2000000000)
    console.log("已验证箱池道具, 玛纳和经验的库存余量, 邮件原子保存和请求重放.")
    console.log(`已验证全部特奖停抽, 无剩余特奖续抽, 可重置箱持续抽取, 以及 9999 次请求按实际 ${bulkCount} 抽扣费.`)
    // //// /通过箱池接口验证特奖停抽和实际数量扣费 ////
} finally {
    mock.restoreAll()
    await server?.close()
    database?.close()
    if (previousDatabasePath === undefined) delete process.env.DATABASE_PATH
    else process.env.DATABASE_PATH = previousDatabasePath
    rmSync(temporaryRoot, { recursive: true, force: true })
}
// //// /使用独立数据库验证箱池奖励和批量抽取 ////
