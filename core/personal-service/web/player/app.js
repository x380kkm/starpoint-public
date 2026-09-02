// audience: external
// # personal-service-player-app
// 此脚本只调用当前 origin 的玩家存档 API, token 只保存在模块内存中.

const elements = {
    accessPanel: document.querySelector("#access-panel"),
    accessState: document.querySelector("#access-state"),
    connectionState: document.querySelector("#connection-state"),
    dashboard: document.querySelector("#dashboard"),
    deviceId: document.querySelector("#device-id"),
    deviceList: document.querySelector("#device-list"),
    importForm: document.querySelector("#import-form"),
    playerSaveList: document.querySelector("#player-save-list"),
    recoveryExportForm: document.querySelector("#recovery-export-form"),
    recoveryExportPassword: document.querySelector("#recovery-export-password"),
    recoveryImportForm: document.querySelector("#recovery-import-form"),
    recoveryImportPassword: document.querySelector("#recovery-import-password"),
    recoveryImportFile: document.querySelector("#recovery-import-file"),
    refreshButton: document.querySelector("#refresh-button"),
    syncDownloadForm: document.querySelector("#sync-download-form"),
    syncDownloadName: document.querySelector("#sync-download-name"),
    syncDownloadObject: document.querySelector("#sync-download-object"),
    syncDownloadTarget: document.querySelector("#sync-download-target"),
    syncState: document.querySelector("#sync-state"),
    syncUploadForm: document.querySelector("#sync-upload-form"),
    syncUploadObject: document.querySelector("#sync-upload-object"),
    syncUploadSlot: document.querySelector("#sync-upload-slot"),
    syncUploadTarget: document.querySelector("#sync-upload-target"),
    tokenForm: document.querySelector("#player-token-form"),
    tokenInput: document.querySelector("#player-token-input"),
    toast: document.querySelector("#toast"),
}

const errorMessages = {
    invalid_local_save_activation: "设备 ID 无效.",
    invalid_local_save_data: "存档内容格式无效.",
    invalid_encrypted_local_save: "密文存档无法在当前设备解密.",
    invalid_encrypted_local_save_import: "密文存档格式无效.",
    invalid_local_save_import: "存档导入请求无效.",
    local_save_not_found: "存档不存在或当前 token 无权访问.",
    player_authorization_required: "玩家 token 无效或已经撤销.",
    recovery_key_conflict: "当前设备已有不同的存档加密密钥.",
    recovery_package_invalid: "恢复包无效或密码不正确.",
    recovery_password_invalid: "恢复密码需要 8 到 128 个字符.",
    recovery_scope_conflict: "当前玩家已经绑定了不同的远端作用域.",
    save_sync_target_not_found: "存档服务器不存在或尚未配置.",
    save_sync_target_unusable: "存档服务器地址不可用.",
    save_sync_remote_conflict: "远端存档已经变化, 请先下载为新槽位.",
    save_sync_remote_not_found: "远端对象不存在.",
    save_sync_remote_unavailable: "无法连接存档服务器.",
    save_sync_authentication_failed: "存档服务器拒绝了登录凭据.",
}

let playerToken = ""
let toastTimer
let playerState = { devices: [], slots: [], targets: [] }

class ApiError extends Error {
    constructor(status, code) {
        super(errorMessages[code] ?? code ?? `HTTP ${status}`)
        this.status = status
        this.code = code
    }
}

function showToast(message, isError = false) {
    clearTimeout(toastTimer)
    elements.toast.textContent = message
    elements.toast.classList.toggle("error", isError)
    elements.toast.hidden = false
    toastTimer = setTimeout(() => {
        elements.toast.hidden = true
    }, 4200)
}

function setConnected(connected) {
    elements.accessPanel.hidden = connected
    elements.dashboard.hidden = !connected
    elements.refreshButton.disabled = !connected
    elements.connectionState.textContent = connected ? "已连接存档" : "未连接"
    elements.connectionState.classList.toggle("offline", !connected)
}

async function requestApi(requestPath, options = {}) {
    const headers = { Authorization: `Bearer ${playerToken}` }
    if (options.body !== undefined) headers["Content-Type"] = "application/json"
    const response = await fetch(requestPath, {
        method: options.method ?? "GET",
        headers,
        body: options.body === undefined ? undefined : JSON.stringify(options.body),
    })
    const text = await response.text()
    let payload
    try {
        payload = text ? JSON.parse(text) : null
    } catch {
        payload = null
    }
    if (!response.ok) {
        if (response.status === 401) {
            playerToken = ""
            setConnected(false)
        }
        throw new ApiError(response.status, payload?.error)
    }
    return payload
}

async function runAction(control, action) {
    if (control) control.disabled = true
    try {
        const message = await action()
        if (message) showToast(message)
    } catch (error) {
        showToast(error.message ?? String(error), true)
    } finally {
        if (control) control.disabled = control === elements.refreshButton && !playerToken
    }
}

function makeElement(tagName, className, text) {
    const element = document.createElement(tagName)
    if (className) element.className = className
    if (text !== undefined) element.textContent = text
    return element
}

function downloadJson(filename, payload) {
    const blob = new Blob([JSON.stringify(payload, null, 2)], { type: "application/json" })
    const url = URL.createObjectURL(blob)
    const link = document.createElement("a")
    link.href = url
    link.download = filename
    document.body.append(link)
    link.click()
    link.remove()
    setTimeout(() => URL.revokeObjectURL(url), 0)
}

function safeFilename(name, suffix) {
    const base = String(name || "starpoint-save").replace(/[^a-zA-Z0-9\u4e00-\u9fff._-]+/g, "-").slice(0, 48)
    return `${base || "starpoint-save"}${suffix}`
}

function renderDevices(devices) {
    elements.deviceList.replaceChildren()
    if (!devices.length) {
        elements.deviceList.append(makeElement("p", "empty-state", "当前 token 尚未绑定设备."))
        return
    }
    for (const device of devices) {
        const item = makeElement("div", "binding-list")
        item.append(makeElement("span", "meta", `设备 ${device.device_id}`))
        item.append(makeElement("span", "badge", `当前存档 #${device.active_slot_id}`))
        elements.deviceList.append(item)
    }
}

function renderSyncControls(state) {
    const targets = state.targets ?? []
    const slots = state.slots ?? []
    for (const select of [elements.syncUploadTarget, elements.syncDownloadTarget]) {
        select.replaceChildren()
        for (const target of targets) {
            const option = makeElement("option", "", `${target.name} (${target.scheme}://${target.host}:${target.port})`)
            option.value = String(target.id)
            select.append(option)
        }
        select.disabled = targets.length === 0
    }
    elements.syncUploadSlot.replaceChildren()
    for (const slot of slots) {
        const option = makeElement("option", "", slot.name)
        option.value = String(slot.id)
        elements.syncUploadSlot.append(option)
    }
    elements.syncUploadSlot.disabled = slots.length === 0
    elements.syncState.textContent = targets.length
        ? `${targets.length} 个密文服务器已就绪, 凭据由本地服务保管.`
        : "管理员尚未配置可用的密文服务器."
}

function renderSaves(state) {
    playerState = { ...state, targets: state.targets ?? playerState.targets ?? [] }
    renderDevices(state.devices ?? [])
    renderSyncControls(playerState)
    if (!elements.deviceId.value && state.devices?.[0]) {
        elements.deviceId.value = String(state.devices[0].device_id)
    }
    elements.playerSaveList.replaceChildren()
    if (!state.slots?.length) {
        elements.playerSaveList.append(makeElement("p", "empty-state", "当前 token 没有可用存档."))
        return
    }
    for (const slot of state.slots) {
        const card = makeElement("article", "save-card")
        const head = makeElement("div", "card-head")
        const title = makeElement("div")
        title.append(makeElement("h3", "card-title", slot.name))
        title.append(makeElement("p", "meta", `更新于 ${slot.updated_at}, 快照 ${slot.snapshot_count}`))
        head.append(title)
        const actions = makeElement("div", "save-actions")
        const exportButton = makeElement("button", "button ghost", "导出 JSON")
        exportButton.type = "button"
        exportButton.addEventListener("click", () => runAction(exportButton, () => exportSave(slot, false)))
        const encryptedButton = makeElement("button", "button ghost", "导出密文")
        encryptedButton.type = "button"
        encryptedButton.addEventListener("click", () => runAction(encryptedButton, () => exportSave(slot, true)))
        const activateButton = makeElement("button", "button primary", "激活此存档")
        activateButton.type = "button"
        activateButton.addEventListener("click", () => runAction(activateButton, () => activateSave(slot)))
        actions.append(exportButton, encryptedButton, activateButton)
        card.append(head, actions)
        elements.playerSaveList.append(card)
    }
}

async function refreshPlayerState() {
    elements.refreshButton.disabled = true
    try {
        const [state, targets] = await Promise.all([
            requestApi("/v1/player/local-saves"),
            requestApi("/v1/player/save-sync-targets"),
        ])
        renderSaves({ ...state, targets })
        elements.accessState.textContent = `${state.slots?.length ?? 0} 个存档槽, ${state.devices?.length ?? 0} 台设备`
        setConnected(true)
    } finally {
        elements.refreshButton.disabled = !playerToken
    }
}

async function exportSave(slot, encrypted) {
    const suffix = encrypted ? ".starpoint-save" : ".json"
    const path = `/v1/player/local-saves/${encodeURIComponent(slot.id)}/${encrypted ? "encrypted-export" : "export"}`
    const payload = await requestApi(path)
    downloadJson(safeFilename(slot.name, suffix), payload)
    return encrypted ? "密文存档已导出." : "存档 JSON 已导出."
}

async function activateSave(slot) {
    const deviceId = Number(elements.deviceId.value)
    if (!Number.isInteger(deviceId) || deviceId <= 0) throw new Error("请输入有效的设备 ID.")
    const state = await requestApi(`/v1/player/local-saves/${encodeURIComponent(slot.id)}/activate`, {
        method: "POST",
        body: { device_id: deviceId },
    })
    renderSaves(state)
    return `存档已激活到设备 ${deviceId}.`
}

elements.tokenForm.addEventListener("submit", async (event) => {
    event.preventDefault()
    playerToken = elements.tokenInput.value.trim()
    elements.tokenInput.value = ""
    if (!playerToken) return
    try {
        await refreshPlayerState()
    } catch (error) {
        playerToken = ""
        setConnected(false)
        showToast(error.message ?? String(error), true)
    }
})

elements.refreshButton.addEventListener("click", () => runAction(elements.refreshButton, async () => {
    await refreshPlayerState()
    return "存档状态已刷新."
}))

elements.importForm.addEventListener("submit", async (event) => {
    event.preventDefault()
    const formElement = event.currentTarget
    const form = new FormData(formElement)
    await runAction(event.submitter, async () => {
        const file = form.get("file")
        if (!(file instanceof File)) throw new Error("请选择存档文件.")
        const payload = JSON.parse(await file.text())
        const requestedName = String(form.get("name") ?? "").trim()
        if (payload?.format === "starpoint-encrypted-save") {
            await requestApi("/v1/player/local-saves/import-encrypted", {
                method: "POST",
                body: { name: requestedName, envelope: payload },
            })
        } else {
            await requestApi("/v1/player/local-saves/import", {
                method: "POST",
                body: { name: requestedName, data: payload },
            })
        }
        formElement.reset()
        await refreshPlayerState()
        return "存档已导入为新槽位."
    })
})

elements.syncUploadForm.addEventListener("submit", async (event) => {
    event.preventDefault()
    await runAction(event.submitter, async () => {
        const slotId = Number(elements.syncUploadSlot.value)
        const targetId = Number(elements.syncUploadTarget.value)
        const objectId = elements.syncUploadObject.value.trim()
        if (!Number.isInteger(slotId) || slotId <= 0) throw new Error("请选择要上传的存档.")
        if (!Number.isInteger(targetId) || targetId <= 0) throw new Error("请选择存档服务器.")
        if (!/^[A-Za-z0-9_-]{1,40}$/.test(objectId)) throw new Error("远端对象 ID 格式无效.")
        await requestApi(`/v1/player/local-saves/${encodeURIComponent(slotId)}/sync/upload`, {
            method: "POST",
            body: { target_id: targetId, object_id: objectId },
        })
        await refreshPlayerState()
        return "密文存档已上传."
    })
})

elements.syncDownloadForm.addEventListener("submit", async (event) => {
    event.preventDefault()
    await runAction(event.submitter, async () => {
        const targetId = Number(elements.syncDownloadTarget.value)
        const objectId = elements.syncDownloadObject.value.trim()
        const name = elements.syncDownloadName.value.trim()
        if (!Number.isInteger(targetId) || targetId <= 0) throw new Error("请选择存档服务器.")
        if (!/^[A-Za-z0-9_-]{1,40}$/.test(objectId)) throw new Error("远端对象 ID 格式无效.")
        if (!name) throw new Error("请输入新存档名称.")
        await requestApi("/v1/player/local-saves/sync/download", {
            method: "POST",
            body: { target_id: targetId, object_id: objectId, name },
        })
        await refreshPlayerState()
        return "密文存档已下载为新槽位."
    })
})

elements.recoveryExportForm.addEventListener("submit", async (event) => {
    event.preventDefault()
    await runAction(event.submitter, async () => {
        const password = elements.recoveryExportPassword.value
        if (password.length < 8) throw new Error("恢复密码需要至少 8 个字符.")
        const response = await requestApi("/v1/player/recovery/export", {
            method: "POST",
            body: { password },
        })
        downloadJson("starpoint-recovery.starpoint-recovery", response.package)
        elements.recoveryExportForm.reset()
        return "恢复包已导出."
    })
})

elements.recoveryImportForm.addEventListener("submit", async (event) => {
    event.preventDefault()
    await runAction(event.submitter, async () => {
        const password = elements.recoveryImportPassword.value
        const file = elements.recoveryImportFile.files?.[0]
        if (password.length < 8) throw new Error("恢复密码需要至少 8 个字符.")
        if (!(file instanceof File)) throw new Error("请选择恢复包文件.")
        const packageData = JSON.parse(await file.text())
        await requestApi("/v1/player/recovery/import", {
            method: "POST",
            body: { password, package: packageData?.package ?? packageData },
        })
        elements.recoveryImportForm.reset()
        return "恢复包已导入."
    })
})

setConnected(false)
