// audience: internal
// # cn-cdn-preparer-test
// 此测试用小型分片 tar 验证 CDN 校验、流式解包、复用和损坏拒绝逻辑.

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import {
    mkdtemp,
    mkdir,
    readFile,
    rm,
    unlink,
    writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArguments, prepareCdn, validateCdn } from "../../deployment/cn/prepare-cdn.mjs";

const SILENT_LOGGER = { log() {} };
const PREPARER_PATH = fileURLToPath(new URL("../../deployment/cn/prepare-cdn.mjs", import.meta.url));

function sha256(content) {
    return createHash("sha256").update(content).digest("hex");
}

async function runCommand(command, args) {
    const child = spawn(command, args, { stdio: ["ignore", "ignore", "pipe"] });
    let standardError = "";
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk) => {
        standardError += chunk;
    });
    const exitCode = await new Promise((resolveExit, rejectExit) => {
        child.once("error", rejectExit);
        child.once("close", resolveExit);
    });
    assert.equal(exitCode, 0, standardError);
}

// //// 构造可流式解包的测试分片 [@x380kkm 2026-07-22] ////
async function createFixture(root) {
    const sourceDirectory = join(root, "source");
    const cnDirectory = join(sourceDirectory, "cn");
    const entityDirectory = join(cnDirectory, "entities");
    const archiveDirectory = join(cnDirectory, "archive-common-full");
    const entityContent = Buffer.from("path,size,hash\nasset.bin,3,abc\n", "utf8");
    const archiveContent = Buffer.from([1, 2, 3, 4, 5]);
    await mkdir(entityDirectory, { recursive: true });
    await mkdir(archiveDirectory, { recursive: true });
    await writeFile(join(entityDirectory, "fixture.csv"), entityContent);
    await writeFile(join(archiveDirectory, "fixture.zip"), archiveContent);

    const archivePath = join(root, "cn-cdn.tar");
    await runCommand("tar", ["-cf", archivePath, "-C", sourceDirectory, "cn"]);
    const archive = await readFile(archivePath);
    const splitAt = Math.ceil(archive.length / 3);
    const downloadDirectory = join(root, "downloads");
    await mkdir(downloadDirectory, { recursive: true });
    const parts = [];
    for (let index = 0; index < 3; index += 1) {
        const content = archive.subarray(index * splitAt, Math.min((index + 1) * splitAt, archive.length));
        const name = `cn-cdn.tar.part.0${index}`;
        await writeFile(join(downloadDirectory, name), content);
        parts.push({ name, size: content.length, sha256: sha256(content) });
    }

    const manifest = {
        schema: "wf-cn-cdn-release",
        version: 1,
        resVersion: "test",
        repo: "fixture/repository",
        tag: "fixture",
        archiveName: "cn-cdn.tar",
        topLevelDir: "cn",
        extractedFileCount: 2,
        extractedSize: entityContent.length + archiveContent.length,
        requiredFiles: [
            {
                path: "entities/fixture.csv",
                size: entityContent.length,
                sha256: sha256(entityContent),
            },
        ],
        partCount: parts.length,
        totalSize: archive.length,
        parts,
    };
    const manifestPath = join(root, "manifest.json");
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
    return { downloadDirectory, manifestPath, parts };
}
// //// /构造可流式解包的测试分片 ////

// //// 验证 CDN 准备完整生命周期 [@x380kkm 2026-07-22] ////
async function run() {
    const root = await mkdtemp(join(tmpdir(), "starpoint-cn-cdn-"));
    try {
        assert.throws(() => parseArguments(["--cdn-dir", ""]), /缺少非空值/);
        assert.throws(() => parseArguments(["--cdn-dir", "--skip-download"]), /缺少非空值/);
        assert.deepEqual(
            parseArguments(["--cdn-dir", "fixture-cdn", "--manifest", "fixture-manifest", "--validate-existing"]),
            {
                cdnDirectory: "fixture-cdn",
                manifestPath: "fixture-manifest",
                validateExisting: true,
            },
        );
        const fixture = await createFixture(root);
        await assert.rejects(
            validateCdn({ manifestPath: fixture.manifestPath, cdnDirectory: join(root, "missing-cdn") }),
            /现有 CN CDN 不存在/,
        );
        const invalidLayoutDirectory = join(root, "invalid-layout");
        await assert.rejects(
            prepareCdn({
                manifestPath: fixture.manifestPath,
                cdnDirectory: invalidLayoutDirectory,
                downloadDirectory: join(invalidLayoutDirectory, "cn", "parts"),
                skipDownload: true,
                logger: SILENT_LOGGER,
            }),
            /下载目录不能位于 CN CDN 目标目录内/,
        );

        const cdnDirectory = join(root, "cdn");
        const staleStagingDirectory = join(cdnDirectory, ".starpoint-cn-staging-2147483646-deadbeef");
        const staleFile = join(staleStagingDirectory, "partial.bin");
        await mkdir(staleStagingDirectory, { recursive: true });
        await writeFile(staleFile, "partial");
        const first = await prepareCdn({
            manifestPath: fixture.manifestPath,
            cdnDirectory,
            downloadDirectory: fixture.downloadDirectory,
            skipDownload: true,
            keepParts: true,
            logger: SILENT_LOGGER,
        });
        assert.equal(first.status, "prepared");
        await assert.rejects(readFile(staleFile), { code: "ENOENT" });
        assert.equal(
            await readFile(join(cdnDirectory, "cn", "entities", "fixture.csv"), "utf8"),
            "path,size,hash\nasset.bin,3,abc\n",
        );

        const validated = await validateCdn({ manifestPath: fixture.manifestPath, cdnDirectory });
        assert.equal(validated.status, "validated");

        await unlink(join(cdnDirectory, ".starpoint-cn-cdn.json"));
        await assert.doesNotReject(validateCdn({ manifestPath: fixture.manifestPath, cdnDirectory }));
        await assert.rejects(readFile(join(cdnDirectory, ".starpoint-cn-cdn.json")), { code: "ENOENT" });
        await runCommand(process.execPath, [
            PREPARER_PATH,
            "--manifest",
            fixture.manifestPath,
            "--cdn-dir",
            cdnDirectory,
            "--validate-existing",
        ]);
        await assert.rejects(readFile(join(cdnDirectory, ".starpoint-cn-cdn.json")), { code: "ENOENT" });
        await assert.rejects(
            prepareCdn({
                manifestPath: fixture.manifestPath,
                cdnDirectory,
                downloadDirectory: fixture.downloadDirectory,
                skipDownload: true,
                keepParts: true,
                logger: SILENT_LOGGER,
            }),
            /没有匹配的安装标记/,
        );
        const adopted = await prepareCdn({
            manifestPath: fixture.manifestPath,
            cdnDirectory,
            downloadDirectory: fixture.downloadDirectory,
            skipDownload: true,
            keepParts: true,
            adoptExisting: true,
            logger: SILENT_LOGGER,
        });
        assert.equal(adopted.status, "adopted-existing");

        await writeFile(
            join(cdnDirectory, "cn", "entities", "fixture.csv"),
            "path,size,hash\nasset.bin,3,abd\n",
        );
        await assert.rejects(
            validateCdn({ manifestPath: fixture.manifestPath, cdnDirectory }),
            /CDN 校验文件 SHA-256 不一致/,
        );

        const corruptDirectory = join(root, "corrupt-downloads");
        await mkdir(corruptDirectory, { recursive: true });
        for (const part of fixture.parts) {
            const content = await readFile(join(fixture.downloadDirectory, part.name));
            await writeFile(join(corruptDirectory, part.name), content);
        }
        await writeFile(join(corruptDirectory, fixture.parts[0].name), Buffer.from("corrupt", "utf8"));
        await assert.rejects(
            prepareCdn({
                manifestPath: fixture.manifestPath,
                cdnDirectory: join(root, "corrupt-cdn"),
                downloadDirectory: corruptDirectory,
                skipDownload: true,
                logger: SILENT_LOGGER,
            }),
            /CDN 分片大小不一致|CDN 分片 SHA-256 不一致/,
        );
        console.log("CN CDN preparer tests passed.");
    } finally {
        await rm(root, { recursive: true, force: true });
    }
}
// //// /验证 CDN 准备完整生命周期 ////

await run();
