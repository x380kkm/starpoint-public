// audience: internal
// # management-save-transfer-tests
// 隔离 DOM 回放存档表单的实际事件, 请求目标, 调度参数和条件恢复响应.

import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { fileURLToPath } from "node:url"
import test from "node:test"
import vm from "node:vm"
import ts from "typescript"

// //// 为页面模块提供隔离文档和请求边界 [@x380kkm 2026-09-26] ////
function harness(requestApi = async () => ({})) {
    const document = { createElement: (tag) => new Element(tag) }
    class Element {
        constructor(tag) {
            this.tagName = tag.toUpperCase(); this.ownerDocument = document
            this.children = []; this.events = {}; this.value = ""; this.textContent = ""
            this.disabled = false; this.hidden = false; this.checked = false
            this.classList = { toggle() {} }
        }
        append(...children) { for (const child of children) { child.parentElement = this; this.children.push(child) } }
        replaceChildren(...children) { this.children = []; this.append(...children) }
        addEventListener(name, handler) { this.events[name] = handler }
        setAttribute(name, value) { this[name] = value }
        focus() {}
        select() {}
    }
    const exports = new Map()
    const messages = []
    const errors = []
    let refreshes = 0
    const actions = {
        requestApi, confirm: () => true, describeError: (error) => error,
        refreshManagementState: async () => { refreshes++ },
        runAction: async (control, action) => {
            if (control) control.disabled = true
            try { const message = await action(); if (message) messages.push(message) }
            catch (error) { errors.push(error) }
            finally { if (control) control.disabled = false }
        },
    }
    function load(name) {
        const url = new URL(name, import.meta.url)
        if (exports.has(url.href)) return exports.get(url.href)
        const result = {}
        exports.set(url.href, result)
        const source = readFileSync(url, "utf8")
        const code = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2017, module: ts.ModuleKind.CommonJS } }).outputText
        vm.runInNewContext(code, { exports: result, require: load, document, window: { confirm: actions.confirm }, setTimeout }, { filename: fileURLToPath(url) })
        return result
    }
    return { document, actions, load, messages, errors, get refreshes() { return refreshes } }
}

function all(root) { return [root, ...root.children.flatMap(all)] }
function field(root, name) { return all(root).find((element) => element.name === name) }
function button(root, text) { return all(root).find((element) => element.tagName === "BUTTON" && element.textContent === text) }
function form(root) { return all(root).find((element) => element.tagName === "FORM") }
function submit(root) { return form(root).events.submit({ preventDefault() {} }) }
function plain(value) { return JSON.parse(JSON.stringify(value)) }
const slot = { id: 7, name: "本机存档" }
const instanceId = "a".repeat(32)
const tokenId = "b".repeat(32)
const targetToken = "slot.authorization"
const model = () => ({ profiles: { profiles: [{ id: 3, name: "另一台设备", mode: "remote" }] }, transferBindings: new Map(), transferConflicts: new Map() })
// //// /为页面模块提供隔离文档和请求边界 ////

// //// 从空配置创建正确的槽授权和传输绑定 [@x380kkm 2026-09-26] ////
test("本机默认配置仍展示首个备份目标的添加面板", () => {
    const h = harness()
    const parent = h.document.createElement("div")
    const elements = Object.fromEntries(["remoteSavePanel", "remotePlayerAccessPanel", "remoteRestorePanel"].map((name) => [name, h.document.createElement("section")]))
    parent.append(elements.remoteSavePanel)
    h.load("./views.js").renderRemotePanels({ profiles: { profiles: [{ mode: "local" }] }, targets: [] }, elements)
    assert.equal(elements.remoteSavePanel.hidden, false)
    assert.equal(elements.remotePlayerAccessPanel.hidden, true)
    assert.equal(elements.remoteRestorePanel.hidden, true)
})

test("签发和撤销使用当前槽接口并交付完整目标身份", async () => {
    const calls = []
    let revoked = false
    const metadata = { id: tokenId, slotId: 7, permission: "both", deviceName: "手机", createdAt: "2026-09-26T00:00:00Z", expiresAt: null, revokedAt: null }
    const h = harness(async (path, options) => {
        calls.push({ path, options })
        if (options && options.method === "POST") return { token: targetToken, tokenType: "slot", instanceId, metadata }
        if (options && options.method === "DELETE") { revoked = true; return { revoked: true } }
        return { instanceId, tokens: [{ ...metadata, revokedAt: revoked ? "2026-09-26T01:00:00Z" : null }] }
    })
    const root = h.document.createElement("div")
    h.load("./save-access.js").renderSlotAccess(slot, root, h.actions)
    field(root, "permission").value = "both"
    field(root, "device_name").value = "手机"
    await submit(root)
    assert.deepEqual(plain(calls[0]), { path: "/v1/local-saves/7/transfer-tokens/slot", options: { method: "POST", body: { permission: "both", deviceName: "手机" } } })
    const secret = all(root).find((element) => element.tagName === "TEXTAREA")
    assert.deepEqual(JSON.parse(secret.value), { target_instance_kind: "local", target_instance_id: instanceId, target_slot_id: 7, target_token: targetToken })
    await button(root, "撤销槽授权").events.click()
    assert.equal(calls.find((call) => call.options && call.options.method === "DELETE").path, `/v1/local-saves/7/transfer-tokens/slot/${tokenId}`)
    assert.equal(all(root).some((element) => element.tagName === "TEXTAREA"), false)
    assert.equal(h.errors.length, 0)
})

test("创建绑定提交用户选择的定时模式并清除授权输入", async () => {
    const calls = []
    const h = harness(async (path, options) => { calls.push({ path, options }); return options.body })
    const root = h.document.createElement("div")
    h.load("./save-transfers.js").renderTransferBindingCreate(slot, model(), root, h.actions)
    assert.deepEqual(field(root, "upload_mode").children.map((child) => child.value), ["manual", "interval"])
    field(root, "authorization").value = JSON.stringify({ target_instance_kind: "local", target_instance_id: instanceId, target_slot_id: 8, target_token: targetToken })
    await button(root, "填入授权资料").events.click()
    field(root, "upload_mode").value = "interval"
    field(root, "upload_mode").events.change()
    field(root, "pull_mode").value = "interval"
    field(root, "interval_seconds").value = "120"
    await submit(root)
    assert.deepEqual(plain(calls[0]), { path: "/v1/local-saves/7/transfer-bindings", options: { method: "POST", body: { target_profile_id: 3, target_instance_kind: "local", target_instance_id: instanceId, target_slot_id: 8, target_token: targetToken, upload_mode: "interval", pull_mode: "interval", conflict_policy: "ask", interval_seconds: 120, enabled: true } } })
    assert.equal(field(root, "target_token").value, "")
    assert.match(h.messages.at(-1), /间隔 120 秒/)
    assert.equal(h.refreshes, 1)
})
// //// /从空配置创建正确的槽授权和传输绑定 ////

// //// 保留现有绑定身份并编辑真实调度参数 [@x380kkm 2026-09-26] ////
test("编辑现有绑定保留连接式值并允许改为定时和更新槽授权", async () => {
    const calls = []
    const h = harness(async (path, options) => { calls.push({ path, options }); return options.body })
    const root = h.document.createElement("div")
    const state = model()
    state.transferBindings.set(7, [{ binding_id: tokenId, target: { profile_id: 3, instance_kind: "local", instance_id: instanceId, slot_id: 8 }, upload_mode: "on_connect", pull_mode: "manual", interval_seconds: 900, conflict_policy: "ask", enabled: true }])
    h.load("./save-transfers.js").renderTransferBindings(slot, state, root, h.actions)
    assert.match(all(root).map((element) => element.textContent).join(" "), /当前实例手动执行/)
    assert.equal(field(root, "upload_mode").value, "on_connect")
    field(root, "upload_mode").value = "interval"
    field(root, "upload_mode").events.change()
    field(root, "interval_seconds").value = "300"
    field(root, "target_token").value = "replacement.slot.token"
    await submit(root)
    assert.deepEqual(plain(calls[0]), { path: `/v1/local-saves/7/transfer-bindings/${tokenId}`, options: { method: "PUT", body: { upload_mode: "interval", pull_mode: "manual", conflict_policy: "ask", interval_seconds: 300, enabled: true, target_token: "replacement.slot.token" } } })
    assert.match(h.messages.at(-1), /上传: 定时, 下载: 手动, 间隔 300 秒/)
})

test("手动绑定的启用状态保持手动表述且空授权不覆盖原值", async () => {
    const calls = []
    const h = harness(async (path, options) => { calls.push({ path, options }); return options.body })
    const root = h.document.createElement("div")
    const state = model()
    state.transferBindings.set(7, [{ binding_id: tokenId, target: { profile_id: 3, instance_kind: "local", instance_id: instanceId, slot_id: 8 }, upload_mode: "manual", pull_mode: "manual", interval_seconds: 900, conflict_policy: "ask", enabled: false }])
    h.load("./save-transfers.js").renderTransferBindings(slot, state, root, h.actions)
    assert.equal(button(root, "立即同步").disabled, true)
    await button(root, "启用绑定").events.click()
    assert.match(h.messages.at(-1), /上传: 手动, 下载: 手动/)
    assert.doesNotMatch(h.messages.at(-1), /自动传输/)
    assert.equal(Object.hasOwn(calls[0].options.body, "target_token"), false)
})
// //// /保留现有绑定身份并编辑真实调度参数 ////

// //// 恢复安全版本时保留当前存档的条件校验 [@x380kkm 2026-09-26] ////
test("安全版本与快照区分且恢复传递当前版本 ETag", async () => {
    const calls = []
    let rejectRestore = true
    const h = harness(async (path, options) => {
        calls.push({ path, options })
        if (options) { if (rejectRestore) throw new Error("save_revision_conflict"); return { restored: true } }
        return { current_revision_id: "current", revisions: [
            { id: "current", etag: "c".repeat(64), label: "Current state", created_at: "2026-09-26T01:00:00Z" },
            { id: "safety", etag: "d".repeat(64), label: "Before transfer upload", created_at: "2026-09-26T00:00:00Z" },
        ] }
    })
    const root = h.document.createElement("div")
    await h.load("./save-revisions.js").renderSaveRevisions(slot, root, h.actions)
    assert.match(all(root).map((element) => element.textContent).join(" "), /同步覆盖前/)
    assert.equal(all(root).filter((element) => element.tagName === "BUTTON").length, 1)
    await button(root, "恢复此版本").events.click()
    assert.equal(h.refreshes, 0)
    assert.match(h.errors[0].message, /save_revision_conflict/)
    assert.equal(h.messages.length, 0)
    rejectRestore = false
    await button(root, "恢复此版本").events.click()
    assert.deepEqual(plain(calls.at(-1)), { path: "/v1/local-saves/7/revisions/safety/restore", options: { method: "POST", headers: { "If-Match": `"${"c".repeat(64)}"` } } })
    assert.equal(h.refreshes, 1)
    assert.match(h.messages[0], /版本已恢复/)
})

test("管理请求实际携带版本恢复的 If-Match 头", async () => {
    const source = readFileSync(new URL("./app.js", import.meta.url), "utf8")
    const start = source.indexOf("async function requestApi(")
    const end = source.indexOf("async function runAction(", start)
    assert.ok(start > 0 && end > start)
    const code = ts.transpileModule(source.slice(start, end), { compilerOptions: { target: ts.ScriptTarget.ES2017 } }).outputText
    const calls = []
    const context = vm.createContext({ fetch: async (path, options) => { calls.push({ path, options }); return { ok: true, text: async () => "{}" } } })
    vm.runInContext(code, context)
    await context.requestApi("/v1/local-saves/7/revisions/safety/restore", { method: "POST", headers: { "If-Match": '"current-etag"' } })
    assert.equal(calls[0].options.headers["If-Match"], '"current-etag"')
})
// //// /恢复安全版本时保留当前存档的条件校验 ////
