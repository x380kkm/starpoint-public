// audience: internal | external
// # management-encrypted-save-routes
// 此模块只接收 AES-256-GCM 加密封装并按当前登录用户隔离存储.

import { FastifyInstance, FastifyReply, FastifyRequest } from "fastify"
import {
    ENCRYPTED_SAVE_BODY_LIMIT_BYTES,
    EncryptedSaveCapacityError,
    EncryptedSaveConflictError,
    EncryptedSaveStore,
    EncryptedSaveWriteCondition,
    parseEncryptedSaveObjectId,
} from "../../control/encryptedSaveStore"
import { getManagementPrincipal } from "./access"

interface EncryptedSaveParams {
    objectId: string
}

export interface EncryptedSaveRoutesOptions {
    store: EncryptedSaveStore
}

interface EncryptedSaveEnvelope {
    format: "starpoint-encrypted-save"
    version: 1
    algorithm: "AES-256-GCM"
    keyId: string
    nonce: string
    ciphertext: string
}

function decodeBase64Url(value: unknown, expectedBytes?: number): Buffer | null {
    if (typeof value !== "string" || !/^[A-Za-z0-9_-]+$/.test(value)) return null
    const decoded = Buffer.from(value, "base64url")
    if (decoded.toString("base64url") !== value) return null
    return expectedBytes === undefined || decoded.length === expectedBytes ? decoded : null
}

function parseEnvelope(value: unknown): EncryptedSaveEnvelope | null {
    if (value === null || typeof value !== "object" || Array.isArray(value)) return null
    const record = value as Record<string, unknown>
    const fields = Object.keys(record).sort()
    if (fields.join(",") !== "algorithm,ciphertext,format,keyId,nonce,version") return null
    if (record.format !== "starpoint-encrypted-save" || record.version !== 1 || record.algorithm !== "AES-256-GCM") return null
    if (typeof record.keyId !== "string" || !/^[A-Za-z0-9_-]{1,64}$/.test(record.keyId)) return null
    if (decodeBase64Url(record.nonce, 12) === null) return null
    const ciphertext = decodeBase64Url(record.ciphertext)
    if (ciphertext === null || ciphertext.length < 16) return null
    return record as unknown as EncryptedSaveEnvelope
}

function getPersistentUserIdOrReply(request: FastifyRequest, reply: FastifyReply): number | null {
    const principal = getManagementPrincipal(request)
    if (principal.id > 0 && principal.authentication === "session") return principal.id
    reply.status(403).send({ error: "persistent_identity_required" })
    return null
}

function getObjectIdOrReply(value: string, reply: FastifyReply): string | null {
    const objectId = parseEncryptedSaveObjectId(value)
    if (objectId === null) reply.status(400).send({ error: "invalid_encrypted_save_id" })
    return objectId
}

function getWriteConditionOrReply(request: FastifyRequest, reply: FastifyReply): EncryptedSaveWriteCondition | null {
    const ifMatch = request.headers["if-match"]
    const ifNoneMatch = request.headers["if-none-match"]
    if (ifMatch === undefined && ifNoneMatch === "*") return { type: "create" }
    if (ifNoneMatch === undefined && typeof ifMatch === "string" && /^"[a-f0-9]{64}"$/.test(ifMatch)) {
        return { type: "replace", sha256: ifMatch.slice(1, -1) }
    }
    reply.status(428).send({ error: "encrypted_save_precondition_required" })
    return null
}

function setEntityTag(reply: FastifyReply, sha256: string): void {
    reply.header("etag", `"${sha256}"`)
}

// //// 注册用户加密存档上传和下载接口 [@x380kkm 2026-07-23] ////
export async function registerEncryptedSaveRoutes(
    fastify: FastifyInstance,
    options: EncryptedSaveRoutesOptions,
): Promise<void> {
    fastify.get("/encrypted-saves", async (request, reply) => {
        const userId = getPersistentUserIdOrReply(request, reply)
        return userId === null ? undefined : { saves: options.store.list(userId) }
    })

    fastify.put<{ Params: EncryptedSaveParams }>(
        "/encrypted-saves/:objectId",
        { bodyLimit: ENCRYPTED_SAVE_BODY_LIMIT_BYTES },
        async (request, reply) => {
            const userId = getPersistentUserIdOrReply(request, reply)
            if (userId === null) return
            const objectId = getObjectIdOrReply(request.params.objectId, reply)
            if (objectId === null) return
            const condition = getWriteConditionOrReply(request, reply)
            if (condition === null) return
            const envelope = parseEnvelope(request.body)
            if (envelope === null) return reply.status(400).send({ error: "invalid_encrypted_save" })
            try {
                const saved = options.store.put(userId, objectId, JSON.stringify(envelope), condition)
                setEntityTag(reply, saved.metadata.sha256)
                return reply.status(saved.created ? 201 : 200).send(saved.metadata)
            } catch (error) {
                if (error instanceof EncryptedSaveConflictError) {
                    return reply.status(412).send({ error: "encrypted_save_precondition_failed" })
                }
                if (error instanceof EncryptedSaveCapacityError) {
                    return reply.status(409).send({ error: "encrypted_save_capacity_exhausted" })
                }
                throw error
            }
        },
    )

    fastify.get<{ Params: EncryptedSaveParams }>("/encrypted-saves/:objectId", async (request, reply) => {
        const userId = getPersistentUserIdOrReply(request, reply)
        if (userId === null) return
        const objectId = getObjectIdOrReply(request.params.objectId, reply)
        if (objectId === null) return
        const saved = options.store.get(userId, objectId)
        if (saved === null) return reply.status(404).send({ error: "encrypted_save_not_found" })
        reply.header("content-disposition", `attachment; filename="${saved.objectId}.starpoint-save.json"`)
        reply.type("application/vnd.starpoint.encrypted-save+json")
        setEntityTag(reply, saved.sha256)
        return JSON.parse(saved.envelopeJson)
    })

    fastify.delete<{ Params: EncryptedSaveParams }>("/encrypted-saves/:objectId", async (request, reply) => {
        const userId = getPersistentUserIdOrReply(request, reply)
        if (userId === null) return
        const objectId = getObjectIdOrReply(request.params.objectId, reply)
        if (objectId === null) return
        if (!options.store.delete(userId, objectId)) {
            return reply.status(404).send({ error: "encrypted_save_not_found" })
        }
        return { deleted: true, objectId }
    })
}
// //// /注册用户加密存档上传和下载接口 ////
