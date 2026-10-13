// audience: internal
// # cn-audit-coverage-tests
// 实际主表验证活动关卡和任务筛选范围, 缺失成员与不相交阶段验证审计失败路径.

import assert from "node:assert/strict"
import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { assertArchiveEntries, assertQuestCoverage } from "./audit-cn-activity-master-projection.mjs"
import { auditParentLinks, readMasterRecords } from "./audit-cn-activity-schedules.mjs"
import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..")
const projectionPath = path.join(root, "core/personal-service/assets/cn-activity-master-projection.json")
const projection = JSON.parse(fs.readFileSync(projectionPath, "utf8"))

// //// 验证同一归档的所有预期成员 [@x380kkm 2026-09-28] ////
const entries = ["first", "last"].map((name) => ({ path: name, type: "File", uncompressedSize: 1 }))
assert.equal(assertArchiveEntries("archive", entries, new Set(["first", "last"])).length, 2)
assert.throws(() => assertArchiveEntries("archive", [entries[1]], new Set(["first", "last"])), /entry missing: archive\/first/)
// //// /验证同一归档的所有预期成员 ////

// //// 验证完整活动关卡集合与结算目录的对应关系 [@x380kkm 2026-09-28] ////
const masters = projection.masters.filter((master) => Number.isInteger(master.quest_category))
const decoded = new Map(masters.map((master) => [master.name,
    decodeOrderedMap(fs.readFileSync(path.resolve(path.dirname(projectionPath), master.binary_path)))]))
const battle = JSON.parse(fs.readFileSync(path.join(root, "core/personal-service/assets/cn-single-battle.json"), "utf8"))
const questCount = assertQuestCoverage(masters, decoded, battle)
delete battle.quests["11:1001"]
assert.throws(() => assertQuestCoverage(masters, decoded, battle), /settlement catalog: ranking_event_single_quest\/11:1001/)
// //// /验证完整活动关卡集合与结算目录的对应关系 ////

// //// 验证共享任务主表的活动类别筛选 [@x380kkm 2026-09-28] ////
const missionMaster = projection.masters.find((master) => master.name === "event_mission")
const missions = readMasterRecords(projectionPath, { masters: [missionMaster] })
assert.deepEqual(missions.issues, [])
const missionRoots = missions.records.get("event_mission").roots
assert.deepEqual([...missionRoots.keys()].sort(), ["1", "2", "3", "4", "5", "6", "7"])
assert.equal([...missionRoots.values()].reduce((count, record) => count + record.row_count, 0), 96)
// //// /验证共享任务主表的活动类别筛选 ////

// //// 每个子阶段分别匹配父活动窗口 [@x380kkm 2026-09-28] ////
const definition = { name: "child", parent_activity_master: "parent" }
const record = (windows) => ({ activity_id: "advent:1", windows, malformed_windows: [] })
const parentAudit = auditParentLinks({ masters: [definition] }, new Map([
    ["parent", { roots: new Map([["1", record([{ start_at_ms: 0, end_at_ms: 20 }])]]) }],
    ["child", { roots: new Map([["1", record([
        { start_at_ms: 5, end_at_ms: 15 }, { start_at_ms: 30, end_at_ms: 40 },
    ])]]) }],
]))
assert.equal(parentAudit.issues.length, 1)
assert.equal(parentAudit.issues[0].type, "child_parent_window_disjoint")
assert.deepEqual(parentAudit.issues[0].child_windows, [{ start_at_ms: 30, end_at_ms: 40 }])
// //// /每个子阶段分别匹配父活动窗口 ////

console.log(JSON.stringify({ archive_members: 2, activity_quests: questCount, raid_mission_rows: 96, rejected_cases: 3 }))
