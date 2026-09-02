// audience: external
// # cn-cdn-preparer
//
// 本模块从固定 GitHub Release 下载 CN CDN 分片, 校验清单, 并把 tar 流解包到目标目录.
// 运行需要 Node.js 20.6.0 和 tar; 首次下载还需要 GitHub CLI.
// 工具只删除自己创建的暂存目录和下载分片, 不覆盖已有的 cn 目录.
// 工具只复用带匹配安装标记的目录, `--adopt-existing` 显式接管未标记目录.
// `--validate-existing` 只读取并校验已有 CDN, 不改变目录内容或安装标记.

import { createHash, randomUUID } from "node:crypto";
import { spawn } from "node:child_process";
import { createReadStream } from "node:fs";
import {
    access,
    mkdir,
    readFile,
    readdir,
    rename,
    rm,
    stat,
    writeFile,
} from "node:fs/promises";
import { dirname, isAbsolute, join, relative, resolve, sep } from "node:path";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import { fileURLToPath } from "node:url";

const MODULE_DIRECTORY = dirname(fileURLToPath(import.meta.url));
const DEFAULT_MANIFEST_PATH = join(MODULE_DIRECTORY, "cdn-manifest.json");
const MARKER_FILE_NAME = ".starpoint-cn-cdn.json";

function isSafeRelativePath(value) {
    if (typeof value !== "string" || value.length === 0 || isAbsolute(value)) {
        return false;
    }

    const normalized = value.replaceAll("\\", "/");
    return !normalized.split("/").includes("..") && normalized !== ".";
}

function isSamePathOrDescendant(parentPath, candidatePath) {
    const relativePath = relative(parentPath, candidatePath);
    return relativePath === ""
        || (!relativePath.startsWith(`..${sep}`) && relativePath !== ".." && !isAbsolute(relativePath));
}

function assertPositiveSafeInteger(value, fieldName) {
    if (!Number.isSafeInteger(value) || value <= 0) {
        throw new Error(`${fieldName} 必须是正安全整数.`);
    }
}

// //// 校验 CDN 清单边界 [@x380kkm 2026-07-22] ////
export function validateManifest(manifest) {
    if (manifest?.schema !== "wf-cn-cdn-release" || manifest.version !== 1) {
        throw new Error("CDN 清单格式不受支持.");
    }

    for (const fieldName of ["resVersion", "repo", "tag", "topLevelDir"]) {
        if (typeof manifest[fieldName] !== "string" || manifest[fieldName].length === 0) {
            throw new Error(`CDN 清单缺少 ${fieldName}.`);
        }
    }

    if (manifest.topLevelDir !== "cn") {
        throw new Error("CDN 清单的顶层目录必须是 cn.");
    }

    assertPositiveSafeInteger(manifest.extractedFileCount, "extractedFileCount");
    assertPositiveSafeInteger(manifest.extractedSize, "extractedSize");
    assertPositiveSafeInteger(manifest.partCount, "partCount");
    assertPositiveSafeInteger(manifest.totalSize, "totalSize");

    if (!Array.isArray(manifest.parts) || manifest.parts.length !== manifest.partCount) {
        throw new Error("CDN 清单的分片数量不一致.");
    }

    const partNames = new Set();
    let totalSize = 0;
    for (const part of manifest.parts) {
        if (!/^[a-zA-Z0-9._-]+$/.test(part.name) || partNames.has(part.name)) {
            throw new Error(`CDN 分片名称无效或重复: ${part.name}.`);
        }
        if (!/^[a-f0-9]{64}$/.test(part.sha256)) {
            throw new Error(`CDN 分片 SHA-256 无效: ${part.name}.`);
        }
        assertPositiveSafeInteger(part.size, `parts.${part.name}.size`);
        partNames.add(part.name);
        totalSize += part.size;
    }

    if (totalSize !== manifest.totalSize) {
        throw new Error("CDN 清单的分片总大小不一致.");
    }

    if (!Array.isArray(manifest.requiredFiles) || manifest.requiredFiles.length === 0) {
        throw new Error("CDN 清单缺少解包结果校验文件.");
    }
    for (const requiredFile of manifest.requiredFiles) {
        if (!isSafeRelativePath(requiredFile.path) || !/^[a-f0-9]{64}$/.test(requiredFile.sha256)) {
            throw new Error(`CDN 解包结果校验项无效: ${requiredFile.path}.`);
        }
        assertPositiveSafeInteger(requiredFile.size, `requiredFiles.${requiredFile.path}.size`);
    }

    return manifest;
}
// //// /校验 CDN 清单边界 ////

async function pathExists(path) {
    try {
        await access(path);
        return true;
    } catch {
        return false;
    }
}

async function calculateSha256(path) {
    const hash = createHash("sha256");
    for await (const chunk of createReadStream(path)) {
        hash.update(chunk);
    }
    return hash.digest("hex");
}

// //// 检查外部命令可用性 [@x380kkm 2026-07-22] ////
async function assertCommandAvailable(command) {
    const child = spawn(command, ["--version"], { stdio: "ignore" });
    const exitCode = await new Promise((resolveExit, rejectExit) => {
        child.once("error", (error) => {
            rejectExit(new Error(`缺少命令 ${command}: ${error.message}`));
        });
        child.once("close", resolveExit);
    });
    if (exitCode !== 0) {
        throw new Error(`${command} --version 执行失败, 退出码 ${exitCode}.`);
    }
}
// //// /检查外部命令可用性 ////

async function inspectTree(directory) {
    let fileCount = 0;
    let totalSize = 0;
    const entries = await readdir(directory, { withFileTypes: true });
    for (const entry of entries) {
        const entryPath = join(directory, entry.name);
        if (entry.isDirectory()) {
            const child = await inspectTree(entryPath);
            fileCount += child.fileCount;
            totalSize += child.totalSize;
        } else if (entry.isFile()) {
            const entryStat = await stat(entryPath);
            fileCount += 1;
            totalSize += entryStat.size;
        }
    }
    return { fileCount, totalSize };
}

function isProcessActive(processId) {
    try {
        process.kill(processId, 0);
        return true;
    } catch (error) {
        return error?.code === "EPERM";
    }
}

async function removeStaleStagingDirectories(cdnDirectory, logger) {
    const entries = await readdir(cdnDirectory, { withFileTypes: true });
    for (const entry of entries) {
        const match = /^\.starpoint-cn-staging-(\d+)-[0-9a-f-]+$/.exec(entry.name);
        if (!entry.isDirectory() || match === null) {
            continue;
        }
        const ownerProcessId = Number(match[1]);
        if (ownerProcessId !== process.pid && isProcessActive(ownerProcessId)) {
            logger.log(`保留正在使用的 CN CDN 暂存目录: ${entry.name}`);
            continue;
        }
        const stagingDirectory = join(cdnDirectory, entry.name);
        assertGeneratedStagingPath(cdnDirectory, stagingDirectory);
        await rm(stagingDirectory, { recursive: true, force: true });
        logger.log(`清理中断遗留的 CN CDN 暂存目录: ${entry.name}`);
    }
}

// //// 校验已解包 CDN 目录 [@x380kkm 2026-07-22] ////
export async function validateExtractedCdn(cnDirectory, manifest) {
    const tree = await inspectTree(cnDirectory);
    if (tree.fileCount !== manifest.extractedFileCount || tree.totalSize !== manifest.extractedSize) {
        throw new Error(
            `现有 CN CDN 不完整: 文件 ${tree.fileCount}/${manifest.extractedFileCount}, `
            + `大小 ${tree.totalSize}/${manifest.extractedSize}.`,
        );
    }

    for (const requiredFile of manifest.requiredFiles) {
        const requiredPath = resolve(cnDirectory, requiredFile.path);
        if (!isSamePathOrDescendant(cnDirectory, requiredPath)) {
            throw new Error(`CDN 校验路径越界: ${requiredFile.path}.`);
        }
        const requiredStat = await stat(requiredPath);
        if (!requiredStat.isFile() || requiredStat.size !== requiredFile.size) {
            throw new Error(`CDN 校验文件大小不一致: ${requiredFile.path}.`);
        }
        const sha256 = await calculateSha256(requiredPath);
        if (sha256 !== requiredFile.sha256) {
            throw new Error(`CDN 校验文件 SHA-256 不一致: ${requiredFile.path}.`);
        }
    }

    return tree;
}
// //// /校验已解包 CDN 目录 ////

// //// 只读校验已有 CN CDN [@x380kkm 2026-07-27] ////
export async function validateCdn(options = {}) {
    const manifestPath = resolve(options.manifestPath ?? DEFAULT_MANIFEST_PATH);
    const manifest = validateManifest(JSON.parse(await readFile(manifestPath, "utf8")));
    const repositoryRoot = resolve(MODULE_DIRECTORY, "../..");
    const cdnDirectory = resolve(options.cdnDirectory ?? join(repositoryRoot, ".cdn"));
    const cnDirectory = join(cdnDirectory, manifest.topLevelDir);

    if (!await pathExists(cnDirectory)) {
        throw new Error(`现有 CN CDN 不存在: ${cnDirectory}.`);
    }

    const tree = await validateExtractedCdn(cnDirectory, manifest);
    return { status: "validated", cnDirectory, tree };
}
// //// /只读校验已有 CN CDN ////

async function validatePart(partPath, part) {
    const partStat = await stat(partPath);
    if (!partStat.isFile() || partStat.size !== part.size) {
        throw new Error(`CDN 分片大小不一致: ${part.name}.`);
    }
    const sha256 = await calculateSha256(partPath);
    if (sha256 !== part.sha256) {
        throw new Error(`CDN 分片 SHA-256 不一致: ${part.name}.`);
    }
}

async function runCommand(command, args) {
    const child = spawn(command, args, { stdio: ["ignore", "inherit", "pipe"] });
    let standardError = "";
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk) => {
        standardError += chunk;
        process.stderr.write(chunk);
    });
    const exitCode = await new Promise((resolveExit, rejectExit) => {
        child.once("error", (error) => {
            rejectExit(new Error(`无法启动 ${command}: ${error.message}`));
        });
        child.once("close", resolveExit);
    });
    if (exitCode !== 0) {
        throw new Error(`${command} 执行失败, 退出码 ${exitCode}: ${standardError.trim()}`);
    }
}

// //// 下载并逐片校验 Release 资源 [@x380kkm 2026-07-22] ////
async function obtainParts(manifest, downloadDirectory, skipDownload, logger) {
    await mkdir(downloadDirectory, { recursive: true });
    const partPaths = [];
    let isDownloadCommandReady = false;
    for (const part of manifest.parts) {
        const partPath = join(downloadDirectory, part.name);
        let isValid = false;
        if (await pathExists(partPath)) {
            try {
                await validatePart(partPath, part);
                isValid = true;
                logger.log(`复用已校验分片: ${part.name}`);
            } catch (error) {
                if (skipDownload) {
                    throw error;
                }
                await rm(partPath, { force: true });
            }
        }

        if (!isValid) {
            if (skipDownload) {
                throw new Error(`缺少 CDN 分片: ${partPath}.`);
            }
            if (!isDownloadCommandReady) {
                await assertCommandAvailable("gh");
                isDownloadCommandReady = true;
            }
            logger.log(`下载 CDN 分片: ${part.name}`);
            await runCommand("gh", [
                "release",
                "download",
                manifest.tag,
                "--repo",
                manifest.repo,
                "--pattern",
                part.name,
                "--dir",
                downloadDirectory,
                "--clobber",
            ]);
            await validatePart(partPath, part);
        }
        partPaths.push(partPath);
    }
    return partPaths;
}
// //// /下载并逐片校验 Release 资源 ////

async function removeDownloadedParts(partPaths, logger) {
    for (const partPath of partPaths) {
        try {
            await rm(partPath, { force: true });
        } catch (error) {
            logger.log(`无法删除 CDN 分片 ${partPath}: ${error.message}`);
        }
    }
}

async function* readPartChunks(partPaths) {
    for (const partPath of partPaths) {
        for await (const chunk of createReadStream(partPath)) {
            yield chunk;
        }
    }
}

async function terminateChildProcess(child) {
    if (child.exitCode !== null || child.signalCode !== null) {
        return;
    }
    const closed = new Promise((resolveClose) => child.once("close", resolveClose));
    child.kill();
    await closed;
}

// //// 无中间 tar 地流式解包分片 [@x380kkm 2026-07-22] ////
async function extractParts(partPaths, stagingDirectory) {
    const tar = spawn("tar", ["-xf", "-", "-C", stagingDirectory], {
        stdio: ["pipe", "ignore", "pipe"],
    });
    let standardError = "";
    tar.stderr.setEncoding("utf8");
    tar.stderr.on("data", (chunk) => {
        standardError += chunk;
    });
    const completion = new Promise((resolveExit, rejectExit) => {
        tar.once("error", rejectExit);
        tar.once("close", (exitCode) => {
            if (exitCode === 0) {
                resolveExit();
            } else {
                rejectExit(new Error(`tar 解包失败, 退出码 ${exitCode}: ${standardError.trim()}`));
            }
        });
    });

    try {
        await Promise.all([
            pipeline(Readable.from(readPartChunks(partPaths)), tar.stdin),
            completion,
        ]);
    } catch (error) {
        await terminateChildProcess(tar);
        throw error;
    }
}
// //// /无中间 tar 地流式解包分片 ////

function assertGeneratedStagingPath(cdnDirectory, stagingDirectory) {
    const relativePath = relative(cdnDirectory, stagingDirectory);
    if (!isSamePathOrDescendant(cdnDirectory, stagingDirectory) || !relativePath.startsWith(".starpoint-cn-staging-")) {
        throw new Error("暂存目录不在 CDN 目录内.");
    }
}

async function readInstallationMarker(cdnDirectory) {
    try {
        return JSON.parse(await readFile(join(cdnDirectory, MARKER_FILE_NAME), "utf8"));
    } catch {
        return null;
    }
}

function isMarkerForManifest(marker, manifest) {
    return marker?.schema === "starpoint-cn-cdn-installation"
        && marker.version === 1
        && marker.repo === manifest.repo
        && marker.tag === manifest.tag
        && marker.resVersion === manifest.resVersion
        && marker.fileCount === manifest.extractedFileCount
        && marker.totalSize === manifest.extractedSize;
}

async function writeMarker(cdnDirectory, manifest, tree) {
    const markerPath = join(cdnDirectory, MARKER_FILE_NAME);
    const temporaryMarkerPath = `${markerPath}.${process.pid}.tmp`;
    const marker = {
        schema: "starpoint-cn-cdn-installation",
        version: 1,
        repo: manifest.repo,
        tag: manifest.tag,
        resVersion: manifest.resVersion,
        fileCount: tree.fileCount,
        totalSize: tree.totalSize,
        preparedAt: new Date().toISOString(),
    };
    await writeFile(temporaryMarkerPath, `${JSON.stringify(marker, null, 2)}\n`, "utf8");
    await rename(temporaryMarkerPath, markerPath);
}

// //// 准备可直接挂载的 CN CDN [@x380kkm 2026-07-22] ////
export async function prepareCdn(options = {}) {
    const manifestPath = resolve(options.manifestPath ?? DEFAULT_MANIFEST_PATH);
    const manifest = validateManifest(JSON.parse(await readFile(manifestPath, "utf8")));
    const repositoryRoot = resolve(MODULE_DIRECTORY, "../..");
    const cdnDirectory = resolve(options.cdnDirectory ?? join(repositoryRoot, ".cdn"));
    const downloadDirectory = resolve(
        options.downloadDirectory ?? join(cdnDirectory, ".downloads", manifest.tag),
    );
    const cnDirectory = join(cdnDirectory, manifest.topLevelDir);
    const logger = options.logger ?? console;
    if (isSamePathOrDescendant(cnDirectory, downloadDirectory)) {
        throw new Error("下载目录不能位于 CN CDN 目标目录内.");
    }
    await mkdir(cdnDirectory, { recursive: true });
    await removeStaleStagingDirectories(cdnDirectory, logger);

    if (await pathExists(cnDirectory)) {
        const marker = await readInstallationMarker(cdnDirectory);
        if (!isMarkerForManifest(marker, manifest) && options.adoptExisting !== true) {
            throw new Error("现有 CN CDN 没有匹配的安装标记; 确认来源后使用 --adopt-existing.");
        }
        const tree = await validateExtractedCdn(cnDirectory, manifest);
        await writeMarker(cdnDirectory, manifest, tree);
        if (options.keepParts !== true) {
            await removeDownloadedParts(
                manifest.parts.map((part) => join(downloadDirectory, part.name)),
                logger,
            );
        }
        logger.log(`CN CDN 已就绪: ${cnDirectory}`);
        return {
            status: isMarkerForManifest(marker, manifest) ? "already-prepared" : "adopted-existing",
            cnDirectory,
            tree,
        };
    }

    await assertCommandAvailable("tar");
    const partPaths = await obtainParts(
        manifest,
        downloadDirectory,
        options.skipDownload === true,
        logger,
    );
    const stagingDirectory = join(cdnDirectory, `.starpoint-cn-staging-${process.pid}-${randomUUID()}`);
    assertGeneratedStagingPath(cdnDirectory, stagingDirectory);
    await mkdir(stagingDirectory, { recursive: true });

    try {
        logger.log("流式解包 CN CDN.");
        await extractParts(partPaths, stagingDirectory);
        const stagedCnDirectory = join(stagingDirectory, manifest.topLevelDir);
        const tree = await validateExtractedCdn(stagedCnDirectory, manifest);
        await rename(stagedCnDirectory, cnDirectory);
        await writeMarker(cdnDirectory, manifest, tree);
        if (options.keepParts !== true) {
            await removeDownloadedParts(partPaths, logger);
        }
        logger.log(`CN CDN 准备完成: ${cnDirectory}`);
        return { status: "prepared", cnDirectory, tree };
    } finally {
        await rm(stagingDirectory, { recursive: true, force: true });
    }
}
// //// /准备可直接挂载的 CN CDN ////

function readOptionValue(args, index, optionName) {
    const value = args[index + 1];
    if (value === undefined || value.trim().length === 0 || value.startsWith("--")) {
        throw new Error(`${optionName} 缺少非空值.`);
    }
    return value;
}

// //// 解析 CDN 准备命令参数 [@x380kkm 2026-07-22] ////
export function parseArguments(args) {
    const options = {};
    for (let index = 0; index < args.length; index += 1) {
        const argument = args[index];
        switch (argument) {
            case "--cdn-dir":
                options.cdnDirectory = readOptionValue(args, index, argument);
                index += 1;
                break;
            case "--download-dir":
                options.downloadDirectory = readOptionValue(args, index, argument);
                index += 1;
                break;
            case "--manifest":
                options.manifestPath = readOptionValue(args, index, argument);
                index += 1;
                break;
            case "--skip-download":
                options.skipDownload = true;
                break;
            case "--keep-parts":
                options.keepParts = true;
                break;
            case "--adopt-existing":
                options.adoptExisting = true;
                break;
            case "--validate-existing":
                options.validateExisting = true;
                break;
            default:
                throw new Error(`未知参数: ${argument}.`);
        }
    }
    return options;
}
// //// /解析 CDN 准备命令参数 ////

const invokedPath = process.argv[1] ? resolve(process.argv[1]) : "";
if (invokedPath === fileURLToPath(import.meta.url)) {
    const options = parseArguments(process.argv.slice(2));
    const operation = options.validateExisting ? validateCdn : prepareCdn;
    operation(options).catch((error) => {
        console.error(error instanceof Error ? error.message : error);
        process.exitCode = 1;
    });
}
