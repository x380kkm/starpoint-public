// audience: internal
// # personal-service-activity-views-tests
//
// 该文件验证活动状态筛选, 日期筛选和临时开放的时间语义.

import assert from "node:assert/strict"
import { readFile } from "node:fs/promises"

const source = await readFile(new URL("./activity-views.js", import.meta.url), "utf8")
const moduleUrl = `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`
const { filterActivitiesByDate } = await import(moduleUrl)

const ended = {
    activity_id: "story:1",
    default_start_at_ms: Date.parse("2023-01-01T00:00:00.000Z"),
    default_end_at_ms: Date.parse("2023-01-02T00:00:00.000Z"),
    status: "ended",
}
const open = {
    activity_id: "story:2",
    default_start_at_ms: Date.parse("2026-08-21T00:00:00.000Z"),
    default_end_at_ms: Date.parse("2026-08-23T00:00:00.000Z"),
    status: "open",
}
const temporaryOpen = {
    activity_id: "gacha:61",
    default_start_at_ms: Date.parse("2023-01-01T00:00:00.000Z"),
    default_end_at_ms: Date.parse("2023-01-02T00:00:00.000Z"),
    status: "open",
    temporary_open_until_ms: Date.parse("2026-08-22T12:00:00.000Z"),
}
const permanent = {
    activity_id: "daily-week:19",
    mode: "always",
    status: "open",
}

const endedResult = filterActivitiesByDate({
    dateFrom: "2026-08-21",
    dateTo: "2026-08-21",
    serverTimeMs: Date.parse("2026-08-21T12:00:00.000Z"),
    status: "ended",
}, [ended, open, temporaryOpen, permanent])
assert.deepEqual(endedResult.map((activity) => activity.activity_id), ["story:1"])

const dateResult = filterActivitiesByDate({
    dateFrom: "2026-08-21",
    dateTo: "2026-08-21",
    serverTimeMs: Date.parse("2026-08-21T12:00:00.000Z"),
    status: "",
}, [ended, open, temporaryOpen, permanent])
assert.deepEqual(dateResult.map((activity) => activity.activity_id), ["story:2", "gacha:61", "daily-week:19"])

const permanentResult = filterActivitiesByDate({
    dateFrom: "2099-01-01",
    dateTo: "2099-01-01",
    serverTimeMs: Date.parse("2026-08-21T12:00:00.000Z"),
    status: "",
}, [permanent])
assert.deepEqual(permanentResult.map((activity) => activity.activity_id), ["daily-week:19"])
