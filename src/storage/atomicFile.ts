// audience: internal
// # atomic-file
// 此模块把完整内容写入同目录临时文件后再原子替换目标文件.

import { randomBytes } from "crypto"
import { promises as fs } from "fs"
import path from "path"

// //// 原子写入 JSON 文件 [@x380kkm 2026-07-22] ////
export async function writeJsonAtomic(filePath: string, value: unknown): Promise<void> {
    await fs.mkdir(path.dirname(filePath), { recursive: true })
    const temporaryPath = `${filePath}.${randomBytes(6).toString("hex")}.tmp`
    await fs.writeFile(temporaryPath, `${JSON.stringify(value, null, 2)}\n`, "utf8")
    await fs.rename(temporaryPath, filePath)
}
// //// /原子写入 JSON 文件 ////
