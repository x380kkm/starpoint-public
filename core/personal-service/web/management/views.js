// audience: external
// # personal-service-management-views
//
// 此模块把个人服务状态转换为安全 DOM 节点. 所有远端文本只写入 textContent.

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

function renderRemotePanels(model, elements) {
    const hasRemoteProfile = model.profiles.profiles.some((profile) => profile.mode === "remote")
    const hasRemoteStorage = model.targets.length > 0
    elements.remoteSavePanel.hidden = !hasRemoteProfile && !hasRemoteStorage
    elements.remoteSavePanel.parentElement.classList.toggle("single-column", elements.remoteSavePanel.hidden)
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

function renderDevicePicker(model, elements) {
    const selected = Number(elements.deviceSelect.value)
    elements.deviceSelect.replaceChildren()
    for (const device of model.saves.devices) {
        const option = createElement("option", "", `设备 ${device.device_id}`)
        option.value = String(device.device_id)
        option.selected = device.device_id === selected
        elements.deviceSelect.append(option)
    }
}

function renderSaves(model, elements, actions) {
    elements.saveList.replaceChildren()
    if (model.saves.slots.length === 0) {
        elements.saveList.append(createElement("p", "empty-state", "客户端完成一次注册后会创建首个存档."))
        return
    }
    for (const slot of model.saves.slots) {
        const activeDevices = model.saves.devices
            .filter((device) => device.active_slot_id === slot.id)
            .map((device) => device.device_id)
        const card = createElement("article", `save-card${activeDevices.length ? " active" : ""}`)
        const head = createElement("div", "card-head")
        const title = createElement("div")
        title.append(createElement("h3", "card-title", slot.name))
        title.append(createElement("p", "meta", `槽位 ${slot.id}, 快照 ${slot.snapshot_count}`))
        if (activeDevices.length) title.append(createElement("p", "meta", `当前设备 ${activeDevices.join(", ")}`))
        head.append(title)
        if (activeDevices.length) head.append(createElement("span", "badge", "活动存档"))
        card.append(head)
        const controls = createElement("div", "save-actions")
        appendSaveActions(controls, slot, model, elements, actions)
        const history = createElement("div")
        controls.append(createButton("查看快照", async () => {
            await renderSnapshots(slot, history, actions)
            return undefined
        }, actions.runAction))
        card.append(controls, history)
        renderBindings(slot, model, card)
        renderTransferBindings(slot, model, card, actions)
        renderAutomation(slot, model, card, actions)
        elements.saveList.append(card)
    }
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
    controls.append(createButton("设为活动", async () => {
        const deviceId = Number(elements.deviceSelect.value)
        if (!Number.isInteger(deviceId) || deviceId <= 0) throw new Error("没有可操作的设备.")
        await actions.requestApi(`/v1/local-saves/${slot.id}/activate`, {
            method: "POST",
            body: { device_id: deviceId },
        })
        await actions.refreshManagementState()
        return `设备 ${deviceId} 已切换存档.`
    }, actions.runAction))
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
    controls.append(createButton("导出存档", async () => {
        const result = await shareOrDownloadJson(
            `starpoint-save-${slot.id}.json`,
            await actions.requestApi(`/v1/local-saves/${slot.id}/export`),
        )
        if (result === "cancelled") return undefined
        return result === "shared" ? "已打开系统分享." : "存档文件已下载."
    }, actions.runAction))
    controls.append(createButton("导出加密备份", async () => {
        const payload = await actions.requestApi(`/v1/local-saves/${slot.id}/encrypted-export`)
        const result = await shareOrDownloadJson(`starpoint-save-${slot.id}.starpoint-save`, payload)
        if (result === "cancelled") return undefined
        return result === "shared" ? "已打开系统分享." : "加密备份已下载."
    }, actions.runAction))
    appendTransferBindingCreateAction(controls, slot, model, actions)
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

function appendTransferBindingCreateAction(controls, slot, model, actions) {
    const profiles = model.profiles.profiles.filter((profile) => profile.mode === "remote")
    if (profiles.length === 0) return
    controls.append(createButton("创建槽位绑定", async () => {
        const choices = profiles.map((profile) => `${profile.id}: ${profile.name}`).join("\n")
        const profileId = Number(prompt(`目标服务器配置 ID:\n${choices}`, String(profiles[0].id)))
        if (!profiles.some((profile) => profile.id === profileId)) throw new Error("服务器配置 ID 无效.")
        const instanceKind = prompt("目标实例类型: remote 或 local", "remote")?.trim()
        if (instanceKind !== "remote" && instanceKind !== "local") return undefined
        const instanceId = prompt("目标实例 ID, 32 位小写十六进制")?.trim()
        if (!instanceId) return undefined
        const targetSlotId = Number(prompt("目标槽 ID")?.trim())
        if (!Number.isInteger(targetSlotId) || targetSlotId <= 0) return undefined
        const targetToken = prompt("目标槽授权码, 需要双向权限")?.trim()
        if (!targetToken) return undefined
        await actions.requestApi(`/v1/local-saves/${slot.id}/transfer-bindings`, {
            method: "POST",
            body: {
                target_profile_id: profileId,
                target_instance_kind: instanceKind,
                target_instance_id: instanceId,
                target_slot_id: targetSlotId,
                target_token: targetToken,
                upload_mode: "manual",
                pull_mode: "manual",
                conflict_policy: "ask",
                interval_seconds: 900,
                enabled: true,
            },
        })
        await actions.refreshManagementState()
        return "槽位绑定已创建, 首次同步需要手动确认."
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

function renderTransferBindings(slot, model, card, actions) {
    const bindings = model.transferBindings.get(slot.id) ?? []
    if (bindings.length === 0) return
    const section = createElement("section", "automation-card")
    section.append(createElement("h4", "card-title", "跨实例槽位绑定"))
    for (const binding of bindings) {
        const profile = model.profiles.profiles.find((item) => item.id === binding.target.profile_id)
        const conflicts = model.transferConflicts.get(binding.binding_id) ?? []
        const openConflict = conflicts.find((conflict) => conflict.status === "open")
        const item = createElement("article", "item-card")
        const head = createElement("div", "card-head")
        const title = createElement("div")
        title.append(createElement("h5", "card-title", profile?.name ?? `服务器 ${binding.target.profile_id}`))
        title.append(createElement(
            "p",
            "meta",
            `${binding.target.instance_kind} ${binding.target.instance_id.slice(0, 8)}, 槽 ${binding.target.slot_id}`,
        ))
        const state = openConflict ? "有冲突" : binding.enabled ? "已启用" : "已关闭"
        head.append(title, createElement("span", "badge", state))
        item.append(head)
        const status = createElement("div", "automation-status")
        if (binding.last_synced_at) status.append(createElement("p", "meta", `最近同步 ${formatTime(binding.last_synced_at)}`))
        if (binding.last_common_etag) status.append(createElement("p", "meta", `共同版本 ${binding.last_common_etag.slice(0, 8)}`))
        if (binding.last_error) status.append(createElement("p", "automation-error", actions.describeError(binding.last_error)))
        item.append(status)
        const controls = createElement("div", "card-actions")
        controls.append(createButton("立即同步", async () => {
            try {
                await actions.requestApi(`/v1/local-saves/${slot.id}/transfer-bindings/${binding.binding_id}/sync`, {
                    method: "POST",
                })
            } finally {
                await actions.refreshManagementState()
            }
            return "槽位同步完成."
        }, actions.runAction))
        controls.append(createButton(binding.enabled ? "关闭" : "启用", async () => {
            await actions.requestApi(`/v1/local-saves/${slot.id}/transfer-bindings/${binding.binding_id}`, {
                method: "PUT",
                body: {
                    upload_mode: binding.upload_mode,
                    pull_mode: binding.pull_mode,
                    conflict_policy: binding.conflict_policy,
                    interval_seconds: binding.interval_seconds,
                    enabled: !binding.enabled,
                },
            })
            await actions.refreshManagementState()
            return binding.enabled ? "自动传输已关闭." : "自动传输已启用."
        }, actions.runAction))
        controls.append(createButton("删除绑定", async () => {
            if (!confirm("删除此传输绑定? 现有存档和 revision 不会删除.")) return undefined
            await actions.requestApi(`/v1/local-saves/${slot.id}/transfer-bindings/${binding.binding_id}`, {
                method: "DELETE",
            })
            await actions.refreshManagementState()
            return "传输绑定已删除."
        }, actions.runAction, "danger"))
        item.append(controls)
        if (openConflict) appendConflictActions(item, slot, binding, openConflict, actions)
        section.append(item)
    }
    card.append(section)
}

function appendConflictActions(container, slot, binding, conflict, actions) {
    const summary = createElement("p", "automation-error", `本地 ${conflict.source_etag.slice(0, 8)}, 远端 ${conflict.target_etag.slice(0, 8)}.`)
    const controls = createElement("div", "card-actions")
    const resolutions = [
        ["本地覆盖远端", "local_wins"],
        ["远端覆盖本地", "remote_wins"],
        ["保留双方并关闭绑定", "keep_both"],
    ]
    for (const [label, resolution] of resolutions) {
        controls.append(createButton(label, async () => {
            if (!confirm(`${label}? 覆盖前会保留安全 revision.`)) return undefined
            await actions.requestApi(
                `/v1/local-saves/${slot.id}/transfer-bindings/${binding.binding_id}/conflicts/${conflict.conflict_id}/resolve`,
                { method: "POST", body: { resolution } },
            )
            await actions.refreshManagementState()
            return "传输冲突已处理."
        }, actions.runAction, resolution === "keep_both" ? "ghost" : "danger"))
    }
    container.append(summary, controls)
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

async function shareOrDownloadJson(filename, payload) {
    const blob = new Blob([JSON.stringify(payload, null, 2)], { type: "application/json" })
    if (typeof File === "function" && typeof navigator.share === "function") {
        const file = new File([blob], filename, { type: blob.type })
        if (typeof navigator.canShare !== "function" || navigator.canShare({ files: [file] })) {
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
    return "downloaded"
}
// //// /呈现服务器, 存档和同步操作 ////
