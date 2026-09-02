// audience: internal | external
// # management-auth-routes
// 此模块提供用户名和密码登录, 持久化 cookie 会话查询和退出接口.
// 登录响应不返回原始 session token.

import { FastifyInstance } from "fastify"
import { managementAccessStore } from "../../control/managementAccess"
import {
    clearManagementSessionCookie,
    getConfiguredSessionLifetimeSeconds,
    getManagementSessionToken,
    resolveManagementPrincipal,
    setManagementSessionCookie,
} from "./access"

interface LoginBody {
    username?: string
    password?: string
}

// //// 注册公开的管理登录和退出接口 [@x380kkm 2026-07-22] ////
export async function registerManagementAuthRoutes(fastify: FastifyInstance): Promise<void> {
    fastify.get("/auth/session", async (request) => {
        const principal = resolveManagementPrincipal(request)
        if (principal === null) {
            return {
                authenticated: false,
                configured: managementAccessStore.hasUsers() || Boolean(process.env.MANAGEMENT_ADMIN_TOKEN),
            }
        }
        return {
            authenticated: true,
            configured: true,
            user: {
                id: principal.id,
                username: principal.username,
                role: principal.role,
                authentication: principal.authentication,
            },
        }
    })

    fastify.post("/auth/login", async (request, reply) => {
        const body = (request.body ?? {}) as LoginBody
        if (typeof body.username !== "string" || typeof body.password !== "string") {
            return reply.status(400).send({ error: "invalid_login" })
        }
        const user = await managementAccessStore.authenticate(body.username, body.password)
        if (user === null) return reply.status(401).send({ error: "invalid_credentials" })
        const session = managementAccessStore.createSession(user.id, getConfiguredSessionLifetimeSeconds())
        setManagementSessionCookie(reply, session.token)
        return {
            user: { id: user.id, username: user.username, role: user.role },
            expiresAt: session.expiresAt,
        }
    })

    fastify.post("/auth/logout", async (request, reply) => {
        const token = getManagementSessionToken(request)
        if (token !== null) managementAccessStore.revokeSession(token)
        clearManagementSessionCookie(reply)
        return { loggedOut: true }
    })
}
// //// /注册公开的管理登录和退出接口 ////
