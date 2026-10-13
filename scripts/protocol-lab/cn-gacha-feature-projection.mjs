// audience: internal
// # cn-gacha-feature-projection
// 此模块为每个客户端卡池提供关联特辑, 复用相同 banner 且展示角色属于目标候选的唯一内容.
// 缺少唯一同内容来源的卡池显示自身 banner, 临时入口与规范卡池共享特辑.

// //// 核对展示角色属于卡池候选 [@x380kkm 2026-09-20] ////
function candidateIds(pool) {
    return new Set(Object.values(pool?.pool ?? {}).flat().map((entry) => String(entry.id)))
}

function featuresFitPool(group, pool) {
    const candidates = candidateIds(pool)
    return Object.values(group).every((row) => {
        const characters = [row[0] === "2" ? row[5] : null, row[6] === "0" ? row[7] : null].filter(Boolean)
        return characters.every((id) => pool?.type === 0 && candidates.has(id))
    })
}

function hasFeatures(group) {
    return group && !Array.isArray(group) && Object.keys(group).length > 0
}
// //// /核对展示角色属于卡池候选 ////

// //// 从可证实的同内容卡池或本池 banner 补齐特辑 [@x380kkm 2026-09-20] ////
export function projectGachaFeatures(gacha, source, policy, servicePools) {
    const result = structuredClone(source)
    const recovered = []
    const aliases = { ...policy.normalizedCoverageAliases, ...policy.temporaryAliases }
    for (const [id, row] of Object.entries(gacha)) {
        if (Object.hasOwn(aliases, id) || hasFeatures(result[id])) continue
        const donors = Object.entries(source).filter(([donorId, group]) => hasFeatures(group)
            && gacha[donorId]?.[3] === row[3] && gacha[donorId]?.[13] === row[13]
            && featuresFitPool(group, servicePools[id]))
        const content = new Set(donors.map(([, group]) => JSON.stringify(group)))
        if (content.size === 1) {
            result[id] = structuredClone(donors[0][1])
            recovered.push({ id, source: "same_banner_and_candidates", donors: donors.map(([donor]) => donor) })
        } else {
            if (!row[3]) throw new Error(`gacha ${id} has no feature or banner`)
            result[id] = { "1": ["1", row[3], "", "", "", "", "(None)", "", ""] }
            recovered.push({ id, source: "own_pool_banner", path: row[3] })
        }
    }
    for (const [alias, canonical] of Object.entries(aliases)) {
        if (!Object.hasOwn(gacha, alias)) continue
        if (!hasFeatures(result[String(canonical)])) throw new Error(`gacha feature source is missing: ${canonical}`)
        result[alias] = structuredClone(result[String(canonical)])
    }
    for (const id of Object.keys(gacha)) {
        if (!hasFeatures(result[id])) throw new Error(`gacha feature group is missing: ${id}`)
    }
    return { master: result, recovered, projectedAliases: Object.keys(aliases).filter((id) => Object.hasOwn(gacha, id)).length }
}
// //// /从可证实的同内容卡池或本池 banner 补齐特辑 ////
