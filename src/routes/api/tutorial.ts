// audience: internal | external
// # tutorial
// 教程接口通过角色日期序列化器和版本对应的扭蛋元数据兼容 Global 和 CN 客户端.

import { FastifyInstance, FastifyPluginOptions, FastifyReply, FastifyRequest } from "fastify";
import { clientSerializeDate } from "../../data/utils";
import { getAccountPlayers, getPlayerSync, getPlayerTriggeredTutorialsSync, getSession, insertDefaultPlayerCharacterSync, insertPlayerTriggeredTutorialSync, updatePlayerSync } from "../../data/wdfpData";
import { generateDataHeaders, getServerTime } from "../../utils";
import { getGachaSync } from "../../lib/assets";
import { drawGachaSync, rewardPlayerGachaDrawResultSync } from "../../lib/gacha";
import { Gacha, GachaCharacterDraw, GachaDrawResult } from "../../lib/types";

interface UpdateStepBody {
    viewer_id: number
    step: number
    api_count: number
    skip: boolean
    statistics: Object
    name?: string
    gacha_id?: number
}

interface FinishTriggerBody {
    api_count: number,
    tutorial_ids: number[],
    viewer_id: number
}

interface TutorialRouteOptions extends FastifyPluginOptions {
    serializeCharacterDate?: (date: Date) => number | string
    resolveGacha?: (gachaId: number) => Gacha | null
    drawGacha?: (gacha: Gacha, drawAmount: number) => GachaDrawResult
}

const freeTutorialCharacterId = 243001

const routes = async (fastify: FastifyInstance, options: TutorialRouteOptions) => {
    const serializeCharacterDate = options.serializeCharacterDate ?? clientSerializeDate
    const resolveGacha = options.resolveGacha ?? getGachaSync
    const drawGacha = options.drawGacha ?? drawGachaSync
    fastify.post("/finish_trigger", async (request: FastifyRequest, reply: FastifyReply) => {
        const body = request.body as FinishTriggerBody

        const viewerId = body.viewer_id
        const tutorialIds = body.tutorial_ids
        if (!viewerId || isNaN(viewerId) || !tutorialIds || !(tutorialIds instanceof Array)) return reply.status(400).send({
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

        // //// 记录首次完成的教程 [@x380kkm 2026-07-22] ////
        const completedTutorialIds = new Set(getPlayerTriggeredTutorialsSync(playerId))
        for (const tutorialId of tutorialIds) {
            if (!completedTutorialIds.has(tutorialId)) {
                insertPlayerTriggeredTutorialSync(playerId, tutorialId)
            }
        }
        // //// /记录首次完成的教程 ////

        reply.header("content-type", "application/x-msgpack")
        reply.status(200).send({
            "data_headers": generateDataHeaders({
                viewer_id: viewerId
            }),
            "data": []
        })
    })

    fastify.post("/update_step", async (request: FastifyRequest, reply: FastifyReply) => {
        const body = request.body as UpdateStepBody

        const viewerId = body.viewer_id
        const completedStep = body.step
        const skip = body.skip || false
        if (!viewerId || isNaN(completedStep) || isNaN(viewerId)) return reply.status(400).send({
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
        const player = !isNaN(playerId) ? getPlayerSync(playerId) : null

        if (player === null) return reply.status(500).send({
            "error": "Internal Server Error",
            "message": "No player bound to account."
        })

        // check if tutorial is already completed
        const completedTutorial = getPlayerTriggeredTutorialsSync(playerId)
        if (completedTutorial.find((value: number) => value === 12)) return reply.status(400).send({
            "error": "Bad Request",
            "message": "Tutorial already completed"
        })

        // update player
        const currentStep = player.tutorialStep
        const storedNextStep = completedStep + 1
        const nextStep = storedNextStep + (skip ? 11 : 0)

        if ((currentStep || 0) > storedNextStep) return reply.status(400).send({
            "error": "Bad Request",
            "message": "Attempt to redo previous tutorial step."
        })

        let tutorialGachaId: number | null = null
        let tutorialGacha: Gacha | null = null
        if (nextStep === 15) {
            if (body.gacha_id === undefined || isNaN(body.gacha_id)) return reply.status(400).send({
                "error": "Bad Request",
                "message": "Tutorial gacha id is required."
            })

            tutorialGachaId = body.gacha_id
            tutorialGacha = resolveGacha(tutorialGachaId)
            if (tutorialGacha === null) return reply.status(400).send({
                "error": "Bad Request",
                "message": `Gacha with id '${tutorialGachaId}' does not exist.`
            })
        }

        updatePlayerSync({
            id: playerId,
            tutorialStep: storedNextStep,
            tutorialSkipFlag: skip,
            name: body.name
        })

        reply.header("content-type", "application/x-msgpack")
        const headers = generateDataHeaders({
            viewer_id: viewerId
        })
        if (nextStep === 15 && tutorialGachaId !== null && tutorialGacha !== null) {
            // perform pull
            const drawResult = drawGacha(tutorialGacha, 1)

            // reward pull
            const rewardResult = rewardPlayerGachaDrawResultSync(playerId, tutorialGacha, drawResult)

            const newFreeVmoney = player.freeVmoney - tutorialGacha.singleCost
            updatePlayerSync({
                id: playerId,
                freeVmoney: newFreeVmoney
            })

            const draw = rewardResult.draw[0] as GachaCharacterDraw
            draw.movie_id = "normal_guarantee"
            draw.seed = 10007656

            return reply.status(200).send({
                "data_headers": headers,
                "data": {
                    "step": nextStep,
                    "user_info": {
                        "free_vmoney": newFreeVmoney,
                    },
                    "gacha": {
                        "draw": rewardResult.draw,
                        "gacha_info_list": [
                            {
                                "gacha_id": tutorialGachaId,
                                "is_account_first": false,
                                "is_daily_first": false,
                            }
                        ],
                    },
                    "character_list": rewardResult.characters,
                    "item_list": rewardResult.items,
                    "encyclopedia_info": [],
                    "mail_arrived": false,
                    "start_time": getServerTime()
                }
            })
        } else if (nextStep === 16) {
            // give 1500 vmoney
            const newVMoney = player.freeVmoney + 1500
            updatePlayerSync({
                id: playerId,
                freeVmoney: newVMoney
            })

            // give free character
            const serializedDate = serializeCharacterDate(new Date())

            insertDefaultPlayerCharacterSync(playerId, freeTutorialCharacterId)

            reply.status(200).send({
                "data_headers": headers,
                "data": {
                    "step": nextStep,
                    "user_info": {
                        "free_vmoney": newVMoney
                    },
                    "character_list": [
                        {
                            "viewer_id": viewerId,
                            "character_id": freeTutorialCharacterId,
                            "entry_count": 1,
                            "exp": 0,
                            "exp_total": 0,
                            "bond_token_list": [
                                {
                                    "mana_board_index": 1,
                                    "status": 0
                                },
                                {
                                    "mana_board_index": 2,
                                    "status": 0
                                }
                            ],
                            "mana_board_index": 1,
                            "create_time": serializedDate,
                            "update_time": serializedDate,
                            "join_time": serializedDate
                        }
                    ],
                    "encyclopedia_info": {
                        [`1${freeTutorialCharacterId}01`]: {
                            "read": false
                        }
                    },
                    "mail_arrived": true,
                    "start_time": getServerTime()
                }
            })
        } else {
            
            reply.status(200).send({
                "data_headers": headers,
                "data": {
                    "step": nextStep,
                    "mail_arrived": true,
                    "start_time": getServerTime()
                }
            })
        }
        
        
    })
}

export default routes;
