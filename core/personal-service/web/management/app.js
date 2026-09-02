// audience: external
// # personal-service-management-app
//
// 此脚本只调用当前 origin 的个人服务 API. 页面打开后直接载入本机管理状态.

import { createActivityController } from "/manage/activity-controller.js"
import { createAiTeamController } from "/manage/ai-team-controller.js"
import { createMailRewardController } from "/manage/mail-reward-controller.js"
import { renderManagement } from "/manage/views.js"

// //// 管理页面连接和表单提交 [@x380kkm 2026-07-24] ////
const elements = {
    aiTeamA: document.querySelector("#ai-team-a"),
    aiTeamB: document.querySelector("#ai-team-b"),
    aiTeamCandidates: document.querySelector("#ai-team-candidates"),
    aiTeamDefault: document.querySelector("#ai-team-default"),
    aiTeamSave: document.querySelector("#ai-team-save"),
    aiTeamSlot: document.querySelector("#ai-team-slot"),
    aiTeamState: document.querySelector("#ai-team-state"),
    activityCalendar: document.querySelector("#activity-calendar"),
    activityCalendarNext: document.querySelector("#activity-calendar-next"),
    activityCalendarNextYear: document.querySelector("#activity-calendar-next-year"),
    activityCalendarPrevious: document.querySelector("#activity-calendar-previous"),
    activityCalendarPreviousYear: document.querySelector("#activity-calendar-previous-year"),
    activityCalendarTitle: document.querySelector("#activity-calendar-title"),
    activityCalendarToday: document.querySelector("#activity-calendar-today"),
    activityCatalogList: document.querySelector("#activity-catalog-list"),
    activityCatalogRefresh: document.querySelector("#activity-catalog-refresh"),
    activityReset: document.querySelector("#activity-reset"),
    activityCatalogState: document.querySelector("#activity-catalog-state"),
    activityDateClear: document.querySelector("#activity-date-clear"),
    activityDateFrom: document.querySelector("#activity-date-from"),
    activityDateTo: document.querySelector("#activity-date-to"),
    activityDetail: document.querySelector("#activity-detail"),
    activityDetailBanner: document.querySelector("#activity-detail-banner"),
    activityDetailBannerPlaceholder: document.querySelector("#activity-detail-banner-placeholder"),
    activityDetailClose: document.querySelector("#activity-detail-close"),
    activityDetailDescription: document.querySelector("#activity-detail-description"),
    activityDetailMeta: document.querySelector("#activity-detail-meta"),
    activityDetailTags: document.querySelector("#activity-detail-tags"),
    activityDetailTitle: document.querySelector("#activity-detail-title"),
    activityFavoriteButton: document.querySelector("#activity-favorite-button"),
    activityFavoriteFilter: document.querySelector("#activity-favorite-filter"),
    activityEventId: document.querySelector("#activity-event-id"),
    activityForm: document.querySelector("#activity-form"),
    activityHp: document.querySelector("#activity-hp"),
    activityKills: document.querySelector("#activity-kills"),
    activityLoadButton: document.querySelector("#activity-load-button"),
    activityKindFilter: document.querySelector("#activity-kind-filter"),
    activityQuickFilters: document.querySelector("#activityQuickFilters"),
    activityMode: document.querySelector("#activity-mode"),
    activityModeForm: document.querySelector("#activity-mode-form"),
    activityOpenButton: document.querySelector("#activity-open-button"),
    activityCloseButton: document.querySelector("#activity-close-button"),
    activityPeriodForm: document.querySelector("#activity-period-form"),
    activityPeriodInterval: document.querySelector("#activity-period-interval"),
    activityPeriodKind: document.querySelector("#activity-period-kind"),
    activitySearch: document.querySelector("#activity-search"),
    activityStatusFilter: document.querySelector("#activity-status-filter"),
    activityTagFilters: document.querySelector("#activityTagFilters"),
    activityTemporaryState: document.querySelector("#activity-temporary-state"),
    activityState: document.querySelector("#activity-state"),
    activityWindowEnd: document.querySelector("#activity-window-end"),
    activityWindowForm: document.querySelector("#activity-window-form"),
    activityWindowStart: document.querySelector("#activity-window-start"),
    connectionState: document.querySelector("#connection-state"),
    deviceSelect: document.querySelector("#device-select"),
    importForm: document.querySelector("#import-form"),
    httpObservationList: document.querySelector("#http-observation-list"),
    gameplayDropMultiplier: document.querySelector("#gameplay-drop-multiplier"),
    gameplayDropMultiplierCurrent: document.querySelector("#gameplay-drop-multiplier-current"),
    gameplaySettingsForm: document.querySelector("#gameplay-settings-form"),
    gameplaySettingsState: document.querySelector("#gameplay-settings-state"),
    mailForm: document.querySelector("#mail-form"),
    mailCatalogState: document.querySelector("#mail-catalog-state"),
    mailList: document.querySelector("#mail-list"),
    mailLoadButton: document.querySelector("#mail-load-button"),
    mailPresetList: document.querySelector("#mail-preset-list"),
    mailRewardFavorites: document.querySelector("#mail-reward-favorites"),
    mailRewardKind: document.querySelector("#mail-reward-kind"),
    mailRewardList: document.querySelector("#mail-reward-list"),
    mailRewardSearch: document.querySelector("#mail-reward-search"),
    mailRewardsPayload: document.querySelector("#mail-rewards-payload"),
    mailSelectionClear: document.querySelector("#mail-selection-clear"),
    mailSelectionCount: document.querySelector("#mail-selection-count"),
    mailSelectionList: document.querySelector("#mail-selection-list"),
    mailSlotId: document.querySelector("#mail-slot-id"),
    mailState: document.querySelector("#mail-state"),
    mailViewerId: document.querySelector("#mail-viewer-id"),
    managementTabPanels: [...document.querySelectorAll("[data-management-tab-panel]")],
    managementTabs: [...document.querySelectorAll("[data-management-tab]")],
    playerAccessForm: document.querySelector("#player-access-form"),
    playerAccessState: document.querySelector("#player-access-state"),
    playerAccessToken: document.querySelector("#player-access-token"),
    playerAccessViewerId: document.querySelector("#player-access-viewer-id"),
    profileForm: document.querySelector("#server-profile-form"),
    profileList: document.querySelector("#server-profile-list"),
    refreshButton: document.querySelector("#refresh-button"),
    remotePlayerAccessPanel: document.querySelector("#remote-player-access-panel"),
    remoteRestorePanel: document.querySelector("#remote-restore-panel"),
    remoteSavePanel: document.querySelector("#remote-save-panel"),
    saveList: document.querySelector("#save-list"),
    shopStockForm: document.querySelector("#shop-stock-form"),
    shopStockSlotId: document.querySelector("#shop-stock-slot-id"),
    shopStockState: document.querySelector("#shop-stock-state"),
    shopStockViewerId: document.querySelector("#shop-stock-viewer-id"),
    syncDownloadForm: document.querySelector("#sync-download-form"),
    syncTargetForm: document.querySelector("#sync-target-form"),
    syncTargetList: document.querySelector("#sync-target-list"),
    timeEnabled: document.querySelector("#time-enabled"),
    timeForm: document.querySelector("#time-form"),
    timeRate: document.querySelector("#time-rate"),
    timeState: document.querySelector("#time-state"),
    timeValue: document.querySelector("#time-value"),
    toast: document.querySelector("#toast"),
}

const errorMessages = {
    save_sync_authentication_failed: "存档服务器拒绝了用户名或密码.",
    save_sync_remote_capacity_exceeded: "存档服务器容量已满.",
    save_sync_remote_conflict: "远端存档已经变化, 请先下载为新槽位后再处理.",
    save_sync_remote_not_found: "远端对象不存在.",
    save_sync_remote_unavailable: "无法连接存档服务器.",
    save_sync_target_unusable: "存档服务器地址不允许使用或无法解析.",
    save_sync_upload_worker_failed: "自动上传线程没有正常完成, 下次到期时会重试.",
    transfer_binding_disabled: "传输绑定已关闭, 手动授权传输仍然可用.",
    transfer_conflict: "两端存档都已变化, 已停止传输并创建冲突记录.",
    transfer_conflict_open: "两端存档都已变化, 请先处理传输冲突.",
    transfer_target_authentication_failed: "目标槽授权无效, 已过期或已撤销.",
    transfer_target_identity_mismatch: "目标服务实例、壳或槽身份与绑定不一致.",
    transfer_target_revision_conflict: "目标槽在上传期间再次变化, 请重新同步.",
    transfer_target_unavailable: "无法连接传输绑定的目标实例.",
    local_save_storage_failed: "本地存档快照写入失败.",
    save_sync_key_unavailable: "当前设备没有解密该存档所需的密钥.",
    server_profile_name_conflict: "游戏服务器名称已经存在.",
    server_profile_unreachable: "无法连接游戏服务器, 当前服务器保持不变.",
    server_profile_incompatible: "目标可以连接, 但不是兼容的 Starpoint 服务.",
    save_sync_target_name_conflict: "存档服务器名称已经存在.",
    invalid_virtual_time: "虚拟时间格式或推进倍率无效.",
    invalid_activity_catalog_filter: "活动筛选条件无效.",
    invalid_activity_catalog_manifest: "活动目录文件无效, 请重新执行资源提取.",
    invalid_activity_schedule: "活动时间窗口无效.",
    invalid_event_id: "活动 ID 无效.",
    invalid_raid_boss_state: "Raid Boss 状态无效.",
    invalid_local_save_id: "本地存档槽 ID 无效.",
    local_save_not_found: "本地存档槽不存在.",
    viewer_not_found: "Viewer ID 不存在或当前会话已失效.",
    activity_catalog_unavailable: "活动目录暂时不可用, 仍可以使用旧版活动状态管理.",
    activity_not_found: "活动不存在或已经从目录移除.",
    activity_action_unavailable: "当前服务版本不支持这个活动操作.",
    ai_team_snapshot_invalid_state: "当前存档无法建立 AI 编队快照.",
    shop_item_not_found: "商店商品不存在或当前版本未收录.",
}

const model = {
    automations: new Map(),
    bindings: new Map(),
    contexts: new Map(),
    httpObservations: [],
    gameplaySettings: { drop_multiplier: 1 },
    profiles: { active_profile_id: 0, profiles: [] },
    saves: { devices: [], slots: [] },
    time: { enabled: false, unix_time_ms: 0, iso: "", rate: 1 },
    targets: [],
    transferBindings: new Map(),
    transferConflicts: new Map(),
}

let toastTimer
let mailLoadGeneration = 0
let playerAccessGeneration = 0
let playerAccessOperationActive = false
const managementTabSessionKey = "starpoint-management-tab"

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
    elements.refreshButton.disabled = !connected
    elements.connectionState.textContent = connected ? "已连接本机" : "未连接"
    elements.connectionState.classList.toggle("offline", !connected)
}

function renderGameplaySettings(settings) {
    const multiplier = Number(settings?.drop_multiplier)
    const normalizedMultiplier = Number.isInteger(multiplier) && multiplier >= 1 && multiplier <= 100 ? multiplier : 1
    model.gameplaySettings = { drop_multiplier: normalizedMultiplier }
    elements.gameplayDropMultiplier.value = String(normalizedMultiplier)
    elements.gameplayDropMultiplierCurrent.textContent = `${normalizedMultiplier}x`
    elements.gameplaySettingsState.textContent = "普通道具, 元素素材和以太素材"
}

function storedManagementTab() {
    try {
        return sessionStorage.getItem(managementTabSessionKey)
    } catch {
        return null
    }
}

function storeManagementTab(tabName) {
    try {
        sessionStorage.setItem(managementTabSessionKey, tabName)
    } catch {
        return
    }
}

function activateManagementTab(tabName, focusTab = false) {
    const activeTab = elements.managementTabs.find((tab) => tab.dataset.managementTab === tabName)
        ?? elements.managementTabs[0]
    if (!activeTab) return

    const activeName = activeTab.dataset.managementTab
    for (const tab of elements.managementTabs) {
        const selected = tab === activeTab
        tab.setAttribute("aria-selected", String(selected))
        tab.tabIndex = selected ? 0 : -1
    }
    for (const panel of elements.managementTabPanels) {
        panel.hidden = panel.dataset.managementTabPanel !== activeName
    }
    storeManagementTab(activeName)
    if (focusTab) activeTab.focus()
}

function handleManagementTabKeydown(event) {
    const currentIndex = elements.managementTabs.indexOf(event.currentTarget)
    if (currentIndex < 0) return

    let targetIndex
    if (event.key === "Home") targetIndex = 0
    if (event.key === "End") targetIndex = elements.managementTabs.length - 1
    if (event.key === "ArrowLeft") targetIndex = (currentIndex - 1 + elements.managementTabs.length) % elements.managementTabs.length
    if (event.key === "ArrowRight") targetIndex = (currentIndex + 1) % elements.managementTabs.length
    if (targetIndex === undefined) return

    event.preventDefault()
    activateManagementTab(elements.managementTabs[targetIndex].dataset.managementTab, true)
}

function initializeManagementTabs() {
    for (const tab of elements.managementTabs) {
        tab.addEventListener("click", () => activateManagementTab(tab.dataset.managementTab))
        tab.addEventListener("keydown", handleManagementTabKeydown)
    }
    activateManagementTab(storedManagementTab() ?? "activity")
}

function renderTimeState(state) {
    model.time = state
    elements.timeEnabled.checked = state.enabled
    elements.timeValue.value = state.iso ? state.iso.slice(0, 19) : ""
    elements.timeRate.value = String(state.rate)
    elements.timeState.textContent = state.enabled
        ? `当前 ${state.iso}, 倍率 ${state.rate}x`
        : `当前 ${state.iso}, 使用设备时间`
}

function activityPath() {
    const eventId = Number(elements.activityEventId.value)
    if (!Number.isInteger(eventId) || eventId <= 0) throw new Error("请输入有效的活动 ID.")
    return `/v1/activities/raid-boss/${eventId}`
}

async function loadActivityState() {
    const state = await requestApi(activityPath())
    elements.activityHp.value = String(state.hp_percentage)
    elements.activityKills.value = String(state.total_kill_count)
    elements.activityState.textContent = `活动 ${state.event_id}: HP ${state.hp_percentage}%, 击杀 ${state.total_kill_count}`
    return state
}

function createMailElement(tagName, className, text) {
    const node = document.createElement(tagName)
    if (className) node.className = className
    if (text !== undefined) node.textContent = text
    return node
}

function renderManagedMails(mails) {
    elements.mailList.replaceChildren()
    if (mails.length === 0) {
        elements.mailList.append(createMailElement("p", "empty-state", "没有未领取邮件."))
        return
    }
    for (const mail of mails) {
        const card = createMailElement("article", "mail-item")
        const heading = createMailElement("div", "mail-item-head")
        heading.append(
            createMailElement("h4", "card-title", mail.title),
            createMailElement("span", "badge", `ID ${mail.id}`),
        )
        card.append(heading)
        card.append(createMailElement("p", "meta", `${mail.sender} / ${mail.created_at}`))
        card.append(createMailElement("p", "mail-body", mail.body))
        card.append(createMailElement("pre", "mail-reward-preview", JSON.stringify(mail.rewards, null, 2)))
        if (mail.expires_at !== null && mail.expires_at !== undefined) {
            card.append(createMailElement("p", "meta", `过期时间 ${new Date(mail.expires_at * 1000).toISOString()}`))
        }
        elements.mailList.append(card)
    }
}

async function loadManagedMails() {
    const context = selectedMailSlotContext()
    const generation = ++mailLoadGeneration
    const slotId = context.slot.id
    const mails = await requestApi(`/v1/local-saves/${context.slot.id}/mails`)
    if (generation !== mailLoadGeneration || Number(elements.mailSlotId.value) !== slotId) return
    renderManagedMails(mails)
    elements.mailState.textContent = `${context.slot.name}, ${mails.length} 封未领取邮件`
}

function selectedMailSlotContext() {
    const slotId = Number(elements.mailSlotId.value)
    const context = model.contexts.get(slotId)
    if (!context) throw new Error("请选择有效的本地存档槽.")
    return context
}

function renderSelectedMailSlotContext() {
    mailLoadGeneration += 1
    playerAccessGeneration += 1
    const context = model.contexts.get(Number(elements.mailSlotId.value))
    elements.mailViewerId.value = context?.viewer_id ?? ""
    elements.playerAccessViewerId.value = context?.viewer_id ?? ""
    elements.playerAccessToken.value = ""
    elements.playerAccessState.textContent = context?.viewer_id
        ? `Viewer ${context.viewer_id} 尚未签发远程存档授权`
        : "当前槽尚未建立 Viewer 会话"
    elements.mailList.replaceChildren(createMailElement("p", "empty-state", "尚未加载邮件."))
    if (!context) {
        elements.mailState.textContent = "当前没有可管理的本地存档槽"
        return
    }
    const viewerText = context.viewer_id === null || context.viewer_id === undefined
        ? "尚未建立 Viewer 会话"
        : `Viewer ${context.viewer_id}`
    elements.mailState.textContent = `${context.slot.name}, ${viewerText}`
}

function renderMailSlotContexts(saves) {
    const selectedSlotId = Number(elements.mailSlotId.value)
    const selectedDeviceId = Number(elements.deviceSelect.value)
    const activeSlotId = saves.devices.find((device) => device.device_id === selectedDeviceId)?.active_slot_id
        ?? saves.devices[0]?.active_slot_id
    elements.mailSlotId.replaceChildren()
    for (const slot of saves.slots) {
        const option = document.createElement("option")
        option.value = String(slot.id)
        option.textContent = `${slot.name} (槽位 ${slot.id})`
        elements.mailSlotId.append(option)
    }
    const nextSlotId = model.contexts.has(selectedSlotId)
        ? selectedSlotId
        : model.contexts.has(activeSlotId)
            ? activeSlotId
            : saves.slots[0]?.id
    if (nextSlotId !== undefined) elements.mailSlotId.value = String(nextSlotId)
    renderSelectedMailSlotContext()
}

function renderShopStockSlotContexts(saves) {
    const selectedSlotId = Number(elements.shopStockSlotId.value)
    const selectedDeviceId = Number(elements.deviceSelect.value)
    const activeSlotId = saves.devices.find((device) => device.device_id === selectedDeviceId)?.active_slot_id
        ?? saves.devices[0]?.active_slot_id
    elements.shopStockSlotId.replaceChildren()
    for (const slot of saves.slots) {
        const option = document.createElement("option")
        option.value = String(slot.id)
        option.textContent = `${slot.name} (槽位 ${slot.id})`
        elements.shopStockSlotId.append(option)
    }
    const nextSlotId = saves.slots.some((slot) => slot.id === selectedSlotId)
        ? selectedSlotId
        : saves.slots.some((slot) => slot.id === activeSlotId)
            ? activeSlotId
            : saves.slots[0]?.id
    if (nextSlotId !== undefined) elements.shopStockSlotId.value = String(nextSlotId)
    updateShopStockViewer()
}

function updateShopStockViewer() {
    const context = model.contexts.get(Number(elements.shopStockSlotId.value))
    const viewerId = Number(context?.viewer_id)
    const hasViewer = Number.isInteger(viewerId) && viewerId > 0
    elements.shopStockViewerId.value = hasViewer ? String(viewerId) : ""
    elements.shopStockState.textContent = hasViewer
        ? `Viewer ${viewerId}, 选择商品后刷新当前周期库存`
        : "当前存档尚未建立 Viewer 会话"
}

async function requestApi(requestPath, options = {}) {
    const headers = {}
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
        if (control) control.disabled = false
    }
}

const activityController = createActivityController({
    elements,
    requestApi,
    ApiError,
    runAction,
})

const aiTeamController = createAiTeamController({
    elements,
    requestApi,
    runAction,
})

const mailRewardController = createMailRewardController({
    elements,
    requestApi,
    runAction,
    sendMail: sendManagedMail,
})

async function sendManagedMail(body) {
    const context = selectedMailSlotContext()
    await requestApi(`/v1/local-saves/${context.slot.id}/mails`, { method: "POST", body })
    await loadManagedMails()
}

async function runPlayerAccessAction(action) {
    if (playerAccessOperationActive) return
    playerAccessOperationActive = true
    const controls = elements.playerAccessForm.querySelectorAll('button[type="submit"]')
    for (const control of controls) control.disabled = true
    try {
        await runAction(null, action)
    } finally {
        for (const control of controls) control.disabled = false
        playerAccessOperationActive = false
    }
}

async function refreshManagementState() {
    elements.refreshButton.disabled = true
    try {
        const [profiles, saves, targets, time, httpObservations, gameplaySettings] = await Promise.all([
            requestApi("/v1/server-profiles"),
            requestApi("/v1/local-saves"),
            requestApi("/v1/save-sync-targets"),
            requestApi("/v1/time"),
            requestApi("/v1/http-observations"),
            requestApi("/v1/gameplay-settings"),
        ])
        model.profiles = profiles
        model.saves = saves
        model.targets = targets
        model.httpObservations = httpObservations.observations
        renderGameplaySettings(gameplaySettings)
        renderTimeState(time)
        activityController.setCurrentTime(time.unix_time_ms)
        const slotEntries = await Promise.all(saves.slots.map(async (slot) => {
            const [bindings, automation, transferBindings, context] = await Promise.all([
                requestApi(`/v1/local-saves/${slot.id}/sync-bindings`),
                requestApi(`/v1/local-saves/${slot.id}/automation`),
                requestApi(`/v1/local-saves/${slot.id}/transfer-bindings`),
                requestApi(`/v1/local-saves/${slot.id}/context`),
            ])
            const conflictEntries = await Promise.all(transferBindings.map(async (binding) => [
                binding.binding_id,
                await requestApi(`/v1/local-saves/${slot.id}/transfer-bindings/${binding.binding_id}/conflicts`),
            ]))
            return [slot.id, { automation, bindings, conflictEntries, context, transferBindings }]
        }))
        model.bindings = new Map(slotEntries.map(([slotId, state]) => [slotId, state.bindings]))
        model.automations = new Map(slotEntries.map(([slotId, state]) => [slotId, state.automation]))
        model.contexts = new Map(slotEntries.map(([slotId, state]) => [slotId, state.context]))
        model.transferBindings = new Map(slotEntries.map(([slotId, state]) => [slotId, state.transferBindings]))
        model.transferConflicts = new Map(slotEntries.flatMap(([, state]) => state.conflictEntries))
        renderManagement(model, elements, {
            describeError: (code) => errorMessages[code] ?? code,
            refreshManagementState,
            requestApi,
            runAction,
        })
        const selectedDeviceId = Number(elements.deviceSelect.value)
        const activeSlotId = saves.devices.find((device) => device.device_id === selectedDeviceId)?.active_slot_id
            ?? saves.devices[0]?.active_slot_id
        aiTeamController.setSlots(saves.slots, activeSlotId)
        await Promise.all([
            activityController.load(),
            aiTeamController.load(),
            mailRewardController.load(),
        ])
        renderMailSlotContexts(saves)
        renderShopStockSlotContexts(saves)
        setConnected(true)
    } finally {
        elements.refreshButton.disabled = false
    }
}

elements.refreshButton.addEventListener("click", () => runAction(elements.refreshButton, async () => {
    await refreshManagementState()
    return "状态已刷新."
}))

elements.profileForm.addEventListener("submit", async (event) => {
    event.preventDefault()
    const formElement = event.currentTarget
    const form = new FormData(formElement)
    await runAction(event.submitter, async () => {
        await requestApi("/v1/server-profiles", {
            method: "POST",
            body: {
                name: form.get("name"),
                scheme: form.get("scheme"),
                host: form.get("host"),
                port: Number(form.get("port")),
            },
        })
        formElement.reset()
        await refreshManagementState()
        return "游戏服务器已保存."
    })
})

elements.syncTargetForm.addEventListener("submit", async (event) => {
    event.preventDefault()
    const formElement = event.currentTarget
    const form = new FormData(formElement)
    await runAction(event.submitter, async () => {
        await requestApi("/v1/save-sync-targets", {
            method: "POST",
            body: {
                name: form.get("name"),
                scheme: form.get("scheme"),
                host: form.get("host"),
                port: Number(form.get("port")),
                username: form.get("username"),
                password: form.get("password"),
            },
        })
        formElement.reset()
        await refreshManagementState()
        return "存档服务器已保存."
    })
})

elements.timeForm.addEventListener("submit", async (event) => {
    event.preventDefault()
    await runAction(event.submitter, async () => {
        const enabled = elements.timeEnabled.checked
        const rate = Number(elements.timeRate.value)
        const body = { enabled, rate }
        if (enabled) {
            const date = new Date(`${elements.timeValue.value}Z`)
            if (!Number.isFinite(date.getTime())) throw new Error("请输入有效的 UTC 时间.")
            body.iso = date.toISOString()
        }
        await requestApi("/v1/time", { method: "PUT", body })
        activityController.resetCalendar()
        await refreshManagementState()
        return "虚拟时间设置已保存."
    })
})

elements.gameplaySettingsForm.addEventListener("submit", async (event) => {
    event.preventDefault()
    await runAction(event.submitter, async () => {
        const multiplier = Number(elements.gameplayDropMultiplier.value)
        if (!Number.isInteger(multiplier) || multiplier < 1 || multiplier > 100) {
            throw new Error("掉落倍率必须是 1 到 100 的整数.")
        }
        const settings = await requestApi("/v1/gameplay-settings", {
            method: "PUT",
            body: { drop_multiplier: multiplier },
        })
        renderGameplaySettings(settings)
        await refreshManagementState()
        return "掉落倍率已保存."
    })
})

elements.activityLoadButton.addEventListener("click", () => runAction(elements.activityLoadButton, async () => {
    await loadActivityState()
    return "活动状态已读取."
}))

elements.activityForm.addEventListener("submit", async (event) => {
    event.preventDefault()
    await runAction(event.submitter, async () => {
        const eventId = Number(elements.activityEventId.value)
        const hpPercentage = Number(elements.activityHp.value)
        const totalKillCount = Number(elements.activityKills.value)
        if (!Number.isInteger(eventId) || eventId <= 0) throw new Error("请输入有效的活动 ID.")
        if (!Number.isInteger(hpPercentage) || hpPercentage < 0 || hpPercentage > 100) throw new Error("请输入 0 到 100 之间的 Boss HP.")
        if (!Number.isInteger(totalKillCount) || totalKillCount < 0) throw new Error("请输入非负击杀数.")
        const state = await requestApi(`/v1/activities/raid-boss/${eventId}`, {
            method: "PUT",
            body: { hp_percentage: hpPercentage, total_kill_count: totalKillCount },
        })
        elements.activityState.textContent = `活动 ${state.event_id}: HP ${state.hp_percentage}%, 击杀 ${state.total_kill_count}`
        return "活动状态已保存."
    })
})

elements.mailLoadButton.addEventListener("click", () => runAction(elements.mailLoadButton, async () => {
    await loadManagedMails()
    return "邮件列表已刷新."
}))

elements.mailSlotId.addEventListener("change", renderSelectedMailSlotContext)

elements.shopStockSlotId.addEventListener("change", updateShopStockViewer)

elements.deviceSelect.addEventListener("change", () => {
    const selectedDeviceId = Number(elements.deviceSelect.value)
    const activeSlotId = model.saves.devices.find((device) => device.device_id === selectedDeviceId)?.active_slot_id
    if (activeSlotId === undefined || !model.contexts.has(activeSlotId)) return
    elements.mailSlotId.value = String(activeSlotId)
    renderSelectedMailSlotContext()
    if (model.contexts.has(activeSlotId)) {
        elements.shopStockSlotId.value = String(activeSlotId)
        updateShopStockViewer()
    }
})

elements.shopStockForm.addEventListener("submit", async (event) => {
    event.preventDefault()
    const formElement = event.currentTarget
    const form = new FormData(formElement)
    await runAction(event.submitter, async () => {
        const context = model.contexts.get(Number(form.get("slot_id")))
        const viewerId = Number(context?.viewer_id)
        const shopType = Number(form.get("shop_type"))
        const shopItemId = Number(form.get("shop_item_id"))
        if (!Number.isInteger(viewerId) || viewerId <= 0) throw new Error("当前存档尚未建立 Viewer 会话.")
        if (!Number.isInteger(shopType) || shopType <= 0) throw new Error("请选择有效的商店类型.")
        if (!Number.isInteger(shopItemId) || shopItemId <= 0) throw new Error("请输入有效的商品 ID.")
        const result = await requestApi("/v1/shop-stock/refresh", {
            method: "POST",
            body: { viewer_id: viewerId, shop_type: shopType, shop_item_id: shopItemId },
        })
        elements.shopStockState.textContent = `Viewer ${viewerId}, 商品 ${shopItemId}: 当前周期已购买 ${result.stock_purchase_num ?? 0} 次, 历史累计 ${result.historical_purchase_num ?? 0} 次`
        return "商店库存已刷新."
    })
})

elements.mailForm.addEventListener("submit", async (event) => {
    event.preventDefault()
    const formElement = event.currentTarget
    const form = new FormData(formElement)
    await runAction(event.submitter, async () => {
        if (!mailRewardController.hasSelection()) throw new Error("请至少选择一项邮件奖励.")
        let rewards
        try {
            rewards = JSON.parse(String(form.get("rewards") ?? ""))
        } catch {
            throw new Error("当前奖励选择无效, 请重新添加奖励.")
        }
        const expiresText = String(form.get("expires_at") ?? "").trim()
        let expiresAt
        if (expiresText) {
            const expiresDate = new Date(`${expiresText}Z`)
            if (!Number.isFinite(expiresDate.getTime())) throw new Error("请输入有效的 UTC 过期时间.")
            expiresAt = Math.floor(expiresDate.getTime() / 1000)
        }
        const body = {
            title: String(form.get("title") ?? ""),
            body: String(form.get("body") ?? ""),
            sender: String(form.get("sender") ?? ""),
            rewards,
        }
        if (expiresAt !== undefined) body.expires_at = expiresAt
        await sendManagedMail(body)
        mailRewardController.clearSelection()
        return "本地邮件已发放."
    })
})

elements.playerAccessForm.addEventListener("submit", async (event) => {
    event.preventDefault()
    const viewerId = Number(elements.playerAccessViewerId.value)
    const generation = ++playerAccessGeneration
    await runPlayerAccessAction(async () => {
        if (!Number.isInteger(viewerId) || viewerId <= 0) throw new Error("请输入有效的 viewer ID.")
        const action = event.submitter?.value
        if (action === "revoke") {
            await requestApi(`/v1/player-access/${viewerId}`, { method: "DELETE" })
            if (generation !== playerAccessGeneration
                || Number(elements.playerAccessViewerId.value) !== viewerId) return undefined
            elements.playerAccessToken.value = ""
            elements.playerAccessState.textContent = `Viewer ${viewerId} 的远程存档授权已撤销.`
            return "远程存档授权已撤销."
        }
        const result = await requestApi("/v1/player-access", {
            method: "POST",
            body: { viewer_id: viewerId },
        })
        if (generation !== playerAccessGeneration
            || Number(elements.playerAccessViewerId.value) !== viewerId) return undefined
        elements.playerAccessToken.value = result.token ?? ""
        elements.playerAccessState.textContent = `Viewer ${viewerId} 的远程存档授权已签发, 只在当前页面显示.`
        return "远程存档授权已签发."
    })
})

elements.importForm.addEventListener("submit", async (event) => {
    event.preventDefault()
    const formElement = event.currentTarget
    const form = new FormData(formElement)
    await runAction(event.submitter, async () => {
        const file = form.get("file")
        if (!(file instanceof File)) throw new Error("请选择存档文件.")
        const name = String(form.get("name") ?? "").trim()
        const payload = JSON.parse(await file.text())
        if (payload.format === "starpoint-encrypted-save") {
            await requestApi("/v1/local-saves/import-encrypted", {
                method: "POST",
                body: { name, envelope: payload },
            })
        } else {
            await requestApi("/v1/local-saves/import", {
                method: "POST",
                body: { name, data: payload },
            })
        }
        formElement.reset()
        await refreshManagementState()
        return "存档已导入为新槽位."
    })
})

elements.syncDownloadForm.addEventListener("submit", async (event) => {
    event.preventDefault()
    const formElement = event.currentTarget
    const form = new FormData(formElement)
    await runAction(event.submitter, async () => {
        await requestApi("/v1/local-saves/sync/download", {
            method: "POST",
            body: {
                target_id: Number(form.get("target_id")),
                object_id: form.get("object_id"),
                name: form.get("name"),
            },
        })
        formElement.reset()
        await refreshManagementState()
        return "远端存档已下载为隔离槽位."
    })
})

async function initialize() {
    initializeManagementTabs()
    setConnected(false)
    if (location.hash) history.replaceState(null, "", `${location.pathname}${location.search}`)
    try {
        await refreshManagementState()
    } catch (error) {
        setConnected(false)
        showToast(error.message ?? String(error), true)
    }
}

initialize()
// //// /管理页面连接和表单提交 ////
