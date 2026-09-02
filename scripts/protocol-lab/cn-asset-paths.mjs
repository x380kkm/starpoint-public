// audience: internal
// # cn-asset-paths
// 此模块生成 CN 客户端资产哈希和对应的 EntityLists 路径.

import crypto from "node:crypto"

const CN_ASSET_HASH_SALT = "K6R9T9Hz22OpeIGEWB0ui6c6PYFQnJGy"

// //// 计算客户端使用的 CN 资源路径 [@x380kkm 2026-08-19] ////
export function hashCnAssetPath(logicalPath) {
    if (typeof logicalPath !== "string" || logicalPath.length === 0) {
        throw new Error("CN asset logical path is required")
    }
    const normalizedPath = logicalPath.replaceAll("\\", "/").replace(/^\/+/, "").replace(/\/{2,}/g, "/")
    return crypto.createHash("sha1").update(normalizedPath + CN_ASSET_HASH_SALT, "utf8").digest("hex")
}

export function assetEntryPaths(assetHash) {
    if (!/^[a-f0-9]{40}$/.test(assetHash)) throw new Error(`invalid CN asset hash: ${assetHash}`)
    const suffix = `${assetHash.slice(0, 2)}/${assetHash.slice(2)}`
    return [
        `production/upload/${suffix}`,
        `production/medium_upload/${suffix}`,
        `production/android_upload/${suffix}`,
        `production/ios_upload/${suffix}`,
    ]
}
// //// /计算客户端使用的 CN 资源路径 ////
