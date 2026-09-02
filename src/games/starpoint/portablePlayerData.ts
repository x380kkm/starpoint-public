// audience: internal
// # starpoint-portable-player-data
//
// 该模块在来源快照, 导入基线和当前 Node 数据之间执行 JSON 三方比较.
// 可移植导出保留未变化的来源值, 客户端载入只补回未建模的来源字段.
// 对象字段递归比较, 数组作为整体处理, 客户端载入时使用当前数组.

import { isDeepStrictEqual } from "node:util"

const missing = Symbol("missing-portable-player-data")
type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue }
type PresentValue = { present: true, value: JsonValue } | { present: false }

// //// 生成可移植导出和客户端载入数据 [@x380kkm 2026-08-03] ////
export function reconcilePortableGameData(
    source: Record<string, unknown>,
    baseline: Record<string, unknown>,
    current: Record<string, unknown>,
): Record<string, unknown> {
    const reconciled = reconcileValue(
        present(source),
        present(baseline),
        present(current),
    )
    if (reconciled === missing || !isRecord(reconciled)) {
        throw new Error("Portable game data reconciliation failed.")
    }
    return reconciled
}

export function mergePortableExtrasIntoClientData(
    source: Record<string, unknown>,
    baseline: Record<string, unknown>,
    current: Record<string, unknown>,
): Record<string, unknown> {
    return mergeClientValue(
        source as JsonValue,
        baseline as JsonValue,
        current as JsonValue,
    ) as Record<string, unknown>
}

function reconcileValue(
    source: PresentValue,
    baseline: PresentValue,
    current: PresentValue,
): JsonValue | typeof missing {
    if (hasSameValue(current, baseline)) {
        return source.present ? cloneJsonValue(source.value) : missing
    }
    if (
        source.present
        && baseline.present
        && current.present
        && isRecord(source.value)
        && isRecord(baseline.value)
        && isRecord(current.value)
    ) {
        const keys = [...new Set([
            ...Object.keys(source.value),
            ...Object.keys(baseline.value),
            ...Object.keys(current.value),
        ])].sort()
        const entries: Array<[string, JsonValue]> = []
        for (const key of keys) {
            const value = reconcileValue(
                property(source.value, key),
                property(baseline.value, key),
                property(current.value, key),
            )
            if (value !== missing) entries.push([key, value])
        }
        return Object.fromEntries(entries)
    }
    return current.present ? cloneJsonValue(current.value) : missing
}

function mergeClientValue(source: JsonValue, baseline: JsonValue, current: JsonValue): JsonValue {
    if (!isRecord(source) || !isRecord(baseline) || !isRecord(current)) {
        return cloneJsonValue(current)
    }
    const keys = [...new Set([...Object.keys(source), ...Object.keys(current)])].sort()
    const entries: Array<[string, JsonValue]> = []
    for (const key of keys) {
        const sourceValue = property(source, key)
        const baselineValue = property(baseline, key)
        const currentValue = property(current, key)
        if (currentValue.present) {
            const value = sourceValue.present && baselineValue.present
                ? mergeClientValue(sourceValue.value, baselineValue.value, currentValue.value)
                : cloneJsonValue(currentValue.value)
            entries.push([key, value])
        } else if (sourceValue.present && !baselineValue.present) {
            entries.push([key, cloneJsonValue(sourceValue.value)])
        }
    }
    return Object.fromEntries(entries)
}

function hasSameValue(left: PresentValue, right: PresentValue): boolean {
    return left.present === right.present
        && (!left.present || (right.present && isDeepStrictEqual(left.value, right.value)))
}

function property(value: Record<string, JsonValue>, key: string): PresentValue {
    return Object.prototype.hasOwnProperty.call(value, key)
        ? present(value[key])
        : { present: false }
}

function present(value: unknown): PresentValue {
    return { present: true, value: value as JsonValue }
}

function cloneJsonValue(value: JsonValue): JsonValue {
    if (Array.isArray(value)) return value.map(cloneJsonValue)
    if (!isRecord(value)) return value
    return Object.fromEntries(
        Object.entries(value).map(([key, entry]) => [key, cloneJsonValue(entry)]),
    )
}

function isRecord(value: unknown): value is Record<string, JsonValue> {
    return value !== null && typeof value === "object" && !Array.isArray(value)
}
// //// /生成可移植导出和客户端载入数据 ////
