// audience: internal
// # cn-master-time
//
// 此模块把 CN 客户端 master 时间按 JST 转换为 Unix 毫秒.

// //// 解析客户端按 JST 解释的 master 时间 [@x380kkm 2026-08-24] ////
export function parseCnMasterTimestamp(value) {
    if (value === undefined || value === null || value === "" || value === "(None)") return null
    if (typeof value !== "string" || !/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/.test(value)) {
        throw new Error(`invalid CN master timestamp: ${value}`)
    }
    const timestamp = Date.parse(`${value.replace(" ", "T")}+09:00`)
    if (!Number.isSafeInteger(timestamp) || timestamp < 0) throw new Error(`invalid CN master timestamp: ${value}`)
    return timestamp
}
// //// /解析客户端按 JST 解释的 master 时间 ////
