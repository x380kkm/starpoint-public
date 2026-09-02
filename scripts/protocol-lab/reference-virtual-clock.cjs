// audience: internal
// # cn-reference-virtual-clock
//
// 该预加载模块让参考服务的原生 Date 与差分服务时间使用同一时钟起点.

const NativeDate = Date
const nativeStartedAt = NativeDate.now()
const virtualStartedAt = NativeDate.parse(process.env.CN_DIFFERENTIAL_NOW)

if (!Number.isFinite(virtualStartedAt)) {
    throw new Error("CN_DIFFERENTIAL_NOW must be an ISO timestamp")
}

// //// 提供持续前进的虚拟 Date [@x380kkm 2026-08-23] ////
class DifferentialDate extends NativeDate {
    constructor(...values) {
        super(...(values.length === 0 ? [DifferentialDate.now()] : values))
    }

    static now() {
        return virtualStartedAt + NativeDate.now() - nativeStartedAt
    }
}

global.Date = DifferentialDate
// //// /提供持续前进的虚拟 Date //
