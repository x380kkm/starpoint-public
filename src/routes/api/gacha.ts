// audience: internal | external
// # gacha
// 此模块执行扭蛋支付, 抽取, 发放和持久化, 并允许客户端版本注入扭蛋解析和抽取策略.

import { FastifyInstance, FastifyPluginOptions, FastifyReply, FastifyRequest } from "fastify";
import { getAccountPlayers, getPlayerGachaCampaignSync, getPlayerGachaInfoListSync, getPlayerGachaInfoSync, getPlayerItemSync, getPlayerSync, getSession, insertPlayerGachaCampaignSync, insertPlayerGachaInfoSync, runPlayerDataTransactionSync, updatePlayerGachaCampaignSync, updatePlayerGachaInfoSync, updatePlayerItemSync, updatePlayerSync } from "../../data/wdfpData";
import { generateDataHeaders } from "../../utils";
import { drawGachaSync, isGachaPoolItem, rewardPlayerGachaDrawResultSync } from "../../lib/gacha";
import { getGachaCampaignIdSync, getGachaSync } from "../../lib/assets";
import { Gacha, GachaDrawResult, GachaType } from "../../lib/types";
import { serializeGachaCampaign } from "../../data/utils";
import { UserGachaCampaign } from "../../data/types";
import { givePlayerCharacterSync } from "../../lib/character";
import { givePlayerEquipmentSync } from "../../lib/equipment";

interface ExecBody {
    api_count: number,
    payment_type: number,
    number_of_exec: number,
    viewer_id: number,
    gacha_id: number,
    type: number
}

interface ExchangeCharacterBody {
    character_id: number,
    api_count: number,
    gacha_id: number,
    viewer_id: number
}

interface ExchangeEquipmentBody {
    equipment_id: number,
    gacha_id: number,
    viewer_id: number,
    api_count: number
}

enum GachaPaymentType {
    EMPTY,
    FREE_VMONEY,
    VMONEY,
    TICKET,
    CAMPAIGN
}

enum GachaExecType {
    EMPTY,
    VMONEY_SINGLE,
    VMONEY_MULTI,
    UNKNOWN_1,
    UNKNOWN_2,
    DAILY_SINGLE,
    UNKNOWN_3,
    CAMPAIGN_SINGLE,
    CAMPAIGN_MULTI,
    MULTI_TICKET,
    SINGLE_TICKET,
    UNKNOWN_4,
    UNKNOWN_5,
    MULTI_WEAPON_TICKET
}

const exchangeRequiredPoints = 250

class GachaRequestError extends Error {}

// //// 验证扭蛋支付方式和执行次数组合 [@x380kkm 2026-07-24] ////
function isValidGachaExecutionRequest(paymentType: number, type: number, numberOfExec: number): boolean {
    if (!Number.isSafeInteger(numberOfExec)) return false
    switch (paymentType) {
        case GachaPaymentType.FREE_VMONEY:
            return numberOfExec === 1 && (type === GachaExecType.VMONEY_SINGLE || type === GachaExecType.VMONEY_MULTI)
        case GachaPaymentType.VMONEY:
            return numberOfExec === 1 && type === GachaExecType.DAILY_SINGLE
        case GachaPaymentType.TICKET:
            return numberOfExec >= 1 && (
                type === GachaExecType.MULTI_TICKET
                || type === GachaExecType.SINGLE_TICKET
                || type === GachaExecType.MULTI_WEAPON_TICKET
            )
        case GachaPaymentType.CAMPAIGN:
            return numberOfExec === 1 && (type === GachaExecType.CAMPAIGN_SINGLE || type === GachaExecType.CAMPAIGN_MULTI)
        default:
            return false
    }
}
// //// /验证扭蛋支付方式和执行次数组合 ////

export interface GachaRouteOptions extends FastifyPluginOptions {
    resolveGacha?: (gachaId: number) => Gacha | null
    drawGacha?: (gacha: Gacha, drawAmount: number) => GachaDrawResult
}

const routes = async (fastify: FastifyInstance, options: GachaRouteOptions) => {
    const resolveGacha = options.resolveGacha ?? getGachaSync
    const drawGacha = options.drawGacha ?? drawGachaSync

    fastify.post("/exchange_equipment", async (request: FastifyRequest, reply: FastifyReply) => {
        const body = request.body as ExchangeEquipmentBody

        const equipmentId = body.equipment_id
        const gachaId = body.gacha_id
        const viewerId = body.viewer_id
        if (isNaN(viewerId) || isNaN(equipmentId) || isNaN(gachaId)) return reply.status(400).send({
            "error": "Bad Request",
            "message": "Invalid request body."
        })

        const viewerIdSession = await getSession(viewerId.toString())
        if (!viewerIdSession) return reply.status(400).send({
            "error": "Bad Request",
            "message": "Invalid viewer id."
        })

        // get player
        const playerIds = await getAccountPlayers(viewerIdSession.accountId)
        const playerId = playerIds[0]
        if (isNaN(playerId)) return reply.status(500).send({
            "error": "Internal Server Error",
            "message": "No players bound to account."
        })

        let exchange: {
            giveResult: ReturnType<typeof givePlayerEquipmentSync>
            gachaInfo: NonNullable<ReturnType<typeof getPlayerGachaInfoSync>>
            newExchangePoints: number
        }
        try {
            exchange = runPlayerDataTransactionSync(() => {
                const gachaInfo = getPlayerGachaInfoSync(playerId, gachaId)
                if (gachaInfo === null) throw new GachaRequestError("No data for gacha with provided id.")

                const newExchangePoints = (gachaInfo.gachaExchangePoint ?? 0) - exchangeRequiredPoints
                if (0 > newExchangePoints) throw new GachaRequestError("Not enough exchange points.")

                const giveResult = givePlayerEquipmentSync(playerId, equipmentId, 1)
                updatePlayerGachaInfoSync(playerId, {
                    gachaId: gachaId,
                    gachaExchangePoint: newExchangePoints,
                })
                return { giveResult, gachaInfo, newExchangePoints }
            })
        } catch (error) {
            if (error instanceof GachaRequestError) return reply.status(400).send({
                "error": "Bad Request",
                "message": error.message,
            })
            throw error
        }

        reply.header("content-type", "application/x-msgpack")
        return reply.status(200).send({
            "data_headers": generateDataHeaders({
                viewer_id: viewerId
            }),
            "data": {
                "equipment_list": [
                    exchange.giveResult
                ],
                "gacha_info_list": [
                    {
                        "gacha_id": gachaId,
                        "is_account_first": exchange.gachaInfo.isAccountFirst,
                        "is_daily_first": exchange.gachaInfo.isDailyFirst,
                        "gacha_exchange_point": exchange.newExchangePoints
                    }
                ],
                "encyclopedia_info": [],
                "mail_arrived": false
            }
        })

    })

    fastify.post("/exchange_character", async (request: FastifyRequest, reply: FastifyReply) => {
        const body = request.body as ExchangeCharacterBody

        const characterId = body.character_id
        const gachaId = body.gacha_id
        const viewerId = body.viewer_id
        if (isNaN(viewerId) || isNaN(characterId) || isNaN(gachaId)) return reply.status(400).send({
            "error": "Bad Request",
            "message": "Invalid request body."
        })

        const gacha = resolveGacha(gachaId)
        if (gacha === null || gacha.type !== GachaType.CHARACTER || !isGachaPoolItem(gacha, characterId)) return reply.status(400).send({
            "error": "Bad Request",
            "message": "Character is not in the requested gacha."
        })

        const viewerIdSession = await getSession(viewerId.toString())
        if (!viewerIdSession) return reply.status(400).send({
            "error": "Bad Request",
            "message": "Invalid viewer id."
        })

        // get player
        const playerIds = await getAccountPlayers(viewerIdSession.accountId)
        const playerId = playerIds[0]
        if (isNaN(playerId)) return reply.status(500).send({
            "error": "Internal Server Error",
            "message": "No players bound to account."
        })

        let exchange: {
            giveResult: NonNullable<ReturnType<typeof givePlayerCharacterSync>>
            gachaInfo: NonNullable<ReturnType<typeof getPlayerGachaInfoSync>>
            newExchangePoints: number
        }
        try {
            exchange = runPlayerDataTransactionSync(() => {
                const gachaInfo = getPlayerGachaInfoSync(playerId, gachaId)
                if (gachaInfo === null) throw new GachaRequestError("No data for gacha with provided id.")

                const newExchangePoints = (gachaInfo.gachaExchangePoint ?? 0) - exchangeRequiredPoints
                if (0 > newExchangePoints) throw new GachaRequestError("Not enough exchange points.")

                const giveResult = givePlayerCharacterSync(playerId, characterId)
                if (giveResult === null) throw new GachaRequestError("Could not give player character.")

                updatePlayerGachaInfoSync(playerId, {
                    gachaId: gachaId,
                    gachaExchangePoint: newExchangePoints,
                })
                return { giveResult, gachaInfo, newExchangePoints }
            })
        } catch (error) {
            if (error instanceof GachaRequestError) return reply.status(400).send({
                "error": "Bad Request",
                "message": error.message,
            })
            throw error
        }

        reply.header("content-type", "application/x-msgpack")
        return reply.status(200).send({
            "data_headers": generateDataHeaders({
                viewer_id: viewerId
            }),
            "data": {
                "character_list": [
                    exchange.giveResult.character
                ],
                "item_list": exchange.giveResult.item !== undefined ? {
                    [exchange.giveResult.item.id]: exchange.giveResult.item.count
                } : [],
                "gacha_info_list": [
                    {
                        "gacha_id": gachaId,
                        "is_account_first": exchange.gachaInfo.isAccountFirst,
                        "is_daily_first": exchange.gachaInfo.isDailyFirst,
                        "gacha_exchange_point": exchange.newExchangePoints
                    }
                ],
                "encyclopedia_info": [],
                "mail_arrived": false
            }
        })

    })

    fastify.post("/exec", async (request: FastifyRequest, reply: FastifyReply) => {
        const body = request.body as ExecBody

        const viewerId = body.viewer_id
        const gachaId = body.gacha_id
        const paymentType = body.payment_type
        const numberOfExec = body.number_of_exec
        const type = body.type
        if (isNaN(viewerId) || isNaN(gachaId) || isNaN(paymentType) || isNaN(numberOfExec) || isNaN(type)) return reply.status(400).send({
            "error": "Bad Request",
            "message": "Invalid request body."
        })
        if (!isValidGachaExecutionRequest(paymentType, type, numberOfExec)) return reply.status(400).send({
            "error": "Bad Request",
            "message": "Invalid gacha execution type."
        })

        const viewerIdSession = await getSession(viewerId.toString())
        if (!viewerIdSession) return reply.status(400).send({
            "error": "Bad Request",
            "message": "Invalid viewer id."
        })

        // get player
        const playerIds = await getAccountPlayers(viewerIdSession.accountId)
        const playerId = playerIds[0]
        if (isNaN(playerId)) return reply.status(500).send({
            "error": "Internal Server Error",
            "message": "No players bound to account."
        })

        // get the gacha
        const gachaData = resolveGacha(gachaId)
        if (gachaData === null) return reply.status(400).send({
            "error": "Bad Request",
            "message": "Gacha doesn't exist."
        })
        const isCharacterGacha = gachaData.type == GachaType.CHARACTER

        let settlement: {
            playerPaidVmoney: number
            playerFreeVmoney: number
            gachaCampaigns: UserGachaCampaign[]
            items: Record<number, number>
            rewardResult: ReturnType<typeof rewardPlayerGachaDrawResultSync>
            newGachaExchangePoint: number
        }
        try {
            settlement = runPlayerDataTransactionSync(() => {
                const player = getPlayerSync(playerId)
                if (player === null) throw new Error("No players bound to account.")
                const existingPlayerGachaData = getPlayerGachaInfoSync(playerId, gachaId)
                const playerGachaData = existingPlayerGachaData ?? {
                    gachaId: gachaId,
                    isAccountFirst: true,
                    isDailyFirst: true,
                    gachaExchangePoint: 0,
                }
                const insertPlayerGachaData = existingPlayerGachaData === null
                let pullCount = 0
                let playerPaidVmoney = player.vmoney
                let playerFreeVmoney = player.freeVmoney
                const gachaCampaigns: UserGachaCampaign[] = []
                const items: Record<number, number> = {}
                let ticketUpdate: { itemId: number; newItemCount: number } | null = null
                let campaignUpdate: {
                    campaignId: number
                    insert: boolean
                    value: NonNullable<ReturnType<typeof getPlayerGachaCampaignSync>>
                } | null = null

                switch (paymentType) {
                    case GachaPaymentType.FREE_VMONEY: {
                        const isMulti = type === GachaExecType.VMONEY_MULTI
                        const cost = isMulti ? gachaData.multiCost : gachaData.singleCost
                        const overflow = cost > playerFreeVmoney ? cost - playerFreeVmoney : 0
                        playerFreeVmoney = overflow > 0 ? 0 : playerFreeVmoney - cost
                        playerPaidVmoney = overflow > 0 ? playerPaidVmoney - overflow : playerPaidVmoney
                        pullCount = isMulti ? 10 : 1
                        break
                    }
                    case GachaPaymentType.VMONEY: {
                        if (!playerGachaData.isDailyFirst) throw new GachaRequestError("Already did daily paid summon.")
                        playerPaidVmoney -= isCharacterGacha ? 50 : 25
                        pullCount = 1
                        break
                    }
                    case GachaPaymentType.TICKET: {
                        const isWeapon = type === GachaExecType.MULTI_WEAPON_TICKET
                        const isMulti = type === GachaExecType.MULTI_TICKET || isWeapon
                        const itemId = isMulti ? (isWeapon ? 999004 : 999001) : (isWeapon ? 999005 : 999003)
                        const itemCount = getPlayerItemSync(playerId, itemId)
                        const useTicketCount = Math.max(1, numberOfExec)
                        const newItemCount = (itemCount ?? -1) - useTicketCount
                        if (0 > newItemCount) throw new GachaRequestError("Not enough tickets.")
                        pullCount = useTicketCount * (isMulti ? 10 : 1)
                        ticketUpdate = { itemId, newItemCount }
                        items[itemId] = newItemCount
                        break
                    }
                    case GachaPaymentType.CAMPAIGN: {
                        const gachaCampaignId = getGachaCampaignIdSync(gachaId)
                        if (gachaCampaignId === null) throw new GachaRequestError("No gacha campaign assigned to gacha.")
                        const existingPlayerCampaignData = getPlayerGachaCampaignSync(playerId, gachaId, gachaCampaignId)
                        const playerCampaignData = existingPlayerCampaignData ?? {
                            gachaId: gachaId,
                            campaignId: gachaCampaignId,
                            count: 1,
                        }
                        const insertCampaignData = existingPlayerCampaignData === null
                        if (0 >= playerCampaignData.count) throw new GachaRequestError("Already redeemed campaign for this period.")
                        const campaignValue = { ...playerCampaignData, count: 0 }
                        campaignUpdate = { campaignId: gachaCampaignId, insert: insertCampaignData, value: campaignValue }
                        gachaCampaigns.push(serializeGachaCampaign(campaignValue))
                        pullCount = type === GachaExecType.CAMPAIGN_MULTI ? 10 : 1
                        break
                    }
                    default:
                        throw new GachaRequestError("Invalid payment type.")
                }

                if (pullCount === 0) throw new GachaRequestError("Invalid payment type.")
                if (playerFreeVmoney < 0 || playerPaidVmoney < 0) throw new GachaRequestError("Not enough beads.")

                const drawResult = drawGacha(gachaData, pullCount)
                const rewardResult = rewardPlayerGachaDrawResultSync(playerId, gachaData, drawResult)
                if (ticketUpdate !== null) updatePlayerItemSync(playerId, ticketUpdate.itemId, ticketUpdate.newItemCount)
                if (campaignUpdate !== null) {
                    if (campaignUpdate.insert) insertPlayerGachaCampaignSync(playerId, campaignUpdate.value)
                    else updatePlayerGachaCampaignSync(playerId, gachaId, campaignUpdate.campaignId, 0)
                }

                const newGachaExchangePoint = (playerGachaData.gachaExchangePoint ?? 0) + pullCount
                if (insertPlayerGachaData) {
                    insertPlayerGachaInfoSync(playerId, {
                        ...playerGachaData,
                        isAccountFirst: false,
                        isDailyFirst: false,
                        gachaExchangePoint: newGachaExchangePoint,
                    })
                } else {
                    updatePlayerGachaInfoSync(playerId, {
                        gachaId: gachaId,
                        isDailyFirst: false,
                        isAccountFirst: false,
                        gachaExchangePoint: newGachaExchangePoint,
                    })
                }
                updatePlayerSync({ id: playerId, vmoney: playerPaidVmoney, freeVmoney: playerFreeVmoney })
                return { playerPaidVmoney, playerFreeVmoney, gachaCampaigns, items, rewardResult, newGachaExchangePoint }
            })
        } catch (error) {
            if (error instanceof GachaRequestError) return reply.status(400).send({
                "error": "Bad Request",
                "message": error.message,
            })
            throw error
        }

        reply.header("content-type", "application/x-msgpack")
        if (isCharacterGacha) {
            return reply.status(200).send({
                "data_headers": generateDataHeaders({
                    viewer_id: viewerId
                }),
                "data": {
                    "user_info": {
                        "free_vmoney": settlement.playerFreeVmoney,
                        "vmoney": settlement.playerPaidVmoney
                    },
                    "draw": settlement.rewardResult.draw,
                    "character_list": settlement.rewardResult.characters,
                    "item_list": {
                        ...settlement.items,
                        ...settlement.rewardResult.items
                    },
                    "gacha_campaign_list": settlement.gachaCampaigns,
                    "gacha_info_list": [
                        {
                            "gacha_id": gachaId,
                            "is_account_first": false,
                            "is_daily_first": false,
                            "gacha_exchange_point": settlement.newGachaExchangePoint
                        }
                    ],
                    "encyclopedia_info": [],
                    "mail_arrived": false
                }
            })
        } else {
            return reply.status(200).send({
                "data_headers": generateDataHeaders({
                    viewer_id: viewerId
                }),
                "data": {
                    "user_info": {
                        "free_vmoney": settlement.playerFreeVmoney,
                        "vmoney": settlement.playerPaidVmoney
                    },
                    "is_erupt": false,
                    "draw_equipment": settlement.rewardResult.draw,
                    "item_list": {
                        ...settlement.items,
                        ...settlement.rewardResult.items
                    },
                    "equipment_list": settlement.rewardResult.equipment,
                    "gacha_info_list": [
                        {
                            "gacha_id": gachaId,
                            "is_account_first": false,
                            "is_daily_first": false,
                            "gacha_exchange_point": settlement.newGachaExchangePoint
                        }
                    ],
                    "encyclopedia_info": [],
                    "mail_arrived": false
                }
            })
        }
        
    })
}

export default routes;
