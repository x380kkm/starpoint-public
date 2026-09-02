// audience: internal | external
// # management-user-routes
// 此模块允许管理员创建登录用户并维护用户和游戏玩家的绑定.
// API 响应不包含密码散列或登录 session token.

import { FastifyInstance } from "fastify"
import { ManagementRole, managementAccessStore } from "../../control/managementAccess"
import { getPlayerSync } from "../../data/wdfpData"
import { parseManagementId } from "./access"

interface CreateUserBody {
    username?: string
    password?: string
    role?: ManagementRole
}

interface UserParams {
    userId: string
}

interface UserPlayerParams extends UserParams {
    playerId: string
}

function isUniqueConstraint(error: unknown): boolean {
    return (error as { code?: string }).code === "SQLITE_CONSTRAINT_UNIQUE"
}

// //// 注册用户创建和玩家绑定接口 [@x380kkm 2026-07-22] ////
export async function registerManagementUserRoutes(fastify: FastifyInstance): Promise<void> {
    fastify.get("/users", async () => ({ users: managementAccessStore.listUsers() }))

    fastify.post("/users", async (request, reply) => {
        const body = (request.body ?? {}) as CreateUserBody
        if (typeof body.username !== "string" || typeof body.password !== "string") {
            return reply.status(400).send({ error: "invalid_user" })
        }
        try {
            return await managementAccessStore.createUser(body.username, body.password, body.role ?? "player")
        } catch (error) {
            if (isUniqueConstraint(error)) return reply.status(409).send({ error: "username_exists" })
            return reply.status(400).send({ error: "invalid_user", message: (error as Error).message })
        }
    })

    fastify.put<{ Params: UserPlayerParams }>("/users/:userId/players/:playerId", async (request, reply) => {
        try {
            const userId = parseManagementId(request.params.userId, "userId")
            const playerId = parseManagementId(request.params.playerId, "playerId")
            if (getPlayerSync(playerId) === null) return reply.status(404).send({ error: "player_not_found" })
            managementAccessStore.bindPlayer(userId, playerId)
            return managementAccessStore.getUser(userId)
        } catch (error) {
            if (isUniqueConstraint(error)) return reply.status(409).send({ error: "player_already_bound" })
            return reply.status(400).send({ error: "binding_failed", message: (error as Error).message })
        }
    })

    fastify.delete<{ Params: UserPlayerParams }>("/users/:userId/players/:playerId", async (request, reply) => {
        try {
            const userId = parseManagementId(request.params.userId, "userId")
            const playerId = parseManagementId(request.params.playerId, "playerId")
            if (!managementAccessStore.unbindPlayer(userId, playerId)) {
                return reply.status(404).send({ error: "binding_not_found" })
            }
            return { unbound: true, userId, playerId }
        } catch (error) {
            return reply.status(400).send({ error: "binding_failed", message: (error as Error).message })
        }
    })
}
// //// /注册用户创建和玩家绑定接口 ////
