// audience: internal
// # decode-cn-orderedmap
// 此模块解码 CN 客户端的 zlib 索引 orderedmap 文件, 并返回嵌套映射和 CSV 行.

import fs from "node:fs"
import zlib from "node:zlib"

const DEFAULT_MAX_DEPTH = 64
const DEFAULT_MAX_ENTRIES = 500000
const DEFAULT_MAX_INFLATED_BYTES = 128 * 1024 * 1024
const DEFAULT_MAX_INPUT_BYTES = 64 * 1024 * 1024
const utf8Decoder = new TextDecoder("utf-8", { fatal: true })

// //// 解码 orderedmap 容器和 CSV 行 [@x380kkm 2026-07-23] ////
function readInt(buffer, offset) {
    if (offset < 0 || offset + 4 > buffer.length) throw new Error("orderedmap integer is out of bounds")
    return buffer.readUInt32LE(offset)
}

function parseCsvRow(text) {
    const values = []
    let value = ""
    let quoted = false
    let quoteClosed = false

    for (let index = 0; index < text.length; index += 1) {
        const character = text[index]
        if (character === '"') {
            if (quoted && text[index + 1] === '"') {
                value += '"'
                index += 1
            } else if (quoted) {
                quoted = false
                quoteClosed = true
            } else if (value.length === 0 && !quoteClosed) {
                quoted = true
            } else {
                throw new Error("orderedmap CSV row has an unexpected quote")
            }
        } else if (quoted) {
            value += character
        } else if (character === ",") {
            values.push(value)
            value = ""
            quoteClosed = false
        } else if (character === "\r" || character === "\n") {
            if (text.slice(index).replace(/[\r\n]/g, "") !== "") {
                throw new Error("orderedmap CSV value contains multiple rows")
            }
            break
        } else {
            if (quoteClosed) throw new Error("orderedmap CSV row has data after a closing quote")
            value += character
        }
    }

    if (quoted) throw new Error("orderedmap CSV row has an unterminated quote")
    values.push(value)
    return values
}

function createDecodeState(options) {
    const maxDepth = options.maxDepth ?? DEFAULT_MAX_DEPTH
    const maxEntries = options.maxEntries ?? DEFAULT_MAX_ENTRIES
    const maxInflatedBytes = options.maxInflatedBytes ?? DEFAULT_MAX_INFLATED_BYTES
    const maxInputBytes = options.maxInputBytes ?? DEFAULT_MAX_INPUT_BYTES
    for (const [name, value] of Object.entries({ maxDepth, maxEntries, maxInflatedBytes, maxInputBytes })) {
        if (!Number.isSafeInteger(value) || value < 1) throw new Error(`${name} must be a positive safe integer`)
    }
    return { maxDepth, remainingEntries: maxEntries, remainingInflatedBytes: maxInflatedBytes, maxInputBytes }
}

function inflateWithBudget(buffer, state, section) {
    if (state.remainingInflatedBytes < 1) throw new Error("orderedmap inflated byte budget is exhausted")
    let result
    try {
        result = zlib.inflateSync(buffer, { info: true, maxOutputLength: state.remainingInflatedBytes })
    } catch (error) {
        throw new Error(`orderedmap ${section} cannot be inflated`, { cause: error })
    }
    if (result.engine.bytesWritten !== buffer.length) {
        throw new Error(`orderedmap ${section} has trailing compressed bytes`)
    }
    const inflated = result.buffer
    state.remainingInflatedBytes -= inflated.length
    return inflated
}

function isNestedContainer(buffer) {
    if (buffer.length < 6) return false
    const indexLength = buffer.readUInt32LE(0)
    return indexLength > 0 && 4 + indexLength <= buffer.length
}

function decodeValue(buffer, state, depth) {
    if (isNestedContainer(buffer)) return decodeContainer(buffer, state, depth + 1)
    const row = inflateWithBudget(buffer, state, "row")
    return parseCsvRow(utf8Decoder.decode(row))
}

function decodeContainer(buffer, state, depth) {
    if (depth > state.maxDepth) throw new Error("orderedmap nesting depth exceeds the configured limit")
    const indexLength = readInt(buffer, 0)
    const indexEnd = 4 + indexLength
    if (indexLength === 0 || indexEnd > buffer.length) throw new Error("orderedmap index is out of bounds")

    const index = inflateWithBudget(buffer.subarray(4, indexEnd), state, "index")
    const count = readInt(index, 0)
    state.remainingEntries -= count
    if (state.remainingEntries < 0) throw new Error("orderedmap entry count exceeds the configured limit")
    const indexTableEnd = 4 + count * 8
    if (indexTableEnd > index.length) throw new Error("orderedmap index table is truncated")

    const keyBytes = index.subarray(indexTableEnd)
    let keyOffset = 0
    let dataOffset = 0
    const entries = {}

    for (let entryIndex = 0; entryIndex < count; entryIndex += 1) {
        const tableOffset = 4 + entryIndex * 8
        const keyEnd = readInt(index, tableOffset)
        const dataEnd = readInt(index, tableOffset + 4)
        if (keyEnd < keyOffset || keyEnd > keyBytes.length) throw new Error("orderedmap key table is invalid")
        const dataStart = indexEnd + dataOffset
        const dataStop = indexEnd + dataEnd
        if (dataEnd < dataOffset || dataStop > buffer.length) throw new Error("orderedmap data table is invalid")

        const key = utf8Decoder.decode(keyBytes.subarray(keyOffset, keyEnd))
        if (Object.hasOwn(entries, key)) throw new Error(`orderedmap contains a duplicate key: ${key}`)
        const chunk = buffer.subarray(dataStart, dataStop)
        const value = decodeValue(chunk, state, depth)
        Object.defineProperty(entries, key, { value, enumerable: true, writable: true, configurable: true })
        keyOffset = keyEnd
        dataOffset = dataEnd
    }

    if (keyOffset !== keyBytes.length) throw new Error("orderedmap key data has trailing bytes")
    if (indexEnd + dataOffset !== buffer.length) throw new Error("orderedmap value data has trailing bytes")
    return entries
}

export function decodeOrderedMap(buffer, options = {}) {
    if (!Buffer.isBuffer(buffer)) throw new Error("orderedmap input must be a Buffer")
    const state = createDecodeState(options)
    if (buffer.length > state.maxInputBytes) throw new Error("orderedmap input exceeds the configured limit")
    return decodeContainer(buffer, state, 1)
}

export function decodeOrderedMapFile(filePath, options = {}) {
    const maxInputBytes = options.maxInputBytes ?? DEFAULT_MAX_INPUT_BYTES
    if (!Number.isSafeInteger(maxInputBytes) || maxInputBytes < 1) {
        throw new Error("maxInputBytes must be a positive safe integer")
    }
    if (fs.statSync(filePath).size > maxInputBytes) throw new Error("orderedmap input exceeds the configured limit")
    return decodeOrderedMap(fs.readFileSync(filePath), options)
}
// //// /解码 orderedmap 容器和 CSV 行 ////

// //// 输出 orderedmap JSON [@x380kkm 2026-07-23] ////
if (process.argv[1] && process.argv[1].endsWith("decode-cn-orderedmap.mjs")) {
    const filePath = process.argv[2]
    if (!filePath) {
        console.error("usage: node decode-cn-orderedmap.mjs <orderedmap-file>")
        process.exitCode = 2
    } else {
        process.stdout.write(`${JSON.stringify(decodeOrderedMapFile(filePath), null, 2)}\n`)
    }
}
// //// /输出 orderedmap JSON ////
