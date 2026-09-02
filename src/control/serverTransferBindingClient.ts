// audience: internal
// # server-transfer-binding-client
//
// 此模块封装完整服务器到目标实例槽位传输接口的 HTTP 边界.
// 响应身份头, ETag 和存档摘要必须互相一致.

import { normalizeRevisionEtag } from "../data/saveRevisions"
import { parseStarpointSavePackage, StarpointSavePackage } from "../games/starpoint/portableSave"
import {
    DownloadedServerTransferSave,
    ServerTransferEndpoint,
} from "./serverTransferBindingTypes"

const MAX_TRANSFER_RESPONSE_BYTES = 8 * 1024 * 1024

export type ServerTransferClientErrorCode =
    | "transfer_target_invalid"
    | "transfer_target_identity_mismatch"
    | "transfer_target_authentication_failed"
    | "transfer_target_revision_conflict"
    | "transfer_target_slot_not_found"
    | "transfer_target_unavailable"
    | "transfer_target_invalid_response"

export class ServerTransferClientError extends Error {
    constructor(readonly code: ServerTransferClientErrorCode) {
        super(code)
    }
}

function getRequestTimeoutMilliseconds(): number {
    const value = Number(process.env.SERVER_TRANSFER_REQUEST_TIMEOUT_MS ?? 10_000)
    return Number.isInteger(value) && value >= 1_000 && value <= 60_000 ? value : 10_000
}

function targetSlotUrl(endpoint: ServerTransferEndpoint): string {
    return `${endpoint.baseUrl}/slots/${endpoint.playerId}`
}

function isJsonResponse(response: Response): boolean {
    return response.headers.get("content-type")?.toLowerCase().startsWith("application/json") === true
}

function mapResponseStatus(response: Response): ServerTransferClientError {
    if (response.status === 401 || response.status === 403) {
        return new ServerTransferClientError("transfer_target_authentication_failed")
    }
    if (response.status === 404) {
        return new ServerTransferClientError("transfer_target_slot_not_found")
    }
    if (response.status === 409 || response.status === 412) {
        return new ServerTransferClientError("transfer_target_revision_conflict")
    }
    return new ServerTransferClientError("transfer_target_invalid_response")
}

function parseIdentityHeaders(
    response: Response,
    endpoint: ServerTransferEndpoint,
): string {
    const instanceId = response.headers.get("x-starpoint-instance-id")
    const shellId = response.headers.get("x-starpoint-shell-id")
    const slotId = response.headers.get("x-starpoint-slot-id")
    if (
        instanceId !== endpoint.instanceId
        || shellId === null
        || shellId.length === 0
        || shellId.length > 128
        || (endpoint.shellId !== undefined && shellId !== endpoint.shellId)
        || slotId !== String(endpoint.playerId)
    ) {
        throw new ServerTransferClientError("transfer_target_identity_mismatch")
    }
    return shellId
}

async function requestTarget(
    endpoint: ServerTransferEndpoint,
    method: "GET" | "PUT",
    body?: StarpointSavePackage,
    etag?: string,
): Promise<Response> {
    const headers: Record<string, string> = {
        authorization: `Bearer ${endpoint.token}`,
        accept: "application/json",
    }
    if (body !== undefined) headers["content-type"] = "application/json"
    if (etag !== undefined) headers["if-match"] = `"${etag}"`
    try {
        return await fetch(targetSlotUrl(endpoint), {
            method,
            headers,
            body: body === undefined ? undefined : JSON.stringify(body),
            redirect: "error",
            signal: AbortSignal.timeout(getRequestTimeoutMilliseconds()),
        })
    } catch {
        throw new ServerTransferClientError("transfer_target_unavailable")
    }
}

async function readJsonResponse(response: Response): Promise<unknown> {
    const contentLength = response.headers.get("content-length")
    if (contentLength !== null && Number(contentLength) > MAX_TRANSFER_RESPONSE_BYTES) {
        throw new ServerTransferClientError("transfer_target_invalid_response")
    }
    if (response.body === null) {
        throw new ServerTransferClientError("transfer_target_invalid_response")
    }
    const reader = response.body.getReader()
    const chunks: Uint8Array[] = []
    let totalBytes = 0
    while (true) {
        let result: ReadableStreamReadResult<Uint8Array>
        try {
            result = await reader.read()
        } catch {
            throw new ServerTransferClientError("transfer_target_unavailable")
        }
        if (result.done) break
        totalBytes += result.value.byteLength
        if (totalBytes > MAX_TRANSFER_RESPONSE_BYTES) {
            await reader.cancel().catch(() => undefined)
            throw new ServerTransferClientError("transfer_target_invalid_response")
        }
        chunks.push(result.value)
    }
    try {
        return JSON.parse(Buffer.concat(chunks, totalBytes).toString("utf8"))
    } catch {
        throw new ServerTransferClientError("transfer_target_invalid_response")
    }
}

// //// 规范化明确的目标 transfer API 地址 [@x380kkm 2026-08-04] ////
export function normalizeServerTransferBaseUrl(value: string): string {
    let url: URL
    try {
        url = new URL(value)
    } catch {
        throw new ServerTransferClientError("transfer_target_invalid")
    }
    if (
        (url.protocol !== "http:" && url.protocol !== "https:")
        || url.username !== ""
        || url.password !== ""
        || url.search !== ""
        || url.hash !== ""
    ) {
        throw new ServerTransferClientError("transfer_target_invalid")
    }
    const pathname = url.pathname.replace(/\/+$/, "")
    if (pathname.length === 0 || pathname === "/" || url.toString().length > 2048) {
        throw new ServerTransferClientError("transfer_target_invalid")
    }
    url.pathname = pathname
    return url.toString().replace(/\/$/, "")
}
// //// /规范化明确的目标 transfer API 地址 ////

// //// 下载并校验目标槽 [@x380kkm 2026-08-04] ////
export async function downloadServerTransferSave(
    endpoint: ServerTransferEndpoint,
): Promise<DownloadedServerTransferSave> {
    const response = await requestTarget(endpoint, "GET")
    if (!response.ok) throw mapResponseStatus(response)
    if (!isJsonResponse(response)) {
        throw new ServerTransferClientError("transfer_target_invalid_response")
    }
    const shellId = parseIdentityHeaders(response, endpoint)
    const etag = normalizeRevisionEtag(response.headers.get("etag") ?? undefined)
    const value = await readJsonResponse(response)
    const portablePackage = parseStarpointSavePackage(value)
    if (portablePackage === null || etag === null || portablePackage.payloadSha256 !== etag) {
        throw new ServerTransferClientError("transfer_target_invalid_response")
    }
    return {
        package: portablePackage,
        revisionId: portablePackage.source.revisionId ?? etag,
        etag,
        shellId,
    }
}
// //// /下载并校验目标槽 ////

// //// 条件覆盖目标槽 [@x380kkm 2026-08-04] ////
export async function uploadServerTransferSave(
    endpoint: ServerTransferEndpoint,
    portablePackage: StarpointSavePackage,
    expectedEtag: string,
): Promise<string> {
    const response = await requestTarget(endpoint, "PUT", portablePackage, expectedEtag)
    if (!response.ok) throw mapResponseStatus(response)
    if (!isJsonResponse(response)) {
        throw new ServerTransferClientError("transfer_target_invalid_response")
    }
    parseIdentityHeaders(response, endpoint)
    const etag = normalizeRevisionEtag(response.headers.get("etag") ?? undefined)
    const value = await readJsonResponse(response)
    if (
        etag === null
        || typeof value !== "object"
        || value === null
        || Array.isArray(value)
        || !("etag" in value)
        || value.etag !== etag
        || etag !== portablePackage.payloadSha256
    ) {
        throw new ServerTransferClientError("transfer_target_invalid_response")
    }
    return etag
}
// //// /条件覆盖目标槽 ////
