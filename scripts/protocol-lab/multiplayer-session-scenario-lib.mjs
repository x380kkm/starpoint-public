// audience: internal
// # multiplayer-session-scenario-lib
//
// 此模块提供多人会话场景抽取器共用的源码读取和事实收集函数.

import { existsSync, readFileSync } from "node:fs"

// //// 提供多人会话场景抽取工具 [@x380kkm 2026-08-24] ////
export function readSource(filePath) {
    if (!existsSync(filePath)) throw new Error(`missing multiplayer session source: ${filePath}`)
    return readFileSync(filePath, "utf8")
}

export function verified(value) {
    return value ? true : null
}

export function hasAll(source, patterns) {
    return patterns.every((pattern) => pattern.test(source))
}

export function finishCollectors(required, policy) {
    const requiredResult = required.finish()
    const policyResult = policy.finish()
    return {
        required: {
            ...requiredResult,
            sources: [...new Set([...requiredResult.sources, ...policyResult.sources])].sort(),
        },
        policy: policyResult,
    }
}
// //// /提供多人会话场景抽取工具 ////
