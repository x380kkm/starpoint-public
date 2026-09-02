// audience: internal | external
// # management-route-access
// 此模块把环境 bearer token 和 SQLite 登录会话转换为统一的管理请求身份.
// 受保护路由读取同一个身份对象, 管理员路由再执行角色检查.

import { timingSafeEqual } from "crypto"
import { FastifyReply, FastifyRequest } from "fastify"
import { ManagementPrincipal, managementAccessStore } from "../../control/managementAccess"

const SESSION_COOKIE_NAME = "starpoint_management_session"
const SESSION_COOKIE_PATH = "/manage"
const DEFAULT_SESSION_SECONDS = 12 * 60 * 60

export interface AuthenticatedManagementRequest extends FastifyRequest {
    managementPrincipal: ManagementPrincipal | null
}

// //// 解析同源管理会话 cookie [@x380kkm 2026-07-22] ////
function getCookie(request: FastifyRequest, name: string): string | null {
    const header = request.headers.cookie
    if (header === undefined) return null
    for (const entry of header.split(";")) {
        const separator = entry.indexOf("=")
        if (separator < 0) continue
        if (entry.slice(0, separator).trim() === name) return decodeURIComponent(entry.slice(separator + 1).trim())
    }
    return null
}

function getSessionLifetimeSeconds(): number {
    const value = process.env.MANAGEMENT_SESSION_SECONDS
    if (value === undefined) return DEFAULT_SESSION_SECONDS
    const parsed = Number(value)
    if (!Number.isInteger(parsed) || parsed < 60 || parsed > 30 * 24 * 60 * 60) {
        throw new Error("MANAGEMENT_SESSION_SECONDS must be between 60 and 2592000.")
    }
    return parsed
}

function createSessionCookie(token: string, maxAge: number): string {
    const attributes = [
        `${SESSION_COOKIE_NAME}=${encodeURIComponent(token)}`,
        `Path=${SESSION_COOKIE_PATH}`,
        "HttpOnly",
        "SameSite=Strict",
        `Max-Age=${maxAge}`,
    ]
    if (process.env.MANAGEMENT_SECURE_COOKIE === "1") attributes.push("Secure")
    return attributes.join("; ")
}

export function setManagementSessionCookie(reply: FastifyReply, token: string): void {
    reply.header("set-cookie", createSessionCookie(token, getSessionLifetimeSeconds()))
}

export function clearManagementSessionCookie(reply: FastifyReply): void {
    reply.header("set-cookie", createSessionCookie("", 0))
}

export function getManagementSessionToken(request: FastifyRequest): string | null {
    return getCookie(request, SESSION_COOKIE_NAME)
}

export function getConfiguredSessionLifetimeSeconds(): number {
    return getSessionLifetimeSeconds()
}
// //// /解析同源管理会话 cookie ////

// //// 将 bearer token 或 cookie 解析为管理身份 [@x380kkm 2026-07-22] ////
function resolveBearerPrincipal(request: FastifyRequest): ManagementPrincipal | null {
    const configured = process.env.MANAGEMENT_ADMIN_TOKEN
    if (configured === undefined || configured.length === 0) return null
    const value = request.headers.authorization
    const prefix = "Bearer "
    if (typeof value !== "string" || !value.startsWith(prefix)) return null
    const provided = Buffer.from(value.slice(prefix.length))
    const expected = Buffer.from(configured)
    if (provided.length !== expected.length || !timingSafeEqual(provided, expected)) return null
    return {
        id: 0,
        username: "environment-admin",
        role: "admin",
        disabled: false,
        createdAt: new Date(0).toISOString(),
        authentication: "bearer",
    }
}

export function resolveManagementPrincipal(request: FastifyRequest): ManagementPrincipal | null {
    const bearer = resolveBearerPrincipal(request)
    if (bearer !== null) return bearer
    const token = getManagementSessionToken(request)
    return token === null ? null : managementAccessStore.getSessionPrincipal(token)
}

export async function authenticateManagementRequest(request: FastifyRequest, reply: FastifyReply): Promise<void> {
    reply.header("cache-control", "no-store")
    const principal = resolveManagementPrincipal(request)
    if (principal !== null) {
        (request as AuthenticatedManagementRequest).managementPrincipal = principal
        return
    }
    if (!managementAccessStore.hasUsers() && !process.env.MANAGEMENT_ADMIN_TOKEN) {
        reply.status(503).send({ error: "management_not_configured" })
        return
    }
    reply.header("www-authenticate", "Bearer")
    reply.status(401).send({ error: "unauthorized" })
}

export async function requireManagementAdmin(request: FastifyRequest, reply: FastifyReply): Promise<void> {
    const principal = (request as AuthenticatedManagementRequest).managementPrincipal
    if (principal === null || principal.role !== "admin") reply.status(403).send({ error: "forbidden" })
}

export function getManagementPrincipal(request: FastifyRequest): ManagementPrincipal {
    const principal = (request as AuthenticatedManagementRequest).managementPrincipal
    if (principal === null) throw new Error("Management request is not authenticated.")
    return principal
}

export function parseManagementId(value: string, field: string): number {
    const parsed = Number(value)
    if (!Number.isInteger(parsed) || parsed <= 0) throw new Error(`${field} must be a positive integer.`)
    return parsed
}
// //// /将 bearer token 或 cookie 解析为管理身份 ////
