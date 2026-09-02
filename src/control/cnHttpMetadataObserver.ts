// audience: internal
// # cn-http-metadata-observer
// 此模块为本机协议实验记录 CN HTTP 响应元数据.
// 记录只包含时间, 方法, 路由模板, 状态码和响应媒体类型.
// 日志写入失败时停用观察器, 游戏请求继续.

import { appendFileSync } from "node:fs"
import { FastifyInstance, FastifyRequest } from "fastify"

export interface CnHttpMetadataRecord {
    observedAtUtc: string
    method: string
    path: string
    status: number
    contentType: string | null
}

const cnApiPrefix = "/api/index.php/"

// //// 构造不含请求值的 CN HTTP 元数据记录 [@x380kkm 2026-08-04] ////
function getCnRoutePath(request: FastifyRequest): string | null {
    const routePath = request.routeOptions.url
    if (typeof routePath !== "string") return null
    return routePath.startsWith(cnApiPrefix) ? routePath : null
}

function normalizeContentType(value: string | string[] | number | undefined): string | null {
    if (value === undefined) return null
    const firstValue = Array.isArray(value) ? value[0] : value
    return String(firstValue).split(";", 1)[0].trim().toLowerCase() || null
}

export function createCnHttpMetadataRecord(
    method: string,
    path: string,
    status: number,
    contentType: string | string[] | number | undefined,
    observedAt: Date = new Date(),
): CnHttpMetadataRecord {
    return {
        observedAtUtc: observedAt.toISOString(),
        method: method.toUpperCase(),
        path,
        status,
        contentType: normalizeContentType(contentType),
    }
}
// //// /构造不含请求值的 CN HTTP 元数据记录 ////

// //// 安装默认关闭的 CN HTTP 元数据观察器 [@x380kkm 2026-08-04] ////
function warnObserverDisabled(fastify: FastifyInstance, message: string): void {
    try {
        fastify.log.warn(message)
    } catch {}
}

export function installCnHttpMetadataObserver(fastify: FastifyInstance, outputPath: string | undefined): void {
    if (!outputPath) return

    try {
        appendFileSync(outputPath, "", { encoding: "utf8" })
    } catch {
        warnObserverDisabled(fastify, "CN HTTP metadata observer disabled after log initialization failure.")
        return
    }

    let isEnabled = true
    fastify.addHook("onResponse", (request, reply, done) => {
        if (!isEnabled) {
            done()
            return
        }

        try {
            const routePath = getCnRoutePath(request)
            if (routePath !== null) {
                const record = createCnHttpMetadataRecord(
                    request.method,
                    routePath,
                    reply.statusCode,
                    reply.getHeader("content-type"),
                )
                appendFileSync(outputPath, `${JSON.stringify(record)}\n`, { encoding: "utf8" })
            }
        } catch {
            isEnabled = false
            warnObserverDisabled(fastify, "CN HTTP metadata observer disabled after log write failure.")
        }
        done()
    })
}
// //// /安装默认关闭的 CN HTTP 元数据观察器 ////
