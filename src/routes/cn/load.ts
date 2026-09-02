// audience: internal | external
// # cn-load
// CN load 接口验证 viewer session 后只序列化该账号的玩家数据.

import { FastifyInstance, FastifyReply, FastifyRequest } from "fastify"
import { SessionType } from "../../data/types"
import {
    collectPlayerDataPooledExpSync,
    dailyResetPlayerDataSync,
    getPlayerFromAccountIdSync,
    getPlayerSync,
    getSession,
} from "../../data/wdfpData"
import { getClientSerializedData } from "../../data/utils"
import { CN_BASELINE_GACHA_ID } from "../../lib/cnAssets"
import { CN_TUTORIAL_GACHA_ID } from "../../lib/tutorial"
import { generateDataHeaders, getServerDate, getServerTime } from "../../utils"

interface CnLoadBody {
    keychain?: number
    viewer_id?: number
}

// //// 补充 CN 载入响应字段 [@x380kkm 2026-07-22] ////
export function addCnLoadCompatibilityFields(data: Record<string, any>, resVer?: string): void {
    data.available_asset_version = resVer ?? process.env.CN_RES_VERSION ?? "1.4.54"
    if (data.user_info !== undefined) {
        const userInfo = data.user_info as Record<string, any>
        if (typeof userInfo.last_login_time === "number") {
            const date = new Date(userInfo.last_login_time * 1000)
            const pad = (value: number) => value.toString().padStart(2, "0")
            userInfo.last_login_time = `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
        }
        userInfo.is_bought_fund_ex_quest ??= false
        userInfo.is_bought_fund_main_quest ??= false
        userInfo.is_bought_fund_laite ??= false
        userInfo.is_bought_fund_laite2 ??= false
        userInfo.is_bought_fund_laite3 ??= false
        userInfo.is_newbie ??= true
        userInfo.is_comeback ??= false
        userInfo.month_card_remain_days ??= 0
        userInfo.weekly_bonus_remain_days ??= 0
        userInfo.monthly_payment_total ??= 0
        userInfo.renewal_gift_remain_days ??= 0
    }
    if (data.user_option !== undefined) {
        const userOption = data.user_option as Record<string, any>
        userOption.episode_encyclopedia_suggest_show ??= false
        userOption.server_push ??= false
        userOption.stamina ??= false
    }
    data.survey_url = ""
    data.qq_group_url = ""
    data.bug_report_url = ""
    data.cn_crash_url = ""
    data.enable_gift = false
    data.enable_customer_service = false
    data.enable_rename = true
    data.enable_delete_file = false
    data.enable_newbie = false
    data.enable_little_assistant = false
    data.mission_tips = false
    data.monthly_tip = false
    data.simple_payment_item_list = []
    data.ex_boost_draw_result = null
    data.pass_force_reward = false
    data.crazy_gacha_result_list = []
    data.last_crazy_gacha_draw_result = []
    data.fund_receive_list = []
    data.login_info = {}
    data.tower_dungeon_list = []
    data.special_exchange_campaign_list = []
    data.win_lottery_active_mission_list = []
    data.stars_gacha_campaign_list = []
    data.favorite_party_group_list = []
    data.ranking_event_reward = []
    data.party_list = []
    if (!Array.isArray(data.gacha_info_list)) data.gacha_info_list = []
    if (!data.gacha_info_list.some((gachaInfo: Record<string, any>) => Number(gachaInfo.gacha_id) === CN_TUTORIAL_GACHA_ID)) {
        data.gacha_info_list.push({ gacha_id: CN_TUTORIAL_GACHA_ID, is_daily_first: true, is_account_first: true })
    }
    if (!data.gacha_info_list.some((gachaInfo: Record<string, any>) => Number(gachaInfo.gacha_id) === CN_BASELINE_GACHA_ID)) {
        data.gacha_info_list.push({ gacha_id: CN_BASELINE_GACHA_ID, is_daily_first: true, is_account_first: true })
    }
    data.payment_rebate_info = { expired_time: 0, status: 0, start_time: 0 }
    data.monthly_charge_bonus_info = { bonus_days: 0, expired_time: 0, init_time: 0, status: 0, start_time: 0 }
    data.comeback_campaign_boss_boost = { period_start_time: 0, period_end_time: 0 }
}
// //// /补充 CN 载入响应字段 ////

// //// 载入并更新 CN 玩家数据 [@x380kkm 2026-07-22] ////
const routes = async (fastify: FastifyInstance) => {
    fastify.post("/load", async (request: FastifyRequest, reply: FastifyReply) => {
        const body = (request.body ?? {}) as CnLoadBody
        const viewerId = Number(body.viewer_id ?? body.keychain)
        if (!Number.isInteger(viewerId) || viewerId <= 0) return reply.status(400).send({ error: "invalid_viewer_id" })

        const session = await getSession(String(viewerId))
        if (session === null || session.type !== SessionType.VIEWER) {
            return reply.status(400).send({ error: "invalid_viewer_session" })
        }

        const player = getPlayerFromAccountIdSync(session.accountId)
        if (player === null) return reply.status(500).send({ error: "no_player" })

        const now = getServerDate()
        dailyResetPlayerDataSync(player, now)
        collectPlayerDataPooledExpSync(player, now)
        const clientData = getClientSerializedData(player.id, { viewerId }) as Record<string, any> | null
        if (clientData === null) return reply.status(500).send({ error: "no_player_data" })

        addCnLoadCompatibilityFields(clientData, typeof request.headers.res_ver === "string" ? request.headers.res_ver : undefined)
        return reply.header("content-type", "application/x-msgpack").send({
            data_headers: generateDataHeaders({ asset_update: true, viewer_id: viewerId, servertime: getServerTime() }),
            data: clientData,
        })
    })
}
// //// /载入并更新 CN 玩家数据 ////

export default routes
