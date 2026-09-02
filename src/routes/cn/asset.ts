// audience: internal | external
// # cn-asset
// CN 资产接口只返回本地 CDN 元数据, 不连接停运的官方 CDN.

import { FastifyInstance, FastifyReply, FastifyRequest } from "fastify"
import { existsSync, readdirSync, statSync } from "fs"
import path from "path"
import { generateDataHeaders } from "../../utils"

const envCdnDir = process.env.CDN_DIR ?? ".cdn"
const cnCdnDir = path.isAbsolute(envCdnDir)
    ? path.join(envCdnDir, "cn")
    : path.join(process.cwd(), envCdnDir, "cn")
const defaultVersion = process.env.CN_RES_VERSION ?? "1.4.54"
const fullAssetVersion = "1.4.0"
const archiveDirectories = [
    "archive-common-full",
    "archive-medium-full",
    "archive-android-full",
    "archive-common-diff",
    "archive-medium-diff",
    "archive-android-diff",
]

function entityListsDirectory(): string {
    if (existsSync(path.join(cnCdnDir, "EntityLists"))) return "EntityLists"
    if (existsSync(path.join(cnCdnDir, "entities"))) return "entities"
    return "EntityLists"
}

function listArchives(subdirectory: string, baseUrl: string) {
    const directory = path.join(cnCdnDir, subdirectory)
    try {
        return readdirSync(directory)
            .filter((name) => name.endsWith(".zip"))
            .map((name) => ({
                location: `${baseUrl}/${subdirectory}/${name}`,
                size: statSync(path.join(directory, name)).size,
                sha256: "",
            }))
    } catch {
        return []
    }
}

function compareVersions(left: string, right: string): number {
    const leftParts = left.split(".").map(Number)
    const rightParts = right.split(".").map(Number)
    for (let index = 0; index < 3; index += 1) {
        const difference = leftParts[index] - rightParts[index]
        if (difference !== 0) return difference
    }
    return 0
}

function listDiffArchives(baseUrl: string) {
    const groups = new Map<string, { original_version: string, archive: ReturnType<typeof listArchives> }>()
    for (const subdirectory of ["archive-common-diff", "archive-medium-diff", "archive-android-diff"]) {
        const directory = path.join(cnCdnDir, subdirectory)
        let names: string[]
        try {
            names = readdirSync(directory).filter((name) => name.endsWith(".zip"))
        } catch {
            continue
        }
        for (const name of names) {
            const match = name.match(/^pinball-(\d+\.\d+\.\d+)-(\d+\.\d+\.\d+)-\d+-/)
            if (match === null) continue
            const originalVersion = match[1]
            const targetVersion = match[2]
            const group = groups.get(targetVersion) ?? { original_version: originalVersion, archive: [] }
            group.archive.push({
                location: `${baseUrl}/${subdirectory}/${name}`,
                size: statSync(path.join(directory, name)).size,
                sha256: "",
            })
            groups.set(targetVersion, group)
        }
    }
    return [...groups.entries()]
        .sort(([left], [right]) => compareVersions(left, right))
        .map(([version, group]) => ({ version, ...group }))
}

function getTotalSize(): number {
    let total = 0
    for (const subdirectory of archiveDirectories) {
        const directory = path.join(cnCdnDir, subdirectory)
        try {
            for (const name of readdirSync(directory)) total += statSync(path.join(directory, name)).size
        } catch { }
    }
    return total
}

export function getCnVersionInfo(baseUrl: string) {
    const entityLists = entityListsDirectory()
    return {
        base_url: `${baseUrl}/${entityLists}/`,
        files_list: `${baseUrl}/${entityLists}/10939-android_medium.csv`,
        total_size: getTotalSize(),
        delayed_assets_size: 0,
    }
}

function buildPathResponse(baseUrl: string, resVer: string | undefined) {
    const full = [
        ...listArchives("archive-common-full", baseUrl),
        ...listArchives("archive-medium-full", baseUrl),
        ...listArchives("archive-android-full", baseUrl),
    ]
    const diff = listDiffArchives(baseUrl)
    const targetVersion = diff.length === 0 ? defaultVersion : diff[diff.length - 1].version
    return {
        info: {
            client_asset_version: resVer ?? "",
            target_asset_version: targetVersion,
            eventual_target_asset_version: targetVersion,
            is_initial: true,
            latest_maj_first_version: fullAssetVersion,
        },
        full: { version: fullAssetVersion, archive: full },
        diff,
        asset_version_hash: "",
    }
}

const routes = async (fastify: FastifyInstance) => {
    fastify.post("/version_info", async (request: FastifyRequest, reply: FastifyReply) => {
        const baseUrl = process.env.CN_CDN_BASE_URL ?? `http://${request.headers.host ?? "localhost:8000"}/patch/cn`
        return reply.type("application/json").send({
            data_headers: generateDataHeaders(),
            data: getCnVersionInfo(baseUrl),
        })
    })

    fastify.post("/get_path", async (request: FastifyRequest, reply: FastifyReply) => {
        const baseUrl = process.env.CN_CDN_BASE_URL ?? `http://${request.headers.host ?? "localhost:8000"}/patch/cn`
        return reply.type("application/json").send({
            data_headers: generateDataHeaders({ asset_update: true }),
            data: buildPathResponse(baseUrl, typeof request.headers.res_ver === "string" ? request.headers.res_ver : undefined),
        })
    })
}

export default routes

export const ENTITY_LISTS_DIR = entityListsDirectory()
