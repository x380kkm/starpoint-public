// audience: internal
// # personal-service-activity-controller-tests
//
// 该文件验证活动筛选日期和临时开放动作.

import assert from "node:assert/strict"
import { readFile } from "node:fs/promises"

let renderedActions
globalThis.__renderActivityCatalog = (_state, _elements, actions) => {
    renderedActions = actions
}
globalThis.__renderActivityDetail = () => {}

const source = (await readFile(new URL("./activity-controller.js", import.meta.url), "utf8"))
    .replace(
        'import { renderActivityCatalog, renderActivityDetail } from "/manage/activity-views.js"',
        "const renderActivityCatalog = globalThis.__renderActivityCatalog\nconst renderActivityDetail = globalThis.__renderActivityDetail",
    )
const moduleUrl = `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`
const { createActivityController } = await import(moduleUrl)

function control() {
    return {
        addEventListener() {},
        checked: false,
        disabled: false,
        scrollIntoView() {},
        value: "",
    }
}

const elements = Object.fromEntries([
    "activityCalendarNext",
    "activityCalendarNextYear",
    "activityCalendarPrevious",
    "activityCalendarPreviousYear",
    "activityCalendarToday",
    "activityCatalogRefresh",
    "activityDateClear",
    "activityDateFrom",
    "activityDateTo",
    "activityFavoriteFilter",
    "activityKindFilter",
    "activityMode",
    "activityModeForm",
    "activityPeriodForm",
    "activityPeriodInterval",
    "activityPeriodKind",
    "activityReset",
    "activitySearch",
    "activityStatusFilter",
    "activityWindowEnd",
    "activityWindowForm",
    "activityWindowStart",
].map((name) => [name, control()]))

const serverTimeMs = Date.parse("2026-08-21T12:00:00.000Z")
const requests = []
const requestApi = async (path, options = {}) => {
    requests.push({ path, options })
    return {
        activities: [{
            activity_id: "story:1",
            default_end_at_ms: Date.parse("2023-01-02T00:00:00.000Z"),
            default_start_at_ms: Date.parse("2023-01-01T00:00:00.000Z"),
            status: "ended",
        }],
        server_time_ms: serverTimeMs,
    }
}
class ApiError extends Error {}
const controller = createActivityController({
    ApiError,
    elements,
    requestApi,
    runAction: async (_control, action) => action(),
})

await controller.load()
assert.equal(elements.activityDateFrom.value, "2026-08-21")
assert.equal(elements.activityDateTo.value, "2026-08-21")

await renderedActions.temporaryActivityAction({ activity_id: "gacha:61" }, true)
assert.deepEqual(requests.at(-2), {
    path: "/v1/activities/gacha%3A61/temporary-open",
    options: { method: "POST", body: {} },
})
await renderedActions.temporaryActivityAction({ activity_id: "gacha:61" }, false)
assert.deepEqual(requests.at(-2), {
    path: "/v1/activities/gacha%3A61/temporary-open",
    options: { method: "DELETE", body: undefined },
})

renderedActions.setActivityDate("2023-01-01")
assert.equal(elements.activityDateFrom.value, "2023-01-01")
await renderedActions.setActivityStatusFilter("ended")
assert.equal(elements.activityDateFrom.value, "2026-08-21")
assert.equal(elements.activityDateTo.value, "2026-08-21")
