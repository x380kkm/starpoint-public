// audience: internal
// # encode-cn-orderedmap
// 此模块将嵌套映射和 CSV 行编码为 CN 客户端 orderedmap 字节流.

import zlib from "node:zlib"

// //// 编码 orderedmap 容器和 CSV 行 [@x380kkm 2026-08-22] ////
function encodeCsvRow(row) {
    return row.map((value) => {
        const text = String(value)
        return /[",\r\n]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text
    }).join(",")
}

export function encodeOrderedMap(entries) {
    const keys = []
    const chunks = []
    let keyLength = 0
    let dataLength = 0
    const offsets = []

    const entryList = Array.isArray(entries) ? entries : Object.entries(entries)
    for (const [key, value] of entryList) {
        const keyBytes = Buffer.from(key)
        const chunk = Buffer.isBuffer(value)
            ? value
            : Array.isArray(value)
                ? zlib.deflateSync(Buffer.from(encodeCsvRow(value)))
                : encodeOrderedMap(value)
        keys.push(keyBytes)
        chunks.push(chunk)
        keyLength += keyBytes.length
        dataLength += chunk.length
        offsets.push([keyLength, dataLength])
    }

    const index = Buffer.alloc(4 + offsets.length * 8)
    index.writeUInt32LE(offsets.length, 0)
    for (let offsetIndex = 0; offsetIndex < offsets.length; offsetIndex += 1) {
        index.writeUInt32LE(offsets[offsetIndex][0], 4 + offsetIndex * 8)
        index.writeUInt32LE(offsets[offsetIndex][1], 8 + offsetIndex * 8)
    }
    const compressedIndex = zlib.deflateSync(Buffer.concat([index, ...keys]))
    const indexLength = Buffer.alloc(4)
    indexLength.writeUInt32LE(compressedIndex.length)
    return Buffer.concat([indexLength, compressedIndex, ...chunks])
}
// //// /编码 orderedmap 容器和 CSV 行 ////
