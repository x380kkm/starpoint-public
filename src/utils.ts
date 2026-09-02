// audience: internal
// # server-utilities
//
// 该模块提供虚拟服务器时间, 随机身份和请求辅助函数.
// 未传日期时读取虚拟时钟, 显式日期始终保留自己的时间值.

import { randomInt } from "crypto"
import { FastifyRequest } from "fastify"

// //// 读取和控制虚拟服务器时间 [@x380kkm 2026-08-03] ////
let serverTime: Date | null = null;
let serverTimeAnchor: number | null = null;
let serverTimeRate = 1;

/**
 * 返回 Unix 时间戳.
 * 
 * @param date 可选的显式日期.
 * @returns Unix 时间戳.
 */
export function getServerTime(
    date?: Date
): number {
    return Math.floor((date ?? getServerDate()).getTime() / 1000)
}

/**
 * 返回当前虚拟服务器日期.
 * 
 * @returns 当前虚拟服务器日期.
 */
export function getServerDate(): Date {
    if (serverTime === null) return new Date()
    const elapsed = Date.now() - (serverTimeAnchor ?? Date.now())
    return new Date(serverTime.getTime() + elapsed * serverTimeRate)
}

export function setServerTime(date: Date | null) {
    serverTime = date;
    serverTimeAnchor = date === null ? null : Date.now()
    if (date === null) serverTimeRate = 1
}

export function setServerTimeRate(rate: number) {
    if (!Number.isFinite(rate) || rate <= 0 || rate > 1000) {
        throw new Error("Server time rate must be greater than 0 and no greater than 1000.")
    }
    if (serverTime !== null) serverTime = getServerDate()
    serverTimeRate = rate
    serverTimeAnchor = serverTime === null ? null : Date.now()
}
// //// /读取和控制虚拟服务器时间 ////

export function getServerTimeRate(): number {
    return serverTimeRate
}

/**
 * Converts a server time value (unix epoch in seconds) into a Date.
 * 
 * @param serverTime The unix epoch value.
 * @returns The date.
 */
export function getDateFromServerTime(serverTime: number): Date {
    return new Date(serverTime * 1000)
}

/**
 * Generates an IdpAlias to identify a particular device.
 * 
 * @param appId 
 * @param idpId 
 * @param serialNo 
 * @returns The generated IdpAlias
 */
export function generateIdpAlias(
    appId: string,
    deviceId: string,
    serialNo: string
): string {
    return `${appId}:${deviceId}:${serialNo}`
}

/**
 * Generates a random viewer ID using the crypto library.
 * 
 * @returns A number between 100,000,000 and 999,999,999
 */
export function generateViewerId(): number {
    return randomInt(100000000, 999999999)
}

export interface DataHeaders {
    force_update?: boolean
    asset_update?: boolean
    short_udid?: number
    viewer_id?: number
    servertime?: number
    result_code?: number
    udid?: string
}

/**
 * Generates a default data headers object, which is used in communication with the client.
 * 
 * @param customValues A partial DataHeaders object with custom fields to replace the default ones.
 * @returns A DataHeaders object.
 */
export function generateDataHeaders(
    customValues: Partial<DataHeaders> = {},
    fields: (keyof DataHeaders)[] = ['force_update', 'asset_update', 'short_udid', 'viewer_id', 'servertime', 'result_code'],
): Record<string, any> {
    const defaultHeaders: DataHeaders = {
        force_update: false,
        asset_update: false,
        short_udid: 0,
        viewer_id: 0,
        servertime: getServerTime(), //1651514014,//getServerTime(),
        result_code: 1
    }
    const headers: Record<string, any> = {}

    for (const field of fields) {
        const customValue = customValues[field]
        const defaultValue = defaultHeaders[field]
        headers[field] = customValue === undefined ? defaultValue : customValue
    }

    return headers
}

export enum Platform {
    ANDROID,
    IOS
}

export function getRequestPlatformSync(
    request: FastifyRequest
): Platform {
    // check user agent
    if ((request.headers["user-agent"] || '').includes('iOS;'))
        return Platform.IOS;

    // check requestedby header
    if ((request.headers["requestedby"] || '') === 'ios')
        return Platform.IOS;

    return Platform.ANDROID
}
