// audience: internal
// # audit-cn-equipment-gacha-assets
// 此脚本核对装备抽卡固定资源, 地区别名导航和抽卡 banner 的可读取实体.

import fs from "node:fs"
import path from "node:path"
import process from "node:process"
import { fileURLToPath } from "node:url"
import unzipper from "unzipper"
import { assetEntryPaths, hashCnAssetPath } from "./cn-asset-paths.mjs"
import { matchesEntityRecord, readEntityRecords } from "./generate-cn-activity-catalog.mjs"

const SCRIPT_DIRECTORY = path.dirname(fileURLToPath(import.meta.url))
const REPOSITORY_ROOT = path.resolve(SCRIPT_DIRECTORY, "..", "..")
const ARCHIVE_DIRECTORIES = Object.freeze([
    "archive-common-full",
    "archive-common-diff",
    "archive-ios-full",
    "archive-ios-diff",
    "archive-medium-full",
    "archive-medium-diff",
])
const EQUIPMENT_GACHA_REQUIREMENTS = Object.freeze([
    Object.freeze({
        name: "sprite_sheet_atlas",
        logicalPaths: ["scene/gacha_equipment/sprite_sheet.atlas.amf3.deflate"],
    }),
    Object.freeze({
        name: "sprite_sheet_texture",
        logicalPaths: [
            "scene/gacha_equipment/sprite_sheet.png",
            "scene/gacha_equipment/sprite_sheet.atf.deflate",
        ],
    }),
    Object.freeze({
        name: "searching_ui",
        logicalPaths: ["scene/gacha_equipment/gacha_equipment_searching.ui.amf3.deflate"],
    }),
    Object.freeze({
        name: "unsealing_ui",
        logicalPaths: ["scene/gacha_equipment/gacha_equipment_unsealing.ui.amf3.deflate"],
    }),
    Object.freeze({
        name: "result_ui",
        logicalPaths: ["scene/gacha_equipment/gacha_equipment_result.ui.amf3.deflate"],
    }),
    Object.freeze({
        name: "effect_new",
        logicalPaths: [
            "scene/gacha_equipment/gacha_equipment_effect_new.parts.amf3.deflate",
            "scene/gacha_equipment/gacha_equipment_effect_new.frame.amf3.deflate",
        ],
    }),
    Object.freeze({
        name: "effect_rarity_star",
        logicalPaths: [
            "scene/gacha_equipment/gacha_equipment_effect_rarity_star.parts.amf3.deflate",
            "scene/gacha_equipment/gacha_equipment_effect_rarity_star.frame.amf3.deflate",
        ],
    }),
    Object.freeze({
        name: "effect_backshine",
        logicalPaths: [
            "scene/gacha_equipment/gacha_equipment_effect_backshine.parts.amf3.deflate",
            "scene/gacha_equipment/gacha_equipment_effect_backshine.frame.amf3.deflate",
        ],
    }),
])
const EQUIPMENT_ALIAS_PAIRS = Object.freeze([
    Object.freeze(["5000", "5009"]),
    Object.freeze(["5006", "5015"]),
    Object.freeze(["5031", "25027"]),
])

// //// 读取审计路径参数 [@x380kkm 2026-08-25] ////
function readOption(args, name, fallback) {
    const index = args.indexOf(name)
    if (index < 0) return fallback
    const value = args[index + 1]
    if (!value || value.startsWith("--")) throw new Error(`missing value for ${name}`)
    return path.resolve(value)
}
// //// /读取审计路径参数 ////

// //// 将逻辑路径解析为当前实体记录 [@x380kkm 2026-08-25] ////
function resolveEntity(logicalPath, entityRecords) {
    const hash = hashCnAssetPath(logicalPath)
    const entryPath = assetEntryPaths(hash).find((candidate) => entityRecords.has(candidate))
    if (!entryPath) return { hash, logicalPath }
    return { entryPath, hash, logicalPath, record: entityRecords.get(entryPath) }
}

function resolveRequirement(requirement, entityRecords) {
    const candidates = requirement.logicalPaths.map((logicalPath) => resolveEntity(logicalPath, entityRecords))
    const resolved = candidates.filter((candidate) => candidate.entryPath)
    if (resolved.length !== 1) {
        throw new Error(`equipment gacha asset resolution is invalid: ${requirement.name}`)
    }
    return { name: requirement.name, ...resolved[0] }
}
// //// /将逻辑路径解析为当前实体记录 ////

// //// 核对当前实体字节存在于 common 归档 [@x380kkm 2026-08-25] ////
async function verifyArchiveEntities(cdnRoot, entities) {
    const pending = new Map(entities.map((entity) => [entity.entryPath, entity]))
    const verified = new Map()
    for (const directoryName of ARCHIVE_DIRECTORIES) {
        const directory = path.join(cdnRoot, directoryName)
        if (!fs.existsSync(directory)) continue
        const archiveNames = fs.readdirSync(directory).filter((name) => name.endsWith(".zip")).sort()
        for (const archiveName of archiveNames) {
            if (pending.size === 0) break
            const archive = await unzipper.Open.file(path.join(directory, archiveName))
            for (const entry of archive.files) {
                const entity = pending.get(entry.path)
                if (!entity || entry.type !== "File" || entry.uncompressedSize !== entity.record.byteLength) continue
                const buffer = await entry.buffer()
                if (!matchesEntityRecord(buffer, entity.record)) continue
                verified.set(entry.path, archiveName)
                pending.delete(entry.path)
            }
        }
    }
    if (pending.size !== 0) {
        throw new Error(`CN archive is missing current equipment gacha entities: ${[...pending.keys()].join(",")}`)
    }
    return verified
}
// //// /核对当前实体字节存在于 common 归档 ////

// //// 核对地区投影池的抽卡 banner [@x380kkm 2026-08-25] ////
function pngDimensions(filePath) {
    const header = fs.readFileSync(filePath).subarray(0, 24)
    const signature = "89504e470d0a1a0a"
    if (header.length !== 24 || header.subarray(0, 8).toString("hex") !== signature
        || header.subarray(12, 16).toString("ascii") !== "IHDR") {
        throw new Error(`gacha banner is not PNG: ${filePath}`)
    }
    return { width: header.readUInt32BE(16), height: header.readUInt32BE(20) }
}

function auditProjectedBanners(document, policy, entityRecords, bannerBundleRoot) {
    const projections = policy.featureLinkProjections?.feature_banner
    if (projections === null || typeof projections !== "object") {
        throw new Error("feature banner projections are missing")
    }
    const banners = []
    for (const [aliasId, canonicalId] of EQUIPMENT_ALIAS_PAIRS) {
        if (String(projections[aliasId]) !== canonicalId) {
            throw new Error(`equipment gacha feature projection is invalid: ${aliasId}`)
        }
        const alias = document[aliasId]
        const canonical = document[canonicalId]
        if (!alias || !canonical || alias.type !== 1 || canonical.type !== 1
            || alias.bannerImage !== canonical.bannerImage) {
            throw new Error(`equipment gacha alias content is invalid: ${aliasId}`)
        }
        const logicalPath = `${canonical.bannerImage}.png`
        const entity = resolveEntity(logicalPath, entityRecords)
        const generatedPath = path.join(bannerBundleRoot, "activity-banners", `${entity.hash}.png`)
        if (entity.entryPath) {
            banners.push({ aliasId, canonicalId, source: "entity", ...entity })
            continue
        }
        if (!fs.existsSync(generatedPath)) {
            throw new Error(`equipment gacha banner is missing: ${logicalPath}`)
        }
        const dimensions = pngDimensions(generatedPath)
        if (dimensions.width !== 510 || dimensions.height !== 180) {
            throw new Error(`equipment gacha banner dimensions are invalid: ${logicalPath}`)
        }
        banners.push({ aliasId, canonicalId, source: "generated", hash: entity.hash, logicalPath })
    }
    return banners
}
// //// /核对地区投影池的抽卡 banner ////

// //// 执行装备抽卡资源审计 [@x380kkm 2026-08-25] ////
async function audit(args) {
    const cdnRoot = readOption(args, "--cdn-root", path.join(process.cwd(), ".cdn", "cn"))
    const bannerBundleRoot = readOption(args, "--banner-bundle", path.join(process.cwd(), "bundle"))
    const document = JSON.parse(fs.readFileSync(path.join(REPOSITORY_ROOT, "assets", "gacha.json"), "utf8"))
    const policy = JSON.parse(fs.readFileSync(path.join(REPOSITORY_ROOT, "assets", "gacha-region-policy.json"), "utf8"))
    const entityRecords = readEntityRecords(cdnRoot)
    const fixedAssets = EQUIPMENT_GACHA_REQUIREMENTS.map((requirement) => resolveRequirement(requirement, entityRecords))
    const banners = auditProjectedBanners(document, policy, entityRecords, bannerBundleRoot)
    const sourceBanners = banners.filter((banner) => banner.entryPath)
    const verifiedArchives = await verifyArchiveEntities(cdnRoot, [...fixedAssets, ...sourceBanners])
    return {
        fixedAssetCount: fixedAssets.length,
        generatedBannerCount: banners.filter((banner) => banner.source === "generated").length,
        projectedBannerCount: banners.length,
        sourceBannerCount: sourceBanners.length,
        verifiedArchiveEntityCount: verifiedArchives.size,
        fixedAssets: fixedAssets.map((asset) => ({
            archive: verifiedArchives.get(asset.entryPath),
            entryPath: asset.entryPath,
            logicalPath: asset.logicalPath,
            name: asset.name,
        })),
        banners,
    }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    try {
        process.stdout.write(`${JSON.stringify(await audit(process.argv.slice(2)))}\n`)
    } catch (error) {
        process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`)
        process.exitCode = 1
    }
}
// //// /执行装备抽卡资源审计 ////
