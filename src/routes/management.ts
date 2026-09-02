// audience: internal | external
// # management-routes
// 此模块装配公开登录接口, 登录后存档接口和管理员控制接口.
// 管理 API 使用统一身份对象, 且所有响应禁止缓存.

import { readFile } from "fs/promises"
import path from "path"
import { FastifyInstance } from "fastify"
import {
    AccountNotFoundError,
    listAccountOverviews,
    revokeAccountSessions,
    rotateAccountViewerId,
} from "../control/accountAdministration"
import { managementAccessStore } from "../control/managementAccess"
import { EncryptedSaveStore } from "../control/encryptedSaveStore"
import { ManagementConfig, NpcMateConfig, managementStore } from "../control/management"
import { ServerTransferBindingRunner } from "../control/serverTransferBindingRunner"
import {
    authenticateManagementRequest,
    getManagementPrincipal,
    requireManagementAdmin,
} from "./management/access"
import { registerManagementAuthRoutes } from "./management/auth"
import { registerEncryptedSaveRoutes } from "./management/encryptedSaves"
import { registerManagementMailRoutes } from "./management/mails"
import { registerManagementSaveRoutes } from "./management/saves"
import { registerTransferShellRoutes, registerTransferSlotRoutes } from "./management/transfer"
import registerManagementTransferBindingRoutes from "./management/transferBindings"
import { registerManagementTransferTokenRoutes } from "./management/transferTokens"
import { registerManagementUserRoutes } from "./management/users"

interface TimeBody {
    enabled?: boolean
    iso?: string | null
    rate?: number
}

interface InstanceBody {
    mode?: ManagementConfig["instance"]["mode"]
    status?: ManagementConfig["instance"]["status"]
    httpPort?: number
    sessionPort?: number
}

interface NpcBody {
    enabled?: boolean
    delaySeconds?: number
    mates?: NpcMateConfig[]
}

interface RestoreParams {
    id: string
}

interface AccountPageQuery {
    limit?: string
    offset?: string
}

interface AccountParams {
    accountId: string
}

interface Pagination {
    limit: number
    offset: number
}

// //// 解析分页参数和内部账号 ID [@x380kkm 2026-07-22] ////
function parseInteger(value: string | undefined, fallback: number, minimum: number, maximum: number, field: string): number {
    if (value === undefined) return fallback
    const parsed = Number(value)
    if (!Number.isInteger(parsed) || parsed < minimum || parsed > maximum) {
        throw new Error(`${field} must be an integer between ${minimum} and ${maximum}.`)
    }
    return parsed
}

function parsePagination(query: AccountPageQuery): Pagination {
    return {
        limit: parseInteger(query.limit, 25, 1, 100, "limit"),
        offset: parseInteger(query.offset, 0, 0, Number.MAX_SAFE_INTEGER, "offset"),
    }
}

function parseAccountId(params: AccountParams): number {
    return parseInteger(params.accountId, 0, 1, Number.MAX_SAFE_INTEGER, "accountId")
}
// //// /解析分页参数和内部账号 ID ////

// //// 注册实例状态和虚拟时间接口 [@x380kkm 2026-07-22] ////
function registerStateRoutes(fastify: FastifyInstance): void {
    fastify.get("/status", async () => managementStore.getStatus())

    fastify.put("/time", async (request, reply) => {
        const body = (request.body ?? {}) as TimeBody
        try {
            return await managementStore.setVirtualTime(body.enabled ?? false, body.iso ?? null, body.rate ?? 1)
        } catch (error) {
            return reply.status(400).send({ error: "invalid_time", message: (error as Error).message })
        }
    })

    fastify.put("/instance", async (request, reply) => {
        try {
            return await managementStore.setInstance((request.body ?? {}) as InstanceBody)
        } catch (error) {
            return reply.status(400).send({ error: "invalid_instance", message: (error as Error).message })
        }
    })
}
// //// /注册实例状态和虚拟时间接口 ////

// //// 注册 COM 队友配置接口 [@x380kkm 2026-07-22] ////
function registerNpcRoutes(fastify: FastifyInstance): void {
    fastify.put("/npc", async (request, reply) => {
        try {
            const body = (request.body ?? {}) as NpcBody
            const current = await managementStore.load()
            return await managementStore.setNpcConfiguration({
                enabled: body.enabled ?? current.npcFill.enabled,
                delaySeconds: body.delaySeconds ?? current.npcFill.delaySeconds,
                mates: body.mates ?? current.npcMates,
            })
        } catch (error) {
            return reply.status(400).send({ error: "invalid_npc", message: (error as Error).message })
        }
    })
}
// //// /注册 COM 队友配置接口 ////

// //// 注册备份和恢复暂存接口 [@x380kkm 2026-07-22] ////
function registerBackupRoutes(fastify: FastifyInstance): void {
    fastify.get("/backups", async () => ({ backups: await managementStore.listBackups() }))

    fastify.post("/backups", async (_request, reply) => {
        try {
            return await managementStore.createBackup()
        } catch (error) {
            return reply.status(409).send({ error: "backup_failed", message: (error as Error).message })
        }
    })

    fastify.post<{ Params: RestoreParams }>("/backups/:id/restore", async (request, reply) => {
        try {
            return { ...(await managementStore.stageRestore(request.params.id)), restartRequired: true }
        } catch (error) {
            return reply.status(400).send({ error: "restore_failed", message: (error as Error).message })
        }
    })
}
// //// /注册备份和恢复暂存接口 ////

// //// 注册脱敏账号查询和会话控制接口 [@x380kkm 2026-07-22] ////
function registerAccountRoutes(fastify: FastifyInstance): void {
    fastify.get<{ Querystring: AccountPageQuery }>("/accounts", async (request, reply) => {
        try {
            return await listAccountOverviews(parsePagination(request.query))
        } catch (error) {
            return reply.status(400).send({ error: "invalid_account_page", message: (error as Error).message })
        }
    })

    fastify.delete<{ Params: AccountParams }>("/accounts/:accountId/sessions", async (request, reply) => {
        try {
            return await revokeAccountSessions(parseAccountId(request.params))
        } catch (error) {
            if (error instanceof AccountNotFoundError) {
                return reply.status(404).send({ error: "account_not_found", message: error.message })
            }
            return reply.status(400).send({ error: "session_revoke_failed", message: (error as Error).message })
        }
    })

    fastify.post<{ Params: AccountParams }>("/accounts/:accountId/viewer-id", async (request, reply) => {
        try {
            return await rotateAccountViewerId(parseAccountId(request.params))
        } catch (error) {
            if (error instanceof AccountNotFoundError) {
                return reply.status(404).send({ error: "account_not_found", message: error.message })
            }
            return reply.status(400).send({ error: "viewer_id_rotation_failed", message: (error as Error).message })
        }
    })
}
// //// /注册脱敏账号查询和会话控制接口 ////

// //// 按公开, 登录和管理员权限装配管理 API [@x380kkm 2026-07-22] ////
interface ManagementApiRoutesOptions {
    encryptedSaveStore: EncryptedSaveStore
}

const registerManagementApiRoutes = async (fastify: FastifyInstance, options: ManagementApiRoutesOptions) => {
    await managementAccessStore.createBootstrapAdmin(
        process.env.MANAGEMENT_ADMIN_USERNAME,
        process.env.MANAGEMENT_ADMIN_PASSWORD,
    )
    fastify.decorateRequest("managementPrincipal", null)
    fastify.addHook("onRequest", async (_request, reply) => {
        reply.header("cache-control", "no-store")
    })
    await fastify.register(registerManagementAuthRoutes)

    await fastify.register(async (authenticatedApi) => {
        authenticatedApi.addHook("onRequest", authenticateManagementRequest)
        authenticatedApi.get("/me", async (request) => {
            const principal = getManagementPrincipal(request)
            return {
                user: {
                    id: principal.id,
                    username: principal.username,
                    role: principal.role,
                    authentication: principal.authentication,
                },
            }
        })
        await authenticatedApi.register(registerManagementSaveRoutes)
        await authenticatedApi.register(registerManagementTransferBindingRoutes)
        await authenticatedApi.register(registerManagementTransferTokenRoutes)
        await authenticatedApi.register(registerEncryptedSaveRoutes, { store: options.encryptedSaveStore })

        await authenticatedApi.register(async (adminApi) => {
            adminApi.addHook("onRequest", requireManagementAdmin)
            registerStateRoutes(adminApi)
            registerNpcRoutes(adminApi)
            registerBackupRoutes(adminApi)
            registerAccountRoutes(adminApi)
            registerManagementMailRoutes(adminApi)
            await adminApi.register(registerManagementUserRoutes)
        })
    })
}
// //// /按公开, 登录和管理员权限装配管理 API ////

// //// 提供不包含凭据的管理页面 [@x380kkm 2026-07-22] ////
const registerManagementRoutes = async (fastify: FastifyInstance) => {
    const encryptedSaveStore = new EncryptedSaveStore(managementAccessStore.databasePath)
    const transferBindingRunner = new ServerTransferBindingRunner()
    fastify.addHook("onReady", async () => transferBindingRunner.start())
    fastify.addHook("onClose", async () => {
        await transferBindingRunner.stop()
        encryptedSaveStore.close()
        managementAccessStore.close()
    })

    await fastify.register(async (transferApi) => {
        transferApi.addHook("onRequest", async (_request, reply) => {
            reply.header("cache-control", "no-store")
        })
        await transferApi.register(registerTransferShellRoutes)
        await transferApi.register(registerTransferSlotRoutes)
    }, { prefix: "/transfer/v1" })

    fastify.get("/", async (_request, reply) => {
        const page = await readFile(path.join(process.cwd(), "web", "pages", "management.html"), "utf8")
        return reply
            .headers({
                "cache-control": "no-store",
                "content-security-policy": "default-src 'self'; connect-src 'self'; img-src 'self'; script-src 'self'; style-src 'unsafe-inline'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'",
                "referrer-policy": "no-referrer",
                "x-content-type-options": "nosniff",
            })
            .type("text/html; charset=utf-8")
            .send(page)
    })

    fastify.register(registerManagementApiRoutes, { prefix: "/api", encryptedSaveStore })
}
// //// /提供不包含凭据的管理页面 ////

export default registerManagementRoutes
