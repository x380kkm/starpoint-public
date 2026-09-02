// audience: internal
// # multiplayer-protocol-differential-lib
//
// 该模块提取 JavaScript 与 Rust 协议分支, 记录源码锚点并比较同名协议事实.

import path from "node:path"

// //// 提取平衡代码块与分派分支 [@x380kkm 2026-08-24] ////
function escapeRegularExpression(value) {
    return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
}

function findClosingDelimiter(source, openIndex) {
    const pairs = new Map([["(", ")"], ["[", "]"], ["{", "}"]])
    const stack = []
    let quote = null
    let escaped = false
    let lineComment = false
    let blockComment = false
    for (let index = openIndex; index < source.length; index += 1) {
        const character = source[index]
        const nextCharacter = source[index + 1]
        if (lineComment) {
            if (character === "\n") lineComment = false
            continue
        }
        if (blockComment) {
            if (character === "*" && nextCharacter === "/") {
                blockComment = false
                index += 1
            }
            continue
        }
        if (quote !== null) {
            if (escaped) escaped = false
            else if (character === "\\") escaped = true
            else if (character === quote) quote = null
            continue
        }
        if (character === "/" && nextCharacter === "/") {
            lineComment = true
            index += 1
            continue
        }
        if (character === "/" && nextCharacter === "*") {
            blockComment = true
            index += 1
            continue
        }
        if (character === '"' || character === "'" || character === "`") {
            quote = character
            continue
        }
        if (pairs.has(character)) stack.push(pairs.get(character))
        else if (stack.at(-1) === character) {
            stack.pop()
            if (stack.length === 0) return index
        }
    }
    return -1
}

export function extractFunction(source, name, language) {
    const pattern = language === "rust"
        ? new RegExp(`\\bfn\\s+${escapeRegularExpression(name)}\\s*\\(`)
        : new RegExp(`\\bfunction\\s+${escapeRegularExpression(name)}\\s*\\(`)
    const definition = pattern.exec(source)
    if (!definition) throw new Error(`missing ${language} function: ${name}`)
    const openIndex = source.indexOf("{", definition.index)
    const closeIndex = findClosingDelimiter(source, openIndex)
    if (openIndex < 0 || closeIndex < 0) throw new Error(`invalid ${language} function: ${name}`)
    return {
        source: source.slice(definition.index, closeIndex + 1),
        start: definition.index,
    }
}

function tryExtractFunction(source, name, language) {
    try {
        return extractFunction(source, name, language)
    } catch (error) {
        if (error instanceof Error && error.message === `missing ${language} function: ${name}`) return null
        throw error
    }
}

export function extractJavaScriptMethod(source, name) {
    const definition = new RegExp(`^\\s*${escapeRegularExpression(name)}\\s*\\(`, "m").exec(source)
    if (!definition) throw new Error(`missing JavaScript method: ${name}`)
    const openIndex = source.indexOf("{", definition.index)
    const closeIndex = findClosingDelimiter(source, openIndex)
    if (openIndex < 0 || closeIndex < 0) throw new Error(`invalid JavaScript method: ${name}`)
    return {
        source: source.slice(definition.index, closeIndex + 1),
        start: definition.index,
    }
}

export function extractBlockAfter(section, pattern, label) {
    const match = pattern.exec(section.source)
    if (!match) throw new Error(`missing ${label}`)
    const openIndex = section.source.indexOf("{", match.index + match[0].length)
    const closeIndex = findClosingDelimiter(section.source, openIndex)
    if (openIndex < 0 || closeIndex < 0) throw new Error(`invalid ${label}`)
    return {
        source: section.source.slice(openIndex, closeIndex + 1),
        start: section.start + openIndex,
    }
}

export function jsSwitchArms(functionSection) {
    const switchBlock = extractBlockAfter(functionSection, /\bswitch\s*\([^)]*\)/, "JavaScript switch")
    const starts = [...switchBlock.source.matchAll(/\bcase\s+(\d+)\s*:/g)]
    return starts.map((match, index) => ({
        tag: Number(match[1]),
        source: switchBlock.source.slice(match.index, starts[index + 1]?.index ?? switchBlock.source.length),
        start: switchBlock.start + match.index,
    }))
}

export function rustMatchArms(matchSection) {
    const starts = [...matchSection.source.matchAll(/^([ \t]*)Some\(([^)\n]*)\)/gm)]
    if (starts.length === 0) return []
    const minimumIndent = Math.min(...starts.map((match) => match[1].length))
    const topLevelStarts = starts.filter((match) => match[1].length === minimumIndent)
    return topLevelStarts.map((match, index) => {
        const end = topLevelStarts[index + 1]?.index ?? matchSection.source.length
        const armSource = matchSection.source.slice(match.index, end)
        const arrowIndex = armSource.indexOf("=>")
        return {
            pattern: match[2].trim(),
            guard: arrowIndex < 0 ? "" : armSource.slice(match[0].length, arrowIndex).trim(),
            source: arrowIndex < 0 ? armSource : armSource.slice(arrowIndex + 2),
            start: matchSection.start + match.index,
        }
    })
}

export function numericPatternValues(pattern) {
    const values = []
    for (const match of pattern.matchAll(/(\d+)\s*\.\.\s*=\s*(\d+)/g)) {
        const start = Number(match[1])
        const end = Number(match[2])
        for (let value = start; value <= end; value += 1) values.push(value)
    }
    const withoutRanges = pattern.replace(/\d+\s*\.\.\s*=\s*\d+/g, "")
    values.push(...[...withoutRanges.matchAll(/\d+/g)].map((match) => Number(match[0])))
    return [...new Set(values)].sort((left, right) => left - right)
}

export function numericRustTags(arms) {
    return [...new Set(arms.flatMap((arm) => numericPatternValues(arm.pattern)))]
        .sort((left, right) => left - right)
}

export function compact(source) {
    return source.replace(/\s+/g, " ").trim()
}

function calledRustMethods(source) {
    return [...new Set([...source.matchAll(/\bself\.([a-z_][a-z0-9_]*)\s*\(/g)]
        .map((match) => match[1]))]
}

export function rustArmCallsSenderAck(arm, moduleSource) {
    if (/queue_frame\(&mut self\.clients\[client_index\]/.test(arm.source)) return true
    return calledRustMethods(arm.source).some((name) => {
        const helper = tryExtractFunction(moduleSource, name, "rust")
        const helperSource = helper ? compact(helper.source) : ""
        return /queue_frame\( &mut self\.clients\[client_index\]/.test(helperSource) &&
            /json!\(\[ 1, \[ 3,/.test(helperSource)
    })
}

export function rustArmRecipientScope(arm, moduleSource) {
    for (const name of calledRustMethods(arm.source)) {
        const helper = tryExtractFunction(moduleSource, name, "rust")
        if (!helper || !/queue_frame\(/.test(helper.source)) continue
        if (/connection_id\s*!=\s*source_connection_id/.test(helper.source)) {
            return "room-excluding-sender"
        }
        if (/client_connection\s*==\s*connection_id/.test(helper.source)) {
            return "listed-connection-ids"
        }
        if (/for client in &mut self\.clients/.test(helper.source)) return "room-including-sender"
    }
    return null
}
// //// /提取平衡代码块与分派分支 ////

// //// 记录协议事实与源码锚点 [@x380kkm 2026-08-24] ////
function lineNumberAt(source, index) {
    return source.slice(0, Math.max(0, index)).split("\n").length
}

function normalizeRelativePath(root, filePath) {
    return path.relative(root, filePath).split(path.sep).join("/")
}

export function createCollector(root) {
    const facts = new Map()
    const sources = new Set()
    return {
        add(factPath, value, filePath, source, index = 0) {
            if (facts.has(factPath)) throw new Error(`duplicate multiplayer fact: ${factPath}`)
            const relativeSource = normalizeRelativePath(root, filePath)
            sources.add(relativeSource)
            facts.set(factPath, {
                value,
                evidence: { source: relativeSource, line: lineNumberAt(source, index) },
            })
        },
        finish() {
            return { facts, sources: [...sources].sort() }
        },
    }
}

function setNestedValue(target, factPath, value) {
    const parts = factPath.split(".")
    let current = target
    for (const part of parts.slice(0, -1)) current = current[part] ??= {}
    current[parts.at(-1)] = value
}

export function materializeFacts(facts) {
    const protocol = {}
    const evidence = {}
    for (const [factPath, fact] of [...facts].sort(([left], [right]) => left.localeCompare(right))) {
        setNestedValue(protocol, factPath, fact.value)
        evidence[factPath] = fact.evidence
    }
    return { protocol, evidence }
}
// //// /记录协议事实与源码锚点 ////

// //// 比较同名协议事实 [@x380kkm 2026-08-24] ////
function sameValue(left, right) {
    return JSON.stringify(left) === JSON.stringify(right)
}

function isStrictArraySubset(left, right) {
    if (!Array.isArray(left) || !Array.isArray(right) || left.length >= right.length) return false
    const rightValues = new Set(right.map((value) => JSON.stringify(value)))
    return left.every((value) => rightValues.has(JSON.stringify(value)))
}

export function compareFacts(referenceFacts, localFacts) {
    const paths = [...new Set([...referenceFacts.keys(), ...localFacts.keys()])].sort()
    const rows = paths.map((factPath) => {
        const reference = referenceFacts.get(factPath)
        const local = localFacts.get(factPath)
        let status
        if (!reference) status = "local-only"
        else if (!local) status = "reference-only"
        else if (reference.value === null || local.value === null) status = "unresolved"
        else if (isStrictArraySubset(reference.value, local.value)) status = "local-superset"
        else status = sameValue(reference.value, local.value) ? "matched" : "different"
        return {
            path: factPath,
            status,
            reference: reference?.value,
            local: local?.value,
            referenceEvidence: reference?.evidence,
            localEvidence: local?.evidence,
        }
    })
    const count = (status) => rows.filter((row) => row.status === status).length
    return {
        summary: {
            total: rows.length,
            matched: count("matched"),
            localSuperset: count("local-superset"),
            different: count("different"),
            unresolved: count("unresolved"),
            referenceOnly: count("reference-only"),
            localOnly: count("local-only"),
        },
        differences: rows.filter((row) => !["matched", "local-superset"].includes(row.status)),
        extensions: rows.filter((row) => row.status === "local-superset"),
        matched: rows.filter((row) => row.status === "matched"),
    }
}
// //// /比较同名协议事实 ////
