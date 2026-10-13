// audience: internal
// # personal-service-activity-history-view-tests
// 该文件验证存档活动切换的异步隔离和明确确认后的重置目标.

import assert from "node:assert/strict"
import { readFile } from "node:fs/promises"
import test from "node:test"

const source = await readFile(new URL("./activity-history.js", import.meta.url), "utf8")
const { createActivityHistory, historySummary } = await import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`)

function element() {
    return {
        value: "", textContent: "", disabled: false, hidden: false, children: [], events: {},
        selectedOptions: [{ textContent: "存档 A" }],
        addEventListener(name, handler) { this.events[name] = handler },
        replaceChildren(...children) { this.children = children },
        append(...children) { this.children.push(...children) },
    }
}

function harness(requestApi, confirm) {
    const controls = Object.fromEntries(["save", "state", "list", "refresh", "more", "reset"].map((key) => [key, element()]))
    const errors = []
    const controller = createActivityHistory({
        requestApi, confirm,
        document: { querySelector: (id) => controls[id.replace("#activity-history-", "")], createElement: element },
        runAction: async (control, operation) => {
            control.disabled = true
            try { await operation() } catch (error) { errors.push(error) }
            finally { control.disabled = false }
        },
    })
    return { controls, controller, errors }
}

test("明确确认后仅重置选中的存档和活动轮次", async () => {
    const calls = []
    let consent = false
    let round = 2
    const { controls, controller } = harness(async (path, options) => {
        calls.push({ path, options })
        if (options?.method === "POST") { assert.deepEqual(options.body, { expected_round: 2 }); round = 3; return {} }
        return { participation_round: round, can_reset: true, entries: [], next_before_id: null }
    }, () => consent)
    await controller.setSaves([{ id: 7, name: "存档 A" }, { id: 8, name: "存档 B" }])
    await controller.selectActivity({ activity_id: "ranking:1", name: "竞速" })
    assert.equal(calls.length, 0)
    controls.save.value = "7"
    await controls.save.events.change()
    await controls.reset.events.click()
    assert.equal(calls.filter((call) => call.options?.method === "POST").length, 0)
    consent = true
    await controls.reset.events.click()
    assert.equal(calls.find((call) => call.options?.method === "POST").path, "/v1/local-saves/7/activities/ranking%3A1/reset")
    assert.match(controls.state.textContent, /第 4 轮/)
})

test("切换活动后忽略旧历史响应且读取失败时关闭重置", async () => {
    let releaseOld
    const { controls, controller, errors } = harness(async (path) => {
        if (path.includes("ranking%3A1")) return new Promise((resolve) => { releaseOld = resolve })
        throw new Error("读取失败")
    }, () => true)
    await controller.setSaves([{ id: 7, name: "存档 A" }])
    controls.save.value = "7"
    const first = controller.selectActivity({ activity_id: "ranking:1", name: "竞速 A" })
    await controller.selectActivity({ activity_id: "ranking:2", name: "竞速 B" })
    releaseOld({ participation_round: 50, can_reset: true, entries: [], next_before_id: null })
    await first
    assert.equal(errors.length, 1)
    assert.equal(controls.reset.disabled, true)
    assert.doesNotMatch(controls.state.textContent, /51/)
})

test("嘉年华与竞速统计分别呈现成绩并显示邮件实际领取状态", () => {
    assert.match(historySummary({ kind: "summary", details: { records: [{ best_score: 100 }, { best_score: 200 }] } }), /总最佳分 300/)
    assert.match(historySummary({ kind: "summary", details: { best_record: { elapsed_time_ms: 40000 }, rank_percentage: 12.5 } }), /40.000 秒, 排名 12.50%/)
    assert.match(historySummary({ kind: "delivery", details: { title: "竞速奖励" }, received_at: null }), /待领取/)
    assert.match(historySummary({ kind: "delivery", details: { title: "竞速奖励" }, received_at: 1700000000 }), /已领取/)
})

test("各期原生成绩与原有通关记录显示各自字段", () => {
    const statistics = (best_record) => historySummary({ kind: "statistics", details: { best_record } })
    assert.match(statistics({ elapsed_time_ms: 60000, is_accomplished: true, score: 100 }), /最佳 60.000 秒, 得分 100/)
    assert.match(statistics({ elapsed_time_ms: 30000, is_accomplished: false, score: 777 }), /最佳 未通关, 得分 777/)
    assert.match(statistics({ elapsed_time_ms: 30000, high_score: 100, kill_count: 1 }), /最佳 30.000 秒, 最高分 100/)
    assert.match(statistics({ best_elapsed_time_ms: 10000, high_score: 999 }), /最佳 10.000 秒, 最高分 999/)
})
