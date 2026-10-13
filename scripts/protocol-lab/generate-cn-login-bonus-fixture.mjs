// audience: internal
// # generate-cn-login-bonus-fixture
// 登录奖励使用当前 CN 主表的分组, 日期, 顺序和七个奖励槽位.

import fs from "node:fs"
import path from "node:path"
import { parseArgs } from "node:util"
import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"
import { readEntityRecords, readMasterBuffers } from "./generate-cn-activity-catalog.mjs"
import { parseCnMasterTimestamp } from "./cn-master-time.mjs"

// //// 读取登录主表并投影奖励和开放条件 [@x380kkm 2026-09-22] ////
const { values } = parseArgs({ options: { "cdn-root": { type: "string" }, output: { type: "string" } } })
const cdnRoot = path.resolve(values["cdn-root"] ?? ".cdn/cn")
const output = path.resolve(values.output ?? "core/personal-service/assets/cn-login-bonus.json")
const buffers = await readMasterBuffers(cdnRoot, readEntityRecords(cdnRoot), [
    { name: "login_bonus", logicalPath: "master/bonus/login_bonus.orderedmap" },
])
const binary = buffers.get("login_bonus")
if (!binary) throw new Error("CN login bonus master is missing")
const groups = Object.fromEntries(Object.entries(decodeOrderedMap(binary)).map(([id, records]) => [id,
    Object.entries(records).map(([index, row]) => ({
        index: Number(index),
        group_type: Number(row[0]),
        name: row[3] === "(None)" ? null : row[3],
        start_time: seconds(row[41]),
        end_time: seconds(row[42]),
        condition_start_time: seconds(row[38]),
        condition_end_time: seconds(row[39]),
        comeback_days: optionalNumber(row[40]),
        annual_start: optionalText(row[43]),
        annual_end: optionalText(row[44]),
        reason_id: Number(row[45]),
        comeback_group: optionalText(row[46]),
        include_beginner: optionalText(row[47]) === "true",
        rewards: Array.from({ length: 7 }, (_, slot) => {
            const offset = 6 + slot * 4
            if (row[offset] === "(None)") return null
            const kind = Number(row[offset])
            const count = Number(row[offset + 1])
            if (!Number.isSafeInteger(count) || count < 1 || ![0, 1, 2, 3, 4].includes(kind)) {
                throw new Error(`Invalid login reward ${id}:${index}:${slot}`)
            }
            return { type: [3, 0, 2, 4, 5][kind], count,
                id: kind === 1 ? Number(row[offset + 2]) : kind === 2 ? Number(row[offset + 3]) : null }
        }).filter(Boolean),
    })).sort((left, right) => left.index - right.index),
]))
fs.writeFileSync(output, `${JSON.stringify({ groups }, null, 2)}\n`, "utf8")
console.log(JSON.stringify({ groups: Object.keys(groups).length, rows: Object.values(groups).reduce((sum, rows) => sum + rows.length, 0), output }))

function optionalText(value) { return value === "(None)" || value === "" ? null : value }
function optionalNumber(value) { const text = optionalText(value); return text === null ? null : Number(text) }
function seconds(value) { const parsed = parseCnMasterTimestamp(value); return parsed === null ? null : Math.floor(parsed / 1000) }
// //// /读取登录主表并投影奖励和开放条件 ////
