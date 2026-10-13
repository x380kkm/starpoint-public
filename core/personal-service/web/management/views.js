// audience: external
// # personal-service-management-views
//
// 此模块把个人服务状态转换为安全 DOM 节点. 所有远端文本只写入 textContent.

import { confirmSaveDeletion } from "./save-deletion.js"
import { requestBackupPassword } from "./password-backup.js"
import { renderSlotAccess } from "./save-access.js"
import { renderSaveRevisions } from "./save-revisions.js"
import { renderTransferBindingCreate, renderTransferBindings } from "./save-transfers.js"

// //// 呈现服务器, 存档和同步操作 [@x380kkm 2026-07-24] ////
function createElement(tagName, className, text) {
    const node = document.createElement(tagName)
    if (className) node.className = className
    if (text !== undefined) node.textContent = text
    return node
}

function createButton(label, action, runAction, style = "ghost") {
    const control = createElement("button", `button ${style}`, label)
    control.type = "button"
    control.addEventListener("click", () => runAction(control, action))
    return control
}

export function renderManagement(model, elements, actions) {
    renderProfiles(model, elements, actions)
    renderSyncTargets(model, elements, actions)
    renderRemotePanels(model, elements)
    renderDevicePicker(model, elements)
    renderSaves(model, elements, actions)
    renderHttpObservations(model, elements)
    renderClientReports(model.clientReports ?? [])
}

// //// 展示客户端故障信息和服务端请求异常 [@x380kkm 2026-09-27] ////
function renderClientReports(reports) {
    const list = document.querySelector("#client-report-list")
    if (!list) return
    list.replaceChildren()
    if (reports.length === 0) {
        list.append(createElement("p", "empty-state", "尚未记录错误报告."))
        return
    }
    for (const report of reports) {
        const details = report.details ?? {}
        const card = createElement("details", "item-card client-report")
        const summary = createElement("summary")
        const title = details.source === "server"
            ? `服务端 ${details.code || "异常"} ${details.method || ""} ${details.path || ""}`
            : details.code || details.error || "客户端错误"
        summary.append(createElement("strong", "client-report-title", String(title).slice(0, 180)))
        summary.append(createElement("span", "meta", formatTime(report.received_at)))
        const body = createElement("pre", "client-report-body", JSON.stringify(details, null, 2))
        card.append(summary, body)
        list.append(card)
    }
}

function httpObservationTime(observation) {
    const time = Date.parse(observation.last_seen)
    return Number.isFinite(time) ? time : Number.NEGATIVE_INFINITY
}

function compareHttpObservationRecency(left, right) {
    return httpObservationTime(right) - httpObservationTime(left)
}

function groupHttpObservations(observations) {
    const groups = new Map()
    for (const observation of observations) {
        const method = String(observation.method ?? "")
        const path = String(observation.path ?? "")
        const key = JSON.stringify([method, path])
        const group = groups.get(key) ?? { method, path, history: [] }
        group.history.push({ ...observation, method, path })
        groups.set(key, group)
    }
    return [...groups.values()]
        .map((group) => {
            group.history.sort(compareHttpObservationRecency)
            const count = group.history.reduce((total, observation) => {
                const value = Number(observation.count)
                return total + (Number.isFinite(value) && value > 0 ? value : 0)
            }, 0)
            return { ...group, count, latest: group.history[0] }
        })
        .sort((left, right) => compareHttpObservationRecency(left.latest, right.latest))
}

function httpObservationStatusClass(status) {
    const code = Number.parseInt(String(status), 10)
    return Number.isInteger(code) && code >= 400
        ? "badge http-observation-status error"
        : "badge http-observation-status"
}

function renderHttpObservations(model, elements) {
    elements.httpObservationList.replaceChildren()
    if (model.httpObservations.length === 0) {
        elements.httpObservationList.append(createElement("p", "empty-state", "尚未记录到请求."))
        return
    }
    for (const group of groupHttpObservations(model.httpObservations)) {
        const current = group.latest
        const card = createElement("article", "item-card http-observation-card")
        card.dataset.method = group.method
        card.dataset.path = group.path
        card.dataset.currentStatus = String(current.status)
        const head = createElement("div", "card-head")
        const title = createElement("div")
        title.append(createElement("h3", "card-title http-observation-path", group.path))
        title.append(createElement(
            "p",
            "meta",
            `${group.method} · 累计 ${group.count} 次 · 当前状态最近 ${formatTime(current.last_seen)}`,
        ))
        head.append(
            title,
            createElement("span", httpObservationStatusClass(current.status), `当前 ${current.status}`),
        )
        card.append(head)

        const history = createElement("details", "http-observation-history")
        history.append(createElement("summary", "", `查看历史状态 (${group.history.length})`))
        const historyList = createElement("div", "http-observation-history-list")
        for (const observation of group.history) {
            const row = createElement("div", "http-observation-history-row")
            row.dataset.status = String(observation.status)
            row.append(
                createElement("span", httpObservationStatusClass(observation.status), String(observation.status)),
                createElement(
                    "span",
                    "meta",
                    `${observation.count} 次 · 首次 ${formatTime(observation.first_seen)} · 最近 ${formatTime(observation.last_seen)}`,
                ),
            )
            historyList.append(row)
        }
        history.append(historyList)
        card.append(history)
        elements.httpObservationList.append(card)
    }
}

export function renderRemotePanels(model, elements) {
    const hasRemoteProfile = model.profiles.profiles.some((profile) => profile.mode === "remote")
    const hasRemoteStorage = model.targets.length > 0
    elements.remoteSavePanel.hidden = false
    elements.remoteSavePanel.parentElement.classList.toggle("single-column", false)
    elements.remotePlayerAccessPanel.hidden = !hasRemoteProfile
    elements.remoteRestorePanel.hidden = !hasRemoteStorage
}

function renderProfiles(model, elements, actions) {
    elements.profileList.replaceChildren()
    for (const profile of model.profiles.profiles) {
        const active = profile.id === model.profiles.active_profile_id
        const profileName = profile.is_builtin ? "当前设备" : profile.name
        const card = createElement("article", `item-card${active ? " active" : ""}`)
        const head = createElement("div", "card-head")
        const title = createElement("div")
        title.append(createElement("h3", "card-title", profileName))
        const endpoint = profile.mode === "local"
            ? "当前设备内置服务"
            : `${profile.scheme}://${profile.host}:${profile.port}`
        title.append(createElement("p", "meta", endpoint))
        head.append(title)
        if (active) head.append(createElement("span", "badge", "正在使用"))
        card.append(head)
        const controls = createElement("div", "card-actions")
        if (profile.mode === "remote") {
            controls.append(createButton("测试连接", async () => {
                const probe = await actions.requestApi(`/v1/server-profiles/${profile.id}/probe`, { method: "POST" })
                const session = probe.session_port ? `, 联机端口 ${probe.session_port}` : ""
                if (!probe.reachable) throw new Error("无法连接游戏服务器, 当前服务器保持不变.")
                if (!probe.compatible) throw new Error("服务器可以连接, 但不是兼容的 Starpoint 服务.")
                return `服务器连接正常, 延迟 ${probe.latency_ms} ms${session}.`
            }, actions.runAction))
        }
        if (!active) {
            controls.append(createButton("检测并切换", async () => {
                await actions.requestApi(`/v1/server-profiles/${profile.id}/activate-verified`, { method: "POST" })
                await actions.refreshManagementState()
                return `已切换到 ${profileName}.`
            }, actions.runAction))
        }
        if (!profile.is_builtin) {
            controls.append(createButton("删除", async () => {
                if (!confirm(`删除服务器 ${profile.name}?`)) return undefined
                await actions.requestApi(`/v1/server-profiles/${profile.id}`, { method: "DELETE" })
                await actions.refreshManagementState()
                return "服务器配置已删除."
            }, actions.runAction, "danger"))
        }
        card.append(controls)
        elements.profileList.append(card)
    }
}

function renderSyncTargets(model, elements, actions) {
    elements.syncTargetList.replaceChildren()
    if (model.targets.length === 0) {
        elements.syncTargetList.append(createElement("p", "empty-state", "尚未配置存档服务器."))
    }
    for (const target of model.targets) {
        const card = createElement("article", "item-card")
        const head = createElement("div", "card-head")
        const title = createElement("div")
        title.append(createElement("h3", "card-title", target.name))
        title.append(createElement("p", "meta", `${target.scheme}://${target.host}:${target.port}`))
        title.append(createElement("p", "meta", `用户 ${target.username}`))
        head.append(title, createElement("span", "badge", target.has_credentials ? "凭据已保存" : "缺少凭据"))
        card.append(head)
        const controls = createElement("div", "card-actions")
        controls.append(createButton("删除", async () => {
            if (!confirm(`删除存档服务器 ${target.name}? 本地存档不会删除.`)) return undefined
            await actions.requestApi(`/v1/save-sync-targets/${target.id}`, { method: "DELETE" })
            await actions.refreshManagementState()
            return "存档服务器配置已删除."
        }, actions.runAction, "danger"))
        card.append(controls)
        elements.syncTargetList.append(card)
    }
    for (const select of document.querySelectorAll(".sync-target-select")) {
        fillTargetSelect(select, model.targets)
    }
}

export function selectedDeviceSlotId(saves, selectedDevice) {
    if (selectedDevice === "local" && saves.local_slot_id !== null && saves.local_slot_id !== undefined) return saves.local_slot_id
    return saves.devices.find((device) => String(device.device_id) === selectedDevice)?.active_slot_id
        ?? saves.devices[0]?.active_slot_id
}

export function renderDevicePicker(model, elements) {
    const selected = elements.deviceSelect.value || "local"
    elements.deviceSelect.replaceChildren()
    const local = createElement("option", "", "本机")
    local.value = "local"
    local.selected = selected === "local" || !model.saves.devices.some((device) => String(device.device_id) === selected)
    elements.deviceSelect.append(local)
    for (const device of model.saves.devices) {
        const option = createElement("option", "", `设备 ${device.device_id}`)
        option.value = String(device.device_id)
        option.selected = String(device.device_id) === selected
        elements.deviceSelect.append(option)
    }
}

function renderSaves(model, elements, actions) {
    elements.saveList.replaceChildren()
    if (model.saves.slots.length === 0) {
        elements.saveList.append(createElement("p", "empty-state", "可直接导入已有存档, 再选择使用此存档并进入游戏."))
        return
    }
    for (const slot of model.saves.slots) {
        const activeDevices = model.saves.devices
            .filter((device) => device.active_slot_id === slot.id)
            .map((device) => device.device_id)
        const isLocal = model.saves.local_slot_id === slot.id
        const card = createElement("article", `save-card${activeDevices.length || isLocal ? " active" : ""}`)
        const head = createElement("div", "card-head")
        const title = createElement("div")
        title.append(createElement("h3", "card-title", slot.name))
        title.append(createElement("p", "meta", `槽位 ${slot.id}, 快照 ${slot.snapshot_count}`))
        title.append(createElement("p", "meta", `最近保存 ${formatTime(slot.updated_at)}`))
        if (activeDevices.length) title.append(createElement("p", "meta", `当前设备 ${activeDevices.join(", ")}`))
        head.append(title)
        if (activeDevices.length || isLocal) head.append(createElement("span", "badge", isLocal ? "本机使用中" : "使用中"))
        card.append(head)
        const controls = createElement("div", "save-actions")
        appendSaveActions(controls, slot, model, elements, actions)
        const history = createElement("div")
        controls.append(createButton("查看快照", async () => {
            await renderSnapshots(slot, history, actions)
            return undefined
        }, actions.runAction))
        const revisions = createElement("div")
        controls.append(createButton("查看版本历史", async () => {
            await renderSaveRevisions(slot, revisions, actions)
            return undefined
        }, actions.runAction))
        card.append(controls, history, revisions)
        const details = createElement("details", "form-drawer save-options")
        details.append(createElement("summary", "", "自动备份与同步"))
        renderBindings(slot, model, details)
        renderTransferBindings(slot, model, details, actions)
        renderTransferBindingCreate(slot, model, details, actions)
        renderSlotAccess(slot, details, actions)
        renderAutomation(slot, model, details, actions)
        details.append(createButton("导出旧加密备份的恢复包", async () => {
            const password = await requestBackupPassword(details)
            if (password === undefined) return undefined
            const response = await actions.requestApi(`/v1/local-saves/${slot.id}/recovery-export`, { method: "POST", body: { password } })
            const result = await shareOrDownloadJson("starpoint-recovery.starpoint-recovery", response.package)
            return documentExportFeedback(result)
        }, actions.runAction))
        card.append(details)
        appendSaveDeletion(card, slot, model, actions)
        elements.saveList.append(card)
    }
}

// //// 展示存档删除入口和设备占用提示 [@x380kkm 2026-09-20] ////
function appendSaveDeletion(card, slot, model, actions) {
    const active = model.saves.local_slot_id === slot.id || model.saves.devices.some((device) => device.active_slot_id === slot.id)
    const lastSlot = model.saves.slots.length === 1
    const reason = lastSlot
        ? "保留至少一个存档. 导入另一份存档并启用后可删除此存档."
        : active ? "此存档正在使用. 请先为使用它的设备切换存档." : "删除前可先导出存档文件."
    const row = createElement("div", "save-delete-row")
    const hint = createElement("p", "meta", reason)
    hint.id = `save-delete-hint-${slot.id}`
    const control = createButton("删除存档", async () => {
        if (!await confirmSaveDeletion(slot, control)) return undefined
        await actions.requestApi(`/v1/local-saves/${slot.id}`, {
            method: "DELETE",
            body: { confirm_name: slot.name },
        })
        await actions.refreshManagementState()
        return `存档 "${slot.name}" 已删除. 已导出的文件和远程副本仍然保留.`
    }, actions.runAction, "danger")
    control.disabled = active || lastSlot
    control.setAttribute("aria-describedby", hint.id)
    control.setAttribute("aria-label", `删除存档 ${slot.name}`)
    row.append(hint, control)
    card.append(row)
}

function renderAutomation(slot, model, card, actions) {
    const automation = model.automations.get(slot.id)
    if (!automation) return
    const section = createElement("section", "automation-card")
    const heading = createElement("div", "card-head")
    const title = createElement("div")
    title.append(createElement("h4", "card-title", "自动快照"))
    const intervalMinutes = Math.max(1, Math.round(automation.interval_seconds / 60))
    const status = automation.enabled
        ? `每 ${intervalMinutes} 分钟在前台运行或恢复时检查, 保留最近 48 个自动快照`
        : "当前未启用"
    title.append(createElement("p", "meta", status))
    heading.append(title, createElement("span", "badge", automation.enabled ? "已开启" : "已关闭"))
    section.append(heading)

    const times = createElement("div", "automation-status")
    if (automation.last_snapshot_at) {
        times.append(createElement("p", "meta", `最近快照 ${formatTime(automation.last_snapshot_at)}`))
    }
    if (automation.last_upload_at) {
        times.append(createElement("p", "meta", `最近上传 ${formatTime(automation.last_upload_at)}`))
    }
    if (automation.pending_upload) {
        times.append(createElement("p", "meta", "密文上传正在等待或进行中."))
    }
    if (automation.last_error) {
        times.append(createElement("p", "automation-error", actions.describeError(automation.last_error)))
    }
    section.append(times)

    const details = createElement("details", "form-drawer automation-drawer")
    const summary = createElement("summary", "", "设置自动快照")
    const form = createElement("form", "stack-form")
    const enabledLabel = createElement("label", "check-row")
    const enabled = createElement("input")
    enabled.type = "checkbox"
    enabled.name = "enabled"
    enabled.checked = automation.enabled
    enabledLabel.append(enabled, createElement("span", "", "启用自动快照"))
    form.append(enabledLabel)

    const row = createElement("div", "form-row")
    const intervalLabel = createElement("label", "grow")
    intervalLabel.append(createElement("span", "", "快照间隔, 分钟"))
    const interval = createElement("input")
    interval.type = "number"
    interval.name = "interval_minutes"
    interval.min = "1"
    interval.max = "43200"
    interval.required = true
    interval.value = String(intervalMinutes)
    intervalLabel.append(interval)
    const targetLabel = createElement("label", "grow")
    targetLabel.append(createElement("span", "", "自动上传, 可选"))
    const target = createElement("select")
    target.name = "target_id"
    fillOptionalTargetSelect(target, model.targets, automation.target_id)
    targetLabel.append(target)
    row.append(intervalLabel, targetLabel)
    form.append(row)

    const objectLabel = createElement("label")
    objectLabel.append(createElement("span", "", "远端对象 ID"))
    const objectId = createElement("input")
    objectId.name = "object_id"
    objectId.maxLength = 64
    objectId.pattern = "[A-Za-z0-9_-]+"
    objectId.value = automation.object_id ?? `slot-${slot.id}`
    objectLabel.append(objectId)
    form.append(objectLabel)
    const submit = createElement("button", "button primary", "保存自动快照设置")
    submit.type = "submit"
    form.append(submit)

    const updateUploadFields = () => {
        objectId.disabled = target.value === ""
        objectId.required = target.value !== ""
    }
    target.addEventListener("change", updateUploadFields)
    updateUploadFields()
    form.addEventListener("submit", (event) => {
        event.preventDefault()
        const selectedTarget = target.value === "" ? null : Number(target.value)
        actions.runAction(submit, async () => {
            await actions.requestApi(`/v1/local-saves/${slot.id}/automation`, {
                method: "PUT",
                body: {
                    enabled: enabled.checked,
                    interval_seconds: Number(interval.value) * 60,
                    target_id: selectedTarget,
                    object_id: selectedTarget === null ? null : objectId.value.trim(),
                },
            })
            await actions.refreshManagementState()
            return "自动快照设置已保存."
        })
    })
    details.append(summary, form)
    section.append(details)
    card.append(section)
}

function appendSaveActions(controls, slot, model, elements, actions) {
    controls.append(createButton("使用此存档", async () => {
        if (elements.deviceSelect.value === "local") {
            await actions.requestApi(`/v1/local-saves/${slot.id}/activate-local`, { method: "POST" })
            await actions.refreshManagementState()
            return "本机已选择此存档, 重新进入游戏后生效."
        }
        const deviceId = Number(elements.deviceSelect.value)
        if (!Number.isInteger(deviceId) || deviceId <= 0) throw new Error("没有可操作的设备.")
        await actions.requestApi(`/v1/local-saves/${slot.id}/activate`, {
            method: "POST",
            body: { device_id: deviceId },
        })
        await actions.refreshManagementState()
        return `设备 ${deviceId} 已切换存档.`
    }, actions.runAction, "primary"))
    controls.append(createButton("复制", async () => {
        const name = prompt("新存档名称", `${slot.name} 副本`)?.trim()
        if (!name) return undefined
        await actions.requestApi(`/v1/local-saves/${slot.id}/copy`, { method: "POST", body: { name } })
        await actions.refreshManagementState()
        return "存档副本已创建."
    }, actions.runAction))
    controls.append(createButton("创建快照", async () => {
        const label = prompt("快照标签", "手动快照")?.trim()
        if (!label) return undefined
        await actions.requestApi(`/v1/local-saves/${slot.id}/snapshots`, { method: "POST", body: { label } })
        await actions.refreshManagementState()
        return "快照已创建."
    }, actions.runAction))
    controls.append(createButton("导出可编辑 JSON", async () => {
        const result = await shareOrDownloadJson(
            `starpoint-save-${slot.id}.json`,
            await actions.requestApi(`/v1/local-saves/${slot.id}/export`),
        )
        return documentExportFeedback(result)
    }, actions.runAction))
    controls.append(createButton("导出加密备份", async () => {
        const password = await requestBackupPassword(controls)
        if (password === undefined) return undefined
        const payload = await actions.requestApi(`/v1/local-saves/${slot.id}/password-export`, { method: "POST", body: { password } })
        const result = await shareOrDownloadJson(`starpoint-save-${slot.id}.starpoint-save`, payload)
        return documentExportFeedback(result)
    }, actions.runAction))
    if (model.targets.length === 0) return
    const targetSelect = createElement("select", "slot-target-select")
    targetSelect.setAttribute("aria-label", `${slot.name} 上传目标`)
    fillTargetSelect(targetSelect, model.targets)
    controls.append(targetSelect)
    controls.append(createButton("上传备份", async () => {
        const objectId = prompt("远端对象 ID", `slot-${slot.id}`)?.trim()
        if (!objectId) return undefined
        await actions.requestApi(`/v1/local-saves/${slot.id}/sync/upload`, {
            method: "POST",
            body: { target_id: Number(targetSelect.value), object_id: objectId },
        })
        await actions.refreshManagementState()
        return "存档备份已上传."
    }, actions.runAction))
}

function renderBindings(slot, model, card) {
    const bindings = model.bindings.get(slot.id) ?? []
    if (bindings.length === 0) return
    const list = createElement("ul", "binding-list")
    for (const binding of bindings) {
        const target = model.targets.find((item) => item.id === binding.target_id)
        const item = createElement("li")
        item.append(
            createElement("span", "", target?.name ?? `服务器 ${binding.target_id}`),
            createElement("span", "meta", `${binding.object_id}, ${binding.etag.slice(0, 8)}`),
        )
        list.append(item)
    }
    card.append(list)
}

async function renderSnapshots(slot, container, actions) {
    const snapshots = await actions.requestApi(`/v1/local-saves/${slot.id}/snapshots`)
    const list = createElement("ul", "snapshot-list")
    if (snapshots.length === 0) list.append(createElement("li", "", "尚无快照."))
    for (const snapshot of snapshots) {
        const item = createElement("li")
        const description = createElement("span")
        description.append(createElement("strong", "", snapshot.label), createElement("span", "meta", snapshot.created_at))
        item.append(description, createButton("回滚", async () => {
            if (!confirm(`回滚到 ${snapshot.label}? 系统会先保存当前状态.`)) return undefined
            await actions.requestApi(`/v1/local-saves/${slot.id}/snapshots/${snapshot.id}/restore`, { method: "POST" })
            await actions.refreshManagementState()
            return "存档已回滚, 回滚前状态已保存为安全快照."
        }, actions.runAction, "danger"))
        list.append(item)
    }
    container.replaceChildren(list)
}

function fillTargetSelect(select, targets) {
    const selected = select.value
    select.replaceChildren()
    for (const target of targets) {
        const option = createElement("option", "", target.name)
        option.value = String(target.id)
        option.selected = option.value === selected
        select.append(option)
    }
    select.disabled = targets.length === 0
}

function fillOptionalTargetSelect(select, targets, selectedTargetId) {
    select.replaceChildren()
    const localOnly = createElement("option", "", "仅保存本地快照")
    localOnly.value = ""
    localOnly.selected = selectedTargetId === null
    select.append(localOnly)
    for (const target of targets) {
        const option = createElement("option", "", target.name)
        option.value = String(target.id)
        option.selected = target.id === selectedTargetId
        select.append(option)
    }
}

function formatTime(value) {
    const time = new Date(value)
    return Number.isNaN(time.valueOf()) ? value : time.toLocaleString()
}

// //// 按文件交付状态呈现操作反馈 [@x380kkm 2026-09-28] ////
function documentExportFeedback(result) {
    const messages = {
        saved: "文件已保存.",
        shared: "已打开系统分享.",
        download_requested: "已发起下载, 请在系统完成保存.",
    }
    return messages[result]
}

async function shareOrDownloadJson(filename, payload) {
    const contents = JSON.stringify(payload, null, 2)
    if (typeof window.StarpointDocuments?.saveJson === "function") {
        return window.StarpointDocuments.saveJson(filename, contents)
    }
    const blob = new Blob([contents], { type: "application/json" })
    if (typeof File === "function" && typeof navigator.share === "function") {
        const file = new File([blob], filename, { type: blob.type })
        if (typeof navigator.canShare === "function" && navigator.canShare({ files: [file] })) {
            try {
                await navigator.share({ files: [file], title: "导出 Starpoint 存档" })
                return "shared"
            } catch (error) {
                if (error?.name === "AbortError") return "cancelled"
            }
        }
    }
    const link = document.createElement("a")
    link.href = URL.createObjectURL(blob)
    link.download = filename
    document.body.append(link)
    link.click()
    link.remove()
    setTimeout(() => URL.revokeObjectURL(link.href), 0)
    return "download_requested"
}
// //// /呈现服务器, 存档和同步操作 ////
