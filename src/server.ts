// audience: internal
// # server
// 此模块装配 HTTP 路由, 静态资源和 CN 多人 TCP 监听器, 并按环境变量启动单进程服务.

import Fastify, { FastifyRequest } from "fastify";
import { ContentTypeParserDoneFunction } from "fastify/types/content-type-parser";
import fastifyStatic from "@fastify/static";
import { pack, unpack } from "msgpackr";
import path from "path";
import { getServerDate, getServerTime } from "./utils";
// api routes
import apiPlugin from "./routes/api";
import assetApiPlugin from "./routes/api/asset";
import toolApiPlugin from "./routes/api/tool";
import reproduceApiPlugin from "./routes/api/reproduce"
import tutorialApiPlugin from "./routes/api/tutorial"
import gachaApiPlugin from "./routes/api/gacha"
import partyApiPlugin from "./routes/api/party"
import expodApiPlugin from "./routes/api/expod"
import storyQuestApiPlugin from "./routes/api/storyQuest"
import optionApiPlugin from "./routes/api/option"
import singleBattleQuestApiPlugin, { registerBattleSettlementRoutes } from "./routes/api/singleBattleQuest"
import multiBattleQuestApiPlugin from "./routes/api/multiBattleQuest"
import attentionApiPlugin from "./routes/api/attention"
import characterApiPlugin from "./routes/api/character"
import partyGroupApiPlugin from "./routes/api/partyGroup"
import equipmentApiPlugin from "./routes/api/equipment"
import exBoostApiPlugin from "./routes/api/exBoost"
import boxGachaApiPlugin from "./routes/api/boxGacha"
import shopApiPlugin from "./routes/api/shop"
import encyclopediaApiPlugin from "./routes/api/encyclopedia"
import mailApiPlugin from "./routes/api/mail"
import rankingEventApiPlugin from "./routes/api/rankingEvent"
import missionApiPlugin from "./routes/api/mission"
import paymentApiPlugin from "./routes/api/payment"
import newsApiPlugin from "./routes/api/news"
import raidEventApiPlugin from "./routes/api/raidEvent"
import rushEventApiPlugin from "./routes/api/rushEvent"
// web routes
import indexWebPlugin from "./routes/web"
// web api routes
import indexWebApiPlugin from "./routes/web_api"
// misc routes
import openapiPlugin from "./routes/openapi";
import infodeskPlugin from "./routes/infodesk";
import managementPlugin from "./routes/management";
import { managementStore } from "./control/management";
import cnVersionCheckPlugin from "./routes/cn/versionCheck";
import cnLeitingAuthPlugin from "./routes/cn/leitingAuth";
import cnLeitingPaymentPlugin from "./routes/cn/leitingPayment";
import cnToolPlugin from "./routes/cn/tool";
import cnLoadPlugin from "./routes/cn/load";
import cnAssetPlugin, { getCnVersionInfo } from "./routes/cn/asset";
import { drawCnGachaSync, getCnTutorialGachaSync } from "./lib/tutorial";
import { startMultiplayerSessionServer, stopMultiplayerSessionServer } from "./multiplayer/sessionServer";
import { matchmakingStore } from "./multiplayer/matchmakingStore";
import { getMultiplayerSessionPort } from "./multiplayer/sessionConfig";
import { starpointParticipantIdentityProvider } from "./games/starpoint/participantIdentityProvider";
import { installCnHttpMetadataObserver } from "./control/cnHttpMetadataObserver";

// gc-openapi-zinny3.kakaogames.com
// gc-infodesk-zinny3.kakaogames.com
// na.wdfp.kakaogames.com

// initialize server
const fastify = Fastify({
    logger: false
})

installCnHttpMetadataObserver(fastify, process.env.CN_PROTOCOL_METADATA_LOG)

const useRawCnMsgpackResponse = process.env.CN_MSGPACK_RESPONSE_ENCODING === "raw"

// serializers
fastify.addHook('onSend', (_, reply, payload, done) => {
    try {
        switch (reply.getHeader('content-type')) {
            case "application/x-msgpack": {
                const packed = pack(payload)
                done(null, useRawCnMsgpackResponse ? packed : packed.toString('base64'))
                break;
            }
            default:
                done(null, payload)
        }
    } catch (error) {
        done(error as Error)
    }

})

// content-type parsers
function jsonParser(_: FastifyRequest, body: string, done: ContentTypeParserDoneFunction) {
    try {
        var json = JSON.parse(body)
        done(null, json)
    } catch (err) {
        done(null, undefined)
    }
}

fastify.addContentTypeParser("application/x-www-form-urlencoded", { parseAs: 'string' }, (request: FastifyRequest, body: string, done) => {
    // on IOS, for some reason, requests to infodesk and openapi are JSON, but the content-type header is set as 'application/x-www-form-urlencoded'
    const routeUrl = request.routeOptions.url || ''
    if (routeUrl.startsWith("/openapi") || routeUrl.startsWith("/infodesk"))
        return jsonParser(request, body, done);

    try {
        const unpacked = unpack(Buffer.from(body, "base64"))
        done(null, unpacked)
    } catch (err) {
        done(err as Error, undefined)
    }
})
fastify.addContentTypeParser('application/json', { parseAs: 'string' }, jsonParser)

// register plugins

//api
const apiPrefix = "/latest/api/index.php"
fastify.register(apiPlugin, { prefix: apiPrefix })
fastify.register(assetApiPlugin, { prefix: `${apiPrefix}/asset` })
fastify.register(toolApiPlugin, { prefix: `${apiPrefix}/tool` })
fastify.register(reproduceApiPlugin, { prefix: `${apiPrefix}/reproduce` })
fastify.register(tutorialApiPlugin, { prefix: `${apiPrefix}/tutorial` })
fastify.register(gachaApiPlugin, { prefix: `${apiPrefix}/gacha` })
fastify.register(partyApiPlugin, { prefix: `${apiPrefix}/party` })
fastify.register(expodApiPlugin, { prefix: `${apiPrefix}/expod` })
fastify.register(storyQuestApiPlugin, { prefix: `${apiPrefix}/story_quest` })
fastify.register(optionApiPlugin, { prefix: `${apiPrefix}/option` })
fastify.register(singleBattleQuestApiPlugin, { prefix: `${apiPrefix}/single_battle_quest` })
fastify.register(multiBattleQuestApiPlugin, { prefix: `${apiPrefix}/multi_battle_quest` })
fastify.register(attentionApiPlugin, { prefix: `${apiPrefix}/attention` })
fastify.register(characterApiPlugin, { prefix: `${apiPrefix}/character` })
fastify.register(partyGroupApiPlugin, { prefix: `${apiPrefix}/party_group` })
fastify.register(equipmentApiPlugin, { prefix: `${apiPrefix}/equipment` })
fastify.register(exBoostApiPlugin, { prefix: `${apiPrefix}/ex_boost` })
fastify.register(boxGachaApiPlugin, { prefix: `${apiPrefix}/box_gacha` })
fastify.register(shopApiPlugin, { prefix: `${apiPrefix}/shop` })
fastify.register(encyclopediaApiPlugin, { prefix: `${apiPrefix}/encyclopedia` })
fastify.register(mailApiPlugin, { prefix: `${apiPrefix}/mail` })
fastify.register(rankingEventApiPlugin, { prefix: `${apiPrefix}/ranking_event` })
fastify.register(missionApiPlugin, { prefix: `${apiPrefix}/mission` })
fastify.register(paymentApiPlugin, { prefix: `${apiPrefix}/payment` })
fastify.register(newsApiPlugin, { prefix: `${apiPrefix}/news` })
fastify.register(raidEventApiPlugin, { prefix: `${apiPrefix}/event/raid` })
fastify.register(rushEventApiPlugin, { prefix: `${apiPrefix}/event/rush` })

// openapi
fastify.register(openapiPlugin, { prefix: "/openapi/service" })

// infodesk
fastify.register(infodeskPlugin, { prefix: "/infodesk" })

// //// 注册 CN 客户端兼容入口并复用主仓库账号和玩家数据 [@x380kkm 2026-07-21] ////
fastify.register(cnVersionCheckPlugin)
fastify.register(cnLeitingAuthPlugin, { prefix: "/api/index.php" })
fastify.register(cnLeitingPaymentPlugin, { prefix: "/api/index.php/channels/channel_leiting_pay" })
fastify.register(cnToolPlugin, { prefix: "/api/index.php/tool" })
fastify.register(cnLoadPlugin, { prefix: "/api/index.php" })
fastify.register(mailApiPlugin, { prefix: "/api/index.php/mail" })
fastify.register(missionApiPlugin, { prefix: "/api/index.php/mission" })
fastify.register(optionApiPlugin, { prefix: "/api/index.php/option" })
fastify.register(expodApiPlugin, { prefix: "/api/index.php/expod" })
fastify.register(characterApiPlugin, { prefix: "/api/index.php/character" })
fastify.register(partyApiPlugin, { prefix: "/api/index.php/party" })
fastify.register(partyGroupApiPlugin, { prefix: "/api/index.php/party_group" })
fastify.register(equipmentApiPlugin, { prefix: "/api/index.php/equipment" })
fastify.register(boxGachaApiPlugin, { prefix: "/api/index.php/box_gacha" })
fastify.register(exBoostApiPlugin, { prefix: "/api/index.php/ex_boost" })
fastify.register(shopApiPlugin, { prefix: "/api/index.php/shop" })
fastify.register(encyclopediaApiPlugin, { prefix: "/api/index.php/encyclopedia" })
fastify.register(paymentApiPlugin, { prefix: "/api/index.php/payment" })
fastify.register(storyQuestApiPlugin, { prefix: "/api/index.php/story_quest" })
fastify.register(attentionApiPlugin, { prefix: "/api/index.php/attention" })
fastify.register(reproduceApiPlugin, { prefix: "/api/index.php/reproduce" })
fastify.register(rankingEventApiPlugin, { prefix: "/api/index.php/ranking_event" })
fastify.register(newsApiPlugin, { prefix: "/api/index.php/news" })
fastify.register(raidEventApiPlugin, { prefix: "/api/index.php/event/raid" })
fastify.register(rushEventApiPlugin, { prefix: "/api/index.php/event/rush" })
fastify.register(tutorialApiPlugin, {
    prefix: "/api/index.php/tutorial",
    resolveGacha: getCnTutorialGachaSync,
    drawGacha: drawCnGachaSync,
})
fastify.register(gachaApiPlugin, {
    prefix: "/api/index.php/gacha",
    resolveGacha: getCnTutorialGachaSync,
    drawGacha: drawCnGachaSync,
})
fastify.register(singleBattleQuestApiPlugin, { prefix: "/api/index.php/single_battle_quest" })
fastify.register(multiBattleQuestApiPlugin, { prefix: "/api/index.php/multi_battle_quest" })
fastify.register(registerBattleSettlementRoutes, {
    prefix: "/api/index.php/multi_battle_quest",
    battleGroup: "multi",
})
fastify.register(cnAssetPlugin, { prefix: "/api/index.php/asset" })
fastify.post("/api/index.php/assetintitle/version_info_in_title", async (request, reply) => {
    const baseUrl = process.env.CN_CDN_BASE_URL ?? `http://${request.headers.host ?? "localhost:8000"}/patch/cn`
    reply.header("content-type", "application/x-msgpack")
    return reply.send({
        data_headers: { force_update: false, asset_update: false, short_udid: 0, viewer_id: 0, servertime: getServerTime(), result_code: 1 },
        data: getCnVersionInfo(baseUrl),
    })
})
// //// /注册 CN 客户端兼容入口并复用主仓库账号和玩家数据 ////

// //// 仅在显式兼容开关下注册无角色隔离的旧 Web 管理入口 [@x380kkm 2026-07-22] ////
if (process.env.ENABLE_LEGACY_WEB_ADMIN === "1") {
    fastify.register(indexWebPlugin, { prefix: "/" })
    fastify.register(indexWebApiPlugin, { prefix: "/api" })
}
// //// /仅在显式兼容开关下注册无角色隔离的旧 Web 管理入口 ////

// //// 注册受 bearer token 保护的跨平台管理控制面 [@x380kkm 2026-07-21] ////
fastify.register(managementPlugin, { prefix: "/manage" })
// //// /注册受 bearer token 保护的跨平台管理控制面 ////

// web static
fastify.register(fastifyStatic, {
    root: path.join(__dirname, "..", "web/public"),
    prefix: "/public",
    decorateReply: false
})

// static CDN
const cdnDir = process.env.CDN_DIR || ".cdn"
fastify.register(fastifyStatic, {
    root: path.isAbsolute(cdnDir) ? cdnDir : path.join(__dirname, "..", process.env.CDN_DIR || ".cdn"),
    prefix: "/patch/Live/2.0.0",
    decorateReply: false
})

const cnCdnDir = path.isAbsolute(cdnDir)
    ? path.join(cdnDir, "cn")
    : path.join(__dirname, "..", cdnDir, "cn")
fastify.register(fastifyStatic, {
    root: cnCdnDir,
    prefix: "/patch/cn",
    decorateReply: false,
})

// //// 提醒绕过待恢复入口并恢复虚拟时间后启动 HTTP 服务 [@x380kkm 2026-07-22] ////
const listenHost = process.env.LISTEN_HOST ?? "localhost"

const envListenPort = process.env.LISTEN_PORT === undefined ? 8000 : Number.parseInt(process.env.LISTEN_PORT)
const listenPort = isNaN(envListenPort) ? 8000 : envListenPort

// //// 提供跨平台部署健康探针 [@x380kkm 2026-07-24] ////
fastify.get("/healthz", async (_request, reply) => {
    return reply.status(200).send({
        status: "ok",
        service: "starpoint",
        serverDate: getServerDate().toISOString(),
        httpPort: listenPort,
        sessionPort: getMultiplayerSessionPort(),
    })
})
// //// /提供跨平台部署健康探针 ////

fastify.addHook("onClose", async () => stopMultiplayerSessionServer())
async function startServer(): Promise<void> {
    if (await managementStore.getPendingRestore() !== null) {
        console.warn("Pending database restore was not applied. Start the service through out/start.js.")
    }
    await managementStore.applyVirtualTime()
    await startMultiplayerSessionServer({
        clock: { getCurrentTimeMilliseconds: () => getServerDate().getTime() },
        identityProvider: starpointParticipantIdentityProvider,
        roomRepository: matchmakingStore,
    })
    try {
        await fastify.listen({ port: listenPort, host: listenHost })
    } catch (error) {
        await stopMultiplayerSessionServer()
        throw error
    }
    console.log(`StarPoint is listening on http://${listenHost}:${listenPort}`)
}

startServer().catch((error: unknown) => {
    console.error(error)
    process.exit(1)
})
// //// /提醒绕过待恢复入口并恢复虚拟时间后启动 HTTP 服务 ////
