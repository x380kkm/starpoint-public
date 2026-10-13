// audience: internal
// # personal-service-competition-controller-tests
// 操作验证覆盖选定存档, 人工结算确认, 期别校验参数和异步切换隔离.

import assert from "node:assert/strict"
import { readFile } from "node:fs/promises"
import test from "node:test"

const viewsSource = await readFile(new URL("./competition-views.js", import.meta.url), "utf8")
const viewsUrl = `data:text/javascript;base64,${Buffer.from(viewsSource).toString("base64")}`
const source = (await readFile(new URL("./competition-controller.js", import.meta.url), "utf8")).replace('"./competition-views.js"', JSON.stringify(viewsUrl))
const { createCompetitionController } = await import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`)
const { selectedCompetitionTiers } = await import(viewsUrl)
const page = await readFile(new URL("./index.html", import.meta.url), "utf8")

function element() {
    return { value: "", textContent: "", disabled: false, hidden: false, children: [], events: {},
        addEventListener(name, handler) { this.events[name] = handler },
        replaceChildren(...children) { this.children = children },
        append(...children) { this.children.push(...children) },
    }
}

function entry(overrides = {}) {
    return { activity_id: "ranking:1", family: "ranking", event_id: 1, name: "竞速挑战", participation_round: 2, period_start_ms: null,
        has_record: true, summary: { best_record: { elapsed_time_ms: 12345, is_accomplished: true, score: 55 }, rank_percentage: 25 },
        received: false, reward_mode: "automatic", suggested_tier_id: 1, availability: { status: "open" },
        tiers: [{ id: 1, label: "前 25%", rewards: [{ label: "星导石", amount: 150 }] }], ...overrides }
}

function harness(requestApi, confirm = () => true) {
    const controls = {}
    const selections = []
    const errors = []
    const controller = createCompetitionController({ requestApi, confirm,
        document: {
            querySelector(id) {
                assert.ok(page.includes(`id="${id.slice(1)}"`), `页面缺少控件 ${id}`)
                return controls[id.replace("#competition-", "")] ??= element()
            },
            createElement: element,
        },
        onSelect: (activity, slot) => selections.push({ activity, slot }),
        runAction: async (control, operation) => {
            if (control) control.disabled = true
            try { return await operation() } catch (error) { errors.push(error) }
            finally { if (control) control.disabled = false }
        },
    })
    return { controls, controller, selections, errors }
}

test("人工发奖确认使用当前存档和期别, 已领后关闭重复操作", async () => {
    const calls = []
    let consent = false
    let received = false
    const state = harness(async (path, options) => {
        calls.push({ path, options })
        if (path.endsWith("/settle")) { received = true; return { status: "delivered" } }
        return { slot_name: "存档 A", entries: [entry({ received })] }
    }, () => consent)
    await state.controller.setSaves([{ id: 7, name: "存档 A" }], 7)
    assert.equal(calls.length, 0)
    await state.controller.activate()
    await state.controls.settle.events.click()
    assert.equal(calls.filter(call => call.options?.method === "POST").length, 0)
    consent = true
    await state.controls.settle.events.click()
    const post = calls.find(call => call.options?.method === "POST")
    assert.equal(post.path, "/v1/local-saves/7/competitions/ranking%3A1/settle")
    assert.deepEqual(post.options.body, { expected_round: 2, expected_period_start_ms: null })
    assert.equal(state.controls.settle.disabled, true)
    assert.deepEqual(state.errors, [])
})

test("全服统计与手动发奖方式分别提交并携带期别", async () => {
    const calls = []
    const raid = entry({ family: "raid", activity_id: "raid:20002", event_id: 20002, period_start_ms: 1750000000000,
        has_record: false, global_state: { hp_percentage: 50, total_kill_count: 10 }, summary: { received_up_to: 1 },
        tiers: [{ id: 1, label: "讨伐奖励", pending_count: 3, rewards: [{ label: "素材", amount: 2 }] }] })
    const state = harness(async (path, options) => {
        calls.push({ path, options })
        return options ? {} : { slot_name: "存档 A", entries: [raid] }
    })
    await state.controller.setSaves([{ id: 7, name: "存档 A" }], 7)
    await state.controller.activate()
    assert.equal(state.controls.settle.disabled, false)
    assert.match(state.controls.preview.children[0].children[1].textContent, /x6/)
    state.controls.policy.value = "manual"
    await state.controls["policy-save"].events.click()
    const policy = calls.find(call => call.path.endsWith("/reward-mode"))
    assert.deepEqual(policy.options, { method: "PUT", body: { mode: "manual" } })
    state.controls.hp.value = "40"
    state.controls.kills.value = "25"
    await state.controls["global-form"].events.submit({ preventDefault() {} })
    const stats = calls.find(call => call.path.endsWith("/server-state"))
    assert.deepEqual(stats.options.body, { expected_period_start_ms: 1750000000000, hp_percentage: 40, total_kill_count: 25 })
    assert.equal(calls.some(call => call.path.endsWith("/settle")), false)
})

test("切换存档后旧响应无法覆盖新活动或解除失败状态", async () => {
    let release
    const state = harness(async path => {
        if (path.includes("/7/")) return new Promise(resolve => { release = resolve })
        throw new Error("请求失败")
    })
    await state.controller.setSaves([{ id: 7, name: "存档 A" }, { id: 8, name: "存档 B" }], 7)
    const loading = state.controller.activate()
    state.controls.save.value = "8"
    await state.controls.save.events.change()
    release({ slot_name: "存档 A", entries: [entry()] })
    await loading
    assert.equal(state.controls.detail.hidden, true)
    assert.equal(state.controls.settle.disabled, true)
    assert.match(state.controls.state.textContent, /读取失败/)
    assert.equal(state.errors.length, 1)
})

test("新存档加载过程中检索不会恢复上一个存档的发奖目标", async () => {
    let release
    const state = harness(async path => path.includes("/7/")
        ? { slot_name: "存档 A", entries: [entry()] }
        : new Promise(resolve => { release = resolve }))
    await state.controller.setSaves([{ id: 7, name: "存档 A" }, { id: 8, name: "存档 B" }], 7)
    await state.controller.activate()
    state.controls.save.value = "8"
    const pending = state.controls.save.events.change()
    state.controls.search.events.input()
    assert.equal(state.controls.detail.hidden, true)
    assert.equal(state.controls.settle.disabled, true)
    release({ slot_name: "存档 B", entries: [entry({ has_record: false })] })
    await pending
    assert.equal(state.controls.settle.disabled, true)
})

test("活动检索保留准确目标, 奖励选择覆盖真实排名区间", async () => {
    const rush = entry({ family: "rush", activity_id: "rush:700001", name: "狂热激战", suggested_rank_number: 1,
        tiers: [{ id: 2, label: "第 1 至 10 名", rank_from: 1, rank_to: 10, rewards: [] }] })
    const state = harness(async () => ({ slot_name: "存档 A", entries: [entry(), rush] }))
    await state.controller.setSaves([{ id: 7, name: "存档 A" }], 7)
    await state.controller.activate()
    state.controls.family.value = "rush"
    state.controls.family.events.change()
    assert.equal(state.controls.event.value, "rush:700001")
    assert.equal(state.controls["rush-options"].hidden, false)
    assert.equal(selectedCompetitionTiers(rush, "", 5).length, 1)
    assert.equal(selectedCompetitionTiers(rush, "", 0).length, 0)
    state.controls.search.value = "找不到"
    state.controls.search.events.input()
    assert.equal(state.controls.detail.hidden, true)
})
