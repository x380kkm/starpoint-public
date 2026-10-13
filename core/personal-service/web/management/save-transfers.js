// audience: internal
// # management-save-transfers
// 绑定表单对应服务器的传输模式和条件覆盖接口, 目标身份与槽授权分开输入.

import { actionButton, field, node, submitForm } from "./save-controls.js"

const UPLOAD_MODES = [["manual", "手动"], ["interval", "定时"]]
const PULL_MODES = [["manual", "手动"], ["interval", "定时"]]
const CONNECTION_MODES = { on_connect: "连接时 (当前实例手动执行)", on_disconnect: "断开时 (当前实例手动执行)" }
const POLICIES = [["ask", "询问并停止同步"], ["local_wins", "本地覆盖远端"], ["remote_wins", "远端覆盖本地"]]

// //// 呈现传输模式并校验调度参数 [@x380kkm 2026-09-26] ////
export function bindingModeSummary(binding) {
    if (!binding.enabled) return "绑定已关闭."
    const upload = UPLOAD_MODES.find(([value]) => value === binding.upload_mode)
    const pull = PULL_MODES.find(([value]) => value === binding.pull_mode)
    const interval = binding.upload_mode === "interval" || binding.pull_mode === "interval"
        ? `, 间隔 ${binding.interval_seconds} 秒` : ""
    return `绑定已启用, 上传: ${upload ? upload[1] : CONNECTION_MODES[binding.upload_mode] || binding.upload_mode}, 下载: ${pull ? pull[1] : CONNECTION_MODES[binding.pull_mode] || binding.pull_mode}${interval}.`
}

function modeFields(document, container, binding) {
    const uploadChoices = binding.upload_mode === "on_connect" ? [...UPLOAD_MODES, [binding.upload_mode, CONNECTION_MODES.on_connect]] : UPLOAD_MODES
    const pullChoices = binding.pull_mode === "on_disconnect" ? [...PULL_MODES, [binding.pull_mode, CONNECTION_MODES.on_disconnect]] : PULL_MODES
    const upload = field(document, container, "upload_mode", "上传时机", { choices: uploadChoices, value: binding.upload_mode })
    const pull = field(document, container, "pull_mode", "下载时机", { choices: pullChoices, value: binding.pull_mode })
    const conflict = field(document, container, "conflict_policy", "两端均变化时", { choices: POLICIES, value: binding.conflict_policy })
    const interval = field(document, container, "interval_seconds", "同步间隔 (秒)", { type: "number", min: 60, max: 2592000, step: 1, required: true, value: binding.interval_seconds })
    const enabled = field(document, container, "enabled", "启用绑定", { type: "checkbox" })
    enabled.checked = binding.enabled
    const update = () => { interval.disabled = upload.value !== "interval" && pull.value !== "interval" }
    upload.addEventListener("change", update)
    pull.addEventListener("change", update)
    update()
    return () => {
        const seconds = Number(interval.value)
        if (!Number.isInteger(seconds) || seconds < 60 || seconds > 2592000) throw new Error("同步间隔须为 60 到 2592000 的整数秒.")
        return { upload_mode: upload.value, pull_mode: pull.value, conflict_policy: conflict.value, interval_seconds: seconds, enabled: enabled.checked }
    }
}
// //// /呈现传输模式并校验调度参数 ////

// //// 创建指向明确目标槽的传输绑定 [@x380kkm 2026-09-26] ////
export function renderTransferBindingCreate(slot, model, container, actions) {
    const profiles = model.profiles.profiles.filter((profile) => profile.mode === "remote")
    const document = container.ownerDocument
    const section = node(document, "details", "form-drawer")
    section.append(node(document, "summary", "", "创建槽位绑定"))
    if (!profiles.length) {
        section.append(node(document, "p", "meta", "请先在游戏服务器中添加要连接的目标实例, 再填写目标槽的双向授权资料."))
        container.append(section)
        return
    }
    const form = node(document, "form", "stack-form")
    const fields = node(document, "fieldset", "stack-form")
    const profile = field(document, fields, "target_profile_id", "目标服务器", { choices: profiles.map((item) => [String(item.id), item.name]), value: profiles[0].id })
    const kind = field(document, fields, "target_instance_kind", "目标实例类型", { choices: [["local", "本机服务实例"], ["remote", "远程服务实例"]], value: "local" })
    const instance = field(document, fields, "target_instance_id", "目标实例 ID", { required: true, pattern: "[0-9a-f]{32}", maxLength: 32 })
    const targetSlot = field(document, fields, "target_slot_id", "目标槽 ID", { type: "number", min: 1, step: 1, required: true })
    const token = field(document, fields, "target_token", "目标槽双向授权码", { type: "password", required: true, autocomplete: "off" })
    const pasteLabel = node(document, "label", "stack-form")
    pasteLabel.append(node(document, "span", "", "粘贴目标设备签发的槽授权资料"))
    const paste = node(document, "textarea")
    paste.name = "authorization"
    paste.rows = 3
    paste.spellcheck = false
    pasteLabel.append(paste)
    fields.append(pasteLabel, actionButton(document, "填入授权资料", () => {
        let authorization
        try { authorization = JSON.parse(paste.value) } catch { throw new Error("槽授权资料格式无效.") }
        if (!authorization || !["local", "remote"].includes(authorization.target_instance_kind)
            || !/^[0-9a-f]{32}$/.test(authorization.target_instance_id)
            || !Number.isInteger(authorization.target_slot_id) || authorization.target_slot_id <= 0
            || typeof authorization.target_token !== "string" || !authorization.target_token) throw new Error("槽授权资料缺少有效的目标身份或授权码.")
        kind.value = authorization.target_instance_kind
        instance.value = authorization.target_instance_id
        targetSlot.value = String(authorization.target_slot_id)
        token.value = authorization.target_token
        paste.value = ""
        return "目标槽授权资料已填入, 保存前请确认目标服务器与同步模式."
    }, actions.runAction))
    const modes = modeFields(document, fields, { upload_mode: "manual", pull_mode: "manual", conflict_policy: "ask", interval_seconds: 900, enabled: true })
    form.append(fields)
    submitForm(document, form, fields, "保存槽位绑定", async () => {
        const targetId = Number(targetSlot.value)
        if (!Number.isSafeInteger(targetId) || targetId <= 0 || !/^[0-9a-f]{32}$/.test(instance.value)) throw new Error("请输入有效的目标实例和槽 ID.")
        const binding = await actions.requestApi(`/v1/local-saves/${slot.id}/transfer-bindings`, {
            method: "POST", body: {
                target_profile_id: Number(profile.value), target_instance_kind: kind.value,
                target_instance_id: instance.value, target_slot_id: targetId, target_token: token.value.trim(), ...modes(),
            },
        })
        token.value = ""
        paste.value = ""
        await actions.refreshManagementState()
        return `槽位绑定已创建. ${bindingModeSummary(binding)}`
    }, actions.runAction)
    section.append(form)
    container.append(section)
}
// //// /创建指向明确目标槽的传输绑定 ////

// //// 编辑现有绑定的模式和目标授权 [@x380kkm 2026-09-26] ////
function appendBindingEditor(slot, binding, container, actions) {
    const document = container.ownerDocument
    const section = node(document, "details", "form-drawer")
    section.append(node(document, "summary", "", "编辑同步设置"))
    const form = node(document, "form", "stack-form")
    const fields = node(document, "fieldset", "stack-form")
    const modes = modeFields(document, fields, binding)
    const token = field(document, fields, "target_token", "更新目标槽授权码 (留空保留当前授权)", { type: "password", autocomplete: "off" })
    form.append(fields)
    submitForm(document, form, fields, "保存同步设置", async () => {
        const body = modes()
        if (token.value.trim()) body.target_token = token.value.trim()
        const updated = await actions.requestApi(`/v1/local-saves/${slot.id}/transfer-bindings/${binding.binding_id}`, { method: "PUT", body })
        token.value = ""
        await actions.refreshManagementState()
        return bindingModeSummary(updated)
    }, actions.runAction)
    section.append(form)
    container.append(section)
}
// //// /编辑现有绑定的模式和目标授权 ////

// //// 展示绑定状态并处理同步冲突 [@x380kkm 2026-09-26] ////
export function renderTransferBindings(slot, model, container, actions) {
    const bindings = model.transferBindings.get(slot.id) || []
    if (!bindings.length) return
    const document = container.ownerDocument
    const section = node(document, "section", "automation-card")
    section.append(node(document, "h4", "card-title", "跨实例槽位绑定"))
    for (const binding of bindings) {
        const profile = model.profiles.profiles.find((item) => item.id === binding.target.profile_id)
        const conflict = (model.transferConflicts.get(binding.binding_id) || []).find((item) => item.status === "open")
        const card = node(document, "article", "item-card")
        card.append(node(document, "h5", "card-title", profile ? profile.name : `服务器 ${binding.target.profile_id}`))
        card.append(node(document, "p", "meta", `${binding.target.instance_kind}, 实例 ${binding.target.instance_id}, 槽 ${binding.target.slot_id}`))
        card.append(node(document, "p", "meta", bindingModeSummary(binding)))
        if (binding.last_synced_at) card.append(node(document, "p", "meta", `最近同步 ${binding.last_synced_at}`))
        if (binding.last_error) card.append(node(document, "p", "automation-error", actions.describeError(binding.last_error)))
        const path = `/v1/local-saves/${slot.id}/transfer-bindings/${binding.binding_id}`
        const controls = node(document, "div", "card-actions")
        const sync = actionButton(document, "立即同步", async () => {
            try { await actions.requestApi(`${path}/sync`, { method: "POST" }) }
            finally { await actions.refreshManagementState() }
            return "槽位同步完成."
        }, actions.runAction)
        sync.disabled = !binding.enabled || Boolean(conflict)
        controls.append(sync, actionButton(document, binding.enabled ? "关闭绑定" : "启用绑定", async () => {
            const updated = await actions.requestApi(path, { method: "PUT", body: {
                upload_mode: binding.upload_mode, pull_mode: binding.pull_mode, conflict_policy: binding.conflict_policy,
                interval_seconds: binding.interval_seconds, enabled: !binding.enabled,
            } })
            await actions.refreshManagementState()
            return bindingModeSummary(updated)
        }, actions.runAction))
        controls.append(actionButton(document, "删除绑定", async () => {
            const confirm = actions.confirm || window.confirm.bind(window)
            if (!confirm("删除此传输绑定? 现有存档和版本历史会保留.")) return undefined
            await actions.requestApi(path, { method: "DELETE" })
            await actions.refreshManagementState()
            return "传输绑定已删除."
        }, actions.runAction, "danger"))
        card.append(controls)
        appendBindingEditor(slot, binding, card, actions)
        if (conflict) appendConflictActions(path, conflict, card, actions)
        section.append(card)
    }
    container.append(section)
}

function appendConflictActions(path, conflict, container, actions) {
    const document = container.ownerDocument
    container.append(node(document, "p", "automation-error", `本地 ${conflict.source_etag.slice(0, 8)}, 远端 ${conflict.target_etag.slice(0, 8)}.`))
    const controls = node(document, "div", "card-actions")
    for (const [label, resolution] of [["本地覆盖远端", "local_wins"], ["远端覆盖本地", "remote_wins"], ["保留双方并关闭绑定", "keep_both"]]) {
        controls.append(actionButton(document, label, async () => {
            const confirm = actions.confirm || window.confirm.bind(window)
            if (!confirm(`${label}? 覆盖前会保留安全版本, 可在存档的版本历史中恢复.`)) return undefined
            await actions.requestApi(`${path}/conflicts/${conflict.conflict_id}/resolve`, { method: "POST", body: { resolution } })
            await actions.refreshManagementState()
            return "传输冲突已处理."
        }, actions.runAction, resolution === "keep_both" ? "ghost" : "danger"))
    }
    container.append(controls)
}
// //// /展示绑定状态并处理同步冲突 ////
