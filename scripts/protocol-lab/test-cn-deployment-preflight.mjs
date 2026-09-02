// audience: internal
// # cn-deployment-preflight-test
// 此测试在临时仓库中执行当前平台的 CN 部署入口, 确认只读预检不会创建环境文件、构建或启动服务.

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { access, cp, mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SOURCE_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

// //// 计算测试资产的 SHA-256 [@x380kkm 2026-07-27] ////
function sha256(content) {
    return createHash("sha256").update(content).digest("hex");
}
// //// /计算测试资产的 SHA-256 ////

// //// 选择当前平台可执行的部署入口 [@x380kkm 2026-07-27] ////
function getRunnerCommands(repositoryRoot) {
    const deploymentDirectory = join(repositoryRoot, "deployment", "cn");
    if (process.platform === "win32") {
        const commands = [
            {
                command: "powershell",
                args: ["-NoProfile", "-File", join(deploymentDirectory, "run.ps1"), "-ValidateOnly"],
            },
        ];
        const gitBashPath = join(process.env.ProgramFiles ?? "C:\\Program Files", "Git", "bin", "bash.exe");
        if (existsSync(gitBashPath)) {
            commands.push({
                command: gitBashPath,
                args: ["-lc", "deployment/cn/run.sh --validate-only"],
            });
        }
        return commands;
    }
    return [{ command: "sh", args: [join(deploymentDirectory, "run.sh"), "--validate-only"] }];
}
// //// /选择当前平台可执行的部署入口 ////

// //// 执行部署入口并验证成功退出 [@x380kkm 2026-07-27] ////
async function runCommand(command, args, cwd) {
    const child = spawn(command, args, { cwd, stdio: ["ignore", "ignore", "pipe"] });
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
// //// /执行部署入口并验证成功退出 ////

// //// 验证预检未创建路径 [@x380kkm 2026-07-27] ////
async function assertPathMissing(path) {
    await assert.rejects(access(path), { code: "ENOENT" });
}
// //// /验证预检未创建路径 ////

// //// 构造只读部署预检仓库 [@x380kkm 2026-07-27] ////
async function createRunnerFixture(root) {
    const repositoryRoot = join(root, "repository");
    const deploymentDirectory = join(repositoryRoot, "deployment", "cn");
    const entityDirectory = join(repositoryRoot, ".cdn", "cn", "entities");
    const entityContent = Buffer.from("path,size,hash\nasset.bin,3,abc\n", "utf8");
    const partContent = Buffer.from([1]);
    await mkdir(deploymentDirectory, { recursive: true });
    await mkdir(entityDirectory, { recursive: true });
    await writeFile(join(entityDirectory, "fixture.csv"), entityContent);
    await cp(join(SOURCE_ROOT, "deployment", "cn", "prepare-cdn.mjs"), join(deploymentDirectory, "prepare-cdn.mjs"));
    await cp(join(SOURCE_ROOT, "deployment", "cn", "run.ps1"), join(deploymentDirectory, "run.ps1"));
    await cp(join(SOURCE_ROOT, "deployment", "cn", "run.sh"), join(deploymentDirectory, "run.sh"));
    const manifest = {
        schema: "wf-cn-cdn-release",
        version: 1,
        resVersion: "test",
        repo: "fixture/repository",
        tag: "fixture",
        archiveName: "fixture.tar",
        topLevelDir: "cn",
        extractedFileCount: 1,
        extractedSize: entityContent.length,
        requiredFiles: [
            {
                path: "entities/fixture.csv",
                size: entityContent.length,
                sha256: sha256(entityContent),
            },
        ],
        partCount: 1,
        totalSize: partContent.length,
        parts: [
            {
                name: "fixture.part.00",
                size: partContent.length,
                sha256: sha256(partContent),
            },
        ],
    };
    await writeFile(join(deploymentDirectory, "cdn-manifest.json"), `${JSON.stringify(manifest)}\n`, "utf8");
    return repositoryRoot;
}
// //// /构造只读部署预检仓库 ////

// //// 验证部署入口只执行 CDN 预检 [@x380kkm 2026-07-27] ////
async function run() {
    const root = await mkdtemp(join(tmpdir(), "starpoint-cn-deployment-"));
    try {
        const repositoryRoot = await createRunnerFixture(root);
        const runners = getRunnerCommands(repositoryRoot);
        for (const runner of runners) {
            await runCommand(runner.command, runner.args, repositoryRoot);
            await assertPathMissing(join(repositoryRoot, ".env.cn"));
        }
        await assertPathMissing(join(repositoryRoot, "out"));
        console.log(`CN ${process.platform} deployment preflight tests passed: ${runners.length} runner(s).`);
    } finally {
        await rm(root, { recursive: true, force: true });
    }
}
// //// /验证部署入口只执行 CDN 预检 ////

run().catch((error) => {
    console.error(error);
    process.exitCode = 1;
});
