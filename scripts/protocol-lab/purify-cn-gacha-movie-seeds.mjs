// audience: internal
// # purify-cn-gacha-movie-seeds
//
// 此脚本使用当前参考物理实现净化服务端扭蛋动画 seed, 并在必要时从同一实现补充对应稀有度的 seed.

import fs from "node:fs"
import path from "node:path"
import { createRequire } from "node:module"
import { fileURLToPath } from "node:url"

const SCRIPT_DIRECTORY = path.dirname(fileURLToPath(import.meta.url))
const REPOSITORY_ROOT = path.resolve(SCRIPT_DIRECTORY, "..", "..")
const MOVIE_SEED_FILES = Object.freeze({
    normal: "gacha_movie_seeds_normal.json",
    fes: "gacha_movie_seeds_fes.json",
    normal_guarantee: "gacha_movie_seeds_normal_guarantee.json",
    fes_guarantee: "gacha_movie_seeds_fes_guarantee.json",
})
const EXPECTED_RARITY = Object.freeze({ "1": 2, "2": 1, "3": 0 })
const REQUIRED_RANKS = Object.freeze({
    normal: new Set(["1", "2", "3"]),
    fes: new Set(["1", "2", "3"]),
    normal_guarantee: new Set(["1", "2"]),
    fes_guarantee: new Set(["1", "2"]),
})
const GENERATED_SEED_MIN = 10_000_000
const GENERATED_SEED_MAX = 10_100_000

// //// 读取净化输入 [@x380kkm 2026-08-25] ////
function optionValue(args, name, fallback = null) {
    const index = args.indexOf(name)
    if (index < 0) return fallback
    const value = args[index + 1]
    if (!value || value.startsWith("--")) throw new Error(`missing value for ${name}`)
    return value
}

function parseArgs(args) {
    const physicsRoot = optionValue(args, "--physics-root")
    if (!physicsRoot) throw new Error("--physics-root is required")
    return {
        assetsRoot: path.resolve(optionValue(args, "--assets-root", path.join(REPOSITORY_ROOT, "assets"))),
        physicsRoot: path.resolve(physicsRoot),
    }
}
// //// /读取净化输入 ////

// //// 载入与源码同步的参考物理实现 [@x380kkm 2026-08-25] ////
function loadPhysics(physicsRoot) {
    const sourcePath = path.join(physicsRoot, "src", "lib", "gacha-physics.ts")
    const modulePath = path.join(physicsRoot, "out", "lib", "gacha-physics.js")
    if (!fs.statSync(sourcePath).isFile() || !fs.statSync(modulePath).isFile()) {
        throw new Error("reference gacha physics source and compiled module are required")
    }
    if (fs.statSync(modulePath).mtimeMs < fs.statSync(sourcePath).mtimeMs) {
        throw new Error("reference gacha physics compiled module is older than its source")
    }
    const physics = createRequire(import.meta.url)(modulePath)
    if (typeof physics.GachaSimulator !== "function" || typeof physics.generateSeedPools !== "function") {
        throw new Error("reference gacha physics module does not expose the required simulation entry points")
    }
    return physics
}
// //// /载入与源码同步的参考物理实现 ////

// //// 按动画和稀有度净化 seed [@x380kkm 2026-08-25] ////
function validateSeedArray(value, movieId, rank, poolKind) {
    if (!Array.isArray(value) || !value.every((seed) => Number.isSafeInteger(seed))) {
        throw new Error(`invalid seed array: movie=${movieId} rank=${rank} pool=${poolKind}`)
    }
    return [...new Set(value)]
}

function generatedSeeds(physics, movieId, expectedRarity) {
    const pools = physics.generateSeedPools(
        physics.MOVIE_CONFIGS[movieId],
        GENERATED_SEED_MIN,
        GENERATED_SEED_MAX,
    )
    return pools[expectedRarity] ?? []
}

function purifySeedArray(physics, movieId, rank, seeds) {
    const expectedRarity = EXPECTED_RARITY[rank]
    const retained = []
    const rejected = []
    for (const seed of seeds) {
        const simulator = new physics.GachaSimulator(seed, physics.MOVIE_CONFIGS[movieId])
        const actualRarity = simulator.simulate()
        if (actualRarity === expectedRarity) {
            retained.push(seed)
        } else {
            rejected.push({ actualRarity, moviePlayable: simulator.moviePlayable, seed })
        }
    }
    return { rejected, retained }
}

function purifyDocument(physics, movieId, document) {
    const rows = []
    for (const rank of Object.keys(EXPECTED_RARITY)) {
        const pools = document[rank]
        if (!pools || typeof pools !== "object" || Array.isArray(pools)) {
            throw new Error(`invalid seed rank document: movie=${movieId} rank=${rank}`)
        }
        for (const poolKind of Object.keys(pools)) {
            const seeds = validateSeedArray(pools[poolKind], movieId, rank, poolKind)
            const result = purifySeedArray(physics, movieId, rank, seeds)
            pools[poolKind] = result.retained
            rows.push({
                movieId,
                rank,
                poolKind,
                before: seeds.length,
                after: result.retained.length,
                rejected: result.rejected,
            })
        }
        if (REQUIRED_RANKS[movieId].has(rank) && (!Array.isArray(pools["0"]) || pools["0"].length === 0)) {
            pools["0"] = generatedSeeds(physics, movieId, EXPECTED_RARITY[rank])
            if (pools["0"].length === 0) {
                throw new Error(`reference physics generated an empty seed pool: movie=${movieId} rank=${rank}`)
            }
            rows.push({ movieId, rank, poolKind: "0", before: 0, after: pools["0"].length, rejected: [] })
        }
    }
    return rows
}
// //// /按动画和稀有度净化 seed ////

// //// 更新服务端 seed 资产 [@x380kkm 2026-08-25] ////
function purifyAssets(options) {
    const physics = loadPhysics(options.physicsRoot)
    const documents = new Map()
    const rows = []
    for (const [movieId, fileName] of Object.entries(MOVIE_SEED_FILES)) {
        const filePath = path.join(options.assetsRoot, fileName)
        const document = JSON.parse(fs.readFileSync(filePath, "utf8"))
        rows.push(...purifyDocument(physics, movieId, document))
        documents.set(filePath, document)
    }
    for (const [filePath, document] of documents) {
        fs.writeFileSync(filePath, JSON.stringify(document), "utf8")
    }
    return {
        rejectedCount: rows.reduce((total, row) => total + row.rejected.length, 0),
        rows: rows.map((row) => ({ ...row, rejectedCount: row.rejected.length })),
    }
}

const report = purifyAssets(parseArgs(process.argv.slice(2)))
process.stdout.write(`${JSON.stringify(report)}\n`)
// //// /更新服务端 seed 资产 ////
