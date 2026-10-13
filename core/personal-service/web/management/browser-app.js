// audience: internal
// # personal-service-browser-bundle
// 此文件由 build-personal-service-web.mjs 从页面源码生成.
// audience: internal
// # personal-service-browser-compatibility
// 管理页与玩家页共用 ES2017 运行时. 补充接口保持浏览器已有实现.

// //// 补充页面使用的集合与文档接口 [@x380kkm 2026-09-26] ////
(function (root) {
    function define(target, name, value) {
        if (!target[name]) Object.defineProperty(target, name, { configurable: true, writable: true, value });
    }
    define(root, "globalThis", root);
    define(Object, "fromEntries", function (entries) {
        const result = {};
        for (const entry of entries) {
            Object.defineProperty(result, entry[0], { configurable: true, enumerable: true, writable: true, value: entry[1] });
        }
        return result;
    });
    define(Array.prototype, "flatMap", function (mapper, thisArg) {
        return this.reduce(function (result, value, index, source) {
            return result.concat(mapper.call(thisArg, value, index, source));
        }, []);
    });
    for (const constructor of [root.Element, root.Document, root.DocumentFragment]) {
        if (!constructor) continue;
        define(constructor.prototype, "replaceChildren", function () {
            const fragment = (this.ownerDocument || this).createDocumentFragment();
            for (const value of arguments) {
                fragment.appendChild(value instanceof root.Node ? value : (this.ownerDocument || this).createTextNode(String(value)));
            }
            while (this.firstChild) this.removeChild(this.firstChild);
            this.appendChild(fragment);
        });
    }
    if (root.Blob) define(root.Blob.prototype, "text", function () {
        const blob = this;
        return new Promise(function (resolve, reject) {
            const reader = new root.FileReader();
            reader.onload = function () { resolve(reader.result); };
            reader.onerror = function () { reject(reader.error || new Error("文件读取失败.")); };
            reader.onabort = function () { reject(new Error("文件读取已取消.")); };
            reader.readAsText(blob, "UTF-8");
        });
    });
    if (root.SubmitEvent && "submitter" in root.SubmitEvent.prototype) return;
    const submitters = new WeakMap();
    root.document.addEventListener("click", function (event) {
        const control = event.target.closest && event.target.closest("button, input");
        if (!control || !control.form || !["submit", "image"].includes(control.type)) return;
        submitters.set(control.form, control);
        root.setTimeout(function () {
            if (submitters.get(control.form) === control) submitters.delete(control.form);
        }, 0);
    }, true);
    root.document.addEventListener("submit", function (event) {
        if ("submitter" in event) return;
        const form = event.target;
        const control = submitters.get(form) || Array.from(form.elements).find(function (element) {
            return element.type === "submit" && !element.disabled;
        }) || null;
        Object.defineProperty(event, "submitter", { value: control });
        submitters.delete(form);
    }, true);
})(window);

(function () {
"use strict";
const modules = {
"/manage/app.js": function(module, exports, require) {
"use strict";
// audience: external
// # personal-service-management-app
//
// 此脚本只调用当前 origin 的个人服务 API. 页面打开后直接载入本机管理状态.
Object.defineProperty(exports, "__esModule", { value: true });
const activity_controller_js_1 = require("/manage/activity-controller.js");
const activity_history_js_1 = require("/manage/activity-history.js");
const ai_team_controller_js_1 = require("/manage/ai-team-controller.js");
const rescue_controller_js_1 = require("/manage/rescue-controller.js");
const activity_rescue_js_1 = require("/manage/activity-rescue.js");
const mail_reward_controller_js_1 = require("/manage/mail-reward-controller.js");
const reward_history_js_1 = require("/manage/reward-history.js");
const competition_controller_js_1 = require("/manage/competition-controller.js");
const views_js_1 = require("/manage/views.js");
const password_backup_js_1 = require("/manage/password-backup.js");
// //// 管理页面连接和表单提交 [@x380kkm 2026-07-24] ////
const elements = {
    aiTeamA: document.querySelector("#ai-team-a"),
    aiTeamB: document.querySelector("#ai-team-b"),
    aiTeamCandidates: document.querySelector("#ai-team-candidates"),
    aiTeamDefault: document.querySelector("#ai-team-default"),
    aiTeamFollow: document.querySelector("#ai-team-follow"),
    aiTeamSave: document.querySelector("#ai-team-save"),
    aiTeamSlot: document.querySelector("#ai-team-slot"),
    aiTeamSource: document.querySelector("#ai-team-source"),
    aiTeamState: document.querySelector("#ai-team-state"),
    activityCalendar: document.querySelector("#activity-calendar"),
    activityCalendarToolbar: document.querySelector("#activity-calendar-toolbar"),
    activitySections: [...document.querySelectorAll("[data-activity-section]")],
    activityGroup: document.querySelector("#activity-group"),
    activityDifficulty: document.querySelector("#activity-difficulty"),
    activityDifficultyLabel: document.querySelector("#activity-difficulty-label"),
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
};
const errorMessages = {
    recovery_password_invalid: "密码需为 8 到 128 个字符.",
    recovery_package_invalid: "密码不正确或备份文件已损坏.",
    invalid_encrypted_local_save: "此加密备份需要原设备的恢复包和密码. 请在下方选择恢复包.",
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
    local_save_active: "此存档正在使用. 请先为使用它的设备切换到其他存档.",
    local_save_last_slot: "请先导入另一份存档并启用, 再删除此存档.",
    local_save_busy: "此存档仍有战斗或联机房间在使用. 请先退出战斗和房间.",
    local_save_import_blocked: "此存档正在战斗或联机房间中使用, 退出后可恢复或同步.",
    save_revision_conflict: "存档已变化, 请重新查看版本历史后再选择恢复.",
    invalid_transfer_binding: "请检查目标实例, 槽授权和同步模式.",
    invalid_transfer_slot_token: "请检查槽授权权限和到期时间.",
    transfer_binding_changed: "同步设置已变化, 请刷新后重试.",
    local_save_delete_confirmation_required: "请在删除确认窗口中确认目标存档.",
    local_save_delete_confirmation_mismatch: "存档名称已变化. 请刷新后重新确认删除.",
    viewer_not_found: "Viewer ID 不存在或当前会话已失效.",
    activity_catalog_unavailable: "活动目录暂时不可用, 仍可以使用旧版活动状态管理.",
    activity_not_found: "活动不存在或已经从目录移除.",
    activity_action_unavailable: "当前服务版本不支持这个活动操作.",
    activity_round_changed: "活动轮次已经变化, 请刷新记录后重新确认.",
    competition_period_changed: "活动期别已经变化, 请刷新竞赛信息后重新操作.",
    invalid_reward_selection: "请选择当前活动目录中的奖励档位或有效排名.",
    invalid_reward_mode: "请选择游戏内领取或管理页手动发奖.",
    invalid_server_state: "请填写有效的全服讨伐数和血量百分比.",
    competition_not_found: "该竞赛活动不存在, 请刷新目录.",
    activity_battle_active: "该活动仍有战斗进行中, 请先结束战斗.",
    activity_reset_unavailable: "此活动类型没有可重置的参赛记录.",
    ai_team_snapshot_invalid_state: "当前存档无法建立 AI 编队快照.",
    invalid_rescue_settings: "救援间隔, 空房保留时间或房间数超出允许范围.",
    invalid_rescue_quest: "救援关卡已变化, 请刷新目录后重新选择.",
    rescue_quest_closed: "该关卡当前关闭, 开放后可以开启铃铛.",
    shop_item_not_found: "商店商品不存在或当前版本未收录.",
};
const model = {
    automations: new Map(),
    bindings: new Map(),
    contexts: new Map(),
    httpObservations: [],
    clientReports: [],
    gameplaySettings: { drop_multiplier: 1 },
    profiles: { active_profile_id: 0, profiles: [] },
    saves: { devices: [], slots: [] },
    time: { enabled: false, unix_time_ms: 0, iso: "", rate: 1 },
    targets: [],
    transferBindings: new Map(),
    transferConflicts: new Map(),
};
let toastTimer;
let mailLoadGeneration = 0;
let playerAccessGeneration = 0;
let playerAccessOperationActive = false;
const managementTabSessionKey = "starpoint-management-tab";
class ApiError extends Error {
    constructor(status, code) {
        var _a, _b;
        super((_b = (_a = errorMessages[code]) !== null && _a !== void 0 ? _a : code) !== null && _b !== void 0 ? _b : `HTTP ${status}`);
        this.status = status;
        this.code = code;
    }
}
function showToast(message, isError = false) {
    clearTimeout(toastTimer);
    elements.toast.textContent = message;
    elements.toast.classList.toggle("error", isError);
    elements.toast.hidden = false;
    toastTimer = setTimeout(() => {
        elements.toast.hidden = true;
    }, 4200);
}
function setConnected(connected) {
    elements.refreshButton.disabled = !connected;
    elements.connectionState.textContent = connected ? "已连接本机" : "未连接";
    elements.connectionState.classList.toggle("offline", !connected);
}
function renderGameplaySettings(settings) {
    const multiplier = Number(settings === null || settings === void 0 ? void 0 : settings.drop_multiplier);
    const normalizedMultiplier = Number.isInteger(multiplier) && multiplier >= 1 && multiplier <= 100 ? multiplier : 1;
    model.gameplaySettings = { drop_multiplier: normalizedMultiplier };
    elements.gameplayDropMultiplier.value = String(normalizedMultiplier);
    elements.gameplayDropMultiplierCurrent.textContent = `${normalizedMultiplier}x`;
    elements.gameplaySettingsState.textContent = "普通道具, 元素素材和以太素材";
}
function storedManagementTab() {
    try {
        return sessionStorage.getItem(managementTabSessionKey);
    }
    catch (_a) {
        return null;
    }
}
function storeManagementTab(tabName) {
    try {
        sessionStorage.setItem(managementTabSessionKey, tabName);
    }
    catch (_a) {
        return;
    }
}
function activateManagementTab(tabName, focusTab = false) {
    var _a;
    const activeTab = (_a = elements.managementTabs.find((tab) => tab.dataset.managementTab === tabName)) !== null && _a !== void 0 ? _a : elements.managementTabs[0];
    if (!activeTab)
        return;
    const activeName = activeTab.dataset.managementTab;
    for (const tab of elements.managementTabs) {
        const selected = tab === activeTab;
        tab.setAttribute("aria-selected", String(selected));
        tab.tabIndex = selected ? 0 : -1;
    }
    for (const panel of elements.managementTabPanels) {
        panel.hidden = panel.dataset.managementTabPanel !== activeName;
    }
    storeManagementTab(activeName);
    if (activeName === "competition")
        competitionController.activate();
    activeTab.scrollIntoView({ block: "nearest", inline: "nearest" });
    if (focusTab)
        activeTab.focus();
}
function handleManagementTabKeydown(event) {
    const currentIndex = elements.managementTabs.indexOf(event.currentTarget);
    if (currentIndex < 0)
        return;
    let targetIndex;
    if (event.key === "Home")
        targetIndex = 0;
    if (event.key === "End")
        targetIndex = elements.managementTabs.length - 1;
    if (event.key === "ArrowLeft")
        targetIndex = (currentIndex - 1 + elements.managementTabs.length) % elements.managementTabs.length;
    if (event.key === "ArrowRight")
        targetIndex = (currentIndex + 1) % elements.managementTabs.length;
    if (targetIndex === undefined)
        return;
    event.preventDefault();
    activateManagementTab(elements.managementTabs[targetIndex].dataset.managementTab, true);
}
function initializeManagementTabs() {
    var _a;
    for (const tab of elements.managementTabs) {
        tab.addEventListener("click", () => activateManagementTab(tab.dataset.managementTab));
        tab.addEventListener("keydown", handleManagementTabKeydown);
    }
    activateManagementTab((_a = storedManagementTab()) !== null && _a !== void 0 ? _a : "activity");
}
function renderTimeState(state) {
    model.time = state;
    elements.timeEnabled.checked = state.enabled;
    elements.timeValue.value = state.iso ? state.iso.slice(0, 19) : "";
    elements.timeRate.value = String(state.rate);
    elements.timeState.textContent = state.enabled
        ? `当前 ${state.iso}, 倍率 ${state.rate}x`
        : `当前 ${state.iso}, 使用设备时间`;
}
function activityPath() {
    const eventId = Number(elements.activityEventId.value);
    if (!Number.isInteger(eventId) || eventId <= 0)
        throw new Error("请输入有效的活动 ID.");
    return `/v1/activities/raid-boss/${eventId}`;
}
async function loadActivityState() {
    const state = await requestApi(activityPath());
    elements.activityHp.value = String(state.hp_percentage);
    elements.activityKills.value = String(state.total_kill_count);
    elements.activityState.textContent = `活动 ${state.event_id}: HP ${state.hp_percentage}%, 击杀 ${state.total_kill_count}`;
    return state;
}
function createMailElement(tagName, className, text) {
    const node = document.createElement(tagName);
    if (className)
        node.className = className;
    if (text !== undefined)
        node.textContent = text;
    return node;
}
function renderManagedMails(mails) {
    elements.mailList.replaceChildren();
    if (mails.length === 0) {
        elements.mailList.append(createMailElement("p", "empty-state", "没有未领取邮件."));
        return;
    }
    for (const mail of mails) {
        const card = createMailElement("article", "mail-item");
        const heading = createMailElement("div", "mail-item-head");
        heading.append(createMailElement("h4", "card-title", mail.title), createMailElement("span", "badge", `ID ${mail.id}`));
        card.append(heading);
        card.append(createMailElement("p", "meta", `${mail.sender} / ${mail.created_at}`));
        card.append(createMailElement("p", "mail-body", mail.body));
        card.append(createMailElement("pre", "mail-reward-preview", JSON.stringify(mail.rewards, null, 2)));
        if (mail.expires_at !== null && mail.expires_at !== undefined) {
            card.append(createMailElement("p", "meta", `过期时间 ${new Date(mail.expires_at * 1000).toISOString()}`));
        }
        elements.mailList.append(card);
    }
}
async function loadManagedMails() {
    const context = selectedMailSlotContext();
    const generation = ++mailLoadGeneration;
    const slotId = context.slot.id;
    const mails = await requestApi(`/v1/local-saves/${context.slot.id}/mails`);
    if (generation !== mailLoadGeneration || Number(elements.mailSlotId.value) !== slotId)
        return;
    renderManagedMails(mails);
    elements.mailState.textContent = `${context.slot.name}, ${mails.length} 封未领取邮件`;
}
function selectedMailSlotContext() {
    const slotId = Number(elements.mailSlotId.value);
    const context = model.contexts.get(slotId);
    if (!context)
        throw new Error("请选择有效的本地存档槽.");
    return context;
}
function renderSelectedMailSlotContext() {
    var _a, _b;
    mailLoadGeneration += 1;
    playerAccessGeneration += 1;
    const context = model.contexts.get(Number(elements.mailSlotId.value));
    elements.mailViewerId.value = (_a = context === null || context === void 0 ? void 0 : context.viewer_id) !== null && _a !== void 0 ? _a : "";
    elements.playerAccessViewerId.value = (_b = context === null || context === void 0 ? void 0 : context.viewer_id) !== null && _b !== void 0 ? _b : "";
    elements.playerAccessToken.value = "";
    elements.playerAccessState.textContent = (context === null || context === void 0 ? void 0 : context.viewer_id)
        ? `Viewer ${context.viewer_id} 尚未签发远程存档授权`
        : "当前槽尚未建立 Viewer 会话";
    elements.mailList.replaceChildren(createMailElement("p", "empty-state", "尚未加载邮件."));
    if (!context) {
        elements.mailState.textContent = "当前没有可管理的本地存档槽";
        return;
    }
    const viewerText = context.viewer_id === null || context.viewer_id === undefined
        ? "尚未建立 Viewer 会话"
        : `Viewer ${context.viewer_id}`;
    elements.mailState.textContent = `${context.slot.name}, ${viewerText}`;
}
function renderMailSlotContexts(saves) {
    var _a;
    const selectedSlotId = Number(elements.mailSlotId.value);
    const activeSlotId = (0, views_js_1.selectedDeviceSlotId)(saves, elements.deviceSelect.value);
    elements.mailSlotId.replaceChildren();
    for (const slot of saves.slots) {
        const option = document.createElement("option");
        option.value = String(slot.id);
        option.textContent = `${slot.name} (槽位 ${slot.id})`;
        elements.mailSlotId.append(option);
    }
    const nextSlotId = model.contexts.has(selectedSlotId)
        ? selectedSlotId
        : model.contexts.has(activeSlotId)
            ? activeSlotId
            : (_a = saves.slots[0]) === null || _a === void 0 ? void 0 : _a.id;
    if (nextSlotId !== undefined)
        elements.mailSlotId.value = String(nextSlotId);
    renderSelectedMailSlotContext();
}
function renderShopStockSlotContexts(saves) {
    var _a;
    const selectedSlotId = Number(elements.shopStockSlotId.value);
    const activeSlotId = (0, views_js_1.selectedDeviceSlotId)(saves, elements.deviceSelect.value);
    elements.shopStockSlotId.replaceChildren();
    for (const slot of saves.slots) {
        const option = document.createElement("option");
        option.value = String(slot.id);
        option.textContent = `${slot.name} (槽位 ${slot.id})`;
        elements.shopStockSlotId.append(option);
    }
    const nextSlotId = saves.slots.some((slot) => slot.id === selectedSlotId)
        ? selectedSlotId
        : saves.slots.some((slot) => slot.id === activeSlotId)
            ? activeSlotId
            : (_a = saves.slots[0]) === null || _a === void 0 ? void 0 : _a.id;
    if (nextSlotId !== undefined)
        elements.shopStockSlotId.value = String(nextSlotId);
    updateShopStockViewer();
}
function updateShopStockViewer() {
    const context = model.contexts.get(Number(elements.shopStockSlotId.value));
    const viewerId = Number(context === null || context === void 0 ? void 0 : context.viewer_id);
    const hasViewer = Number.isInteger(viewerId) && viewerId > 0;
    elements.shopStockViewerId.value = hasViewer ? String(viewerId) : "";
    elements.shopStockState.textContent = hasViewer
        ? `Viewer ${viewerId}, 选择商品后刷新当前周期库存`
        : "当前存档尚未建立 Viewer 会话";
}
async function requestApi(requestPath, options = {}) {
    var _a;
    const headers = Object.assign({}, options.headers);
    if (options.body !== undefined)
        headers["Content-Type"] = "application/json";
    const response = await fetch(requestPath, {
        method: (_a = options.method) !== null && _a !== void 0 ? _a : "GET",
        headers,
        body: options.body === undefined ? undefined : JSON.stringify(options.body),
    });
    const text = await response.text();
    let payload;
    try {
        payload = text ? JSON.parse(text) : null;
    }
    catch (_b) {
        payload = null;
    }
    if (!response.ok) {
        if (response.status === 401) {
            setConnected(false);
        }
        throw new ApiError(response.status, payload === null || payload === void 0 ? void 0 : payload.error);
    }
    return payload;
}
async function runAction(control, action) {
    var _a;
    if (control)
        control.disabled = true;
    try {
        const message = await action();
        if (message)
            showToast(message);
    }
    catch (error) {
        showToast((_a = error.message) !== null && _a !== void 0 ? _a : String(error), true);
    }
    finally {
        if (control)
            control.disabled = false;
    }
}
const activityHistory = (0, activity_history_js_1.createActivityHistory)({ requestApi, runAction });
const rewardHistory = (0, reward_history_js_1.createRewardHistory)({ requestApi, runAction });
const competitionHistory = (0, activity_history_js_1.createActivityHistory)({
    requestApi, runAction, prefix: "competition-history",
    onReset: () => competitionController.refresh(),
});
const competitionController = (0, competition_controller_js_1.createCompetitionController)({
    requestApi, runAction,
    onSelect: (activity, slot) => competitionHistory.selectContext({ activity, slot }),
    onChange: () => activityController.load(),
});
const activityController = (0, activity_controller_js_1.createActivityController)({
    elements,
    requestApi,
    ApiError,
    runAction,
    onSelect: activity => {
        activityHistory.selectActivity(activity);
        runAction(null, () => activityRescue.selectActivity(activity));
    },
});
const aiTeamController = (0, ai_team_controller_js_1.createAiTeamController)({
    elements,
    requestApi,
    runAction,
});
const rescueController = (0, rescue_controller_js_1.createRescueController)({
    panel: document.querySelector("#rescue-management"),
    requestApi,
    runAction,
});
const activityRescue = (0, activity_rescue_js_1.createActivityRescue)({
    panel: document.querySelector("#activity-rescue"), requestApi, runAction,
    onChange: change => rescueController.load(change),
});
const mailRewardController = (0, mail_reward_controller_js_1.createMailRewardController)({
    elements,
    requestApi,
    runAction,
    sendMail: sendManagedMail,
});
async function sendManagedMail(body) {
    const context = selectedMailSlotContext();
    await requestApi(`/v1/local-saves/${context.slot.id}/mails`, { method: "POST", body });
    await loadManagedMails();
}
async function runPlayerAccessAction(action) {
    if (playerAccessOperationActive)
        return;
    playerAccessOperationActive = true;
    const controls = elements.playerAccessForm.querySelectorAll('button[type="submit"]');
    for (const control of controls)
        control.disabled = true;
    try {
        await runAction(null, action);
    }
    finally {
        for (const control of controls)
            control.disabled = false;
        playerAccessOperationActive = false;
    }
}
async function refreshManagementState() {
    var _a;
    elements.refreshButton.disabled = true;
    try {
        const [profiles, saves, targets, time, httpObservations, gameplaySettings] = await Promise.all([
            requestApi("/v1/server-profiles"),
            requestApi("/v1/local-saves"),
            requestApi("/v1/save-sync-targets"),
            requestApi("/v1/time"),
            requestApi("/v1/http-observations"),
            requestApi("/v1/gameplay-settings"),
        ]);
        model.profiles = profiles;
        model.saves = saves;
        activityHistory.setSaves(saves.slots);
        model.targets = targets;
        model.httpObservations = httpObservations.observations;
        model.clientReports = (_a = httpObservations.client_reports) !== null && _a !== void 0 ? _a : [];
        renderGameplaySettings(gameplaySettings);
        renderTimeState(time);
        activityController.setCurrentTime(time.unix_time_ms);
        const slotEntries = await Promise.all(saves.slots.map(async (slot) => {
            const [bindings, automation, transferBindings, context] = await Promise.all([
                requestApi(`/v1/local-saves/${slot.id}/sync-bindings`),
                requestApi(`/v1/local-saves/${slot.id}/automation`),
                requestApi(`/v1/local-saves/${slot.id}/transfer-bindings`),
                requestApi(`/v1/local-saves/${slot.id}/context`),
            ]);
            const conflictEntries = await Promise.all(transferBindings.map(async (binding) => [
                binding.binding_id,
                await requestApi(`/v1/local-saves/${slot.id}/transfer-bindings/${binding.binding_id}/conflicts`),
            ]));
            return [slot.id, { automation, bindings, conflictEntries, context, transferBindings }];
        }));
        model.bindings = new Map(slotEntries.map(([slotId, state]) => [slotId, state.bindings]));
        model.automations = new Map(slotEntries.map(([slotId, state]) => [slotId, state.automation]));
        model.contexts = new Map(slotEntries.map(([slotId, state]) => [slotId, state.context]));
        model.transferBindings = new Map(slotEntries.map(([slotId, state]) => [slotId, state.transferBindings]));
        model.transferConflicts = new Map(slotEntries.flatMap(([, state]) => state.conflictEntries));
        (0, views_js_1.renderManagement)(model, elements, {
            describeError: (code) => { var _a; return (_a = errorMessages[code]) !== null && _a !== void 0 ? _a : code; },
            refreshManagementState,
            requestApi,
            runAction,
        });
        const activeSlotId = (0, views_js_1.selectedDeviceSlotId)(saves, elements.deviceSelect.value);
        aiTeamController.setSlots(saves.slots, activeSlotId);
        await Promise.all([
            activityController.load(),
            aiTeamController.load(),
            rescueController.load(),
            mailRewardController.load(),
            rewardHistory.setSaves(saves.slots, activeSlotId),
            competitionController.setSaves(saves.slots, activeSlotId),
        ]);
        renderMailSlotContexts(saves);
        renderShopStockSlotContexts(saves);
        setConnected(true);
    }
    finally {
        elements.refreshButton.disabled = false;
    }
}
elements.refreshButton.addEventListener("click", () => runAction(elements.refreshButton, async () => {
    await refreshManagementState();
    return "状态已刷新.";
}));
elements.profileForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    const formElement = event.currentTarget;
    const form = new FormData(formElement);
    await runAction(event.submitter, async () => {
        await requestApi("/v1/server-profiles", {
            method: "POST",
            body: {
                name: form.get("name"),
                scheme: form.get("scheme"),
                host: form.get("host"),
                port: Number(form.get("port")),
            },
        });
        formElement.reset();
        await refreshManagementState();
        return "游戏服务器已保存.";
    });
});
elements.syncTargetForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    const formElement = event.currentTarget;
    const form = new FormData(formElement);
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
        });
        formElement.reset();
        await refreshManagementState();
        return "存档服务器已保存.";
    });
});
elements.timeForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    await runAction(event.submitter, async () => {
        const enabled = elements.timeEnabled.checked;
        const rate = Number(elements.timeRate.value);
        const body = { enabled, rate };
        if (enabled) {
            const date = new Date(`${elements.timeValue.value}Z`);
            if (!Number.isFinite(date.getTime()))
                throw new Error("请输入有效的 UTC 时间.");
            body.iso = date.toISOString();
        }
        await requestApi("/v1/time", { method: "PUT", body });
        activityController.resetCalendar();
        await refreshManagementState();
        return "虚拟时间设置已保存.";
    });
});
elements.gameplaySettingsForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    await runAction(event.submitter, async () => {
        const multiplier = Number(elements.gameplayDropMultiplier.value);
        if (!Number.isInteger(multiplier) || multiplier < 1 || multiplier > 100) {
            throw new Error("掉落倍率必须是 1 到 100 的整数.");
        }
        const settings = await requestApi("/v1/gameplay-settings", {
            method: "PUT",
            body: { drop_multiplier: multiplier },
        });
        renderGameplaySettings(settings);
        await refreshManagementState();
        return "掉落倍率已保存.";
    });
});
elements.activityLoadButton.addEventListener("click", () => runAction(elements.activityLoadButton, async () => {
    await loadActivityState();
    return "活动状态已读取.";
}));
elements.activityForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    await runAction(event.submitter, async () => {
        const eventId = Number(elements.activityEventId.value);
        const hpPercentage = Number(elements.activityHp.value);
        const totalKillCount = Number(elements.activityKills.value);
        if (!Number.isInteger(eventId) || eventId <= 0)
            throw new Error("请输入有效的活动 ID.");
        if (!Number.isInteger(hpPercentage) || hpPercentage < 0 || hpPercentage > 100)
            throw new Error("请输入 0 到 100 之间的 Boss HP.");
        if (!Number.isInteger(totalKillCount) || totalKillCount < 0)
            throw new Error("请输入非负击杀数.");
        const state = await requestApi(`/v1/activities/raid-boss/${eventId}`, {
            method: "PUT",
            body: { hp_percentage: hpPercentage, total_kill_count: totalKillCount },
        });
        elements.activityState.textContent = `活动 ${state.event_id}: HP ${state.hp_percentage}%, 击杀 ${state.total_kill_count}`;
        return "活动状态已保存.";
    });
});
elements.mailLoadButton.addEventListener("click", () => runAction(elements.mailLoadButton, async () => {
    await loadManagedMails();
    return "邮件列表已刷新.";
}));
elements.mailSlotId.addEventListener("change", renderSelectedMailSlotContext);
elements.shopStockSlotId.addEventListener("change", updateShopStockViewer);
elements.deviceSelect.addEventListener("change", () => {
    const activeSlotId = (0, views_js_1.selectedDeviceSlotId)(model.saves, elements.deviceSelect.value);
    if (activeSlotId === undefined || !model.contexts.has(activeSlotId))
        return;
    elements.mailSlotId.value = String(activeSlotId);
    renderSelectedMailSlotContext();
    if (model.contexts.has(activeSlotId)) {
        elements.shopStockSlotId.value = String(activeSlotId);
        updateShopStockViewer();
    }
});
elements.shopStockForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    const formElement = event.currentTarget;
    const form = new FormData(formElement);
    await runAction(event.submitter, async () => {
        var _a, _b;
        const context = model.contexts.get(Number(form.get("slot_id")));
        const viewerId = Number(context === null || context === void 0 ? void 0 : context.viewer_id);
        const shopType = Number(form.get("shop_type"));
        const shopItemId = Number(form.get("shop_item_id"));
        if (!Number.isInteger(viewerId) || viewerId <= 0)
            throw new Error("当前存档尚未建立 Viewer 会话.");
        if (!Number.isInteger(shopType) || shopType <= 0)
            throw new Error("请选择有效的商店类型.");
        if (!Number.isInteger(shopItemId) || shopItemId <= 0)
            throw new Error("请输入有效的商品 ID.");
        const result = await requestApi("/v1/shop-stock/refresh", {
            method: "POST",
            body: { viewer_id: viewerId, shop_type: shopType, shop_item_id: shopItemId },
        });
        elements.shopStockState.textContent = `Viewer ${viewerId}, 商品 ${shopItemId}: 当前周期已购买 ${(_a = result.stock_purchase_num) !== null && _a !== void 0 ? _a : 0} 次, 历史累计 ${(_b = result.historical_purchase_num) !== null && _b !== void 0 ? _b : 0} 次`;
        return "商店库存已刷新.";
    });
});
elements.mailForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    const formElement = event.currentTarget;
    const form = new FormData(formElement);
    await runAction(event.submitter, async () => {
        var _a, _b, _c, _d, _e;
        if (!mailRewardController.hasSelection())
            throw new Error("请至少选择一项邮件奖励.");
        let rewards;
        try {
            rewards = JSON.parse(String((_a = form.get("rewards")) !== null && _a !== void 0 ? _a : ""));
        }
        catch (_f) {
            throw new Error("当前奖励选择无效, 请重新添加奖励.");
        }
        const expiresText = String((_b = form.get("expires_at")) !== null && _b !== void 0 ? _b : "").trim();
        let expiresAt;
        if (expiresText) {
            const expiresDate = new Date(`${expiresText}Z`);
            if (!Number.isFinite(expiresDate.getTime()))
                throw new Error("请输入有效的 UTC 过期时间.");
            expiresAt = Math.floor(expiresDate.getTime() / 1000);
        }
        const body = {
            title: String((_c = form.get("title")) !== null && _c !== void 0 ? _c : ""),
            body: String((_d = form.get("body")) !== null && _d !== void 0 ? _d : ""),
            sender: String((_e = form.get("sender")) !== null && _e !== void 0 ? _e : ""),
            rewards,
        };
        if (expiresAt !== undefined)
            body.expires_at = expiresAt;
        await sendManagedMail(body);
        mailRewardController.clearSelection();
        return "本地邮件已发放.";
    });
});
elements.playerAccessForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    const viewerId = Number(elements.playerAccessViewerId.value);
    const generation = ++playerAccessGeneration;
    await runPlayerAccessAction(async () => {
        var _a, _b;
        if (!Number.isInteger(viewerId) || viewerId <= 0)
            throw new Error("请输入有效的 viewer ID.");
        const action = (_a = event.submitter) === null || _a === void 0 ? void 0 : _a.value;
        if (action === "revoke") {
            await requestApi(`/v1/player-access/${viewerId}`, { method: "DELETE" });
            if (generation !== playerAccessGeneration
                || Number(elements.playerAccessViewerId.value) !== viewerId)
                return undefined;
            elements.playerAccessToken.value = "";
            elements.playerAccessState.textContent = `Viewer ${viewerId} 的远程存档授权已撤销.`;
            return "远程存档授权已撤销.";
        }
        const result = await requestApi("/v1/player-access", {
            method: "POST",
            body: { viewer_id: viewerId },
        });
        if (generation !== playerAccessGeneration
            || Number(elements.playerAccessViewerId.value) !== viewerId)
            return undefined;
        elements.playerAccessToken.value = (_b = result.token) !== null && _b !== void 0 ? _b : "";
        elements.playerAccessState.textContent = `Viewer ${viewerId} 的远程存档授权已签发, 只在当前页面显示.`;
        return "远程存档授权已签发.";
    });
});
elements.importForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    const formElement = event.currentTarget;
    const form = new FormData(formElement);
    await runAction(event.submitter, async () => {
        await (0, password_backup_js_1.importSaveFile)(form, requestApi);
        formElement.reset();
        await refreshManagementState();
        return "存档已导入为新槽位.";
    });
});
elements.syncDownloadForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    const formElement = event.currentTarget;
    const form = new FormData(formElement);
    await runAction(event.submitter, async () => {
        await requestApi("/v1/local-saves/sync/download", {
            method: "POST",
            body: {
                target_id: Number(form.get("target_id")),
                object_id: form.get("object_id"),
                name: form.get("name"),
            },
        });
        formElement.reset();
        await refreshManagementState();
        return "远端存档已下载为隔离槽位.";
    });
});
async function initialize() {
    var _a;
    initializeManagementTabs();
    setConnected(false);
    if (location.hash)
        history.replaceState(null, "", `${location.pathname}${location.search}`);
    try {
        await refreshManagementState();
    }
    catch (error) {
        setConnected(false);
        showToast((_a = error.message) !== null && _a !== void 0 ? _a : String(error), true);
    }
}
initialize();
// //// /管理页面连接和表单提交 ////
},
"/manage/activity-controller.js": function(module, exports, require) {
"use strict";
// audience: external
// # personal-service-activity-controller
//
// 此模块管理活动目录状态、活动 API 调用和活动表单事件. 调用方提供同源请求函数和通用操作反馈.
Object.defineProperty(exports, "__esModule", { value: true });
exports.createActivityController = void 0;
const activity_views_js_1 = require("/manage/activity-views.js");
// //// 管理活动目录和活动规则 [@x380kkm 2026-08-19] ////
function createActivityController({ elements, requestApi, ApiError, runAction, onSelect = () => { } }) {
    const state = {
        calendarInitialized: false,
        calendarMonth: new Date().toISOString().slice(0, 7),
        dateFrom: "",
        dateTo: "",
        defaultDateInitialized: false,
        error: null,
        favoriteOnly: false,
        items: [],
        kind: "",
        knownKinds: [],
        manifestState: "unknown",
        manifestVersion: null,
        assetVersion: null,
        region: null,
        clientVersion: null,
        selectedId: null,
        serverTimeMs: null,
        status: "",
        search: "",
        section: "events",
        groupId: null,
        tag: "",
    };
    let catalogGeneration = 0;
    let searchTimer;
    function catalogQuery() {
        const query = new URLSearchParams();
        if (state.search)
            query.set("q", state.search);
        if (state.kind)
            query.set("kind", state.kind);
        if (state.status)
            query.set("status", state.status);
        if (state.favoriteOnly)
            query.set("favorite", "true");
        return query.toString();
    }
    function catalogPath() {
        const query = new URLSearchParams(catalogQuery());
        const suffix = query.toString();
        return suffix ? `/v1/activities/catalog?${suffix}` : "/v1/activities/catalog";
    }
    function normalizeImageCandidate(candidate) {
        var _a, _b, _c, _d, _e, _f, _g, _h, _j, _k;
        const source = candidate !== null && candidate !== void 0 ? candidate : {};
        const url = (_c = (_b = (_a = source.url) !== null && _a !== void 0 ? _a : source.banner_url) !== null && _b !== void 0 ? _b : source.bannerUrl) !== null && _c !== void 0 ? _c : "";
        if (!url)
            return null;
        return Object.assign(Object.assign({}, source), { evidence: String((_d = source.evidence) !== null && _d !== void 0 ? _d : ""), height: Number((_f = (_e = source.height) !== null && _e !== void 0 ? _e : source.banner_height) !== null && _f !== void 0 ? _f : source.bannerHeight) || null, source_type: String((_h = (_g = source.source_type) !== null && _g !== void 0 ? _g : source.sourceType) !== null && _h !== void 0 ? _h : "unclassified"), url: String(url), width: Number((_k = (_j = source.width) !== null && _j !== void 0 ? _j : source.banner_width) !== null && _k !== void 0 ? _k : source.bannerWidth) || null });
    }
    function normalizeEntry(entry, index) {
        var _a, _b, _c, _d, _e, _f, _g, _h, _j, _k, _l, _m, _o, _p, _q, _r, _s, _t, _u, _v, _w, _x, _y, _z, _0, _1, _2, _3, _4, _5, _6;
        const source = entry !== null && entry !== void 0 ? entry : {};
        const id = String((_c = (_b = (_a = source.activity_id) !== null && _a !== void 0 ? _a : source.id) !== null && _b !== void 0 ? _b : source.event_id) !== null && _c !== void 0 ? _c : `activity-${index + 1}`);
        const sourceTags = Array.isArray(source.tags)
            ? source.tags.map((tag) => String(tag))
            : typeof source.tags === "string"
                ? source.tags.split(",").map((tag) => tag.trim()).filter(Boolean)
                : [];
        const tags = [...new Set(sourceTags.map((tag) => tag.trim()).filter(Boolean))];
        const imageCandidates = Array.isArray(source.image_candidates)
            ? source.image_candidates.map(normalizeImageCandidate).filter(Boolean)
            : [];
        const legacyCandidate = normalizeImageCandidate({
            evidence: source.banner_evidence,
            height: (_d = source.banner_height) !== null && _d !== void 0 ? _d : source.bannerHeight,
            source_type: (_f = (_e = source.banner_source_type) !== null && _e !== void 0 ? _e : source.bannerSourceType) !== null && _f !== void 0 ? _f : "activity_banner",
            url: (_h = (_g = source.banner_url) !== null && _g !== void 0 ? _g : source.bannerUrl) !== null && _h !== void 0 ? _h : (_j = source.banner) === null || _j === void 0 ? void 0 : _j.url,
            width: (_k = source.banner_width) !== null && _k !== void 0 ? _k : source.bannerWidth,
        });
        if (legacyCandidate && !imageCandidates.some((candidate) => candidate.url === legacyCandidate.url)) {
            imageCandidates.unshift(legacyCandidate);
        }
        const primaryImage = (_l = imageCandidates[0]) !== null && _l !== void 0 ? _l : legacyCandidate;
        const temporaryOpenUntilMs = Number((_m = source.temporary_open_until_ms) !== null && _m !== void 0 ? _m : source.temporaryOpenUntilMs);
        return Object.assign(Object.assign({}, source), { activity_id: id, banner_evidence: (_o = primaryImage === null || primaryImage === void 0 ? void 0 : primaryImage.evidence) !== null && _o !== void 0 ? _o : "", banner_height: (_p = primaryImage === null || primaryImage === void 0 ? void 0 : primaryImage.height) !== null && _p !== void 0 ? _p : null, banner_source_type: (_q = primaryImage === null || primaryImage === void 0 ? void 0 : primaryImage.source_type) !== null && _q !== void 0 ? _q : "", banner_url: (_r = primaryImage === null || primaryImage === void 0 ? void 0 : primaryImage.url) !== null && _r !== void 0 ? _r : "", banner_width: (_s = primaryImage === null || primaryImage === void 0 ? void 0 : primaryImage.width) !== null && _s !== void 0 ? _s : null, default_end_at: (_u = (_t = source.default_end_at) !== null && _t !== void 0 ? _t : source.defaultEndAt) !== null && _u !== void 0 ? _u : null, default_start_at: (_w = (_v = source.default_start_at) !== null && _v !== void 0 ? _v : source.defaultStartAt) !== null && _w !== void 0 ? _w : null, description: String((_x = source.description) !== null && _x !== void 0 ? _x : ""), favorite: Boolean((_y = source.favorite) !== null && _y !== void 0 ? _y : source.is_favorite), image_candidates: imageCandidates, kind: String((_0 = (_z = source.kind) !== null && _z !== void 0 ? _z : source.type) !== null && _0 !== void 0 ? _0 : "other"), name: String((_2 = (_1 = source.name) !== null && _1 !== void 0 ? _1 : source.title) !== null && _2 !== void 0 ? _2 : id), underlying_status: String((_5 = (_4 = (_3 = source.underlying_status) !== null && _3 !== void 0 ? _3 : source.underlyingStatus) !== null && _4 !== void 0 ? _4 : source.status) !== null && _5 !== void 0 ? _5 : "unscheduled"), status: String((_6 = source.status) !== null && _6 !== void 0 ? _6 : "unscheduled"), tags, temporary_open_until_ms: Number.isFinite(temporaryOpenUntilMs) && temporaryOpenUntilMs > 0
                ? temporaryOpenUntilMs
                : null });
    }
    function normalizeCatalog(payload) {
        var _a, _b, _c, _d, _e, _f, _g, _h, _j, _k;
        const entries = Array.isArray(payload)
            ? payload
            : (_d = (_c = (_b = (_a = payload === null || payload === void 0 ? void 0 : payload.activities) !== null && _a !== void 0 ? _a : payload === null || payload === void 0 ? void 0 : payload.catalog) !== null && _b !== void 0 ? _b : payload === null || payload === void 0 ? void 0 : payload.items) !== null && _c !== void 0 ? _c : payload === null || payload === void 0 ? void 0 : payload.schedules) !== null && _d !== void 0 ? _d : [];
        return {
            clientVersion: (_e = payload === null || payload === void 0 ? void 0 : payload.client_version) !== null && _e !== void 0 ? _e : null,
            assetVersion: (_f = payload === null || payload === void 0 ? void 0 : payload.asset_version) !== null && _f !== void 0 ? _f : null,
            formatVersion: (_g = payload === null || payload === void 0 ? void 0 : payload.format_version) !== null && _g !== void 0 ? _g : null,
            items: entries.map(normalizeEntry),
            manifestState: (_h = payload === null || payload === void 0 ? void 0 : payload.manifest_state) !== null && _h !== void 0 ? _h : "unknown",
            region: (_j = payload === null || payload === void 0 ? void 0 : payload.region) !== null && _j !== void 0 ? _j : null,
            serverTimeMs: (_k = payload === null || payload === void 0 ? void 0 : payload.server_time_ms) !== null && _k !== void 0 ? _k : null,
        };
    }
    function setCurrentTime(timeMs) {
        const normalizedTimeMs = Number(timeMs);
        if (!Number.isFinite(normalizedTimeMs) || normalizedTimeMs <= 0)
            return;
        state.serverTimeMs = normalizedTimeMs;
        if (state.calendarInitialized)
            return;
        state.calendarMonth = new Date(normalizedTimeMs).toISOString().slice(0, 7);
        state.calendarInitialized = true;
    }
    function selectCurrentDate() {
        if (!Number.isFinite(state.serverTimeMs))
            return;
        const date = new Date(state.serverTimeMs).toISOString().slice(0, 10);
        state.dateFrom = date;
        state.dateTo = date;
        elements.activityDateFrom.value = date;
        elements.activityDateTo.value = date;
        state.defaultDateInitialized = true;
    }
    function selectDefaultDate() {
        if (state.defaultDateInitialized)
            return;
        selectCurrentDate();
    }
    async function load() {
        var _a;
        const generation = ++catalogGeneration;
        try {
            const payload = await requestApi(catalogPath());
            const catalog = normalizeCatalog(payload);
            if (generation !== catalogGeneration)
                return;
            state.items = catalog.items;
            state.knownKinds = [...new Set([
                    ...state.knownKinds,
                    ...catalog.items.map((activity) => activity.kind),
                ])].sort((left, right) => left.localeCompare(right));
            state.error = null;
            state.manifestState = catalog.manifestState;
            state.manifestVersion = catalog.formatVersion;
            state.region = catalog.region;
            state.clientVersion = catalog.clientVersion;
            state.assetVersion = catalog.assetVersion;
            setCurrentTime(catalog.serverTimeMs);
            if (state.section === "events") {
                if (state.status === "ended")
                    selectCurrentDate();
                else
                    selectDefaultDate();
            }
        }
        catch (error) {
            if (generation !== catalogGeneration)
                return;
            state.items = [];
            state.error = error;
        }
        render();
        onSelect((_a = selectedActivity()) !== null && _a !== void 0 ? _a : null);
    }
    function render() {
        const actions = {
            render,
            setActivitySection,
            closeActivity,
            closeActivityDetail: () => {
                state.selectedId = null;
                onSelect(null);
                (0, activity_views_js_1.renderActivityDetail)(state, elements, actions);
            },
            runAction,
            selectActivity: (activityId) => {
                var _a, _b;
                state.selectedId = activityId ? String(activityId) : null;
                onSelect(selectedActivity());
                (0, activity_views_js_1.renderActivityDetail)(state, elements, actions);
                (_b = (_a = elements.activityDetail).scrollIntoView) === null || _b === void 0 ? void 0 : _b.call(_a, { block: "nearest" });
            },
            setActivityDate: setActivityDate,
            setActivityFavoriteFilter,
            setActivityKindFilter,
            setActivityStatusFilter,
            setActivityTagFilter,
            clearActivityFilters,
            temporaryActivityAction,
            toggleActivityFavorite,
        };
        (0, activity_views_js_1.renderActivityCatalog)(state, elements, actions);
    }
    function selectedActivity() {
        return state.items.find((item) => String(item.activity_id) === String(state.selectedId));
    }
    async function setActivitySection(section) {
        state.section = section;
        state.defaultDateInitialized = false;
        state.groupId = null;
        state.selectedId = null;
        onSelect(null);
        await clearActivityFilters();
    }
    function setActivityDate(date) {
        state.dateFrom = date;
        state.dateTo = date;
        state.defaultDateInitialized = true;
        elements.activityDateFrom.value = date;
        elements.activityDateTo.value = date;
        render();
    }
    async function calendarFallback(activityId, changes) {
        var _a, _b, _c, _d, _e, _f, _g;
        let current = {};
        try {
            current = await requestApi(`/v1/activities/calendar/${encodeURIComponent(activityId)}`);
        }
        catch (error) {
            if (!(error instanceof ApiError) || error.status !== 404)
                throw error;
        }
        const startAt = (_c = (_b = (_a = changes.start_at) !== null && _a !== void 0 ? _a : changes.start_at_ms) !== null && _b !== void 0 ? _b : current.start_at) !== null && _c !== void 0 ? _c : current.start_at_ms;
        const endAt = (_f = (_e = (_d = changes.end_at) !== null && _d !== void 0 ? _d : changes.end_at_ms) !== null && _e !== void 0 ? _e : current.end_at) !== null && _f !== void 0 ? _f : current.end_at_ms;
        if (startAt === undefined || endAt === undefined || startAt === null || endAt === null) {
            throw new ApiError(400, "activity_action_unavailable");
        }
        return requestApi(`/v1/activities/calendar/${encodeURIComponent(activityId)}`, {
            method: "PUT",
            body: {
                enabled: (_g = changes.enabled) !== null && _g !== void 0 ? _g : Boolean(current.enabled),
                start_at: typeof startAt === "number" ? undefined : startAt,
                start_at_ms: typeof startAt === "number" ? startAt : undefined,
                end_at: typeof endAt === "number" ? undefined : endAt,
                end_at_ms: typeof endAt === "number" ? endAt : undefined,
            },
        });
    }
    async function mutate(activityId, path, body, fallbackChanges) {
        try {
            return await requestApi(`/v1/activities/${encodeURIComponent(activityId)}${path}`, {
                method: "POST",
                body,
            });
        }
        catch (error) {
            if (!(error instanceof ApiError) || ![404, 405].includes(error.status))
                throw error;
            return calendarFallback(activityId, fallbackChanges);
        }
    }
    async function closeActivity(activity) {
        var _a, _b, _c, _d;
        const id = String(activity.activity_id);
        const defaultWindow = {
            start_at_ms: (_b = (_a = activity.start_at_ms) !== null && _a !== void 0 ? _a : activity.default_start_at_ms) !== null && _b !== void 0 ? _b : 0,
            end_at_ms: (_d = (_c = activity.end_at_ms) !== null && _c !== void 0 ? _c : activity.default_end_at_ms) !== null && _d !== void 0 ? _d : 253402300799000,
        };
        await mutate(id, "/close", {}, Object.assign({ enabled: false }, defaultWindow));
        await load();
        return "活动已结束.";
    }
    async function temporaryActivityAction(activity, shouldOpen) {
        const id = encodeURIComponent(String(activity.activity_id));
        await requestApi("/v1/activities/" + id + "/temporary-open", {
            method: shouldOpen ? "POST" : "DELETE",
            body: shouldOpen ? {} : undefined,
        });
        await load();
        return shouldOpen ? "活动已开放 24 小时." : "临时开放已结束.";
    }
    async function toggleActivityFavorite(activity) {
        const id = encodeURIComponent(String(activity.activity_id));
        const nextFavorite = !activity.favorite;
        try {
            await requestApi(`/v1/activities/catalog/${id}/favorite`, {
                method: nextFavorite ? "PUT" : "DELETE",
            });
        }
        catch (error) {
            if (!(error instanceof ApiError) || ![404, 405].includes(error.status))
                throw error;
            await requestApi("/v1/activities/catalog/favorite", {
                method: "PUT",
                body: { activity_id: String(activity.activity_id), favorite: nextFavorite },
            });
        }
        await load();
        return nextFavorite ? "已收藏活动." : "已取消收藏.";
    }
    async function updateWindow(activity, startAt, endAt) {
        const id = String(activity.activity_id);
        try {
            await requestApi(`/v1/activities/${encodeURIComponent(id)}/window`, {
                method: "PUT",
                body: { start_at: startAt || null, end_at: endAt || null },
            });
        }
        catch (error) {
            if (!(error instanceof ApiError) || ![404, 405].includes(error.status))
                throw error;
            await calendarFallback(id, { enabled: true, start_at: startAt, end_at: endAt });
        }
        await load();
        return "活动时间窗口已保存.";
    }
    async function updateMode(activity, mode) {
        var _a, _b, _c, _d;
        const id = encodeURIComponent(String(activity.activity_id));
        try {
            await requestApi(`/v1/activities/${id}/mode`, { method: "PUT", body: { mode } });
        }
        catch (error) {
            if (!(error instanceof ApiError) || ![404, 405].includes(error.status))
                throw error;
            if (mode === "always" || mode === "manual") {
                await calendarFallback(activity.activity_id, {
                    enabled: mode === "always",
                    start_at_ms: (_b = (_a = activity.start_at_ms) !== null && _a !== void 0 ? _a : activity.default_start_at_ms) !== null && _b !== void 0 ? _b : 0,
                    end_at_ms: (_d = (_c = activity.end_at_ms) !== null && _c !== void 0 ? _c : activity.default_end_at_ms) !== null && _d !== void 0 ? _d : 253402300799000,
                });
            }
            else {
                throw new ApiError(error.status, "activity_action_unavailable");
            }
        }
        await load();
        return "活动模式已保存.";
    }
    async function updatePeriod(activity, period, intervalDays) {
        const id = String(activity.activity_id);
        const body = { period };
        if (period === "interval_days") {
            if (!Number.isInteger(intervalDays) || intervalDays < 1 || intervalDays > 3650) {
                throw new Error("间隔天数必须位于 1 到 3650.");
            }
            body.interval_days = intervalDays;
        }
        try {
            await requestApi(`/v1/activities/${encodeURIComponent(id)}/period`, { method: "PUT", body });
        }
        catch (error) {
            if (!(error instanceof ApiError) || ![404, 405].includes(error.status))
                throw error;
            throw new ApiError(error.status, "activity_action_unavailable");
        }
        await load();
        return "活动周期已保存.";
    }
    function refreshFromFilters() {
        state.search = elements.activitySearch.value.trim();
        state.kind = elements.activityKindFilter.value;
        state.status = elements.activityStatusFilter.value;
        state.favoriteOnly = elements.activityFavoriteFilter.checked;
        return load();
    }
    function setActivityKindFilter(kind) {
        state.kind = String(kind !== null && kind !== void 0 ? kind : "");
        elements.activityKindFilter.value = state.kind;
        return load();
    }
    function setActivityStatusFilter(status) {
        state.status = String(status !== null && status !== void 0 ? status : "");
        elements.activityStatusFilter.value = state.status;
        return load();
    }
    function setActivityFavoriteFilter(favoriteOnly) {
        state.favoriteOnly = Boolean(favoriteOnly);
        elements.activityFavoriteFilter.checked = state.favoriteOnly;
        return load();
    }
    function setActivityTagFilter(tag) {
        state.tag = String(tag !== null && tag !== void 0 ? tag : "");
        render();
    }
    function clearActivityFilters() {
        clearTimeout(searchTimer);
        state.search = "";
        state.kind = "";
        state.status = "";
        state.favoriteOnly = false;
        state.tag = "";
        elements.activitySearch.value = "";
        elements.activityKindFilter.value = "";
        elements.activityStatusFilter.value = "";
        elements.activityFavoriteFilter.checked = false;
        clearDateRange();
        return load();
    }
    function clearDateRange() {
        state.dateFrom = "";
        state.dateTo = "";
        elements.activityDateFrom.value = "";
        elements.activityDateTo.value = "";
    }
    function changeCalendarMonth(delta) {
        const current = /^\d{4}-\d{2}$/.test(state.calendarMonth)
            ? new Date(`${state.calendarMonth}-01T00:00:00.000Z`)
            : new Date();
        current.setUTCMonth(current.getUTCMonth() + delta);
        state.calendarInitialized = true;
        state.calendarMonth = `${current.getUTCFullYear()}-${String(current.getUTCMonth() + 1).padStart(2, "0")}`;
        render();
    }
    async function resetActivities() {
        const result = await requestApi("/v1/activities/reset", {
            method: "POST",
            body: {},
        });
        await load();
        const resetCount = Number(result.reset_schedule_count || 0)
            + Number(result.reset_temporary_open_count || 0);
        return `已恢复包内活动时间, 清除 ${resetCount} 项设置.`;
    }
    function bindEvents() {
        elements.activityCatalogRefresh.addEventListener("click", () => runAction(elements.activityCatalogRefresh, refreshFromFilters));
        elements.activityReset.addEventListener("click", () => runAction(elements.activityReset, resetActivities));
        elements.activitySearch.addEventListener("input", () => {
            clearTimeout(searchTimer);
            searchTimer = setTimeout(refreshFromFilters, 280);
        });
        elements.activityKindFilter.addEventListener("change", refreshFromFilters);
        elements.activityStatusFilter.addEventListener("change", refreshFromFilters);
        elements.activityFavoriteFilter.addEventListener("change", refreshFromFilters);
        elements.activityDateFrom.addEventListener("change", () => {
            state.dateFrom = elements.activityDateFrom.value;
            state.defaultDateInitialized = true;
            if (state.dateTo && state.dateFrom > state.dateTo) {
                state.dateTo = state.dateFrom;
                elements.activityDateTo.value = state.dateTo;
            }
            render();
        });
        elements.activityDateTo.addEventListener("change", () => {
            state.dateTo = elements.activityDateTo.value;
            state.defaultDateInitialized = true;
            if (state.dateFrom && state.dateTo < state.dateFrom) {
                state.dateFrom = state.dateTo;
                elements.activityDateFrom.value = state.dateFrom;
            }
            render();
        });
        elements.activityDateClear.addEventListener("click", () => {
            clearDateRange();
            render();
        });
        elements.activityCalendarPrevious.addEventListener("click", () => runAction(elements.activityCalendarPrevious, () => changeCalendarMonth(-1)));
        elements.activityCalendarPreviousYear.addEventListener("click", () => runAction(elements.activityCalendarPreviousYear, () => changeCalendarMonth(-12)));
        elements.activityCalendarNext.addEventListener("click", () => runAction(elements.activityCalendarNext, () => changeCalendarMonth(1)));
        elements.activityCalendarNextYear.addEventListener("click", () => runAction(elements.activityCalendarNextYear, () => changeCalendarMonth(12)));
        elements.activityCalendarToday.addEventListener("click", () => runAction(elements.activityCalendarToday, () => {
            var _a;
            const now = new Date((_a = state.serverTimeMs) !== null && _a !== void 0 ? _a : Date.now());
            state.calendarInitialized = true;
            state.calendarMonth = `${now.getUTCFullYear()}-${String(now.getUTCMonth() + 1).padStart(2, "0")}`;
            setActivityDate(now.toISOString().slice(0, 10));
        }));
        elements.activityWindowForm.addEventListener("submit", async (event) => {
            event.preventDefault();
            const activity = selectedActivity();
            if (!activity)
                return;
            await runAction(event.submitter, () => updateWindow(activity, elements.activityWindowStart.value
                ? new Date(`${elements.activityWindowStart.value}Z`).toISOString()
                : null, elements.activityWindowEnd.value
                ? new Date(`${elements.activityWindowEnd.value}Z`).toISOString()
                : null));
        });
        elements.activityModeForm.addEventListener("submit", async (event) => {
            event.preventDefault();
            const activity = selectedActivity();
            if (!activity)
                return;
            await runAction(event.submitter, () => updateMode(activity, elements.activityMode.value));
        });
        elements.activityPeriodForm.addEventListener("submit", async (event) => {
            event.preventDefault();
            const activity = selectedActivity();
            if (!activity)
                return;
            await runAction(event.submitter, () => updatePeriod(activity, elements.activityPeriodKind.value, Number(elements.activityPeriodInterval.value)));
        });
        elements.activityPeriodKind.addEventListener("change", () => {
            elements.activityPeriodInterval.disabled = elements.activityPeriodKind.value !== "interval_days";
        });
    }
    bindEvents();
    return {
        load,
        resetCalendar() {
            state.calendarInitialized = false;
            state.defaultDateInitialized = false;
            clearDateRange();
        },
        setCurrentTime,
    };
}
exports.createActivityController = createActivityController;
// //// /管理活动目录和活动规则 ////
},
"/manage/activity-views.js": function(module, exports, require) {
"use strict";
// audience: external
// # personal-service-activity-views
//
// 此模块把活动目录和图片候选元数据转换为安全 DOM 节点. 所有目录文本只写入 textContent.
Object.defineProperty(exports, "__esModule", { value: true });
exports.renderActivityDetail = exports.filterActivitiesByDate = exports.renderActivityCatalog = void 0;
// //// 呈现活动目录和图片 [@x380kkm 2026-08-19] ////
const activity_navigation_js_1 = require("/manage/activity-navigation.js");
function createElement(tagName, className, text) {
    const node = document.createElement(tagName);
    if (className)
        node.className = className;
    if (text !== undefined)
        node.textContent = text;
    return node;
}
function createButton(label, action, runAction, style = "ghost") {
    const control = createElement("button", `button ${style}`, label);
    control.type = "button";
    control.addEventListener("click", () => runAction(control, action));
    return control;
}
function renderActivityCatalog(activities, elements, actions) {
    const state = activities;
    activities = Object.assign(Object.assign({}, activities), { items: activities.items.filter(item => !activities.section || (0, activity_navigation_js_1.activitySection)(item) === activities.section) });
    activities.knownKinds = [...new Set(activities.items.map(item => item.kind))];
    fillActivityKindFilter(activities, elements);
    const taggedActivities = filterActivitiesByTag(activities);
    renderActivityCalendar(activities, elements, actions, taggedActivities);
    const visibleActivities = filterActivitiesByDate(activities, taggedActivities);
    (0, activity_navigation_js_1.renderActivityNavigation)(state, elements, actions, visibleActivities);
    activities.selectedId = state.selectedId;
    renderActivityQuickFilters(activities, elements, actions, visibleActivities.length);
    renderActivityTagFilters(activities, elements, actions);
    elements.activityCatalogList.replaceChildren();
    if (activities.error) {
        elements.activityCatalogState.textContent = "活动目录尚不可用, 旧版 Raid Boss 管理仍可使用.";
        elements.activityCatalogList.append(createElement("p", "empty-state", activities.error.message));
        renderActivityDetail(activities, elements, actions);
        return;
    }
    const catalogSummary = `当前结果 ${visibleActivities.length} 项, 目录匹配 ${activities.items.length} 项`;
    const sourceSummary = activities.manifestState === "missing"
        ? "活动资源目录尚未提取"
        : [
            activities.region,
            activities.clientVersion,
            activities.assetVersion ? `资源 ${activities.assetVersion}` : null,
            activities.manifestVersion ? `目录 v${activities.manifestVersion}` : null,
        ]
            .filter(Boolean)
            .join(" ");
    elements.activityCatalogState.textContent = sourceSummary
        ? `${catalogSummary}, ${sourceSummary}`
        : catalogSummary;
    if (visibleActivities.length === 0) {
        elements.activityCatalogList.append(createElement("p", "empty-state", "没有符合当前筛选条件的活动."));
    }
    for (const activity of (activities.section === "boss" || activities.section === "daily" ? [] : visibleActivities)) {
        elements.activityCatalogList.append(createActivityCard(activity, actions));
    }
    renderActivityDetail(activities, elements, actions);
}
exports.renderActivityCatalog = renderActivityCatalog;
function renderActivityQuickFilters(activities, elements, actions, resultCount) {
    const container = activityFilterContainer(elements, "activityQuickFilters", "activityQuickFilters");
    if (!container)
        return;
    container.classList.add("activity-filter-chips");
    container.replaceChildren();
    const kindGroup = createFilterGroup("分类", "活动分类");
    kindGroup.append(createFilterChip("全部", activities.kind === "", () => {
        return actions.setActivityKindFilter("");
    }, actions.runAction));
    for (const kind of activities.knownKinds) {
        kindGroup.append(createFilterChip(activityKindLabel(kind), activities.kind === kind, () => {
            return actions.setActivityKindFilter(kind);
        }, actions.runAction));
    }
    const statusGroup = createFilterGroup("状态", "常用活动状态");
    for (const [status, label] of [["open", "进行中"], ["not_started", "即将开始"], ["ended", "已结束"]]) {
        statusGroup.append(createFilterChip(label, activities.status === status, () => {
            return actions.setActivityStatusFilter(activities.status === status ? "" : status);
        }, actions.runAction));
    }
    statusGroup.append(createFilterChip("收藏", activities.favoriteOnly, () => {
        return actions.setActivityFavoriteFilter(!activities.favoriteOnly);
    }, actions.runAction));
    const result = createElement("span", "activity-filter-result activity-filter-count", `显示 ${resultCount} 项`);
    result.setAttribute("aria-live", "polite");
    container.append(kindGroup, statusGroup, result);
    if (hasActivityFilters(activities)) {
        container.append(createFilterAction("清除筛选", actions.clearActivityFilters, actions.runAction));
    }
}
function renderActivityTagFilters(activities, elements, actions) {
    const container = activityFilterContainer(elements, "activityTagFilters", "activityTagFilters");
    if (!container)
        return;
    const tags = activityTagChoices(activities);
    container.classList.add("activity-filter-chips");
    container.replaceChildren();
    container.hidden = tags.length === 0;
    if (tags.length === 0)
        return;
    container.append(createElement("span", "activity-filter-label", "标签"));
    container.append(createFilterChip("全部标签", activities.tag === "", () => {
        actions.setActivityTagFilter("");
    }, actions.runAction));
    for (const tag of tags) {
        container.append(createFilterChip(tag, activities.tag === tag, () => {
            actions.setActivityTagFilter(activities.tag === tag ? "" : tag);
        }, actions.runAction));
    }
}
function activityFilterContainer(elements, property, id) {
    var _a, _b, _c;
    return (_c = (_a = elements[property]) !== null && _a !== void 0 ? _a : (_b = document.getElementById) === null || _b === void 0 ? void 0 : _b.call(document, id)) !== null && _c !== void 0 ? _c : null;
}
function createFilterGroup(label, ariaLabel) {
    const group = createElement("div", "activity-filter-group");
    group.setAttribute("role", "group");
    group.setAttribute("aria-label", ariaLabel);
    group.append(createElement("span", "activity-filter-label", label));
    return group;
}
function createFilterChip(label, active, action, runAction) {
    const control = createElement("button", `activity-filter-chip${active ? " is-active" : ""}`, label);
    control.type = "button";
    control.setAttribute("aria-pressed", String(active));
    control.addEventListener("click", () => runAction(control, action));
    return control;
}
function createFilterAction(label, action, runAction) {
    const control = createElement("button", "activity-filter-chip activity-filter-clear", label);
    control.type = "button";
    control.addEventListener("click", () => runAction(control, action));
    return control;
}
function activityTagChoices(activities) {
    var _a;
    const counts = new Map();
    for (const activity of activities.items) {
        for (const tag of activity.tags)
            counts.set(tag, ((_a = counts.get(tag)) !== null && _a !== void 0 ? _a : 0) + 1);
    }
    if (activities.tag && !counts.has(activities.tag))
        counts.set(activities.tag, 0);
    const tags = [...counts.keys()].sort((left, right) => {
        return (counts.get(right) - counts.get(left)) || left.localeCompare(right, "zh-CN");
    });
    const choices = tags.slice(0, 16);
    if (activities.tag && !choices.includes(activities.tag))
        choices[choices.length - 1] = activities.tag;
    return choices;
}
function hasActivityFilters(activities) {
    return Boolean(activities.search
        || activities.kind
        || activities.status
        || activities.favoriteOnly
        || activities.tag
        || activities.dateFrom
        || activities.dateTo);
}
function fillActivityKindFilter(activities, elements) {
    const selected = activities.kind;
    elements.activityKindFilter.replaceChildren();
    const allKinds = createElement("option", "", "全部类型");
    allKinds.value = "";
    elements.activityKindFilter.append(allKinds);
    for (const kind of activities.knownKinds) {
        const option = createElement("option", "", activityKindLabel(kind));
        option.value = kind;
        option.selected = kind === selected;
        elements.activityKindFilter.append(option);
    }
}
function filterActivitiesByTag(activities) {
    if (!activities.tag)
        return activities.items;
    return activities.items.filter((activity) => activity.tags.includes(activities.tag));
}
function filterActivitiesByDate(activities, items) {
    if (activities.status === "ended") {
        return items.filter((activity) => activity.status === "ended");
    }
    const start = activities.dateFrom ? Date.parse(`${activities.dateFrom}T00:00:00.000Z`) : null;
    const end = activities.dateTo ? Date.parse(`${activities.dateTo}T23:59:59.999Z`) : null;
    if (start === null && end === null)
        return items;
    return items.filter((activity) => {
        if (Number.isFinite(activity.temporary_open_until_ms)
            && Number.isFinite(activities.serverTimeMs)
            && activities.serverTimeMs >= (start !== null && start !== void 0 ? start : -8640000000000000)
            && activities.serverTimeMs <= (end !== null && end !== void 0 ? end : 8640000000000000)) {
            return true;
        }
        return activityOverlapsRange(activity, start !== null && start !== void 0 ? start : -8640000000000000, end !== null && end !== void 0 ? end : 8640000000000000);
    });
}
exports.filterActivitiesByDate = filterActivitiesByDate;
function createActivityCard(activity, actions) {
    const card = createElement("article", "activity-card");
    card.append(createActivityImage(activity, "activity-card-banner"));
    const content = createElement("div", "activity-card-content");
    const head = createElement("div", "card-head");
    const title = createElement("div");
    title.append(createElement("h3", "card-title", activity.name));
    title.append(createElement("p", "meta", `${activityKindLabel(activity.kind)} / ${activity.activity_id}`));
    head.append(title, createActivityStatusBadge(activity.status));
    content.append(head);
    if (activity.description)
        content.append(createElement("p", "activity-card-description", activity.description));
    content.append(createElement("p", "meta", activityWindowLabel(activity)));
    const controls = createElement("div", "card-actions");
    controls.append(createButton("查看详情", async () => {
        actions.selectActivity(activity.activity_id);
        return undefined;
    }, actions.runAction));
    controls.append(createButton(activity.favorite ? "取消收藏" : "收藏", async () => {
        return actions.toggleActivityFavorite(activity);
    }, actions.runAction));
    const hasTemporaryOpen = Number.isFinite(activity.temporary_open_until_ms);
    if (hasTemporaryOpen) {
        controls.append(createButton("结束临时开放", async () => {
            return actions.temporaryActivityAction(activity, false);
        }, actions.runAction, "danger"));
    }
    else if (isUnderlyingActivityOpen(activity)) {
        controls.append(createButton("结束", async () => actions.closeActivity(activity), actions.runAction, "danger"));
    }
    else {
        controls.append(createButton("开放 24 小时", async () => {
            return actions.temporaryActivityAction(activity, true);
        }, actions.runAction, "primary"));
    }
    content.append(controls);
    card.append(content);
    return card;
}
function renderActivityCalendar(activities, elements, actions, items) {
    const month = /^\d{4}-\d{2}$/.test(activities.calendarMonth)
        ? activities.calendarMonth
        : new Date().toISOString().slice(0, 7);
    const firstDay = new Date(`${month}-01T00:00:00.000Z`);
    const year = firstDay.getUTCFullYear();
    const monthIndex = firstDay.getUTCMonth();
    const dayCount = new Date(Date.UTC(year, monthIndex + 1, 0)).getUTCDate();
    const leadingDays = (firstDay.getUTCDay() + 6) % 7;
    const currentDate = Number.isFinite(activities.serverTimeMs)
        ? new Date(activities.serverTimeMs).toISOString().slice(0, 10)
        : null;
    elements.activityCalendarTitle.textContent = `${year} 年 ${monthIndex + 1} 月`;
    elements.activityCalendar.replaceChildren();
    for (const weekday of ["一", "二", "三", "四", "五", "六", "日"]) {
        elements.activityCalendar.append(createElement("span", "activity-calendar-weekday", weekday));
    }
    for (let index = 0; index < leadingDays; index += 1) {
        elements.activityCalendar.append(createElement("span", "activity-calendar-spacer"));
    }
    for (let day = 1; day <= dayCount; day += 1) {
        const date = `${month}-${String(day).padStart(2, "0")}`;
        const dayActivities = items.filter((activity) => {
            return (Number.isFinite(activity.temporary_open_until_ms) && date === currentDate)
                || activityOverlapsDate(activity, date);
        });
        const isSelected = activities.dateFrom === date && activities.dateTo === date;
        const cell = createElement("button", `activity-calendar-day${dayActivities.length ? " has-activity" : ""}${date === currentDate ? " is-current" : ""}${isSelected ? " is-selected" : ""}`);
        cell.type = "button";
        cell.setAttribute("role", "gridcell");
        cell.setAttribute("aria-label", `${date}, ${dayActivities.length} 个活动`);
        if (date === currentDate)
            cell.setAttribute("aria-current", "date");
        cell.setAttribute("aria-pressed", String(isSelected));
        cell.append(createElement("span", "activity-calendar-number", String(day)));
        if (dayActivities.length) {
            cell.append(createElement("span", "activity-calendar-count", String(dayActivities.length)));
            cell.title = dayActivities.slice(0, 4).map((activity) => activity.name).join("\n");
        }
        cell.addEventListener("click", () => actions.setActivityDate(date));
        elements.activityCalendar.append(cell);
    }
}
function renderActivityDetail(activities, elements, actions) {
    var _a, _b, _c, _d, _e, _f, _g, _h, _j;
    const activity = activities.items.find((item) => String(item.activity_id) === String(activities.selectedId));
    elements.activityDetail.hidden = !activity;
    if (!activity)
        return;
    elements.activityDetailTitle.textContent = activity.name;
    elements.activityDetailMeta.textContent = `${activityKindLabel(activity.kind)} / ${activity.activity_id} / ${activityStatusLabel(activity.status)}`;
    elements.activityDetailDescription.textContent = activity.description || "目录没有提供活动说明.";
    elements.activityDetailTags.replaceChildren();
    for (const tag of activity.tags)
        elements.activityDetailTags.append(createElement("span", "activity-tag", tag));
    renderActivityDetailImage(activity, elements);
    elements.activityFavoriteButton.textContent = activity.favorite ? "取消收藏" : "收藏";
    const hasTemporaryOpen = Number.isFinite(activity.temporary_open_until_ms);
    const hasUnderlyingOpen = isUnderlyingActivityOpen(activity);
    elements.activityOpenButton.textContent = "开放 24 小时";
    elements.activityOpenButton.disabled = hasTemporaryOpen || hasUnderlyingOpen;
    elements.activityCloseButton.textContent = hasTemporaryOpen ? "结束临时开放" : "快速结束";
    elements.activityCloseButton.disabled = !hasTemporaryOpen && !hasUnderlyingOpen;
    elements.activityTemporaryState.textContent = hasTemporaryOpen
        ? "临时开放至 " + formatLocalTime(activity.temporary_open_until_ms) + " (本机时间)"
        : "当前没有临时开放.";
    elements.activityWindowStart.value = activityInputTime(activityTime(activity, "start"));
    elements.activityWindowEnd.value = activityInputTime(activityTime(activity, "end"));
    elements.activityMode.value = (_c = (_a = activity.mode) !== null && _a !== void 0 ? _a : (_b = activity.schedule) === null || _b === void 0 ? void 0 : _b.mode) !== null && _c !== void 0 ? _c : "manual";
    elements.activityPeriodKind.value = (_f = (_d = activity.period) !== null && _d !== void 0 ? _d : (_e = activity.schedule) === null || _e === void 0 ? void 0 : _e.period) !== null && _f !== void 0 ? _f : "once";
    elements.activityPeriodInterval.value = String((_j = (_g = activity.interval_days) !== null && _g !== void 0 ? _g : (_h = activity.schedule) === null || _h === void 0 ? void 0 : _h.interval_days) !== null && _j !== void 0 ? _j : 1);
    elements.activityPeriodInterval.disabled = elements.activityPeriodKind.value !== "interval_days";
    elements.activityDetailClose.onclick = actions.closeActivityDetail;
    elements.activityFavoriteButton.onclick = () => actions.runAction(elements.activityFavoriteButton, () => actions.toggleActivityFavorite(activity));
    elements.activityOpenButton.onclick = () => actions.runAction(elements.activityOpenButton, () => actions.temporaryActivityAction(activity, true));
    elements.activityCloseButton.onclick = () => actions.runAction(elements.activityCloseButton, () => hasTemporaryOpen
        ? actions.temporaryActivityAction(activity, false)
        : actions.closeActivity(activity));
}
exports.renderActivityDetail = renderActivityDetail;
function renderActivityDetailImage(activity, elements) {
    const frame = elements.activityDetailBanner.parentElement;
    renderActivityImage(activity, frame, elements.activityDetailBanner, elements.activityDetailBannerPlaceholder);
}
function createActivityImage(activity, className) {
    const frame = createElement("figure", `activity-banner-frame ${className}`);
    const image = createElement("img");
    image.loading = "lazy";
    image.decoding = "async";
    const placeholder = createElement("span", "activity-banner-placeholder", "暂无活动图片");
    frame.append(image, placeholder);
    renderActivityImage(activity, frame, image, placeholder);
    return frame;
}
function renderActivityImage(activity, frame, image, placeholder) {
    const candidates = activityImageCandidates(activity);
    let candidateIndex = 0;
    image.alt = `${activity.name} 活动图片`;
    image.decoding = "async";
    image.loading = "lazy";
    image.onerror = null;
    image.removeAttribute("src");
    const loadNextCandidate = () => {
        var _a;
        const candidate = candidates[candidateIndex++];
        if (!candidate) {
            image.hidden = true;
            placeholder.hidden = false;
            frame.style.aspectRatio = "";
            delete frame.dataset.imageSource;
            frame.removeAttribute("title");
            return;
        }
        const sourceLabel = activityImageSourceLabel(candidate.source_type);
        frame.style.aspectRatio = (_a = activityImageAspectRatio(candidate)) !== null && _a !== void 0 ? _a : "";
        frame.dataset.imageSource = sourceLabel;
        if (candidate.evidence)
            frame.title = `${sourceLabel}: ${candidate.evidence}`;
        else
            frame.removeAttribute("title");
        image.hidden = false;
        placeholder.hidden = true;
        image.onerror = () => {
            if (image.getAttribute("src") !== candidate.url)
                return;
            loadNextCandidate();
        };
        image.src = candidate.url;
    };
    loadNextCandidate();
}
function activityImageCandidates(activity) {
    var _a, _b;
    const candidates = Array.isArray(activity.image_candidates) ? activity.image_candidates : [];
    const normalized = candidates.flatMap((candidate) => {
        var _a, _b;
        const url = safeActivityImageUrl(candidate === null || candidate === void 0 ? void 0 : candidate.url);
        if (!url)
            return [];
        return [{
                evidence: String((_a = candidate.evidence) !== null && _a !== void 0 ? _a : ""),
                height: Number(candidate.height) || null,
                source_type: String((_b = candidate.source_type) !== null && _b !== void 0 ? _b : "unclassified"),
                url,
                width: Number(candidate.width) || null,
            }];
    });
    const legacyUrl = safeActivityImageUrl(activity.banner_url);
    if (legacyUrl && !normalized.some((candidate) => candidate.url === legacyUrl)) {
        normalized.unshift({
            evidence: String((_a = activity.banner_evidence) !== null && _a !== void 0 ? _a : ""),
            height: Number(activity.banner_height) || null,
            source_type: String((_b = activity.banner_source_type) !== null && _b !== void 0 ? _b : "activity_banner"),
            url: legacyUrl,
            width: Number(activity.banner_width) || null,
        });
    }
    return normalized;
}
function activityImageAspectRatio(image) {
    const width = Number(image.width);
    const height = Number(image.height);
    if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0)
        return null;
    return `${width} / ${height}`;
}
function activityImageSourceLabel(sourceType) {
    var _a, _b;
    return (_b = (_a = {
        activity_banner: "活动 banner",
        dedicated_banner: "活动 banner",
        event_banner: "活动 banner",
        home_banner: "首页滚动图",
        home_carousel: "首页滚动图",
        notice_banner: "公告图",
        activity_entry: "活动入口图",
        event_entry: "活动入口图",
        boss_cover: "Boss 封面",
        quest_cover: "关卡封面",
        stage_cover: "关卡封面",
        exchange_icon: "交换所图标",
        mission_icon: "任务图标",
        shop_icon: "商店图标",
        shop_exchange: "商店或交换所图片",
        activity_logo: "活动 logo",
        activity_background: "活动背景",
        master_image: "活动附属图片",
        type_placeholder: "类型占位图",
        unclassified: "图片来源未标注",
    }[sourceType]) !== null && _a !== void 0 ? _a : sourceType) !== null && _b !== void 0 ? _b : "图片来源未标注";
}
function safeActivityImageUrl(value) {
    if (typeof value !== "string" || !value)
        return null;
    try {
        const url = new URL(value, location.origin);
        if (url.origin !== location.origin)
            return null;
        if (!url.pathname.startsWith("/manage/assets/activity-banners/"))
            return null;
        return `${url.pathname}${url.search}`;
    }
    catch (_a) {
        return null;
    }
}
function activityTime(activity, boundary) {
    var _a, _b;
    const names = boundary === "start"
        ? ["start_at_ms", "start_at", "default_start_at_ms", "default_start_at", "next_start_at_ms", "next_start_at"]
        : ["end_at_ms", "end_at", "default_end_at_ms", "default_end_at", "next_end_at_ms", "next_end_at"];
    for (const name of names) {
        const value = (_a = activity[name]) !== null && _a !== void 0 ? _a : (_b = activity.schedule) === null || _b === void 0 ? void 0 : _b[name];
        if (typeof value === "number" && Number.isFinite(value))
            return value;
        if (typeof value === "string" && value) {
            const parsed = Date.parse(value);
            if (Number.isFinite(parsed))
                return parsed;
        }
    }
    return null;
}
function activityOverlapsDate(activity, date) {
    const startOfDay = Date.parse(`${date}T00:00:00.000Z`);
    const endOfDay = startOfDay + 86400000 - 1;
    return activityOverlapsRange(activity, startOfDay, endOfDay);
}
// //// 按活动周期判断日期范围是否有活动窗口 [@x380kkm 2026-08-19] ////
function activityOverlapsRange(activity, rangeStart, rangeEnd) {
    var _a, _b, _c, _d, _e, _f, _g;
    const mode = (_c = (_a = activity.mode) !== null && _a !== void 0 ? _a : (_b = activity.schedule) === null || _b === void 0 ? void 0 : _b.mode) !== null && _c !== void 0 ? _c : "window";
    if (mode === "always")
        return true;
    if (mode === "manual")
        return false;
    if (activity.enabled === false || ((_d = activity.schedule) === null || _d === void 0 ? void 0 : _d.enabled) === false || activity.status === "disabled")
        return false;
    const start = activityNumber(activity, ["start_at_ms", "default_start_at_ms", "start_at", "default_start_at"]);
    const end = activityNumber(activity, ["end_at_ms", "default_end_at_ms", "end_at", "default_end_at"]);
    if (start === null || end === null || end <= start || rangeEnd < rangeStart)
        return false;
    const period = (_g = (_e = activity.period) !== null && _e !== void 0 ? _e : (_f = activity.schedule) === null || _f === void 0 ? void 0 : _f.period) !== null && _g !== void 0 ? _g : "once";
    if (mode !== "periodic" || period === "once")
        return start <= rangeEnd && end > rangeStart;
    const duration = end - start;
    if (!Number.isFinite(duration) || duration <= 0)
        return false;
    if (period === "monthly")
        return monthlyActivityOverlapsRange(start, duration, rangeStart, rangeEnd);
    const periodMs = activityPeriodMilliseconds(activity, period);
    if (periodMs === null)
        return start <= rangeEnd && end > rangeStart;
    if (rangeEnd < start)
        return false;
    const effectiveStart = Math.max(rangeStart, start);
    if (rangeEnd - effectiveStart >= periodMs)
        return true;
    const firstIndex = Math.max(0, Math.floor((effectiveStart - start - duration) / periodMs));
    const lastIndex = Math.max(0, Math.floor((rangeEnd - start) / periodMs));
    if (lastIndex < firstIndex)
        return false;
    for (let index = firstIndex; index <= lastIndex; index += 1) {
        const occurrenceStart = start + index * periodMs;
        if (occurrenceStart <= rangeEnd && occurrenceStart + duration > rangeStart)
            return true;
    }
    return false;
}
function activityNumber(activity, names) {
    var _a, _b;
    for (const name of names) {
        const value = (_a = activity[name]) !== null && _a !== void 0 ? _a : (_b = activity.schedule) === null || _b === void 0 ? void 0 : _b[name];
        if (typeof value === "number" && Number.isFinite(value))
            return value;
        if (typeof value === "string" && value) {
            const parsed = Date.parse(value);
            if (Number.isFinite(parsed))
                return parsed;
        }
    }
    return null;
}
function activityPeriodMilliseconds(activity, period) {
    var _a, _b;
    if (period === "daily")
        return 86400000;
    if (period === "weekly")
        return 7 * 86400000;
    if (period !== "interval_days")
        return null;
    const intervalDays = Number((_a = activity.interval_days) !== null && _a !== void 0 ? _a : (_b = activity.schedule) === null || _b === void 0 ? void 0 : _b.interval_days);
    return Number.isInteger(intervalDays) && intervalDays > 0 ? intervalDays * 86400000 : null;
}
function monthlyActivityOverlapsRange(start, duration, rangeStart, rangeEnd) {
    const startDate = new Date(start);
    const rangeStartDate = new Date(rangeStart);
    const rangeEndDate = new Date(rangeEnd);
    if ([startDate, rangeStartDate, rangeEndDate].some((date) => Number.isNaN(date.valueOf())))
        return false;
    const startOrdinal = startDate.getUTCFullYear() * 12 + startDate.getUTCMonth();
    const rangeStartOrdinal = rangeStartDate.getUTCFullYear() * 12 + rangeStartDate.getUTCMonth();
    const currentIndex = Math.max(0, rangeStartOrdinal - startOrdinal);
    const firstIndex = Math.max(0, currentIndex - 1);
    const lastIndex = currentIndex + 1;
    for (let index = firstIndex; index <= lastIndex; index += 1) {
        const occurrenceStart = addUtcMonths(startDate, index);
        if (occurrenceStart.getTime() <= rangeEnd && occurrenceStart.getTime() + duration > rangeStart)
            return true;
    }
    return false;
}
function addUtcMonths(date, months) {
    const target = new Date(Date.UTC(date.getUTCFullYear(), date.getUTCMonth() + months, 1, date.getUTCHours(), date.getUTCMinutes(), date.getUTCSeconds(), date.getUTCMilliseconds()));
    const lastDay = new Date(Date.UTC(target.getUTCFullYear(), target.getUTCMonth() + 1, 0)).getUTCDate();
    target.setUTCDate(Math.min(date.getUTCDate(), lastDay));
    return target;
}
// //// /按活动周期判断日期范围是否有活动窗口 ////
function activityWindowLabel(activity) {
    const start = activityTime(activity, "start");
    const end = activityTime(activity, "end");
    if (start === null && end === null)
        return "未设置活动时间";
    if (start !== null && end !== null)
        return `${formatUtcTime(start)} 至 ${formatUtcTime(end)}`;
    return start !== null ? `${formatUtcTime(start)} 开始` : `${formatUtcTime(end)} 结束`;
}
function activityInputTime(value) {
    if (value === null)
        return "";
    const date = new Date(value);
    return Number.isNaN(date.valueOf()) ? "" : date.toISOString().slice(0, 19);
}
function formatUtcTime(value) {
    const date = new Date(value);
    return Number.isNaN(date.valueOf()) ? String(value) : date.toISOString().replace(".000Z", "Z");
}
function formatLocalTime(value) {
    const date = new Date(value);
    return Number.isNaN(date.valueOf()) ? String(value) : date.toLocaleString(undefined, { hour12: false });
}
function createActivityStatusBadge(status) {
    return createElement("span", `badge activity-status ${status}`, activityStatusLabel(status));
}
function isUnderlyingActivityOpen(activity) {
    var _a;
    const status = (_a = activity.underlying_status) !== null && _a !== void 0 ? _a : activity.status;
    return status === "open" || status === "unscheduled";
}
function activityStatusLabel(status) {
    var _a;
    return (_a = {
        disabled: "已关闭",
        ended: "已结束",
        not_started: "未开始",
        open: "进行中",
        unscheduled: "未排期",
    }[status]) !== null && _a !== void 0 ? _a : status;
}
function activityKindLabel(kind) {
    var _a;
    return (_a = {
        "active-mission": "限时任务",
        advent: "Advent 活动",
        "box-gacha": "Box Gacha",
        carnival: "Carnival 活动",
        challenge: "挑战关卡",
        "collect-item": "收集活动",
        daily: "每日轮换",
        event: "活动",
        "event-shop": "活动商店",
        payment: "内购商品",
        expert: "高难关卡",
        gacha: "卡池",
        "gacha-campaign": "抽卡活动",
        multi: "高难共斗",
        other: "其他",
        "pass-card": "Pass Card",
        "special-exchange": "普通自选",
        "start-dash-exchange": "新手自选",
        "treasure-vault-exchange": "宝库装备兑换",
        "character-election": "角色投票",
        raid: "Raid",
        ranking: "排名战",
        rush: "Rush 活动",
        "score-attack": "Score Attack",
        story: "剧情",
        "time-attack": "Time Attack",
        tower: "爬塔",
        "world-story": "World Story",
    }[kind]) !== null && _a !== void 0 ? _a : kind;
}
// //// /呈现活动目录和图片 ////
},
"/manage/activity-navigation.js": function(module, exports, require) {
"use strict";
// audience: external
// # personal-service-activity-navigation
// 活动入口按玩法分类, 同一关卡的难度共用一个选择入口.
Object.defineProperty(exports, "__esModule", { value: true });
exports.renderActivityNavigation = exports.activityGroups = exports.questStageKey = exports.questStageName = exports.activitySection = void 0;
const COMMERCE_KINDS = new Set(["payment", "pass-card", "special-exchange", "start-dash-exchange", "treasure-vault-exchange", "payment-rebate"]);
const DIFFICULTY_SUFFIX = /\s+(初级|中级|高级\+?|超级\+?|地狱级|地狱|超級|超級\+|极级|Lv\s*\d+)$/i;
// //// 区分限时活动, 领主战, 日常和内购 [@x380kkm 2026-09-26] ////
function activitySection(activity) {
    const id = String(activity.activity_id);
    if (id.startsWith("quest:2:"))
        return "boss";
    if (/^quest:(6|14):/.test(id) || /^(daily-week|daily-exp-mana):/.test(id) || activity.kind === "daily")
        return "daily";
    if (COMMERCE_KINDS.has(activity.kind))
        return "commerce";
    return "events";
}
exports.activitySection = activitySection;
// //// 从关卡名称分离难度标签 [@x380kkm 2026-09-26] ////
function questStageName(quest) {
    const name = String(quest.name || quest.quest_id || quest.activity_id);
    const suffix = quest.difficulty && ` ${quest.difficulty}`;
    return suffix && name.endsWith(suffix) ? name.slice(0, -suffix.length) : name.replace(DIFFICULTY_SUFFIX, "");
}
exports.questStageName = questStageName;
// //// 按关卡编号区分同名领主与活动复刻 [@x380kkm 2026-09-26] ////
function questStageKey(quest) {
    const match = /^quest:(\d+):(\d+)$/.exec(quest.activity_id || "");
    const category = Number((match === null || match === void 0 ? void 0 : match[1]) || quest.category_id);
    const id = Number((match === null || match === void 0 ? void 0 : match[2]) || quest.quest_id);
    return [2, 6, 14].includes(category) ? `${category}:${Math.floor(id / 1000)}`
        : `${category}:${quest.activity_id || ""}:${questStageName(quest)}`;
}
exports.questStageKey = questStageKey;
// //// 按分类和活动归属组合关卡难度 [@x380kkm 2026-09-26] ////
function activityGroups(items) {
    const groups = new Map();
    for (const activity of items) {
        const match = /^quest:(\d+):/.exec(activity.activity_id);
        const name = match ? questStageName(activity) : activity.name;
        const key = match ? questStageKey(activity) : activity.activity_id;
        if (!groups.has(key))
            groups.set(key, { key, name, items: [] });
        groups.get(key).items.push(activity);
    }
    const result = [...groups.values()];
    for (const group of result) {
        const name = group.name;
        if (result.some(other => other !== group && other.name === name)) {
            for (const other of result.filter(item => item.name === name))
                other.name += ` (${other.key})`;
        }
        group.items.sort((a, b) => a.activity_id.localeCompare(b.activity_id, "en", { numeric: true }));
    }
    return result.sort((a, b) => a.name.localeCompare(b.name, "zh-CN"));
}
exports.activityGroups = activityGroups;
function fillOptions(select, entries, value) {
    var _a;
    select.replaceChildren();
    for (const [id, label] of entries) {
        const option = document.createElement("option");
        option.value = id;
        option.textContent = label;
        select.append(option);
    }
    select.value = value || ((_a = entries[0]) === null || _a === void 0 ? void 0 : _a[0]) || "";
    select.disabled = entries.length === 0;
}
// //// 呈现独立活动分类与关卡难度下拉框 [@x380kkm 2026-09-26] ////
function renderActivityNavigation(state, elements, actions, items) {
    var _a, _b, _c, _d;
    const select = elements.activityGroup;
    if (!select)
        return;
    const groups = activityGroups(items);
    const selected = (_b = (_a = groups.find(group => group.items.some(item => item.activity_id === state.selectedId))) !== null && _a !== void 0 ? _a : groups.find(group => group.key === state.groupId)) !== null && _b !== void 0 ? _b : groups[0];
    const section = state.section || "events";
    for (const tab of elements.activitySections) {
        const active = tab.dataset.activitySection === section;
        tab.setAttribute("aria-selected", String(active));
        tab.tabIndex = active ? 0 : -1;
        tab.onclick = () => actions.runAction(tab, () => actions.setActivitySection(tab.dataset.activitySection));
        tab.onkeydown = event => {
            const index = elements.activitySections.indexOf(tab);
            const next = event.key === "ArrowRight" ? index + 1 : event.key === "ArrowLeft" ? index - 1
                : event.key === "Home" ? 0 : event.key === "End" ? elements.activitySections.length - 1 : null;
            if (next === null)
                return;
            event.preventDefault();
            const target = elements.activitySections[(next + elements.activitySections.length) % elements.activitySections.length];
            target.focus();
            actions.runAction(target, () => actions.setActivitySection(target.dataset.activitySection));
        };
    }
    const calendar = section === "events";
    elements.activityCalendar.hidden = !calendar;
    elements.activityCalendarToolbar.hidden = !calendar;
    elements.activityCatalogList.hidden = section === "boss" || section === "daily";
    fillOptions(select, groups.map(group => [group.key, group.name]), selected === null || selected === void 0 ? void 0 : selected.key);
    const choices = (selected === null || selected === void 0 ? void 0 : selected.items) || [];
    fillOptions(elements.activityDifficulty, choices.map(item => {
        var _a;
        return [item.activity_id,
            ((_a = DIFFICULTY_SUFFIX.exec(item.name)) === null || _a === void 0 ? void 0 : _a[1]) || (item.kind === "quest" ? item.name : "开放与轮换规则")];
    }), choices.some(item => item.activity_id === state.selectedId) ? state.selectedId : (_c = choices[0]) === null || _c === void 0 ? void 0 : _c.activity_id);
    elements.activityDifficultyLabel.hidden = choices.length === 1 && ((_d = choices[0]) === null || _d === void 0 ? void 0 : _d.kind) !== "quest";
    state.groupId = (selected === null || selected === void 0 ? void 0 : selected.key) || null;
    if (section !== "events" && state.selectedId !== elements.activityDifficulty.value) {
        actions.selectActivity(elements.activityDifficulty.value || null);
    }
    select.onchange = () => {
        const group = groups.find(candidate => candidate.key === select.value);
        if (group)
            actions.selectActivity(group.items[0].activity_id);
        actions.render();
    };
    elements.activityDifficulty.onchange = () => actions.selectActivity(elements.activityDifficulty.value);
}
exports.renderActivityNavigation = renderActivityNavigation;
// //// /呈现独立活动分类与关卡难度下拉框 ////
},
"/manage/activity-history.js": function(module, exports, require) {
"use strict";
// audience: internal
// # personal-service-activity-history-view
// 活动详情按选定存档展示各轮成绩, 领奖和投递记录. 重置以当前轮次和用户确认提交.
Object.defineProperty(exports, "__esModule", { value: true });
exports.createActivityHistory = exports.historySummary = void 0;
const kindLabels = { statistics: "战斗结算", summary: "排名统计", claim: "奖励领取", delivery: "邮件投递", exchange: "商店兑换", reset: "活动重置", vote: "角色投票" };
const missionRewardLabels = ["星导石", "道具", "装备", "玛纳", "角色", "经验", "称号"];
// //// 将活动记录转换为可读摘要 [@x380kkm 2026-09-21] ////
function historySummary(entry) {
    var _a, _b, _c, _d, _e, _f, _g, _h;
    const details = (_a = entry.details) !== null && _a !== void 0 ? _a : {};
    if (entry.kind === "statistics") {
        const result = details.is_accomplished ? "通关" : "未通关";
        const best = details.best_record;
        const bestText = best ? `, ${bestRecordSummary(best)}` : "";
        return `${details.quest_name || `关卡 ${details.quest_id}`} - ${result}, ${seconds(details.elapsed_time_ms)}, 得分 ${(_b = details.score) !== null && _b !== void 0 ? _b : 0}${bestText}`;
    }
    if (entry.kind === "summary") {
        if (details.best_record)
            return `最佳 ${seconds(details.best_record.elapsed_time_ms)}, 排名 ${Number((_c = details.rank_percentage) !== null && _c !== void 0 ? _c : 100).toFixed(2)}%`;
        if (Array.isArray(details.records))
            return `${details.records.length} 个关卡, 总最佳分 ${details.records.reduce((sum, record) => { var _a; return sum + Number((_a = record.best_score) !== null && _a !== void 0 ? _a : 0); }, 0)}`;
        return "活动统计已记录, 展开查看成绩明细.";
    }
    if (entry.kind === "reset")
        return `第 ${details.previous_round + 1} 轮结束, 开始第 ${details.next_round + 1} 轮`;
    if (entry.kind === "vote")
        return `投票角色关键词 ${details.keyword_id}`;
    if (entry.kind === "claim") {
        const receipt = (_d = details.receipt) !== null && _d !== void 0 ? _d : {};
        const rewards = (Array.isArray(receipt.rewards) ? receipt.rewards : []).map((reward) => {
            var _a, _b, _c, _d, _e;
            const id = (_c = (_b = (_a = reward.item_id) !== null && _a !== void 0 ? _a : reward.equipment_id) !== null && _b !== void 0 ? _b : reward.character_id) !== null && _c !== void 0 ? _c : reward.degree_id;
            return `${(_d = missionRewardLabels[reward.kind]) !== null && _d !== void 0 ? _d : "奖励"}${id ? ` ${id}` : ""} x${(_e = reward.amount) !== null && _e !== void 0 ? _e : 1}`;
        });
        if (receipt.tier_id)
            rewards.unshift(`排名档位 ${receipt.tier_id}`);
        return rewards.join(", ") || `领取凭据 ${details.receipt_id}`;
    }
    if (entry.kind === "delivery")
        return `${(_e = details.title) !== null && _e !== void 0 ? _e : "活动奖励"} - ${entry.received_at ? `已领取 (${formatTime(entry.received_at)})` : "已投递, 待领取"}`;
    if (entry.kind === "exchange")
        return `商店 ${(_g = (_f = details.request) === null || _f === void 0 ? void 0 : _f.shop_type) !== null && _g !== void 0 ? _g : ""}, 获得 ${((_h = details.rewards) !== null && _h !== void 0 ? _h : []).map((reward) => { var _a; return `${(_a = reward.type_id) !== null && _a !== void 0 ? _a : reward.kind} x${reward.number}`; }).join(", ")}`;
    return "";
}
exports.historySummary = historySummary;
// //// 按成绩结构显示通关竞速和关卡最佳记录 [@x380kkm 2026-09-28] ////
function bestRecordSummary(best) {
    var _a, _b, _c;
    if (typeof best.is_accomplished === "boolean" && best.elapsed_time_ms !== undefined) {
        const result = best.is_accomplished ? seconds(best.elapsed_time_ms) : "未通关";
        return `最佳 ${result}, 得分 ${(_a = best.score) !== null && _a !== void 0 ? _a : 0}`;
    }
    return `最佳 ${seconds((_b = best.best_elapsed_time_ms) !== null && _b !== void 0 ? _b : best.elapsed_time_ms)}, 最高分 ${(_c = best.high_score) !== null && _c !== void 0 ? _c : 0}`;
}
function seconds(value) { return value !== null && value !== undefined && Number.isFinite(Number(value)) ? `${(Number(value) / 1000).toFixed(3)} 秒` : "无成绩"; }
function formatTime(value) { return new Date(value * 1000).toLocaleString("zh-CN", { hour12: false }); }
// //// 管理活动历史读取和明确的重置操作 [@x380kkm 2026-09-21] ////
function createActivityHistory({ requestApi, runAction, document = globalThis.document, confirm = globalThis.confirm, prefix = "activity-history", onReset }) {
    const controls = Object.fromEntries(["save", "state", "list", "refresh", "more", "reset"].map((key) => [key, document.querySelector(`#${prefix}-${key}`)]));
    let activity = null;
    let round = null;
    let entries = [];
    let nextBeforeId = null;
    let generation = 0;
    let canReset = false;
    function clear() {
        generation += 1;
        round = null;
        canReset = false;
        entries = [];
        nextBeforeId = null;
        controls.list.replaceChildren();
        controls.reset.disabled = true;
        controls.more.hidden = true;
        controls.state.textContent = "选择存档后查看活动记录.";
    }
    function render() {
        var _a;
        controls.list.replaceChildren();
        for (const entry of entries) {
            const card = document.createElement("details");
            card.className = "form-drawer";
            const summary = document.createElement("summary");
            summary.textContent = `第 ${entry.participation_round + 1} 轮 · ${(_a = kindLabels[entry.kind]) !== null && _a !== void 0 ? _a : entry.kind} · ${formatTime(entry.created_at)}`;
            const description = document.createElement("p");
            description.textContent = historySummary(entry);
            const raw = document.createElement("pre");
            raw.className = "activity-history-data";
            raw.textContent = JSON.stringify(entry.details, null, 2);
            card.append(summary, description, raw);
            controls.list.append(card);
        }
        controls.more.hidden = nextBeforeId === null;
        controls.state.textContent = `当前第 ${round + 1} 轮, 显示 ${entries.length} 条记录.`;
    }
    async function load(append = false) {
        if (!activity || !controls.save.value) {
            clear();
            return;
        }
        const selectedActivity = activity.activity_id;
        const slotId = controls.save.value;
        const requestGeneration = ++generation;
        canReset = false;
        controls.reset.disabled = true;
        const query = append && nextBeforeId ? `?before_id=${nextBeforeId}` : "";
        const payload = await requestApi(`/v1/local-saves/${slotId}/activities/${encodeURIComponent(selectedActivity)}/history${query}`);
        if (requestGeneration !== generation)
            return;
        round = payload.participation_round;
        entries = append ? [...entries, ...payload.entries] : payload.entries;
        nextBeforeId = payload.next_before_id;
        canReset = payload.can_reset === true;
        controls.reset.disabled = !canReset;
        render();
    }
    async function reset() {
        var _a, _b;
        if (!canReset || round === null || !activity || !controls.save.value)
            return;
        const slotName = (_b = (_a = controls.save.selectedOptions[0]) === null || _a === void 0 ? void 0 : _a.textContent) !== null && _b !== void 0 ? _b : controls.save.value;
        const selectedActivity = activity.activity_id;
        const slotId = controls.save.value;
        const expectedRound = round;
        if (!confirm(`重置存档 "${slotName}" 的活动 "${activity.name}" (第 ${expectedRound + 1} 轮)?\n成绩, 领奖资格和活动商店限购会重新开始. 历史记录, 邮件和已获得资产会保留.\n完成后请返回游戏标题画面重新加载存档.`))
            return;
        await requestApi(`/v1/local-saves/${slotId}/activities/${encodeURIComponent(selectedActivity)}/reset`, {
            method: "POST", body: { expected_round: expectedRound },
        });
        await load();
        await (onReset === null || onReset === void 0 ? void 0 : onReset());
        return "活动已重置, 请重新加载游戏存档后参加.";
    }
    function renderSaves(slots, selected) {
        controls.save.replaceChildren();
        const empty = document.createElement("option");
        empty.value = "";
        empty.textContent = "选择存档";
        controls.save.append(empty);
        for (const slot of slots) {
            const option = document.createElement("option");
            option.value = String(slot.id);
            option.textContent = `${slot.name} (#${slot.id})`;
            controls.save.append(option);
        }
        controls.save.value = slots.some(slot => String(slot.id) === selected) ? selected : "";
        clear();
    }
    controls.save.addEventListener("change", () => { clear(); return runAction(controls.refresh, () => load()); });
    controls.refresh.addEventListener("click", () => runAction(controls.refresh, () => load()));
    controls.more.addEventListener("click", () => runAction(controls.more, () => load(true)));
    controls.reset.addEventListener("click", async () => {
        await runAction(controls.reset, reset);
        controls.reset.disabled = !canReset;
    });
    clear();
    return {
        selectActivity(value) { activity = value; clear(); return runAction(controls.refresh, () => load()); },
        selectContext({ activity: value, slot }) {
            activity = value;
            renderSaves(slot ? [slot] : [], slot ? String(slot.id) : "");
            return runAction(controls.refresh, () => load());
        },
        setSaves(slots) {
            renderSaves(slots, controls.save.value);
            return runAction(controls.refresh, () => load());
        },
    };
}
exports.createActivityHistory = createActivityHistory;
},
"/manage/ai-team-controller.js": function(module, exports, require) {
"use strict";
// audience: external
// # personal-service-ai-team-controller
//
// 此模块从所选本地存档读取有效编队, 并保存供本机建房和救援使用的两个编队快照.
Object.defineProperty(exports, "__esModule", { value: true });
exports.createAiTeamController = exports.describePartyCandidate = exports.requireDistinctPartyIds = void 0;
// //// 选择并保存两个不同的存档编队 [@x380kkm 2026-08-20] ////
function createElement(tagName, className, text) {
    const node = document.createElement(tagName);
    if (className)
        node.className = className;
    if (text !== undefined)
        node.textContent = text;
    return node;
}
function requireDistinctPartyIds(values) {
    const partyIds = values.map(Number);
    if (partyIds.length !== 2 || partyIds.some((partyId) => !Number.isInteger(partyId) || partyId <= 0)) {
        throw new Error("请为两个 AI 分别选择有效编队.");
    }
    if (partyIds[0] === partyIds[1])
        throw new Error("两个 AI 必须使用不同编队.");
    return partyIds;
}
exports.requireDistinctPartyIds = requireDistinctPartyIds;
function describePartyCandidate(candidate) {
    var _a;
    const partyId = Number(candidate.party_id);
    const name = String((_a = candidate.name) !== null && _a !== void 0 ? _a : "").trim() || `编队 ${partyId}`;
    const characterIds = Array.isArray(candidate.character_ids)
        ? candidate.character_ids.filter((value) => Number.isInteger(Number(value)) && Number(value) > 0)
        : [];
    const characterText = characterIds.length > 0
        ? `角色 ${characterIds.join(" / ")}`
        : "没有可显示的角色";
    return { partyId, name, characterText };
}
exports.describePartyCandidate = describePartyCandidate;
function createAiTeamController({ elements, requestApi, runAction }) {
    const state = {
        candidates: [],
        generation: 0,
        slots: [],
        sourceLoaded: false,
        currentSlotId: null,
    };
    function selectedSlotId() {
        const slotId = Number(elements.aiTeamSlot.value);
        return Number.isInteger(slotId) && slotId > 0 ? slotId : null;
    }
    function setStatus(message) {
        elements.aiTeamState.textContent = message;
    }
    function renderSource(source) {
        const slot = state.slots.find((slot) => Number(slot.id) === Number(source === null || source === void 0 ? void 0 : source.slot_id));
        elements.aiTeamSource.textContent = slot
            ? `实际来源: ${slot.name} (槽位 ${slot.id}). 新建房间和救援使用该存档的两个已保存编队.`
            : "实际来源: 跟随当前游戏存档. 新建房间和救援读取当前玩家的编队.";
        elements.aiTeamFollow.disabled = !slot;
    }
    function renderCandidateList() {
        elements.aiTeamCandidates.replaceChildren();
        if (state.candidates.length === 0) {
            elements.aiTeamCandidates.append(createElement("p", "empty-state", "所选存档没有可用编队."));
            return;
        }
        for (const candidate of state.candidates) {
            const description = describePartyCandidate(candidate);
            const card = createElement("article", "ai-party-candidate");
            card.append(createElement("strong", "", description.name), createElement("span", "meta", `编队 ID ${description.partyId}`), createElement("span", "meta", description.characterText));
            elements.aiTeamCandidates.append(card);
        }
    }
    function fillSelector(select, selectedPartyId) {
        select.replaceChildren();
        for (const candidate of state.candidates) {
            const description = describePartyCandidate(candidate);
            const option = createElement("option", "", `${description.name} · ${description.characterText}`);
            option.value = String(description.partyId);
            option.selected = description.partyId === selectedPartyId;
            select.append(option);
        }
    }
    function keepSelectionsDistinct(changedSelect) {
        const otherSelect = changedSelect === elements.aiTeamA ? elements.aiTeamB : elements.aiTeamA;
        for (const option of otherSelect.options)
            option.disabled = option.value === changedSelect.value;
        if (otherSelect.value !== changedSelect.value)
            return;
        const alternative = Array.from(otherSelect.options).find((option) => !option.disabled);
        if (alternative)
            otherSelect.value = alternative.value;
    }
    function renderSelection(response) {
        var _a, _b, _c, _d;
        renderSource(response.source);
        state.candidates = Array.isArray(response.candidates) ? response.candidates : [];
        const selected = Array.isArray(response.selected_party_ids)
            ? response.selected_party_ids.map(Number)
            : [];
        const defaults = state.candidates.slice(0, 2).map((candidate) => Number(candidate.party_id));
        fillSelector(elements.aiTeamA, (_a = selected[0]) !== null && _a !== void 0 ? _a : defaults[0]);
        fillSelector(elements.aiTeamB, (_b = selected[1]) !== null && _b !== void 0 ? _b : defaults[1]);
        if (elements.aiTeamA.value)
            keepSelectionsDistinct(elements.aiTeamA);
        if (elements.aiTeamB.value)
            keepSelectionsDistinct(elements.aiTeamB);
        renderCandidateList();
        const usable = state.candidates.length >= 2;
        elements.aiTeamA.disabled = !usable;
        elements.aiTeamB.disabled = !usable;
        elements.aiTeamSave.disabled = !usable;
        elements.aiTeamDefault.disabled = !usable;
        const isActiveSource = ((_c = response.source) === null || _c === void 0 ? void 0 : _c.mode) === "selected"
            && Number(response.source.slot_id) === Number(response.slot_id);
        const messages = {
            ready: isActiveSource
                ? `该存档的 AI A 和 AI B 已用于后续建房和救援, 共发现 ${state.candidates.length} 个有效编队.`
                : `所选存档已保存两个编队模板, 共发现 ${state.candidates.length} 个有效编队. 点击保存并使用可将它设为实际来源.`,
            default_template_required: "所选存档使用内置 AI 队伍. 保存两个有效编队后可选用自己的队伍.",
            manual_selection_required: "所选存档使用内置 AI 队伍. 可在这里选择并保存自己的两个编队.",
            selection_unavailable: "所选存档无法建立两个 AI 编队快照.",
        };
        setStatus((_d = messages[response.selection_status]) !== null && _d !== void 0 ? _d : "AI 编队状态已载入.");
    }
    async function load() {
        const generation = ++state.generation;
        elements.aiTeamA.disabled = true;
        elements.aiTeamB.disabled = true;
        elements.aiTeamSave.disabled = true;
        elements.aiTeamDefault.disabled = true;
        const source = await requestApi("/v1/local-saves/ai-team-source");
        if (generation !== state.generation)
            return;
        renderSource(source);
        if (!state.sourceLoaded) {
            const slot = state.slots.find((slot) => Number(slot.id) === Number(source.slot_id));
            if (slot)
                elements.aiTeamSlot.value = String(slot.id);
            state.sourceLoaded = true;
        }
        const slotId = selectedSlotId();
        if (slotId === null) {
            state.candidates = [];
            renderCandidateList();
            setStatus("当前没有可管理的本地存档槽.");
            return;
        }
        setStatus("正在读取存档编队.");
        const response = await requestApi(`/v1/local-saves/${slotId}/ai-teams`);
        if (generation !== state.generation || slotId !== selectedSlotId())
            return;
        renderSelection(response);
    }
    async function saveSelected() {
        const slotId = selectedSlotId();
        const generation = state.generation;
        if (slotId === null)
            throw new Error("请选择本地存档槽.");
        const partyIds = requireDistinctPartyIds([elements.aiTeamA.value, elements.aiTeamB.value]);
        const response = await requestApi(`/v1/local-saves/${slotId}/ai-teams`, {
            method: "PUT",
            body: { party_ids: partyIds },
        });
        renderSource(response.source);
        if (generation === state.generation && slotId === selectedSlotId())
            renderSelection(response);
    }
    function setSlots(slots, preferredSlotId) {
        var _a;
        const currentSlotId = selectedSlotId();
        state.slots = Array.isArray(slots) ? slots : [];
        state.currentSlotId = Number(preferredSlotId);
        elements.aiTeamSlot.replaceChildren();
        for (const slot of state.slots) {
            const option = createElement("option", "", `${slot.name} (槽位 ${slot.id})`);
            option.value = String(slot.id);
            elements.aiTeamSlot.append(option);
        }
        const availableIds = new Set(state.slots.map((slot) => Number(slot.id)));
        const nextSlotId = availableIds.has(currentSlotId)
            ? currentSlotId
            : availableIds.has(Number(preferredSlotId))
                ? Number(preferredSlotId)
                : Number((_a = state.slots[0]) === null || _a === void 0 ? void 0 : _a.id);
        if (Number.isInteger(nextSlotId))
            elements.aiTeamSlot.value = String(nextSlotId);
    }
    elements.aiTeamA.addEventListener("change", () => keepSelectionsDistinct(elements.aiTeamA));
    elements.aiTeamB.addEventListener("change", () => keepSelectionsDistinct(elements.aiTeamB));
    elements.aiTeamSlot.addEventListener("change", () => runAction(null, load));
    elements.aiTeamSave.addEventListener("click", () => runAction(elements.aiTeamSave, async () => {
        await saveSelected();
        return "两个 AI 编队已保存, 后续建房和救援使用该存档.";
    }));
    elements.aiTeamDefault.addEventListener("click", () => runAction(elements.aiTeamDefault, async () => {
        const defaults = state.candidates.slice(0, 2).map((candidate) => candidate.party_id);
        const partyIds = requireDistinctPartyIds(defaults);
        elements.aiTeamA.value = String(partyIds[0]);
        elements.aiTeamB.value = String(partyIds[1]);
        await saveSelected();
        return "已保存并使用所选存档的前两个有效编队.";
    }));
    elements.aiTeamFollow.addEventListener("click", () => runAction(elements.aiTeamFollow, async () => {
        renderSource(await requestApi("/v1/local-saves/ai-team-source", { method: "DELETE" }));
        if (state.slots.some((slot) => Number(slot.id) === state.currentSlotId)) {
            elements.aiTeamSlot.value = String(state.currentSlotId);
        }
        await load();
        return "AI 来源已恢复为当前游戏存档, 已有房间的队友保持原编队.";
    }));
    return { load, setSlots };
}
exports.createAiTeamController = createAiTeamController;
// //// /选择并保存两个不同的存档编队 ////
},
"/manage/rescue-controller.js": function(module, exports, require) {
"use strict";
// audience: external
// # personal-service-rescue-controller
//
// 救援设置与关卡选择保存在本机服务, 关卡开放状态由活动日历计算.
// 列表筛选与分页保留完整选择, 刷新开放状态保留尚未保存的编辑.
Object.defineProperty(exports, "__esModule", { value: true });
exports.createRescueController = exports.filterRescueQuests = void 0;
const activity_navigation_js_1 = require("/manage/activity-navigation.js");
const PAGE_SIZE = 24;
const DIFFICULTY_ORDER = [1, 2, 3, 5, 4, 6, 7, 8];
function questKey(quest) {
    return `${quest.category_id}:${quest.quest_id}`;
}
function isInAvailabilityScope(quest, filters, selected) {
    if (filters.selectedOnly)
        return selected.has(questKey(quest));
    if (filters.status === "open")
        return quest.is_open;
    if (filters.status === "closed")
        return !quest.is_open;
    return true;
}
// //// 按玩法, 活动, 难度和开放状态筛选救援关卡 [@x380kkm 2026-09-23] ////
function filterRescueQuests(quests, filters, selected) {
    const search = filters.search.trim().toLocaleLowerCase("zh-CN");
    return quests.filter((quest) => {
        var _a;
        if (filters.category && String(quest.category_id) !== filters.category)
            return false;
        if (filters.activity && ((_a = quest.activity_id) !== null && _a !== void 0 ? _a : "permanent") !== filters.activity)
            return false;
        if (filters.stage && (0, activity_navigation_js_1.questStageKey)(quest) !== filters.stage)
            return false;
        if (filters.difficulty && String(quest.difficulty_id) !== filters.difficulty)
            return false;
        if (!isInAvailabilityScope(quest, filters, selected))
            return false;
        const text = [quest.name, quest.activity_name, quest.category_name, quest.difficulty, quest.quest_id]
            .join(" ").toLocaleLowerCase("zh-CN");
        return !search || text.includes(search);
    });
}
exports.filterRescueQuests = filterRescueQuests;
// //// /按玩法, 活动, 难度和开放状态筛选救援关卡 ////
// //// 读取, 编辑并保存机器人救援设置 [@x380kkm 2026-09-23] ////
function createRescueController({ panel, requestApi, runAction }) {
    const document = panel.ownerDocument;
    const elements = Object.fromEntries([
        "form", "settings-controls", "quest-controls", "enabled", "spawn-interval", "room-lifetime",
        "max-rooms", "state", "selection-state", "refresh", "disable", "category", "activity",
        "difficulty", "stage", "search", "status", "selected-only", "list", "page-state", "previous", "next",
        "select-page", "clear-page", "clear-all",
    ].map((name) => [name, panel.querySelector(`#rescue-${name}`)]));
    const state = {
        quests: [], selected: new Map(), settings: null, page: 0, busy: false, loaded: false,
        previousStatus: "open",
    };
    function createElement(tag, className, text) {
        const node = document.createElement(tag);
        if (className)
            node.className = className;
        if (text !== undefined)
            node.textContent = text;
        return node;
    }
    function filters() {
        return {
            category: elements.category.value,
            activity: elements.activity.value,
            stage: elements.stage.value,
            difficulty: elements.difficulty.value,
            search: elements.search.value,
            status: elements.status.value,
            selectedOnly: elements["selected-only"].checked,
        };
    }
    function filteredQuests() {
        return filterRescueQuests(state.quests, filters(), state.selected);
    }
    function availabilityQuests() {
        const currentFilters = filters();
        return state.quests.filter((quest) => isInAvailabilityScope(quest, currentFilters, state.selected));
    }
    function hasChanges() {
        if (!state.settings)
            return false;
        const saved = state.settings;
        return elements.enabled.checked !== saved.enabled
            || Number(elements["spawn-interval"].value) !== saved.spawn_interval_seconds
            || Number(elements["room-lifetime"].value) !== saved.room_lifetime_seconds
            || Number(elements["max-rooms"].value) !== saved.max_rooms
            || state.selected.size !== saved.quests.length
            || saved.quests.some((quest) => !state.selected.has(questKey(quest)));
    }
    function updateControls() {
        var _a;
        const disabled = state.busy || !state.loaded;
        elements["settings-controls"].disabled = disabled;
        elements["quest-controls"].disabled = disabled;
        elements.refresh.disabled = state.busy;
        elements.disable.disabled = disabled || !((_a = state.settings) === null || _a === void 0 ? void 0 : _a.enabled);
        elements.status.disabled = elements["selected-only"].checked;
        panel.setAttribute("aria-busy", String(state.busy));
    }
    async function withBusy(action) {
        if (state.busy)
            return;
        state.busy = true;
        updateControls();
        try {
            return await action();
        }
        finally {
            state.busy = false;
            updateControls();
        }
    }
    function renderSummary() {
        const selectedOpen = state.quests.filter((quest) => quest.is_open && state.selected.has(questKey(quest))).length;
        const selectedClosed = state.selected.size - selectedOpen;
        elements["selection-state"].textContent = `已选 ${state.selected.size} 个关卡, 当前可进入 ${selectedOpen} 个.`
            + (selectedClosed > 0 ? ` 暂不可进入的 ${selectedClosed} 个选择会保留.` : "")
            + (hasChanges() ? " 有修改, 保存后生效." : "");
        if (!state.settings)
            return;
        elements.state.textContent = !state.settings.enabled
            ? "救援已关闭."
            : state.settings.quests.length === 0
                ? "救援已开启, 请先选择关卡."
                : state.settings.eligible_quest_count > 0
                    ? `救援已开启, 当前可用 ${state.settings.eligible_quest_count} 个关卡.`
                    : "救援已开启, 等待所选关卡开放.";
    }
    function applySettings(settings) {
        state.settings = settings;
        state.selected = new Map(settings.quests.map((quest) => [questKey(quest), {
                category_id: quest.category_id, quest_id: quest.quest_id,
            }]));
        elements.enabled.checked = settings.enabled;
        elements["spawn-interval"].value = String(settings.spawn_interval_seconds);
        elements["room-lifetime"].value = String(settings.room_lifetime_seconds);
        elements["max-rooms"].value = String(settings.max_rooms);
    }
    function fillOptions(select, entries, allLabel) {
        const previous = select.value;
        const all = createElement("option", "", allLabel);
        all.value = "";
        select.replaceChildren(all);
        for (const [value, label] of entries) {
            const option = createElement("option", "", label);
            option.value = String(value);
            select.append(option);
        }
        select.value = entries.some(([value]) => String(value) === previous) ? previous : "";
    }
    function fillActivityOptions() {
        const quests = availabilityQuests().filter((quest) => !elements.category.value
            || String(quest.category_id) === elements.category.value);
        const activities = new Map(quests.map((quest) => {
            var _a;
            return [
                (_a = quest.activity_id) !== null && _a !== void 0 ? _a : "permanent",
                quest.activity_id ? `${quest.activity_name} (${quest.activity_id})` : "领主战",
            ];
        }));
        fillOptions(elements.activity, [...activities], "全部活动");
    }
    function fillFilters() {
        fillOptions(elements.category, [...new Map(availabilityQuests().map((quest) => [quest.category_id, quest.category_name]))], "全部类型");
        fillActivityOptions();
        fillStageOptions();
        fillDifficultyOptions();
    }
    function fillStageOptions() {
        const quests = availabilityQuests().filter(quest => {
            var _a;
            return (!elements.category.value || String(quest.category_id) === elements.category.value)
                && (!elements.activity.value || ((_a = quest.activity_id) !== null && _a !== void 0 ? _a : "permanent") === elements.activity.value);
        });
        fillOptions(elements.stage, [...new Map(quests.map(quest => [(0, activity_navigation_js_1.questStageKey)(quest), `${(0, activity_navigation_js_1.questStageName)(quest)} (${(0, activity_navigation_js_1.questStageKey)(quest)})`]))], "全部关卡");
    }
    function fillDifficultyOptions() {
        const quests = availabilityQuests().filter((quest) => {
            var _a;
            return (!elements.category.value || String(quest.category_id) === elements.category.value)
                && (!elements.activity.value || ((_a = quest.activity_id) !== null && _a !== void 0 ? _a : "permanent") === elements.activity.value)
                && (!elements.stage.value || (0, activity_navigation_js_1.questStageKey)(quest) === elements.stage.value);
        });
        const difficulties = [...new Map(quests.map((quest) => [quest.difficulty_id, quest.difficulty]))]
            .sort((left, right) => DIFFICULTY_ORDER.indexOf(left[0]) - DIFFICULTY_ORDER.indexOf(right[0]));
        fillOptions(elements.difficulty, difficulties, "全部难度");
    }
    function createQuestRow(quest) {
        var _a;
        const key = questKey(quest);
        const card = createElement("label", "rescue-quest");
        const checkbox = createElement("input");
        checkbox.type = "checkbox";
        checkbox.checked = state.selected.has(key);
        checkbox.disabled = !quest.is_open && !checkbox.checked;
        card.classList.toggle("is-selected", checkbox.checked);
        const description = createElement("span", "rescue-quest-description");
        description.append(createElement("strong", "", quest.name), createElement("span", "meta", `${quest.category_name} · ${(_a = quest.activity_name) !== null && _a !== void 0 ? _a : "领主战"}`));
        const status = createElement("span", quest.is_open ? "badge" : "badge rescue-closed", quest.is_open ? "可进入" : "暂不可进入");
        checkbox.addEventListener("change", () => {
            if (checkbox.checked)
                state.selected.set(key, { category_id: quest.category_id, quest_id: quest.quest_id });
            else
                state.selected.delete(key);
            card.classList.toggle("is-selected", checkbox.checked);
            if (elements["selected-only"].checked) {
                fillFilters();
                renderList();
            }
            else
                renderSummary();
        });
        card.append(checkbox, description, status);
        return card;
    }
    function renderList() {
        const quests = filteredQuests();
        state.page = Math.min(state.page, Math.max(0, Math.ceil(quests.length / PAGE_SIZE) - 1));
        const start = state.page * PAGE_SIZE;
        elements.list.replaceChildren(...quests.slice(start, start + PAGE_SIZE).map(createQuestRow));
        if (!quests.length) {
            const message = !elements["selected-only"].checked && elements.status.value === "open"
                && availabilityQuests().length === 0
                ? "当前没有可进入的救援关卡, 可切换全部查看目录."
                : "没有符合筛选条件的关卡.";
            elements.list.append(createElement("p", "empty-state", message));
        }
        elements["page-state"].textContent = quests.length
            ? `第 ${state.page + 1} / ${Math.ceil(quests.length / PAGE_SIZE)} 页, 共 ${quests.length} 个关卡`
            : "共 0 个关卡";
        elements.previous.disabled = state.page === 0;
        elements.next.disabled = start + PAGE_SIZE >= quests.length;
        elements["select-page"].disabled = quests.length === 0;
        elements["clear-page"].disabled = quests.length === 0;
        renderSummary();
    }
    async function load(change) {
        return withBusy(async () => {
            const preserveDraft = hasChanges();
            try {
                const [settings, catalog] = await Promise.all([
                    requestApi("/v1/rescue-settings"),
                    requestApi("/v1/rescue-settings/quests"),
                ]);
                state.quests = catalog.quests;
                if (preserveDraft) {
                    state.settings = settings;
                    if (change) {
                        const key = questKey(change.quest);
                        if (change.checked)
                            state.selected.set(key, { category_id: change.quest.category_id, quest_id: change.quest.quest_id });
                        else
                            state.selected.delete(key);
                        if (change.checked)
                            elements.enabled.checked = settings.enabled;
                    }
                }
                else
                    applySettings(settings);
                state.loaded = true;
                fillFilters();
                renderList();
            }
            catch (error) {
                elements.state.textContent = "救援设置读取失败, 点击刷新重试.";
                throw error;
            }
        });
    }
    function settingInteger(name, label, min, max) {
        const value = Number(elements[name].value);
        if (!Number.isInteger(value) || value < min || value > max) {
            throw new Error(`${label}须为 ${min} 到 ${max} 的整数.`);
        }
        return value;
    }
    async function save() {
        const body = {
            enabled: elements.enabled.checked,
            spawn_interval_seconds: settingInteger("spawn-interval", "生成间隔", 5, 3600),
            room_lifetime_seconds: settingInteger("room-lifetime", "空房保留时间", 30, 3600),
            max_rooms: settingInteger("max-rooms", "同时开放房间数", 1, 10),
            quests: [...state.selected.values()].sort((left, right) => left.category_id - right.category_id || left.quest_id - right.quest_id),
        };
        applySettings(await requestApi("/v1/rescue-settings", { method: "PUT", body }));
        renderList();
        return "救援设置已保存.";
    }
    async function disable() {
        const preserveDraft = hasChanges();
        const settings = await requestApi("/v1/rescue-settings", { method: "PUT", body: { enabled: false } });
        if (preserveDraft) {
            state.settings = settings;
            elements.enabled.checked = false;
        }
        else {
            applySettings(settings);
        }
        renderList();
        return "救援已关闭, 当前战斗可正常完成.";
    }
    function selectPage(checked) {
        for (const quest of filteredQuests().slice(state.page * PAGE_SIZE, (state.page + 1) * PAGE_SIZE)) {
            if (checked && quest.is_open)
                state.selected.set(questKey(quest), { category_id: quest.category_id, quest_id: quest.quest_id });
            else if (checked)
                continue;
            else
                state.selected.delete(questKey(quest));
        }
        if (elements["selected-only"].checked)
            fillFilters();
        renderList();
    }
    elements.form.addEventListener("submit", (event) => {
        event.preventDefault();
        runAction(null, () => withBusy(save));
    });
    elements.refresh.addEventListener("click", () => runAction(null, load));
    elements.disable.addEventListener("click", () => runAction(null, () => withBusy(disable)));
    elements["settings-controls"].addEventListener("input", renderSummary);
    elements["settings-controls"].addEventListener("change", renderSummary);
    for (const name of ["category", "activity", "stage", "difficulty", "status", "selected-only", "search"]) {
        elements[name].addEventListener(name === "search" ? "input" : "change", () => {
            state.page = 0;
            if (name === "selected-only") {
                if (elements["selected-only"].checked) {
                    state.previousStatus = elements.status.value;
                    elements.status.value = "";
                    for (const filter of ["category", "activity", "stage", "difficulty", "search"])
                        elements[filter].value = "";
                }
                else
                    elements.status.value = state.previousStatus;
                updateControls();
            }
            if (name === "status" || name === "selected-only")
                fillFilters();
            if (name === "category") {
                fillActivityOptions();
                fillStageOptions();
                fillDifficultyOptions();
            }
            if (name === "activity") {
                fillStageOptions();
                fillDifficultyOptions();
            }
            if (name === "stage")
                fillDifficultyOptions();
            renderList();
        });
    }
    elements.previous.addEventListener("click", () => { state.page--; renderList(); });
    elements.next.addEventListener("click", () => { state.page++; renderList(); });
    elements["select-page"].addEventListener("click", () => selectPage(true));
    elements["clear-page"].addEventListener("click", () => selectPage(false));
    elements["clear-all"].addEventListener("click", () => {
        state.selected.clear();
        if (elements["selected-only"].checked)
            fillFilters();
        renderList();
    });
    updateControls();
    return { load };
}
exports.createRescueController = createRescueController;
// //// /读取, 编辑并保存机器人救援设置 ////
},
"/manage/activity-rescue.js": function(module, exports, require) {
"use strict";
// audience: external
// # personal-service-activity-rescue
// 每个关卡独立保存铃铛开关, 开放状态由服务端当前时窗和存档资格计算.
Object.defineProperty(exports, "__esModule", { value: true });
exports.createActivityRescue = void 0;
const activity_navigation_js_1 = require("/manage/activity-navigation.js");
// //// 管理活动内的协战关卡与铃铛开关 [@x380kkm 2026-09-26] ////
function createActivityRescue({ panel, requestApi, runAction, onChange }) {
    const stage = panel.querySelector("#activity-rescue-stage");
    const difficulty = panel.querySelector("#activity-rescue-difficulty");
    const enabled = panel.querySelector("#activity-rescue-enabled");
    const status = panel.querySelector("#activity-rescue-state");
    let generation = 0;
    let quests = [];
    let settings = null;
    let busy = false;
    function key(quest) { return `${quest.category_id}:${quest.quest_id}`; }
    function groupKey(quest) { return (0, activity_navigation_js_1.questStageKey)(quest); }
    function options(select, entries) {
        var _a;
        const previous = select.value;
        select.replaceChildren();
        for (const [value, label] of entries) {
            const option = panel.ownerDocument.createElement("option");
            option.value = value;
            option.textContent = label;
            select.append(option);
        }
        select.value = entries.some(([value]) => value === previous) ? previous : ((_a = entries[0]) === null || _a === void 0 ? void 0 : _a[0]) || "";
    }
    function renderStatus() {
        const quest = quests.find(item => key(item) === difficulty.value);
        const selected = quest && settings.quests.some(item => key(item) === key(quest));
        enabled.checked = Boolean(selected);
        enabled.disabled = busy || !quest || (!quest.is_open && !selected);
        stage.disabled = busy;
        difficulty.disabled = busy;
        status.textContent = !quest ? "此活动没有协战关卡."
            : !quest.is_open ? "关卡关闭或存档尚未解锁, 铃铛暂停."
                : !selected ? "铃铛已关闭."
                    : settings.enabled ? "铃铛已开启, 跟随关卡开放时间生效."
                        : "此关卡已选中, 救援总开关已关闭.";
    }
    function renderDifficulties() {
        options(difficulty, quests.filter(quest => groupKey(quest) === stage.value)
            .map(quest => [key(quest), `${quest.difficulty} (${quest.is_open ? "可进入" : "关闭或未解锁"})`]));
        renderStatus();
    }
    async function selectActivity(activity) {
        const current = ++generation;
        panel.hidden = true;
        if (!activity)
            return;
        const [catalog, policy] = await Promise.all([
            requestApi("/v1/rescue-settings/quests"), requestApi("/v1/rescue-settings"),
        ]);
        if (current !== generation)
            return;
        quests = catalog.quests.filter(quest => activity.activity_id === `quest:${key(quest)}`
            || quest.activity_id === activity.activity_id);
        settings = policy;
        panel.hidden = quests.length === 0;
        options(stage, [...new Map(quests.map(quest => [groupKey(quest), (0, activity_navigation_js_1.questStageName)(quest)]))]);
        renderDifficulties();
    }
    enabled.addEventListener("change", () => runAction(null, async () => {
        const quest = quests.find(item => key(item) === difficulty.value);
        if (!quest || busy)
            return;
        const current = generation;
        const checked = enabled.checked;
        busy = true;
        renderStatus();
        try {
            const result = await requestApi(`/v1/rescue-settings/quests/${quest.category_id}/${quest.quest_id}`, {
                method: "PUT", body: { enabled: checked },
            });
            if (current === generation)
                settings = result;
            await (onChange === null || onChange === void 0 ? void 0 : onChange({ quest, checked }));
            return checked ? "此关卡铃铛已开启." : "此关卡铃铛已关闭.";
        }
        finally {
            busy = false;
            renderStatus();
        }
    }));
    stage.addEventListener("change", renderDifficulties);
    difficulty.addEventListener("change", renderStatus);
    return { selectActivity };
}
exports.createActivityRescue = createActivityRescue;
// //// /管理活动内的协战关卡与铃铛开关 ////
},
"/manage/mail-reward-controller.js": function(module, exports, require) {
"use strict";
// audience: external
// # personal-service-mail-reward-controller
//
// 此模块从个人服务读取邮件奖励目录, 并把用户选择转换为现有邮件奖励结构.
Object.defineProperty(exports, "__esModule", { value: true });
exports.createMailRewardController = exports.buildMailRewards = exports.configureRewardAmountInput = void 0;
// //// 管理邮件奖励目录, 收藏, 选择和快捷发放 [@x380kkm 2026-08-20] ////
function createElement(tagName, className, text) {
    const node = document.createElement(tagName);
    if (className)
        node.className = className;
    if (text !== undefined)
        node.textContent = text;
    return node;
}
function normalizedSearchText(value) {
    return String(value !== null && value !== void 0 ? value : "").trim().toLocaleLowerCase("zh-CN");
}
function safeImageUrl(value) {
    if (!value)
        return null;
    try {
        const url = new URL(value, location.origin);
        return url.origin === location.origin && url.pathname.startsWith("/manage/assets/")
            ? url.href
            : null;
    }
    catch (_a) {
        return null;
    }
}
const FALLBACK_REWARD_IMAGE_URL = "/manage/assets/item-placeholder.svg";
function appendOptionalImage(container, item) {
    const catalogImageUrl = safeImageUrl(item.image_url);
    const imageUrl = catalogImageUrl !== null && catalogImageUrl !== void 0 ? catalogImageUrl : FALLBACK_REWARD_IMAGE_URL;
    const frame = createElement("figure", "mail-reward-art");
    const image = createElement("img");
    image.alt = "";
    image.loading = "lazy";
    image.dataset.rewardImageSource = catalogImageUrl === null ? "fallback" : "catalog";
    image.dataset.rewardImageState = "loading";
    image.addEventListener("load", () => {
        image.dataset.rewardImageState = "loaded";
    });
    image.addEventListener("error", () => {
        if (image.getAttribute("src") === FALLBACK_REWARD_IMAGE_URL) {
            image.dataset.rewardImageState = "failed";
            image.hidden = true;
            return;
        }
        image.dataset.rewardImageSource = "fallback";
        image.dataset.rewardImageState = "loading";
        image.src = FALLBACK_REWARD_IMAGE_URL;
    });
    image.src = imageUrl;
    frame.append(image);
    container.append(frame);
}
function mergeRewardValues(target, source, multiplier) {
    var _a, _b;
    for (const [key, value] of Object.entries(source)) {
        if (typeof value === "number") {
            target[key] = ((_a = target[key]) !== null && _a !== void 0 ? _a : 0) + value * multiplier;
            continue;
        }
        if (value && typeof value === "object" && !Array.isArray(value)) {
            const nested = (_b = target[key]) !== null && _b !== void 0 ? _b : {};
            target[key] = nested;
            mergeRewardValues(nested, value, multiplier);
        }
    }
}
function requireValidRewardAmount(amount) {
    if (!Number.isInteger(amount) || amount <= 0) {
        throw new Error("数量必须是正整数.");
    }
    return amount;
}
function configureRewardAmountInput(input, itemName, initialAmount) {
    input.type = "number";
    input.min = "1";
    input.max = "999999999";
    input.step = "1";
    input.value = String(requireValidRewardAmount(initialAmount));
    input.setAttribute("aria-label", `${itemName} 数量`);
}
exports.configureRewardAmountInput = configureRewardAmountInput;
function buildMailRewards(selection) {
    const rewards = {};
    for (const { item, amount } of selection) {
        mergeRewardValues(rewards, item.rewards, requireValidRewardAmount(amount));
    }
    return rewards;
}
exports.buildMailRewards = buildMailRewards;
function hasRewards(rewards) {
    return Object.keys(rewards).length > 0;
}
function createMailRewardController({ elements, requestApi, runAction, sendMail }) {
    const state = {
        catalog: { items: [], kinds: [], presets: [] },
        selection: new Map(),
    };
    function renderKinds() {
        const selected = elements.mailRewardKind.value || "all";
        elements.mailRewardKind.replaceChildren();
        const all = createElement("option", "", "全部");
        all.value = "all";
        all.selected = selected === "all";
        elements.mailRewardKind.append(all);
        for (const kind of state.catalog.kinds) {
            const option = createElement("option", "", kind.name);
            option.value = kind.key;
            option.selected = kind.key === selected;
            elements.mailRewardKind.append(option);
        }
        if (selected !== "all" && !state.catalog.kinds.some((kind) => kind.key === selected)) {
            elements.mailRewardKind.value = "all";
        }
    }
    function renderSelection() {
        elements.mailSelectionList.replaceChildren();
        if (state.selection.size === 0) {
            elements.mailSelectionList.append(createElement("p", "empty-state", "从上方目录添加奖励."));
        }
        for (const [key, entry] of state.selection) {
            const row = createElement("article", "mail-selection-item");
            const copy = createElement("div");
            copy.append(createElement("strong", "", entry.item.name), createElement("span", "meta", entry.item.key));
            const amount = createElement("input", "mail-selection-amount");
            configureRewardAmountInput(amount, entry.item.name, entry.amount);
            amount.addEventListener("change", () => {
                var _a;
                const nextAmount = Number(amount.value);
                try {
                    requireValidRewardAmount(nextAmount);
                    entry.amount = nextAmount;
                    amount.setCustomValidity("");
                    syncPayload();
                }
                catch (error) {
                    amount.value = String(entry.amount);
                    amount.setCustomValidity((_a = error.message) !== null && _a !== void 0 ? _a : String(error));
                    amount.reportValidity();
                }
            });
            const remove = createElement("button", "button danger", "移除");
            remove.type = "button";
            remove.addEventListener("click", () => {
                state.selection.delete(key);
                renderSelection();
            });
            row.append(copy, amount, remove);
            elements.mailSelectionList.append(row);
        }
        elements.mailSelectionCount.textContent = `${state.selection.size} 项`;
        syncPayload();
    }
    function syncPayload() {
        elements.mailRewardsPayload.value = JSON.stringify(buildMailRewards(state.selection.values()));
    }
    function addSelection(item, amount) {
        var _a;
        requireValidRewardAmount(amount);
        const current = state.selection.get(item.key);
        state.selection.set(item.key, {
            item,
            amount: ((_a = current === null || current === void 0 ? void 0 : current.amount) !== null && _a !== void 0 ? _a : 0) + amount,
        });
        renderSelection();
    }
    function renderCatalog() {
        var _a, _b;
        const query = normalizedSearchText(elements.mailRewardSearch.value);
        const kind = elements.mailRewardKind.value;
        const favoritesOnly = elements.mailRewardFavorites.checked;
        const visibleItems = state.catalog.items.filter((item) => {
            var _a, _b, _c, _d, _e, _f, _g, _h, _j;
            if (kind !== "all" && item.kind !== kind)
                return false;
            if (favoritesOnly && !item.favorite)
                return false;
            const haystack = normalizedSearchText(`${item.name} ${item.key} ${(_a = item.resource_id) !== null && _a !== void 0 ? _a : ""} ${(_b = item.string_id) !== null && _b !== void 0 ? _b : ""} `
                + `${(_c = item.thumbnail_id) !== null && _c !== void 0 ? _c : ""} ${(_d = item.description) !== null && _d !== void 0 ? _d : ""} ${(_e = item.kind) !== null && _e !== void 0 ? _e : ""} `
                + `${(_f = item.kind_name) !== null && _f !== void 0 ? _f : ""} ${(_g = item.effect_kind) !== null && _g !== void 0 ? _g : ""} ${(_h = item.category) !== null && _h !== void 0 ? _h : ""} ${(_j = item.group) !== null && _j !== void 0 ? _j : ""}`);
            return !query || haystack.includes(query);
        });
        elements.mailRewardList.replaceChildren();
        if (visibleItems.length === 0) {
            elements.mailRewardList.append(createElement("p", "empty-state", "没有符合条件的奖励."));
            return;
        }
        for (const item of visibleItems) {
            const card = createElement("article", "mail-reward-card");
            appendOptionalImage(card, item);
            const content = createElement("div", "mail-reward-card-content");
            const heading = createElement("div", "mail-reward-card-heading");
            const title = createElement("div");
            title.append(createElement("h4", "card-title", item.name), createElement("span", "meta", item.resource_id
                ? `ID ${item.resource_id} / ${(_a = item.kind_name) !== null && _a !== void 0 ? _a : item.kind} / ${item.key}`
                : item.key));
            const favorite = createElement("button", `mail-favorite${item.favorite ? " active" : ""}`, item.favorite ? "★" : "☆");
            favorite.type = "button";
            favorite.setAttribute("aria-label", item.favorite ? `取消收藏 ${item.name}` : `收藏 ${item.name}`);
            favorite.addEventListener("click", () => runAction(favorite, async () => {
                const result = await requestApi(`/v1/mail-rewards/catalog/${encodeURIComponent(item.key)}/favorite`, {
                    method: "PUT",
                    body: { favorite: !item.favorite },
                });
                item.favorite = result.favorite;
                renderCatalog();
                return result.favorite ? `已收藏 ${item.name}.` : `已取消收藏 ${item.name}.`;
            }));
            heading.append(title, favorite);
            content.append(heading);
            if (item.description)
                content.append(createElement("p", "meta mail-reward-description", item.description));
            const actions = createElement("div", "mail-reward-actions");
            const amount = createElement("input");
            configureRewardAmountInput(amount, item.name, (_b = item.default_amount) !== null && _b !== void 0 ? _b : 1);
            const add = createElement("button", "button ghost", "添加");
            add.type = "button";
            add.addEventListener("click", () => addSelection(item, Number(amount.value)));
            actions.append(amount, add);
            content.append(actions);
            card.append(content);
            elements.mailRewardList.append(card);
        }
    }
    function renderPresets() {
        var _a;
        elements.mailPresetList.replaceChildren();
        if (state.catalog.presets.length === 0) {
            elements.mailPresetList.append(createElement("p", "empty-state", "当前资源版本没有可用的快捷补给."));
            return;
        }
        for (const preset of state.catalog.presets) {
            const card = createElement("article", "mail-preset-card");
            appendOptionalImage(card, preset);
            const content = createElement("div", "mail-preset-content");
            content.append(createElement("h4", "card-title", preset.name));
            content.append(createElement("p", "meta", (_a = preset.description) !== null && _a !== void 0 ? _a : ""));
            const grant = createElement("button", "button primary", "立即发放");
            grant.type = "button";
            grant.addEventListener("click", () => runAction(grant, async () => {
                if (!confirm(`向当前存档发放“${preset.name}”?`))
                    return undefined;
                await sendMail({
                    title: preset.name,
                    body: preset.description || "由当前设备的个人服务发放.",
                    sender: "Starpoint",
                    rewards: preset.rewards,
                });
                return `${preset.name} 已发送到游戏邮箱.`;
            }));
            content.append(grant);
            card.append(content);
            elements.mailPresetList.append(card);
        }
    }
    async function load() {
        elements.mailCatalogState.dataset.loadState = "loading";
        elements.mailCatalogState.dataset.itemCount = "0";
        try {
            state.catalog = await requestApi("/v1/mail-rewards/catalog");
        }
        catch (error) {
            state.catalog = { items: [], kinds: [], presets: [] };
            renderKinds();
            renderPresets();
            renderCatalog();
            elements.mailCatalogState.textContent = "奖励目录暂时不可用";
            elements.mailCatalogState.dataset.loadState = "failed";
            return;
        }
        renderKinds();
        renderPresets();
        renderCatalog();
        elements.mailCatalogState.textContent = `${state.catalog.items.length} 项奖励, ${state.catalog.presets.length} 个快捷补给`;
        elements.mailCatalogState.dataset.itemCount = String(state.catalog.items.length);
        elements.mailCatalogState.dataset.loadState = state.catalog.items.length > 0 ? "loaded" : "failed";
    }
    elements.mailRewardSearch.addEventListener("input", renderCatalog);
    elements.mailRewardKind.addEventListener("change", renderCatalog);
    elements.mailRewardFavorites.addEventListener("change", renderCatalog);
    elements.mailSelectionClear.addEventListener("click", () => {
        state.selection.clear();
        renderSelection();
    });
    renderSelection();
    return {
        clearSelection() {
            state.selection.clear();
            renderSelection();
        },
        hasSelection() {
            return hasRewards(buildMailRewards(state.selection.values()));
        },
        load,
    };
}
exports.createMailRewardController = createMailRewardController;
// //// /管理邮件奖励目录, 收藏, 选择和快捷发放 ////
},
"/manage/reward-history.js": function(module, exports, require) {
"use strict";
// audience: internal
// # personal-service-reward-history-view
// 此视图按存档分页显示到账资产, 已发放商品和邮件投递状态. 所有展示内容使用文本节点.
Object.defineProperty(exports, "__esModule", { value: true });
exports.createRewardHistory = exports.rewardAssetText = void 0;
const sourceLabels = { product: "商品领取", mail: "邮件领取", login: "登录与订阅奖励", mission: "任务与活动奖励", exchange: "商店兑换", gacha: "扭蛋奖励", reward: "奖励领取" };
const assetLabels = { 1: "道具", 3: "付费星导石", 4: "免费星导石", 5: "角色", 6: "装备", 7: "星之碎片", 8: "玛纳", 9: "经验池", 10: "羁绊之证", 11: "Boss Boost 点数", 12: "Boost 点数", 15: "段位点数" };
const deliveryLabels = { received: "已领取到账", pending: "已投递, 待领取", expired: "已过期, 未领取" };
const entitlementLabels = { growth_fund: "成长基金资格", pass_card: "通行证高阶奖励资格", subscription: "订阅资格" };
function formatTime(value) { return new Date(value * 1000).toLocaleString("zh-CN", { hour12: false }); }
function rewardAssetText(reward) {
    var _a;
    const label = (_a = assetLabels[reward.kind]) !== null && _a !== void 0 ? _a : `奖励类型 ${reward.kind}`;
    const identity = reward.type_id === null || reward.type_id === undefined ? "" : ` #${reward.type_id}`;
    return `${label}${identity} x${reward.number}`;
}
exports.rewardAssetText = rewardAssetText;
// //// 按存档和记录类型管理分页与展示 [@x380kkm 2026-09-22] ////
function createRewardHistory({ requestApi, runAction, document = globalThis.document }) {
    const controls = Object.fromEntries(["save", "view", "state", "list", "refresh", "more"].map((key) => [key, document.querySelector(`#reward-history-${key}`)]));
    let entries = [];
    let next = null;
    let generation = 0;
    function element(tag, className, text) {
        const value = document.createElement(tag);
        value.className = className;
        value.textContent = text;
        return value;
    }
    function reset() {
        generation += 1;
        entries = [];
        next = null;
        controls.list.replaceChildren();
        controls.more.hidden = true;
        controls.state.textContent = "选择存档后查看奖励记录.";
    }
    function receivedCard(entry) {
        var _a;
        const card = element("article", "item-card", "");
        card.append(element("strong", "", rewardAssetText(entry)));
        card.append(element("p", "meta", `${(_a = sourceLabels[entry.source]) !== null && _a !== void 0 ? _a : sourceLabels.reward} / 已到账 / ${formatTime(entry.created_at)}`));
        if (entry.subject)
            card.append(element("p", "", entry.subject));
        if (entry.description)
            card.append(element("p", "mail-body", entry.description));
        if (entry.reason_id)
            card.append(element("p", "meta", `来源编号 ${entry.reason_id}`));
        return card;
    }
    function deliveryCard(entry) {
        var _a;
        const card = element("article", "item-card", "");
        card.append(element("strong", "", entry.title));
        card.append(element("p", "meta", `${(_a = deliveryLabels[entry.status]) !== null && _a !== void 0 ? _a : "投递状态未知"} / 邮件 #${entry.id}`));
        card.append(element("p", "meta", `${entry.sender} / 投递 ${formatTime(entry.created_at)}`));
        card.append(element("p", "mail-body", entry.body));
        const rewards = element("ul", "", "");
        for (const reward of entry.rewards)
            rewards.append(element("li", "", rewardAssetText(reward)));
        card.append(rewards);
        if (entry.received_at !== null)
            card.append(element("p", "meta", `领取 ${formatTime(entry.received_at)}`));
        if (entry.expires_at !== null)
            card.append(element("p", "meta", `到期 ${formatTime(entry.expires_at)}`));
        return card;
    }
    function productCard(entry) {
        var _a;
        const card = element("article", "item-card", "");
        card.append(element("strong", "", entry.product_id));
        card.append(element("p", "meta", `商品内容已发放 / ${formatTime(entry.created_at)}`));
        for (const kind of entry.entitlement_kinds) {
            card.append(element("p", "", (_a = entitlementLabels[kind]) !== null && _a !== void 0 ? _a : kind));
        }
        card.append(element("p", "meta", entry.confirmed_at === null
            ? "完成界面待确认, 商品内容已发放."
            : `完成界面已确认 / ${formatTime(entry.confirmed_at)}`));
        return card;
    }
    const views = {
        received: { route: "reward-history", renderEntry: receivedCard, label: "到账", empty: "此存档尚无到账记录." },
        products: { route: "product-history", renderEntry: productCard, label: "商品领取", empty: "此存档尚无商品领取记录." },
        deliveries: { route: "reward-deliveries", renderEntry: deliveryCard, label: "投递", empty: "此存档尚无邮件投递记录.", total: (payload) => `, 共 ${payload.total} 封邮件` },
    };
    function render(view) {
        controls.list.replaceChildren();
        for (const entry of entries)
            controls.list.append(view.renderEntry(entry));
        if (!entries.length)
            controls.list.append(element("p", "empty-state", view.empty));
        controls.more.hidden = next === null;
    }
    async function load(append = false) {
        var _a, _b, _c;
        if (!controls.save.value) {
            reset();
            return;
        }
        if (!append)
            reset();
        const requestGeneration = ++generation;
        const view = (_a = views[controls.view.value]) !== null && _a !== void 0 ? _a : views.received;
        const query = append && next !== null ? `?before_id=${next}` : "";
        controls.state.textContent = "正在读取奖励记录.";
        try {
            const payload = await requestApi(`/v1/local-saves/${controls.save.value}/${view.route}${query}`);
            if (requestGeneration !== generation)
                return;
            entries = append ? [...entries, ...payload.entries] : payload.entries;
            next = payload.next_before_id;
            render(view);
            controls.state.textContent = `${payload.slot_name}, 显示 ${entries.length} 条${view.label}记录${(_c = (_b = view.total) === null || _b === void 0 ? void 0 : _b.call(view, payload)) !== null && _c !== void 0 ? _c : ""}.`;
        }
        catch (error) {
            if (requestGeneration !== generation)
                return;
            controls.state.textContent = "奖励记录读取失败, 请重试.";
            throw error;
        }
    }
    controls.save.addEventListener("change", () => runAction(controls.refresh, () => load()));
    controls.view.addEventListener("change", () => runAction(controls.refresh, () => load()));
    controls.refresh.addEventListener("click", () => runAction(controls.refresh, () => load()));
    controls.more.addEventListener("click", () => runAction(controls.more, () => load(true)));
    reset();
    return {
        setSaves(slots, activeSlotId) {
            var _a, _b;
            const selected = controls.save.value;
            controls.save.replaceChildren(element("option", "", "选择存档"));
            controls.save.options[0].value = "";
            for (const slot of slots) {
                const option = element("option", "", `${slot.name} (#${slot.id})`);
                option.value = String(slot.id);
                controls.save.append(option);
            }
            const preferred = (_b = (_a = slots.find((slot) => String(slot.id) === selected)) !== null && _a !== void 0 ? _a : slots.find((slot) => slot.id === activeSlotId)) !== null && _b !== void 0 ? _b : slots[0];
            controls.save.value = preferred ? String(preferred.id) : "";
            return runAction(controls.refresh, () => load());
        },
    };
}
exports.createRewardHistory = createRewardHistory;
// //// /按存档和记录类型管理分页与展示 ////
},
"/manage/competition-controller.js": function(module, exports, require) {
"use strict";
// audience: internal
// # personal-service-competition-controller
// 管理操作绑定选定存档, 活动期别和参与轮次. 发奖和全服统计分别提交, 切换存档后忽略旧响应.
Object.defineProperty(exports, "__esModule", { value: true });
exports.createCompetitionController = void 0;
const competition_views_js_1 = require("./competition-views.js");
// //// 管理竞赛目录, 开放状态和人工奖励结算 [@x380kkm 2026-09-28] ////
function createCompetitionController({ requestApi, runAction, onSelect, onChange, document = globalThis.document, confirm = globalThis.confirm }) {
    const keys = ["save", "family", "search", "event", "state", "detail", "title", "meta", "summary", "policy", "policySave", "policyState", "open", "close", "always", "global", "globalForm", "hp", "kills", "statsSave", "rankingOptions", "tier", "rushOptions", "rank", "preview", "settle", "refresh"];
    const controls = Object.fromEntries(keys.map(key => [key, document.querySelector(`#competition-${key.replace(/[A-Z]/g, letter => `-${letter.toLowerCase()}`)}`)]));
    let slots = [];
    let entries = [];
    let selected = null;
    let generation = 0;
    let busy = false;
    let active = false;
    function option(value, text) {
        const node = document.createElement("option");
        node.value = String(value);
        node.textContent = text;
        return node;
    }
    function updateControls() {
        for (const key of ["save", "family", "search", "event", "refresh"])
            controls[key].disabled = busy;
        for (const key of ["policy", "policySave", "open", "close", "always", "statsSave", "hp", "kills", "tier", "rank"])
            controls[key].disabled = busy || !selected;
        const eligible = selected && (selected.family === "raid" || selected.has_record);
        controls.settle.disabled = busy || !eligible || (selected.family !== "raid" && selected.received)
            || !(0, competition_views_js_1.selectedCompetitionTiers)(selected, controls.tier.value, controls.rank.value).length;
    }
    function showRewards() {
        if (selected)
            (0, competition_views_js_1.renderCompetitionRewards)(document, controls.preview, (0, competition_views_js_1.selectedCompetitionTiers)(selected, controls.tier.value, controls.rank.value));
        updateControls();
    }
    function showPolicy() {
        controls.policyState.textContent = controls.policy.value === "manual"
            ? "手动发奖: 游戏内保留成绩和已领记录, 由本页结算奖励."
            : "游戏内领取: 按活动原有领奖流程发奖, 本页也可提前人工结算.";
    }
    function selectEntry() {
        var _a, _b, _c, _d, _e, _f, _g, _h, _j;
        selected = (_a = entries.find(entry => entry.activity_id === controls.event.value)) !== null && _a !== void 0 ? _a : null;
        controls.detail.hidden = !selected;
        if (!selected) {
            onSelect === null || onSelect === void 0 ? void 0 : onSelect(null, null);
            updateControls();
            return;
        }
        controls.title.textContent = selected.name;
        controls.meta.textContent = (0, competition_views_js_1.competitionPeriod)(selected);
        controls.summary.textContent = (0, competition_views_js_1.competitionSummary)(selected);
        controls.policy.value = (_b = selected.reward_mode) !== null && _b !== void 0 ? _b : "automatic";
        showPolicy();
        controls.rankingOptions.hidden = selected.family !== "ranking";
        controls.rushOptions.hidden = selected.family !== "rush";
        controls.global.hidden = selected.family !== "raid";
        controls.tier.replaceChildren(option("", "按当前成绩计算"));
        for (const tier of (_c = selected.tiers) !== null && _c !== void 0 ? _c : [])
            controls.tier.append(option(tier.id, (_d = tier.label) !== null && _d !== void 0 ? _d : `档位 ${tier.id}`));
        controls.tier.value = "";
        controls.rank.value = String((_e = selected.suggested_rank_number) !== null && _e !== void 0 ? _e : 1);
        controls.hp.value = String((_g = (_f = selected.global_state) === null || _f === void 0 ? void 0 : _f.hp_percentage) !== null && _g !== void 0 ? _g : 100);
        controls.kills.value = String((_j = (_h = selected.global_state) === null || _h === void 0 ? void 0 : _h.total_kill_count) !== null && _j !== void 0 ? _j : 0);
        controls.settle.textContent = selected.received && selected.family !== "raid" ? "本轮已发放" : "结算并发放奖励";
        showRewards();
        onSelect === null || onSelect === void 0 ? void 0 : onSelect(selected, slots.find(slot => String(slot.id) === controls.save.value));
    }
    function filterEntries() {
        var _a, _b, _c;
        const previous = controls.event.value;
        const query = controls.search.value.trim().toLocaleLowerCase();
        const visible = entries.filter(entry => (!controls.family.value || entry.family === controls.family.value)
            && `${entry.name} ${entry.activity_id}`.toLocaleLowerCase().includes(query));
        controls.event.replaceChildren(option("", visible.length ? "选择活动" : "没有匹配的竞赛活动"));
        for (const entry of visible)
            controls.event.append(option(entry.activity_id, `${(_a = competition_views_js_1.competitionFamilies[entry.family]) !== null && _a !== void 0 ? _a : entry.family} / ${entry.name}`));
        controls.event.value = visible.some(entry => entry.activity_id === previous) ? previous : (_c = (_b = visible[0]) === null || _b === void 0 ? void 0 : _b.activity_id) !== null && _c !== void 0 ? _c : "";
        selectEntry();
    }
    async function load() {
        const requestGeneration = ++generation;
        const slotId = controls.save.value;
        entries = [];
        selected = null;
        controls.detail.hidden = true;
        updateControls();
        onSelect === null || onSelect === void 0 ? void 0 : onSelect(null, null);
        if (!slotId) {
            entries = [];
            filterEntries();
            controls.state.textContent = "导入或创建存档后可管理竞赛奖励.";
            return;
        }
        controls.state.textContent = "正在读取竞赛成绩和奖励.";
        try {
            const payload = await requestApi(`/v1/local-saves/${slotId}/competitions`);
            if (requestGeneration !== generation || slotId !== controls.save.value)
                return;
            entries = payload.entries;
            filterEntries();
            controls.state.textContent = `${payload.slot_name}, 共 ${entries.length} 个竞赛活动. 发奖仅针对所选存档.`;
        }
        catch (error) {
            if (requestGeneration !== generation)
                return;
            entries = [];
            filterEntries();
            controls.state.textContent = "竞赛信息读取失败, 请刷新后操作.";
            throw error;
        }
    }
    function context() {
        if (!selected || !controls.save.value)
            throw new Error("请先选择存档和竞赛活动.");
        return { entry: selected, slot: slots.find(slot => String(slot.id) === controls.save.value), path: `/v1/local-saves/${controls.save.value}/competitions/${encodeURIComponent(selected.activity_id)}` };
    }
    async function act(control, action) {
        if (busy || !selected)
            return;
        const current = context();
        busy = true;
        updateControls();
        try {
            await runAction(control, async () => {
                const message = await action(current);
                if (message) {
                    await load();
                    await (onChange === null || onChange === void 0 ? void 0 : onChange());
                    return message;
                }
            });
        }
        finally {
            busy = false;
            updateControls();
        }
    }
    function scope(entry) {
        var _a;
        return { expected_round: entry.participation_round, expected_period_start_ms: (_a = entry.period_start_ms) !== null && _a !== void 0 ? _a : null };
    }
    async function settle({ entry, slot, path }) {
        var _a;
        const body = scope(entry);
        if (entry.family === "ranking" && controls.tier.value !== "")
            body.tier_id = Number(controls.tier.value);
        if (entry.family === "rush") {
            const rank = Number(controls.rank.value);
            if (!Number.isSafeInteger(rank) || rank < 1)
                throw new Error("排名必须是正整数.");
            body.rank_number = rank;
        }
        const tiers = (0, competition_views_js_1.selectedCompetitionTiers)(entry, controls.tier.value, controls.rank.value);
        if (!tiers.length || (entry.family !== "raid" && !entry.has_record))
            return;
        const rewards = tiers.map(tier => { var _a; return (_a = tier.label) !== null && _a !== void 0 ? _a : `档位 ${tier.id}`; }).join(", ");
        if (!confirm(`向存档 "${slot.name}" 发放 "${entry.name}" 第 ${entry.participation_round + 1} 轮奖励?\n${rewards}\n奖励和统计结果会记录, 同一轮已发奖励保持原样.`))
            return;
        const result = await requestApi(`${path}/settle`, { method: "POST", body });
        return (_a = { delivered: "竞赛奖励已发放, 可在记录和游戏邮箱查看.", already_received: "此轮奖励已经发放, 已保留原领取记录.", no_reward: "当前成绩或全服统计没有满足新的奖励档位." }[result.status]) !== null && _a !== void 0 ? _a : "竞赛结算已完成.";
    }
    controls.event.addEventListener("change", selectEntry);
    controls.family.addEventListener("change", filterEntries);
    controls.search.addEventListener("input", filterEntries);
    controls.save.addEventListener("change", () => runAction(controls.refresh, load));
    controls.refresh.addEventListener("click", () => runAction(controls.refresh, load));
    controls.tier.addEventListener("change", showRewards);
    controls.rank.addEventListener("input", showRewards);
    controls.policy.addEventListener("change", showPolicy);
    controls.settle.addEventListener("click", () => act(controls.settle, settle));
    controls.policySave.addEventListener("click", () => act(controls.policySave, async ({ path }) => {
        await requestApi(`${path}/reward-mode`, { method: "PUT", body: { mode: controls.policy.value } });
        return "发奖方式已保存.";
    }));
    for (const [key, endpoint, method, body, message] of [
        ["open", "temporary-open", "POST", {}, "活动已开放 24 小时."],
        ["close", "close", "POST", {}, "活动已关闭, 已有成绩和奖励记录保留."],
        ["always", "mode", "PUT", { mode: "always" }, "活动已设为始终开放."],
    ])
        controls[key].addEventListener("click", () => act(controls[key], async ({ entry }) => {
            await requestApi(`/v1/activities/${encodeURIComponent(entry.activity_id)}/${endpoint}`, { method, body });
            return message;
        }));
    controls.globalForm.addEventListener("submit", event => {
        event.preventDefault();
        return act(controls.statsSave, async ({ entry, path }) => {
            var _a;
            const hp = Number(controls.hp.value);
            const kills = Number(controls.kills.value);
            if (!Number.isInteger(hp) || hp < 0 || hp > 100 || !Number.isSafeInteger(kills) || kills < 0)
                throw new Error("请填写 0 到 100 的血量百分比及非负整数讨伐数.");
            if (!confirm(`保存 "${entry.name}" 当前期别的全服数据?\n总讨伐 ${kills} 次, 剩余血量 ${hp}%.\n本机所有存档共享这些统计值. 各存档按其发奖方式领取奖励.`))
                return;
            await requestApi(`${path}/server-state`, { method: "PUT", body: { expected_period_start_ms: (_a = entry.period_start_ms) !== null && _a !== void 0 ? _a : null, hp_percentage: hp, total_kill_count: kills } });
            return "全服统计已保存.";
        });
    });
    controls.detail.hidden = true;
    updateControls();
    return {
        refresh: load,
        activate() { active = true; return runAction(controls.refresh, load); },
        setSaves(values, activeSlotId) {
            var _a, _b, _c, _d;
            const previous = controls.save.value;
            slots = values;
            controls.save.replaceChildren(option("", "选择存档"));
            for (const slot of slots)
                controls.save.append(option(slot.id, `${slot.name} (#${slot.id})`));
            controls.save.value = slots.some(slot => String(slot.id) === previous) ? previous : String((_d = (_b = (_a = slots.find(slot => slot.id === activeSlotId)) === null || _a === void 0 ? void 0 : _a.id) !== null && _b !== void 0 ? _b : (_c = slots[0]) === null || _c === void 0 ? void 0 : _c.id) !== null && _d !== void 0 ? _d : "");
            generation += 1;
            entries = [];
            selected = null;
            controls.detail.hidden = true;
            updateControls();
            return active ? runAction(controls.refresh, load) : undefined;
        },
    };
}
exports.createCompetitionController = createCompetitionController;
},
"/manage/competition-views.js": function(module, exports, require) {
"use strict";
// audience: internal
// # personal-service-competition-views
// 竞赛页面使用真实成绩, 奖励目录和期别统计, 文本通过 DOM 节点呈现.
Object.defineProperty(exports, "__esModule", { value: true });
exports.renderCompetitionRewards = exports.selectedCompetitionTiers = exports.competitionPeriod = exports.competitionSummary = exports.competitionFamilies = void 0;
exports.competitionFamilies = { ranking: "竞速挑战", rush: "狂热激战", raid: "全服讨伐" };
const availabilityLabels = { open: "开放中", ended: "已结束", disabled: "已关闭", not_started: "未开始", closed: "已关闭" };
// //// 显示当前参赛记录与活动范围 [@x380kkm 2026-09-28] ////
function competitionSummary(entry) {
    var _a, _b, _c, _d, _e, _f, _g, _h, _j, _k;
    const summary = (_a = entry.summary) !== null && _a !== void 0 ? _a : {};
    if (entry.family === "ranking") {
        const record = summary.best_record;
        if (!record)
            return "尚未参赛. 完成挑战后可按成绩或所选档位发奖.";
        return `${record.is_accomplished ? `最佳 ${(Number(record.elapsed_time_ms) / 1000).toFixed(3)} 秒` : "尚未通关"}, 得分 ${(_b = record.score) !== null && _b !== void 0 ? _b : 0}, 排名 ${Number((_c = summary.rank_percentage) !== null && _c !== void 0 ? _c : 100).toFixed(2)}%.`;
    }
    if (entry.family === "rush") {
        return entry.has_record
            ? `最佳第 ${(_d = summary.best_round) !== null && _d !== void 0 ? _d : 0} 轮, ${(Number((_e = summary.elapsed_time_ms) !== null && _e !== void 0 ? _e : 0) / 1000).toFixed(3)} 秒, 当前第 ${(_g = (_f = summary.rank_number) !== null && _f !== void 0 ? _f : entry.suggested_rank_number) !== null && _g !== void 0 ? _g : 1} 名.`
            : "尚无无限战成绩. 完成挑战后可结算排名奖励.";
    }
    return `全服累计讨伐 ${(_j = (_h = entry.global_state) === null || _h === void 0 ? void 0 : _h.total_kill_count) !== null && _j !== void 0 ? _j : 0} 次, 此存档已发放至 ${(_k = summary.received_up_to) !== null && _k !== void 0 ? _k : 0} 次.`;
}
exports.competitionSummary = competitionSummary;
function competitionPeriod(entry) {
    var _a, _b, _c;
    const availability = (_a = entry.availability) !== null && _a !== void 0 ? _a : {};
    const status = (_b = availabilityLabels[availability.status]) !== null && _b !== void 0 ? _b : "按活动时间开放";
    const start = entry.period_start_ms;
    const period = start === null || start === undefined ? "原始期别" : new Date(start).toLocaleString("zh-CN", { hour12: false });
    return `${(_c = exports.competitionFamilies[entry.family]) !== null && _c !== void 0 ? _c : entry.family} / ${status} / 第 ${entry.participation_round + 1} 轮 / ${period}`;
}
exports.competitionPeriod = competitionPeriod;
// //// 从当前输入选择目录中的奖励档位 [@x380kkm 2026-09-28] ////
function selectedCompetitionTiers(entry, tierId, rankNumber) {
    var _a;
    const tiers = (_a = entry.tiers) !== null && _a !== void 0 ? _a : [];
    if (entry.family === "ranking") {
        const selected = tierId === "" ? entry.suggested_tier_id : Number(tierId);
        return tiers.filter(tier => tier.id === selected);
    }
    if (entry.family === "rush") {
        const rank = Number(rankNumber);
        return tiers.filter(tier => Number.isSafeInteger(rank) && rank >= tier.rank_from && rank <= tier.rank_to);
    }
    return tiers.filter(tier => { var _a; return Number((_a = tier.pending_count) !== null && _a !== void 0 ? _a : 0) > 0; });
}
exports.selectedCompetitionTiers = selectedCompetitionTiers;
// //// 展示奖励预览并保留每个阈值的实际金额 [@x380kkm 2026-09-28] ////
function renderCompetitionRewards(document, container, tiers) {
    var _a, _b, _c;
    container.replaceChildren();
    for (const tier of tiers) {
        const card = document.createElement("article");
        card.className = "item-card";
        const title = document.createElement("strong");
        const count = (_a = tier.pending_count) !== null && _a !== void 0 ? _a : 1;
        title.textContent = `${(_b = tier.label) !== null && _b !== void 0 ? _b : `奖励档位 ${tier.id}`}${count > 1 ? ` / ${count} 次奖励` : ""}`;
        const rewards = document.createElement("p");
        rewards.className = "meta";
        rewards.textContent = ((_c = tier.rewards) !== null && _c !== void 0 ? _c : []).map(reward => { var _a, _b; return `${(_a = reward.label) !== null && _a !== void 0 ? _a : `奖励 ${(_b = reward.id) !== null && _b !== void 0 ? _b : reward.kind}`} x${reward.amount * count}`; }).join(", ") || "此档位没有额外奖励.";
        card.append(title, rewards);
        container.append(card);
    }
    if (!tiers.length) {
        const empty = document.createElement("p");
        empty.className = "empty-state";
        empty.textContent = "当前没有满足条件的新奖励.";
        container.append(empty);
    }
}
exports.renderCompetitionRewards = renderCompetitionRewards;
},
"/manage/views.js": function(module, exports, require) {
"use strict";
// audience: external
// # personal-service-management-views
//
// 此模块把个人服务状态转换为安全 DOM 节点. 所有远端文本只写入 textContent.
Object.defineProperty(exports, "__esModule", { value: true });
exports.renderDevicePicker = exports.selectedDeviceSlotId = exports.renderRemotePanels = exports.renderManagement = void 0;
const save_deletion_js_1 = require("./save-deletion.js");
const password_backup_js_1 = require("./password-backup.js");
const save_access_js_1 = require("./save-access.js");
const save_revisions_js_1 = require("./save-revisions.js");
const save_transfers_js_1 = require("./save-transfers.js");
// //// 呈现服务器, 存档和同步操作 [@x380kkm 2026-07-24] ////
function createElement(tagName, className, text) {
    const node = document.createElement(tagName);
    if (className)
        node.className = className;
    if (text !== undefined)
        node.textContent = text;
    return node;
}
function createButton(label, action, runAction, style = "ghost") {
    const control = createElement("button", `button ${style}`, label);
    control.type = "button";
    control.addEventListener("click", () => runAction(control, action));
    return control;
}
function renderManagement(model, elements, actions) {
    var _a;
    renderProfiles(model, elements, actions);
    renderSyncTargets(model, elements, actions);
    renderRemotePanels(model, elements);
    renderDevicePicker(model, elements);
    renderSaves(model, elements, actions);
    renderHttpObservations(model, elements);
    renderClientReports((_a = model.clientReports) !== null && _a !== void 0 ? _a : []);
}
exports.renderManagement = renderManagement;
// //// 展示客户端故障信息和服务端请求异常 [@x380kkm 2026-09-27] ////
function renderClientReports(reports) {
    var _a;
    const list = document.querySelector("#client-report-list");
    if (!list)
        return;
    list.replaceChildren();
    if (reports.length === 0) {
        list.append(createElement("p", "empty-state", "尚未记录错误报告."));
        return;
    }
    for (const report of reports) {
        const details = (_a = report.details) !== null && _a !== void 0 ? _a : {};
        const card = createElement("details", "item-card client-report");
        const summary = createElement("summary");
        const title = details.source === "server"
            ? `服务端 ${details.code || "异常"} ${details.method || ""} ${details.path || ""}`
            : details.code || details.error || "客户端错误";
        summary.append(createElement("strong", "client-report-title", String(title).slice(0, 180)));
        summary.append(createElement("span", "meta", formatTime(report.received_at)));
        const body = createElement("pre", "client-report-body", JSON.stringify(details, null, 2));
        card.append(summary, body);
        list.append(card);
    }
}
function httpObservationTime(observation) {
    const time = Date.parse(observation.last_seen);
    return Number.isFinite(time) ? time : Number.NEGATIVE_INFINITY;
}
function compareHttpObservationRecency(left, right) {
    return httpObservationTime(right) - httpObservationTime(left);
}
function groupHttpObservations(observations) {
    var _a, _b, _c;
    const groups = new Map();
    for (const observation of observations) {
        const method = String((_a = observation.method) !== null && _a !== void 0 ? _a : "");
        const path = String((_b = observation.path) !== null && _b !== void 0 ? _b : "");
        const key = JSON.stringify([method, path]);
        const group = (_c = groups.get(key)) !== null && _c !== void 0 ? _c : { method, path, history: [] };
        group.history.push(Object.assign(Object.assign({}, observation), { method, path }));
        groups.set(key, group);
    }
    return [...groups.values()]
        .map((group) => {
        group.history.sort(compareHttpObservationRecency);
        const count = group.history.reduce((total, observation) => {
            const value = Number(observation.count);
            return total + (Number.isFinite(value) && value > 0 ? value : 0);
        }, 0);
        return Object.assign(Object.assign({}, group), { count, latest: group.history[0] });
    })
        .sort((left, right) => compareHttpObservationRecency(left.latest, right.latest));
}
function httpObservationStatusClass(status) {
    const code = Number.parseInt(String(status), 10);
    return Number.isInteger(code) && code >= 400
        ? "badge http-observation-status error"
        : "badge http-observation-status";
}
function renderHttpObservations(model, elements) {
    elements.httpObservationList.replaceChildren();
    if (model.httpObservations.length === 0) {
        elements.httpObservationList.append(createElement("p", "empty-state", "尚未记录到请求."));
        return;
    }
    for (const group of groupHttpObservations(model.httpObservations)) {
        const current = group.latest;
        const card = createElement("article", "item-card http-observation-card");
        card.dataset.method = group.method;
        card.dataset.path = group.path;
        card.dataset.currentStatus = String(current.status);
        const head = createElement("div", "card-head");
        const title = createElement("div");
        title.append(createElement("h3", "card-title http-observation-path", group.path));
        title.append(createElement("p", "meta", `${group.method} · 累计 ${group.count} 次 · 当前状态最近 ${formatTime(current.last_seen)}`));
        head.append(title, createElement("span", httpObservationStatusClass(current.status), `当前 ${current.status}`));
        card.append(head);
        const history = createElement("details", "http-observation-history");
        history.append(createElement("summary", "", `查看历史状态 (${group.history.length})`));
        const historyList = createElement("div", "http-observation-history-list");
        for (const observation of group.history) {
            const row = createElement("div", "http-observation-history-row");
            row.dataset.status = String(observation.status);
            row.append(createElement("span", httpObservationStatusClass(observation.status), String(observation.status)), createElement("span", "meta", `${observation.count} 次 · 首次 ${formatTime(observation.first_seen)} · 最近 ${formatTime(observation.last_seen)}`));
            historyList.append(row);
        }
        history.append(historyList);
        card.append(history);
        elements.httpObservationList.append(card);
    }
}
function renderRemotePanels(model, elements) {
    const hasRemoteProfile = model.profiles.profiles.some((profile) => profile.mode === "remote");
    const hasRemoteStorage = model.targets.length > 0;
    elements.remoteSavePanel.hidden = false;
    elements.remoteSavePanel.parentElement.classList.toggle("single-column", false);
    elements.remotePlayerAccessPanel.hidden = !hasRemoteProfile;
    elements.remoteRestorePanel.hidden = !hasRemoteStorage;
}
exports.renderRemotePanels = renderRemotePanels;
function renderProfiles(model, elements, actions) {
    elements.profileList.replaceChildren();
    for (const profile of model.profiles.profiles) {
        const active = profile.id === model.profiles.active_profile_id;
        const profileName = profile.is_builtin ? "当前设备" : profile.name;
        const card = createElement("article", `item-card${active ? " active" : ""}`);
        const head = createElement("div", "card-head");
        const title = createElement("div");
        title.append(createElement("h3", "card-title", profileName));
        const endpoint = profile.mode === "local"
            ? "当前设备内置服务"
            : `${profile.scheme}://${profile.host}:${profile.port}`;
        title.append(createElement("p", "meta", endpoint));
        head.append(title);
        if (active)
            head.append(createElement("span", "badge", "正在使用"));
        card.append(head);
        const controls = createElement("div", "card-actions");
        if (profile.mode === "remote") {
            controls.append(createButton("测试连接", async () => {
                const probe = await actions.requestApi(`/v1/server-profiles/${profile.id}/probe`, { method: "POST" });
                const session = probe.session_port ? `, 联机端口 ${probe.session_port}` : "";
                if (!probe.reachable)
                    throw new Error("无法连接游戏服务器, 当前服务器保持不变.");
                if (!probe.compatible)
                    throw new Error("服务器可以连接, 但不是兼容的 Starpoint 服务.");
                return `服务器连接正常, 延迟 ${probe.latency_ms} ms${session}.`;
            }, actions.runAction));
        }
        if (!active) {
            controls.append(createButton("检测并切换", async () => {
                await actions.requestApi(`/v1/server-profiles/${profile.id}/activate-verified`, { method: "POST" });
                await actions.refreshManagementState();
                return `已切换到 ${profileName}.`;
            }, actions.runAction));
        }
        if (!profile.is_builtin) {
            controls.append(createButton("删除", async () => {
                if (!confirm(`删除服务器 ${profile.name}?`))
                    return undefined;
                await actions.requestApi(`/v1/server-profiles/${profile.id}`, { method: "DELETE" });
                await actions.refreshManagementState();
                return "服务器配置已删除.";
            }, actions.runAction, "danger"));
        }
        card.append(controls);
        elements.profileList.append(card);
    }
}
function renderSyncTargets(model, elements, actions) {
    elements.syncTargetList.replaceChildren();
    if (model.targets.length === 0) {
        elements.syncTargetList.append(createElement("p", "empty-state", "尚未配置存档服务器."));
    }
    for (const target of model.targets) {
        const card = createElement("article", "item-card");
        const head = createElement("div", "card-head");
        const title = createElement("div");
        title.append(createElement("h3", "card-title", target.name));
        title.append(createElement("p", "meta", `${target.scheme}://${target.host}:${target.port}`));
        title.append(createElement("p", "meta", `用户 ${target.username}`));
        head.append(title, createElement("span", "badge", target.has_credentials ? "凭据已保存" : "缺少凭据"));
        card.append(head);
        const controls = createElement("div", "card-actions");
        controls.append(createButton("删除", async () => {
            if (!confirm(`删除存档服务器 ${target.name}? 本地存档不会删除.`))
                return undefined;
            await actions.requestApi(`/v1/save-sync-targets/${target.id}`, { method: "DELETE" });
            await actions.refreshManagementState();
            return "存档服务器配置已删除.";
        }, actions.runAction, "danger"));
        card.append(controls);
        elements.syncTargetList.append(card);
    }
    for (const select of document.querySelectorAll(".sync-target-select")) {
        fillTargetSelect(select, model.targets);
    }
}
function selectedDeviceSlotId(saves, selectedDevice) {
    var _a, _b, _c;
    if (selectedDevice === "local" && saves.local_slot_id !== null && saves.local_slot_id !== undefined)
        return saves.local_slot_id;
    return (_b = (_a = saves.devices.find((device) => String(device.device_id) === selectedDevice)) === null || _a === void 0 ? void 0 : _a.active_slot_id) !== null && _b !== void 0 ? _b : (_c = saves.devices[0]) === null || _c === void 0 ? void 0 : _c.active_slot_id;
}
exports.selectedDeviceSlotId = selectedDeviceSlotId;
function renderDevicePicker(model, elements) {
    const selected = elements.deviceSelect.value || "local";
    elements.deviceSelect.replaceChildren();
    const local = createElement("option", "", "本机");
    local.value = "local";
    local.selected = selected === "local" || !model.saves.devices.some((device) => String(device.device_id) === selected);
    elements.deviceSelect.append(local);
    for (const device of model.saves.devices) {
        const option = createElement("option", "", `设备 ${device.device_id}`);
        option.value = String(device.device_id);
        option.selected = String(device.device_id) === selected;
        elements.deviceSelect.append(option);
    }
}
exports.renderDevicePicker = renderDevicePicker;
function renderSaves(model, elements, actions) {
    elements.saveList.replaceChildren();
    if (model.saves.slots.length === 0) {
        elements.saveList.append(createElement("p", "empty-state", "可直接导入已有存档, 再选择使用此存档并进入游戏."));
        return;
    }
    for (const slot of model.saves.slots) {
        const activeDevices = model.saves.devices
            .filter((device) => device.active_slot_id === slot.id)
            .map((device) => device.device_id);
        const isLocal = model.saves.local_slot_id === slot.id;
        const card = createElement("article", `save-card${activeDevices.length || isLocal ? " active" : ""}`);
        const head = createElement("div", "card-head");
        const title = createElement("div");
        title.append(createElement("h3", "card-title", slot.name));
        title.append(createElement("p", "meta", `槽位 ${slot.id}, 快照 ${slot.snapshot_count}`));
        title.append(createElement("p", "meta", `最近保存 ${formatTime(slot.updated_at)}`));
        if (activeDevices.length)
            title.append(createElement("p", "meta", `当前设备 ${activeDevices.join(", ")}`));
        head.append(title);
        if (activeDevices.length || isLocal)
            head.append(createElement("span", "badge", isLocal ? "本机使用中" : "使用中"));
        card.append(head);
        const controls = createElement("div", "save-actions");
        appendSaveActions(controls, slot, model, elements, actions);
        const history = createElement("div");
        controls.append(createButton("查看快照", async () => {
            await renderSnapshots(slot, history, actions);
            return undefined;
        }, actions.runAction));
        const revisions = createElement("div");
        controls.append(createButton("查看版本历史", async () => {
            await (0, save_revisions_js_1.renderSaveRevisions)(slot, revisions, actions);
            return undefined;
        }, actions.runAction));
        card.append(controls, history, revisions);
        const details = createElement("details", "form-drawer save-options");
        details.append(createElement("summary", "", "自动备份与同步"));
        renderBindings(slot, model, details);
        (0, save_transfers_js_1.renderTransferBindings)(slot, model, details, actions);
        (0, save_transfers_js_1.renderTransferBindingCreate)(slot, model, details, actions);
        (0, save_access_js_1.renderSlotAccess)(slot, details, actions);
        renderAutomation(slot, model, details, actions);
        details.append(createButton("导出旧加密备份的恢复包", async () => {
            const password = await (0, password_backup_js_1.requestBackupPassword)(details);
            if (password === undefined)
                return undefined;
            const response = await actions.requestApi(`/v1/local-saves/${slot.id}/recovery-export`, { method: "POST", body: { password } });
            const result = await shareOrDownloadJson("starpoint-recovery.starpoint-recovery", response.package);
            return documentExportFeedback(result);
        }, actions.runAction));
        card.append(details);
        appendSaveDeletion(card, slot, model, actions);
        elements.saveList.append(card);
    }
}
// //// 展示存档删除入口和设备占用提示 [@x380kkm 2026-09-20] ////
function appendSaveDeletion(card, slot, model, actions) {
    const active = model.saves.local_slot_id === slot.id || model.saves.devices.some((device) => device.active_slot_id === slot.id);
    const lastSlot = model.saves.slots.length === 1;
    const reason = lastSlot
        ? "保留至少一个存档. 导入另一份存档并启用后可删除此存档."
        : active ? "此存档正在使用. 请先为使用它的设备切换存档." : "删除前可先导出存档文件.";
    const row = createElement("div", "save-delete-row");
    const hint = createElement("p", "meta", reason);
    hint.id = `save-delete-hint-${slot.id}`;
    const control = createButton("删除存档", async () => {
        if (!await (0, save_deletion_js_1.confirmSaveDeletion)(slot, control))
            return undefined;
        await actions.requestApi(`/v1/local-saves/${slot.id}`, {
            method: "DELETE",
            body: { confirm_name: slot.name },
        });
        await actions.refreshManagementState();
        return `存档 "${slot.name}" 已删除. 已导出的文件和远程副本仍然保留.`;
    }, actions.runAction, "danger");
    control.disabled = active || lastSlot;
    control.setAttribute("aria-describedby", hint.id);
    control.setAttribute("aria-label", `删除存档 ${slot.name}`);
    row.append(hint, control);
    card.append(row);
}
function renderAutomation(slot, model, card, actions) {
    var _a;
    const automation = model.automations.get(slot.id);
    if (!automation)
        return;
    const section = createElement("section", "automation-card");
    const heading = createElement("div", "card-head");
    const title = createElement("div");
    title.append(createElement("h4", "card-title", "自动快照"));
    const intervalMinutes = Math.max(1, Math.round(automation.interval_seconds / 60));
    const status = automation.enabled
        ? `每 ${intervalMinutes} 分钟在前台运行或恢复时检查, 保留最近 48 个自动快照`
        : "当前未启用";
    title.append(createElement("p", "meta", status));
    heading.append(title, createElement("span", "badge", automation.enabled ? "已开启" : "已关闭"));
    section.append(heading);
    const times = createElement("div", "automation-status");
    if (automation.last_snapshot_at) {
        times.append(createElement("p", "meta", `最近快照 ${formatTime(automation.last_snapshot_at)}`));
    }
    if (automation.last_upload_at) {
        times.append(createElement("p", "meta", `最近上传 ${formatTime(automation.last_upload_at)}`));
    }
    if (automation.pending_upload) {
        times.append(createElement("p", "meta", "密文上传正在等待或进行中."));
    }
    if (automation.last_error) {
        times.append(createElement("p", "automation-error", actions.describeError(automation.last_error)));
    }
    section.append(times);
    const details = createElement("details", "form-drawer automation-drawer");
    const summary = createElement("summary", "", "设置自动快照");
    const form = createElement("form", "stack-form");
    const enabledLabel = createElement("label", "check-row");
    const enabled = createElement("input");
    enabled.type = "checkbox";
    enabled.name = "enabled";
    enabled.checked = automation.enabled;
    enabledLabel.append(enabled, createElement("span", "", "启用自动快照"));
    form.append(enabledLabel);
    const row = createElement("div", "form-row");
    const intervalLabel = createElement("label", "grow");
    intervalLabel.append(createElement("span", "", "快照间隔, 分钟"));
    const interval = createElement("input");
    interval.type = "number";
    interval.name = "interval_minutes";
    interval.min = "1";
    interval.max = "43200";
    interval.required = true;
    interval.value = String(intervalMinutes);
    intervalLabel.append(interval);
    const targetLabel = createElement("label", "grow");
    targetLabel.append(createElement("span", "", "自动上传, 可选"));
    const target = createElement("select");
    target.name = "target_id";
    fillOptionalTargetSelect(target, model.targets, automation.target_id);
    targetLabel.append(target);
    row.append(intervalLabel, targetLabel);
    form.append(row);
    const objectLabel = createElement("label");
    objectLabel.append(createElement("span", "", "远端对象 ID"));
    const objectId = createElement("input");
    objectId.name = "object_id";
    objectId.maxLength = 64;
    objectId.pattern = "[A-Za-z0-9_-]+";
    objectId.value = (_a = automation.object_id) !== null && _a !== void 0 ? _a : `slot-${slot.id}`;
    objectLabel.append(objectId);
    form.append(objectLabel);
    const submit = createElement("button", "button primary", "保存自动快照设置");
    submit.type = "submit";
    form.append(submit);
    const updateUploadFields = () => {
        objectId.disabled = target.value === "";
        objectId.required = target.value !== "";
    };
    target.addEventListener("change", updateUploadFields);
    updateUploadFields();
    form.addEventListener("submit", (event) => {
        event.preventDefault();
        const selectedTarget = target.value === "" ? null : Number(target.value);
        actions.runAction(submit, async () => {
            await actions.requestApi(`/v1/local-saves/${slot.id}/automation`, {
                method: "PUT",
                body: {
                    enabled: enabled.checked,
                    interval_seconds: Number(interval.value) * 60,
                    target_id: selectedTarget,
                    object_id: selectedTarget === null ? null : objectId.value.trim(),
                },
            });
            await actions.refreshManagementState();
            return "自动快照设置已保存.";
        });
    });
    details.append(summary, form);
    section.append(details);
    card.append(section);
}
function appendSaveActions(controls, slot, model, elements, actions) {
    controls.append(createButton("使用此存档", async () => {
        if (elements.deviceSelect.value === "local") {
            await actions.requestApi(`/v1/local-saves/${slot.id}/activate-local`, { method: "POST" });
            await actions.refreshManagementState();
            return "本机已选择此存档, 重新进入游戏后生效.";
        }
        const deviceId = Number(elements.deviceSelect.value);
        if (!Number.isInteger(deviceId) || deviceId <= 0)
            throw new Error("没有可操作的设备.");
        await actions.requestApi(`/v1/local-saves/${slot.id}/activate`, {
            method: "POST",
            body: { device_id: deviceId },
        });
        await actions.refreshManagementState();
        return `设备 ${deviceId} 已切换存档.`;
    }, actions.runAction, "primary"));
    controls.append(createButton("复制", async () => {
        var _a;
        const name = (_a = prompt("新存档名称", `${slot.name} 副本`)) === null || _a === void 0 ? void 0 : _a.trim();
        if (!name)
            return undefined;
        await actions.requestApi(`/v1/local-saves/${slot.id}/copy`, { method: "POST", body: { name } });
        await actions.refreshManagementState();
        return "存档副本已创建.";
    }, actions.runAction));
    controls.append(createButton("创建快照", async () => {
        var _a;
        const label = (_a = prompt("快照标签", "手动快照")) === null || _a === void 0 ? void 0 : _a.trim();
        if (!label)
            return undefined;
        await actions.requestApi(`/v1/local-saves/${slot.id}/snapshots`, { method: "POST", body: { label } });
        await actions.refreshManagementState();
        return "快照已创建.";
    }, actions.runAction));
    controls.append(createButton("导出可编辑 JSON", async () => {
        const result = await shareOrDownloadJson(`starpoint-save-${slot.id}.json`, await actions.requestApi(`/v1/local-saves/${slot.id}/export`));
        return documentExportFeedback(result);
    }, actions.runAction));
    controls.append(createButton("导出加密备份", async () => {
        const password = await (0, password_backup_js_1.requestBackupPassword)(controls);
        if (password === undefined)
            return undefined;
        const payload = await actions.requestApi(`/v1/local-saves/${slot.id}/password-export`, { method: "POST", body: { password } });
        const result = await shareOrDownloadJson(`starpoint-save-${slot.id}.starpoint-save`, payload);
        return documentExportFeedback(result);
    }, actions.runAction));
    if (model.targets.length === 0)
        return;
    const targetSelect = createElement("select", "slot-target-select");
    targetSelect.setAttribute("aria-label", `${slot.name} 上传目标`);
    fillTargetSelect(targetSelect, model.targets);
    controls.append(targetSelect);
    controls.append(createButton("上传备份", async () => {
        var _a;
        const objectId = (_a = prompt("远端对象 ID", `slot-${slot.id}`)) === null || _a === void 0 ? void 0 : _a.trim();
        if (!objectId)
            return undefined;
        await actions.requestApi(`/v1/local-saves/${slot.id}/sync/upload`, {
            method: "POST",
            body: { target_id: Number(targetSelect.value), object_id: objectId },
        });
        await actions.refreshManagementState();
        return "存档备份已上传.";
    }, actions.runAction));
}
function renderBindings(slot, model, card) {
    var _a, _b;
    const bindings = (_a = model.bindings.get(slot.id)) !== null && _a !== void 0 ? _a : [];
    if (bindings.length === 0)
        return;
    const list = createElement("ul", "binding-list");
    for (const binding of bindings) {
        const target = model.targets.find((item) => item.id === binding.target_id);
        const item = createElement("li");
        item.append(createElement("span", "", (_b = target === null || target === void 0 ? void 0 : target.name) !== null && _b !== void 0 ? _b : `服务器 ${binding.target_id}`), createElement("span", "meta", `${binding.object_id}, ${binding.etag.slice(0, 8)}`));
        list.append(item);
    }
    card.append(list);
}
async function renderSnapshots(slot, container, actions) {
    const snapshots = await actions.requestApi(`/v1/local-saves/${slot.id}/snapshots`);
    const list = createElement("ul", "snapshot-list");
    if (snapshots.length === 0)
        list.append(createElement("li", "", "尚无快照."));
    for (const snapshot of snapshots) {
        const item = createElement("li");
        const description = createElement("span");
        description.append(createElement("strong", "", snapshot.label), createElement("span", "meta", snapshot.created_at));
        item.append(description, createButton("回滚", async () => {
            if (!confirm(`回滚到 ${snapshot.label}? 系统会先保存当前状态.`))
                return undefined;
            await actions.requestApi(`/v1/local-saves/${slot.id}/snapshots/${snapshot.id}/restore`, { method: "POST" });
            await actions.refreshManagementState();
            return "存档已回滚, 回滚前状态已保存为安全快照.";
        }, actions.runAction, "danger"));
        list.append(item);
    }
    container.replaceChildren(list);
}
function fillTargetSelect(select, targets) {
    const selected = select.value;
    select.replaceChildren();
    for (const target of targets) {
        const option = createElement("option", "", target.name);
        option.value = String(target.id);
        option.selected = option.value === selected;
        select.append(option);
    }
    select.disabled = targets.length === 0;
}
function fillOptionalTargetSelect(select, targets, selectedTargetId) {
    select.replaceChildren();
    const localOnly = createElement("option", "", "仅保存本地快照");
    localOnly.value = "";
    localOnly.selected = selectedTargetId === null;
    select.append(localOnly);
    for (const target of targets) {
        const option = createElement("option", "", target.name);
        option.value = String(target.id);
        option.selected = target.id === selectedTargetId;
        select.append(option);
    }
}
function formatTime(value) {
    const time = new Date(value);
    return Number.isNaN(time.valueOf()) ? value : time.toLocaleString();
}
// //// 按文件交付状态呈现操作反馈 [@x380kkm 2026-09-28] ////
function documentExportFeedback(result) {
    const messages = {
        saved: "文件已保存.",
        shared: "已打开系统分享.",
        download_requested: "已发起下载, 请在系统完成保存.",
    };
    return messages[result];
}
async function shareOrDownloadJson(filename, payload) {
    var _a;
    const contents = JSON.stringify(payload, null, 2);
    if (typeof ((_a = window.StarpointDocuments) === null || _a === void 0 ? void 0 : _a.saveJson) === "function") {
        return window.StarpointDocuments.saveJson(filename, contents);
    }
    const blob = new Blob([contents], { type: "application/json" });
    if (typeof File === "function" && typeof navigator.share === "function") {
        const file = new File([blob], filename, { type: blob.type });
        if (typeof navigator.canShare === "function" && navigator.canShare({ files: [file] })) {
            try {
                await navigator.share({ files: [file], title: "导出 Starpoint 存档" });
                return "shared";
            }
            catch (error) {
                if ((error === null || error === void 0 ? void 0 : error.name) === "AbortError")
                    return "cancelled";
            }
        }
    }
    const link = document.createElement("a");
    link.href = URL.createObjectURL(blob);
    link.download = filename;
    document.body.append(link);
    link.click();
    link.remove();
    setTimeout(() => URL.revokeObjectURL(link.href), 0);
    return "download_requested";
}
// //// /呈现服务器, 存档和同步操作 ////
},
"/manage/save-deletion.js": function(module, exports, require) {
"use strict";
// audience: external
// # management-save-deletion
//
// 删除确认显示存档身份和本地备份范围. 关闭对话框保留存档.
Object.defineProperty(exports, "__esModule", { value: true });
exports.confirmSaveDeletion = void 0;
// //// 等待用户确认指定存档的删除 [@x380kkm 2026-09-20] ////
async function confirmSaveDeletion(slot, opener) {
    const dialog = document.querySelector("#save-delete-dialog");
    if (!dialog || typeof dialog.showModal !== "function") {
        return window.confirm(`删除存档 "${slot.name}" (槽位 ${slot.id})?\n\n此存档的进度, 本地快照和 AI 编队将永久删除. 已导出的文件和远程副本会保留.`);
    }
    if (dialog.open)
        return false;
    dialog.querySelector("#save-delete-name").textContent = slot.name;
    dialog.querySelector("#save-delete-detail").textContent = `槽位 ${slot.id} · ${slot.snapshot_count} 个本地快照`;
    dialog.returnValue = "cancel";
    return new Promise((resolve) => {
        dialog.addEventListener("close", () => {
            resolve(dialog.returnValue === "delete");
            requestAnimationFrame(() => {
                if (opener.isConnected && !opener.disabled)
                    opener.focus({ preventScroll: true });
            });
        }, { once: true });
        dialog.showModal();
    });
}
exports.confirmSaveDeletion = confirmSaveDeletion;
},
"/manage/password-backup.js": function(module, exports, require) {
"use strict";
// audience: external
// # management-password-backup
// 密码备份包含完整存档. 旧加密存档可与原设备恢复包一起导入.
Object.defineProperty(exports, "__esModule", { value: true });
exports.importSaveFile = exports.requestBackupPassword = void 0;
// //// 在存档卡片内输入并确认备份密码 [@x380kkm 2026-09-26] ////
function requestBackupPassword(container) {
    return new Promise((resolve) => {
        const form = document.createElement("form");
        form.className = "stack-form save-password-panel";
        const fields = ["备份密码, 8 到 128 个字符", "再次输入密码"].map((text) => {
            const label = document.createElement("label");
            const title = document.createElement("span");
            title.textContent = text;
            const input = document.createElement("input");
            input.type = "password";
            input.autocomplete = "new-password";
            input.required = true;
            label.append(title, input);
            form.append(label);
            return input;
        });
        const message = document.createElement("p");
        message.className = "meta";
        message.textContent = "在另一台设备导入此文件时使用同一密码.";
        const submit = document.createElement("button");
        submit.className = "button primary";
        submit.type = "submit";
        submit.textContent = "加密并导出";
        const cancel = document.createElement("button");
        cancel.className = "button secondary";
        cancel.type = "button";
        cancel.textContent = "取消";
        const finish = (value) => {
            fields.forEach((field) => { field.value = ""; });
            form.remove();
            resolve(value);
        };
        cancel.addEventListener("click", () => finish(undefined));
        form.addEventListener("submit", (event) => {
            event.preventDefault();
            const password = fields[0].value;
            const length = Array.from(password).length;
            if (length < 8 || length > 128 || password !== fields[1].value) {
                message.textContent = "请输入 8 到 128 个字符的密码, 并确保两次输入一致.";
                return;
            }
            finish(password);
        });
        form.append(message, submit, cancel);
        container.append(form);
        fields[0].focus();
    });
}
exports.requestBackupPassword = requestBackupPassword;
// //// 按存档封装选择导入与密码恢复接口 [@x380kkm 2026-09-26] ////
async function importSaveFile(form, requestApi) {
    var _a, _b, _c;
    const file = form.get("file");
    if (!(file instanceof File) || file.size === 0)
        throw new Error("请选择存档文件.");
    const name = String((_a = form.get("name")) !== null && _a !== void 0 ? _a : "").trim();
    const payload = JSON.parse(await file.text());
    const password = String((_b = form.get("password")) !== null && _b !== void 0 ? _b : "");
    if (payload.format === "starpoint-password-save") {
        return requestApi("/v1/local-saves/import-password", { method: "POST", body: { name, password, package: payload } });
    }
    if (payload.format === "starpoint-encrypted-save") {
        const recoveryFile = form.get("recovery");
        if (recoveryFile instanceof File && recoveryFile.size > 0) {
            const recoveryData = JSON.parse(await recoveryFile.text());
            const recovery = (_c = recoveryData === null || recoveryData === void 0 ? void 0 : recoveryData.package) !== null && _c !== void 0 ? _c : recoveryData;
            return requestApi("/v1/local-saves/import-encrypted-recovery", { method: "POST", body: { name, password, envelope: payload, recovery } });
        }
        return requestApi("/v1/local-saves/import-encrypted", { method: "POST", body: { name, envelope: payload } });
    }
    return requestApi("/v1/local-saves/import", { method: "POST", body: { name, data: payload } });
}
exports.importSaveFile = importSaveFile;
},
"/manage/save-access.js": function(module, exports, require) {
"use strict";
// audience: internal
// # management-save-access
// 槽授权仅在签发响应中显示明文, 列表使用授权元数据, 撤销后清除页面保存的对应授权.
Object.defineProperty(exports, "__esModule", { value: true });
exports.renderSlotAccess = void 0;
const save_controls_js_1 = require("./save-controls.js");
const PERMISSIONS = [["download", "仅下载"], ["upload", "仅上传"], ["both", "双向同步"]];
// //// 管理单个存档槽的传输授权 [@x380kkm 2026-09-26] ////
function renderSlotAccess(slot, container, actions) {
    const document = container.ownerDocument;
    const section = (0, save_controls_js_1.node)(document, "details", "form-drawer");
    section.append((0, save_controls_js_1.node)(document, "summary", "", "槽位传输授权"));
    const identity = (0, save_controls_js_1.node)(document, "p", "meta", `本机实例, 槽位 ${slot.id}. 展开后读取完整实例标识.`);
    section.append(identity);
    const list = (0, save_controls_js_1.node)(document, "div", "card-list");
    const issuedContainer = (0, save_controls_js_1.node)(document, "div", "stack-form");
    const form = (0, save_controls_js_1.node)(document, "form", "stack-form");
    const fields = (0, save_controls_js_1.node)(document, "fieldset", "stack-form");
    const device = (0, save_controls_js_1.field)(document, fields, "device_name", "授权名称", { maxLength: 128 });
    const permission = (0, save_controls_js_1.field)(document, fields, "permission", "允许操作", { choices: PERMISSIONS, value: "download" });
    const expires = (0, save_controls_js_1.field)(document, fields, "expires_at", "到期时间 (UTC, 留空为长期)", { type: "datetime-local" });
    fields.append((0, save_controls_js_1.node)(document, "p", "meta", "跨实例槽位绑定使用双向同步授权. 授权资料只交给你信任的目标设备."));
    form.append(fields);
    let issued = null;
    let generation = 0;
    let loaded = false;
    const path = `/v1/local-saves/${slot.id}/transfer-tokens/slot`;
    async function load() {
        const request = ++generation;
        const response = await actions.requestApi(path);
        if (request !== generation)
            return;
        identity.textContent = `实例类型 local, 实例 ID ${response.instanceId}, 槽位 ID ${slot.id}.`;
        list.replaceChildren();
        for (const token of response.tokens) {
            const card = (0, save_controls_js_1.node)(document, "article", "item-card");
            const permissionLabel = PERMISSIONS.find(([value]) => value === token.permission);
            const expired = token.expiresAt && Date.parse(token.expiresAt) <= Date.now();
            const status = token.revokedAt ? "已撤销" : expired ? "已过期" : "有效";
            card.append((0, save_controls_js_1.node)(document, "strong", "", token.deviceName || token.id));
            card.append((0, save_controls_js_1.node)(document, "p", "meta", `${permissionLabel ? permissionLabel[1] : token.permission} · ${status} · 创建 ${token.createdAt}`));
            if (token.expiresAt)
                card.append((0, save_controls_js_1.node)(document, "p", "meta", `到期 ${token.expiresAt}`));
            if (!token.revokedAt) {
                card.append((0, save_controls_js_1.actionButton)(document, "撤销槽授权", async () => {
                    const confirm = actions.confirm || window.confirm.bind(window);
                    if (!confirm(`撤销 ${token.deviceName || token.id} 的槽授权? 使用此授权的同步需要重新授权.`))
                        return undefined;
                    await actions.requestApi(`${path}/${encodeURIComponent(token.id)}`, { method: "DELETE" });
                    if (issued && issued.metadata.id === token.id) {
                        issued = null;
                        issuedContainer.replaceChildren();
                    }
                    await load();
                    return "槽授权已撤销.";
                }, actions.runAction, "danger"));
            }
            list.append(card);
        }
        if (!response.tokens.length)
            list.append((0, save_controls_js_1.node)(document, "p", "empty-state", "此槽位尚无传输授权."));
        loaded = true;
    }
    (0, save_controls_js_1.submitForm)(document, form, fields, "签发槽授权", async () => {
        const body = { permission: permission.value };
        if (device.value.trim())
            body.deviceName = device.value.trim();
        if (expires.value) {
            const date = new Date(`${expires.value}Z`);
            if (!Number.isFinite(date.getTime()) || date.getTime() <= Date.now())
                throw new Error("请输入将来的 UTC 到期时间.");
            body.expiresAt = date.toISOString();
        }
        issued = await actions.requestApi(path, { method: "POST", body });
        const label = (0, save_controls_js_1.node)(document, "label", "stack-form");
        label.append((0, save_controls_js_1.node)(document, "span", "", "本次槽授权资料, 可粘贴到另一设备的创建槽位绑定表单"));
        const contents = (0, save_controls_js_1.node)(document, "textarea");
        contents.readOnly = true;
        contents.rows = 7;
        contents.spellcheck = false;
        contents.value = JSON.stringify({
            target_instance_kind: "local", target_instance_id: issued.instanceId,
            target_slot_id: slot.id, target_token: issued.token,
        }, null, 2);
        label.append(contents);
        issuedContainer.replaceChildren(label, (0, save_controls_js_1.actionButton)(document, "选择授权资料", () => { contents.focus(); contents.select(); }, actions.runAction));
        identity.textContent = `实例类型 local, 实例 ID ${issued.instanceId}, 槽位 ID ${slot.id}.`;
        await load();
        return "槽授权已签发, 明文仅在当前页面显示.";
    }, actions.runAction);
    section.addEventListener("toggle", () => {
        if (section.open && !loaded)
            return actions.runAction(null, load);
    });
    section.append((0, save_controls_js_1.actionButton)(document, "刷新槽授权", load, actions.runAction), list, form, issuedContainer);
    container.append(section);
}
exports.renderSlotAccess = renderSlotAccess;
// //// /管理单个存档槽的传输授权 ////
},
"/manage/save-controls.js": function(module, exports, require) {
"use strict";
// audience: internal
// # management-save-controls
// 存档表单使用安全 DOM 文本节点, 提交期间锁定同一表单的全部字段.
Object.defineProperty(exports, "__esModule", { value: true });
exports.submitForm = exports.actionButton = exports.field = exports.node = void 0;
// //// 创建存档表单控件 [@x380kkm 2026-09-26] ////
function node(document, tag, className, text) {
    const element = document.createElement(tag);
    if (className)
        element.className = className;
    if (text !== undefined)
        element.textContent = text;
    return element;
}
exports.node = node;
function field(document, container, name, label, options = {}) {
    const wrapper = node(document, "label", "grow");
    wrapper.append(node(document, "span", "", label));
    const input = node(document, options.choices ? "select" : "input");
    input.name = name;
    if (options.choices) {
        for (const [value, text] of options.choices) {
            const option = node(document, "option", "", text);
            option.value = value;
            input.append(option);
        }
    }
    else
        input.type = options.type || "text";
    for (const key of ["min", "max", "step", "pattern", "placeholder", "maxLength", "autocomplete"]) {
        if (options[key] !== undefined)
            input[key] = options[key];
    }
    input.required = Boolean(options.required);
    input.value = String(options.value === undefined ? "" : options.value);
    wrapper.append(input);
    if (options.type === "checkbox") {
        wrapper.className = "check-row";
        wrapper.replaceChildren(input, node(document, "span", "", label));
    }
    container.append(wrapper);
    return input;
}
exports.field = field;
function actionButton(document, label, action, runAction, style = "ghost") {
    const button = node(document, "button", `button ${style}`, label);
    button.type = "button";
    button.addEventListener("click", () => runAction(button, action));
    return button;
}
exports.actionButton = actionButton;
function submitForm(document, form, fields, label, action, runAction) {
    const button = node(document, "button", "button primary", label);
    button.type = "submit";
    form.append(button);
    let busy = false;
    form.addEventListener("submit", (event) => {
        event.preventDefault();
        if (busy)
            return;
        return runAction(button, async () => {
            busy = true;
            fields.disabled = true;
            try {
                return await action();
            }
            finally {
                fields.disabled = false;
                busy = false;
            }
        });
    });
    return button;
}
exports.submitForm = submitForm;
// //// /创建存档表单控件 ////
},
"/manage/save-revisions.js": function(module, exports, require) {
"use strict";
// audience: internal
// # management-save-revisions
// 版本历史与普通快照分别展示. 恢复携带读取时的当前 ETag, 存档变化由服务拒绝覆盖.
Object.defineProperty(exports, "__esModule", { value: true });
exports.renderSaveRevisions = void 0;
const save_controls_js_1 = require("./save-controls.js");
// //// 展示版本历史并恢复选定版本 [@x380kkm 2026-09-26] ////
async function renderSaveRevisions(slot, container, actions) {
    const document = container.ownerDocument;
    const path = `/v1/local-saves/${slot.id}/revisions`;
    const state = await actions.requestApi(path);
    const current = state.revisions.find((revision) => revision.id === state.current_revision_id);
    const section = (0, save_controls_js_1.node)(document, "section", "automation-card");
    section.append((0, save_controls_js_1.node)(document, "h4", "card-title", "版本历史"));
    section.append((0, save_controls_js_1.node)(document, "p", "meta", "包含同步覆盖前的安全版本. 普通快照在查看快照中单独展示."));
    const list = (0, save_controls_js_1.node)(document, "ul", "snapshot-list");
    const sources = {
        "Before transfer upload": "同步覆盖前", "Transfer upload": "同步导入",
        "Before restore": "恢复前", "Snapshot restore": "快照恢复",
        "Revision restore": "版本恢复", "Initial save": "初始存档",
        "Current state": "当前状态",
    };
    let restoring = false;
    for (const revision of state.revisions) {
        const item = (0, save_controls_js_1.node)(document, "li");
        const description = (0, save_controls_js_1.node)(document, "span");
        description.append((0, save_controls_js_1.node)(document, "strong", "", sources[revision.label] || revision.label));
        description.append((0, save_controls_js_1.node)(document, "span", "meta", `${revision.created_at} · ${revision.id}`));
        item.append(description);
        if (revision.id === state.current_revision_id)
            item.append((0, save_controls_js_1.node)(document, "span", "badge", "当前版本"));
        else {
            const button = (0, save_controls_js_1.actionButton)(document, "恢复此版本", async () => {
                if (restoring)
                    return undefined;
                const confirm = actions.confirm || window.confirm.bind(window);
                if (!confirm(`将 ${slot.name} 恢复到 ${revision.created_at} 的版本? 当前状态会保留为安全版本.`))
                    return undefined;
                if (!current)
                    throw new Error("当前版本已变化, 请重新读取版本历史.");
                restoring = true;
                try {
                    await actions.requestApi(`${path}/${encodeURIComponent(revision.id)}/restore`, {
                        method: "POST", headers: { "If-Match": `"${current.etag}"` },
                    });
                    await actions.refreshManagementState();
                    return "存档版本已恢复, 覆盖前状态保留在版本历史中.";
                }
                finally {
                    restoring = false;
                }
            }, actions.runAction, "danger");
            button.disabled = !current;
            item.append(button);
        }
        list.append(item);
    }
    if (!state.revisions.length)
        list.append((0, save_controls_js_1.node)(document, "li", "", "尚无版本历史."));
    section.append(list);
    container.replaceChildren(section);
}
exports.renderSaveRevisions = renderSaveRevisions;
// //// /展示版本历史并恢复选定版本 ////
},
"/manage/save-transfers.js": function(module, exports, require) {
"use strict";
// audience: internal
// # management-save-transfers
// 绑定表单对应服务器的传输模式和条件覆盖接口, 目标身份与槽授权分开输入.
Object.defineProperty(exports, "__esModule", { value: true });
exports.renderTransferBindings = exports.renderTransferBindingCreate = exports.bindingModeSummary = void 0;
const save_controls_js_1 = require("./save-controls.js");
const UPLOAD_MODES = [["manual", "手动"], ["interval", "定时"]];
const PULL_MODES = [["manual", "手动"], ["interval", "定时"]];
const CONNECTION_MODES = { on_connect: "连接时 (当前实例手动执行)", on_disconnect: "断开时 (当前实例手动执行)" };
const POLICIES = [["ask", "询问并停止同步"], ["local_wins", "本地覆盖远端"], ["remote_wins", "远端覆盖本地"]];
// //// 呈现传输模式并校验调度参数 [@x380kkm 2026-09-26] ////
function bindingModeSummary(binding) {
    if (!binding.enabled)
        return "绑定已关闭.";
    const upload = UPLOAD_MODES.find(([value]) => value === binding.upload_mode);
    const pull = PULL_MODES.find(([value]) => value === binding.pull_mode);
    const interval = binding.upload_mode === "interval" || binding.pull_mode === "interval"
        ? `, 间隔 ${binding.interval_seconds} 秒` : "";
    return `绑定已启用, 上传: ${upload ? upload[1] : CONNECTION_MODES[binding.upload_mode] || binding.upload_mode}, 下载: ${pull ? pull[1] : CONNECTION_MODES[binding.pull_mode] || binding.pull_mode}${interval}.`;
}
exports.bindingModeSummary = bindingModeSummary;
function modeFields(document, container, binding) {
    const uploadChoices = binding.upload_mode === "on_connect" ? [...UPLOAD_MODES, [binding.upload_mode, CONNECTION_MODES.on_connect]] : UPLOAD_MODES;
    const pullChoices = binding.pull_mode === "on_disconnect" ? [...PULL_MODES, [binding.pull_mode, CONNECTION_MODES.on_disconnect]] : PULL_MODES;
    const upload = (0, save_controls_js_1.field)(document, container, "upload_mode", "上传时机", { choices: uploadChoices, value: binding.upload_mode });
    const pull = (0, save_controls_js_1.field)(document, container, "pull_mode", "下载时机", { choices: pullChoices, value: binding.pull_mode });
    const conflict = (0, save_controls_js_1.field)(document, container, "conflict_policy", "两端均变化时", { choices: POLICIES, value: binding.conflict_policy });
    const interval = (0, save_controls_js_1.field)(document, container, "interval_seconds", "同步间隔 (秒)", { type: "number", min: 60, max: 2592000, step: 1, required: true, value: binding.interval_seconds });
    const enabled = (0, save_controls_js_1.field)(document, container, "enabled", "启用绑定", { type: "checkbox" });
    enabled.checked = binding.enabled;
    const update = () => { interval.disabled = upload.value !== "interval" && pull.value !== "interval"; };
    upload.addEventListener("change", update);
    pull.addEventListener("change", update);
    update();
    return () => {
        const seconds = Number(interval.value);
        if (!Number.isInteger(seconds) || seconds < 60 || seconds > 2592000)
            throw new Error("同步间隔须为 60 到 2592000 的整数秒.");
        return { upload_mode: upload.value, pull_mode: pull.value, conflict_policy: conflict.value, interval_seconds: seconds, enabled: enabled.checked };
    };
}
// //// /呈现传输模式并校验调度参数 ////
// //// 创建指向明确目标槽的传输绑定 [@x380kkm 2026-09-26] ////
function renderTransferBindingCreate(slot, model, container, actions) {
    const profiles = model.profiles.profiles.filter((profile) => profile.mode === "remote");
    const document = container.ownerDocument;
    const section = (0, save_controls_js_1.node)(document, "details", "form-drawer");
    section.append((0, save_controls_js_1.node)(document, "summary", "", "创建槽位绑定"));
    if (!profiles.length) {
        section.append((0, save_controls_js_1.node)(document, "p", "meta", "请先在游戏服务器中添加要连接的目标实例, 再填写目标槽的双向授权资料."));
        container.append(section);
        return;
    }
    const form = (0, save_controls_js_1.node)(document, "form", "stack-form");
    const fields = (0, save_controls_js_1.node)(document, "fieldset", "stack-form");
    const profile = (0, save_controls_js_1.field)(document, fields, "target_profile_id", "目标服务器", { choices: profiles.map((item) => [String(item.id), item.name]), value: profiles[0].id });
    const kind = (0, save_controls_js_1.field)(document, fields, "target_instance_kind", "目标实例类型", { choices: [["local", "本机服务实例"], ["remote", "远程服务实例"]], value: "local" });
    const instance = (0, save_controls_js_1.field)(document, fields, "target_instance_id", "目标实例 ID", { required: true, pattern: "[0-9a-f]{32}", maxLength: 32 });
    const targetSlot = (0, save_controls_js_1.field)(document, fields, "target_slot_id", "目标槽 ID", { type: "number", min: 1, step: 1, required: true });
    const token = (0, save_controls_js_1.field)(document, fields, "target_token", "目标槽双向授权码", { type: "password", required: true, autocomplete: "off" });
    const pasteLabel = (0, save_controls_js_1.node)(document, "label", "stack-form");
    pasteLabel.append((0, save_controls_js_1.node)(document, "span", "", "粘贴目标设备签发的槽授权资料"));
    const paste = (0, save_controls_js_1.node)(document, "textarea");
    paste.name = "authorization";
    paste.rows = 3;
    paste.spellcheck = false;
    pasteLabel.append(paste);
    fields.append(pasteLabel, (0, save_controls_js_1.actionButton)(document, "填入授权资料", () => {
        let authorization;
        try {
            authorization = JSON.parse(paste.value);
        }
        catch (_a) {
            throw new Error("槽授权资料格式无效.");
        }
        if (!authorization || !["local", "remote"].includes(authorization.target_instance_kind)
            || !/^[0-9a-f]{32}$/.test(authorization.target_instance_id)
            || !Number.isInteger(authorization.target_slot_id) || authorization.target_slot_id <= 0
            || typeof authorization.target_token !== "string" || !authorization.target_token)
            throw new Error("槽授权资料缺少有效的目标身份或授权码.");
        kind.value = authorization.target_instance_kind;
        instance.value = authorization.target_instance_id;
        targetSlot.value = String(authorization.target_slot_id);
        token.value = authorization.target_token;
        paste.value = "";
        return "目标槽授权资料已填入, 保存前请确认目标服务器与同步模式.";
    }, actions.runAction));
    const modes = modeFields(document, fields, { upload_mode: "manual", pull_mode: "manual", conflict_policy: "ask", interval_seconds: 900, enabled: true });
    form.append(fields);
    (0, save_controls_js_1.submitForm)(document, form, fields, "保存槽位绑定", async () => {
        const targetId = Number(targetSlot.value);
        if (!Number.isSafeInteger(targetId) || targetId <= 0 || !/^[0-9a-f]{32}$/.test(instance.value))
            throw new Error("请输入有效的目标实例和槽 ID.");
        const binding = await actions.requestApi(`/v1/local-saves/${slot.id}/transfer-bindings`, {
            method: "POST", body: Object.assign({ target_profile_id: Number(profile.value), target_instance_kind: kind.value, target_instance_id: instance.value, target_slot_id: targetId, target_token: token.value.trim() }, modes()),
        });
        token.value = "";
        paste.value = "";
        await actions.refreshManagementState();
        return `槽位绑定已创建. ${bindingModeSummary(binding)}`;
    }, actions.runAction);
    section.append(form);
    container.append(section);
}
exports.renderTransferBindingCreate = renderTransferBindingCreate;
// //// /创建指向明确目标槽的传输绑定 ////
// //// 编辑现有绑定的模式和目标授权 [@x380kkm 2026-09-26] ////
function appendBindingEditor(slot, binding, container, actions) {
    const document = container.ownerDocument;
    const section = (0, save_controls_js_1.node)(document, "details", "form-drawer");
    section.append((0, save_controls_js_1.node)(document, "summary", "", "编辑同步设置"));
    const form = (0, save_controls_js_1.node)(document, "form", "stack-form");
    const fields = (0, save_controls_js_1.node)(document, "fieldset", "stack-form");
    const modes = modeFields(document, fields, binding);
    const token = (0, save_controls_js_1.field)(document, fields, "target_token", "更新目标槽授权码 (留空保留当前授权)", { type: "password", autocomplete: "off" });
    form.append(fields);
    (0, save_controls_js_1.submitForm)(document, form, fields, "保存同步设置", async () => {
        const body = modes();
        if (token.value.trim())
            body.target_token = token.value.trim();
        const updated = await actions.requestApi(`/v1/local-saves/${slot.id}/transfer-bindings/${binding.binding_id}`, { method: "PUT", body });
        token.value = "";
        await actions.refreshManagementState();
        return bindingModeSummary(updated);
    }, actions.runAction);
    section.append(form);
    container.append(section);
}
// //// /编辑现有绑定的模式和目标授权 ////
// //// 展示绑定状态并处理同步冲突 [@x380kkm 2026-09-26] ////
function renderTransferBindings(slot, model, container, actions) {
    const bindings = model.transferBindings.get(slot.id) || [];
    if (!bindings.length)
        return;
    const document = container.ownerDocument;
    const section = (0, save_controls_js_1.node)(document, "section", "automation-card");
    section.append((0, save_controls_js_1.node)(document, "h4", "card-title", "跨实例槽位绑定"));
    for (const binding of bindings) {
        const profile = model.profiles.profiles.find((item) => item.id === binding.target.profile_id);
        const conflict = (model.transferConflicts.get(binding.binding_id) || []).find((item) => item.status === "open");
        const card = (0, save_controls_js_1.node)(document, "article", "item-card");
        card.append((0, save_controls_js_1.node)(document, "h5", "card-title", profile ? profile.name : `服务器 ${binding.target.profile_id}`));
        card.append((0, save_controls_js_1.node)(document, "p", "meta", `${binding.target.instance_kind}, 实例 ${binding.target.instance_id}, 槽 ${binding.target.slot_id}`));
        card.append((0, save_controls_js_1.node)(document, "p", "meta", bindingModeSummary(binding)));
        if (binding.last_synced_at)
            card.append((0, save_controls_js_1.node)(document, "p", "meta", `最近同步 ${binding.last_synced_at}`));
        if (binding.last_error)
            card.append((0, save_controls_js_1.node)(document, "p", "automation-error", actions.describeError(binding.last_error)));
        const path = `/v1/local-saves/${slot.id}/transfer-bindings/${binding.binding_id}`;
        const controls = (0, save_controls_js_1.node)(document, "div", "card-actions");
        const sync = (0, save_controls_js_1.actionButton)(document, "立即同步", async () => {
            try {
                await actions.requestApi(`${path}/sync`, { method: "POST" });
            }
            finally {
                await actions.refreshManagementState();
            }
            return "槽位同步完成.";
        }, actions.runAction);
        sync.disabled = !binding.enabled || Boolean(conflict);
        controls.append(sync, (0, save_controls_js_1.actionButton)(document, binding.enabled ? "关闭绑定" : "启用绑定", async () => {
            const updated = await actions.requestApi(path, { method: "PUT", body: {
                    upload_mode: binding.upload_mode, pull_mode: binding.pull_mode, conflict_policy: binding.conflict_policy,
                    interval_seconds: binding.interval_seconds, enabled: !binding.enabled,
                } });
            await actions.refreshManagementState();
            return bindingModeSummary(updated);
        }, actions.runAction));
        controls.append((0, save_controls_js_1.actionButton)(document, "删除绑定", async () => {
            const confirm = actions.confirm || window.confirm.bind(window);
            if (!confirm("删除此传输绑定? 现有存档和版本历史会保留."))
                return undefined;
            await actions.requestApi(path, { method: "DELETE" });
            await actions.refreshManagementState();
            return "传输绑定已删除.";
        }, actions.runAction, "danger"));
        card.append(controls);
        appendBindingEditor(slot, binding, card, actions);
        if (conflict)
            appendConflictActions(path, conflict, card, actions);
        section.append(card);
    }
    container.append(section);
}
exports.renderTransferBindings = renderTransferBindings;
function appendConflictActions(path, conflict, container, actions) {
    const document = container.ownerDocument;
    container.append((0, save_controls_js_1.node)(document, "p", "automation-error", `本地 ${conflict.source_etag.slice(0, 8)}, 远端 ${conflict.target_etag.slice(0, 8)}.`));
    const controls = (0, save_controls_js_1.node)(document, "div", "card-actions");
    for (const [label, resolution] of [["本地覆盖远端", "local_wins"], ["远端覆盖本地", "remote_wins"], ["保留双方并关闭绑定", "keep_both"]]) {
        controls.append((0, save_controls_js_1.actionButton)(document, label, async () => {
            const confirm = actions.confirm || window.confirm.bind(window);
            if (!confirm(`${label}? 覆盖前会保留安全版本, 可在存档的版本历史中恢复.`))
                return undefined;
            await actions.requestApi(`${path}/conflicts/${conflict.conflict_id}/resolve`, { method: "POST", body: { resolution } });
            await actions.refreshManagementState();
            return "传输冲突已处理.";
        }, actions.runAction, resolution === "keep_both" ? "ghost" : "danger"));
    }
    container.append(controls);
}
// //// /展示绑定状态并处理同步冲突 ////
}
};
const links = {"/manage/app.js":{"/manage/activity-controller.js":"/manage/activity-controller.js","/manage/activity-history.js":"/manage/activity-history.js","/manage/ai-team-controller.js":"/manage/ai-team-controller.js","/manage/rescue-controller.js":"/manage/rescue-controller.js","/manage/activity-rescue.js":"/manage/activity-rescue.js","/manage/mail-reward-controller.js":"/manage/mail-reward-controller.js","/manage/reward-history.js":"/manage/reward-history.js","/manage/competition-controller.js":"/manage/competition-controller.js","/manage/views.js":"/manage/views.js","/manage/password-backup.js":"/manage/password-backup.js"},"/manage/activity-controller.js":{"/manage/activity-views.js":"/manage/activity-views.js"},"/manage/activity-views.js":{"/manage/activity-navigation.js":"/manage/activity-navigation.js"},"/manage/activity-navigation.js":{},"/manage/activity-history.js":{},"/manage/ai-team-controller.js":{},"/manage/rescue-controller.js":{"/manage/activity-navigation.js":"/manage/activity-navigation.js"},"/manage/activity-rescue.js":{"/manage/activity-navigation.js":"/manage/activity-navigation.js"},"/manage/mail-reward-controller.js":{},"/manage/reward-history.js":{},"/manage/competition-controller.js":{"./competition-views.js":"/manage/competition-views.js"},"/manage/competition-views.js":{},"/manage/views.js":{"./save-deletion.js":"/manage/save-deletion.js","./password-backup.js":"/manage/password-backup.js","./save-access.js":"/manage/save-access.js","./save-revisions.js":"/manage/save-revisions.js","./save-transfers.js":"/manage/save-transfers.js"},"/manage/save-deletion.js":{},"/manage/password-backup.js":{},"/manage/save-access.js":{"./save-controls.js":"/manage/save-controls.js"},"/manage/save-controls.js":{},"/manage/save-revisions.js":{"./save-controls.js":"/manage/save-controls.js"},"/manage/save-transfers.js":{"./save-controls.js":"/manage/save-controls.js"}};
const cache = Object.create(null);
function load(id) {
    if (cache[id]) return cache[id].exports;
    const module = { exports: {} };
    cache[id] = module;
    modules[id](module, module.exports, function (dependency) {
        return load(links[id][dependency]);
    });
    return module.exports;
}
load("/manage/app.js");
})();
