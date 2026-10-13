// audience: external
// # personal-service-rescue-controller
//
// 救援设置与关卡选择保存在本机服务, 关卡开放状态由活动日历计算.
// 列表筛选与分页保留完整选择, 刷新开放状态保留尚未保存的编辑.

import { questStageName, questStageKey } from "/manage/activity-navigation.js"

const PAGE_SIZE = 24
const DIFFICULTY_ORDER = [1, 2, 3, 5, 4, 6, 7, 8]

function questKey(quest) {
    return `${quest.category_id}:${quest.quest_id}`
}

function isInAvailabilityScope(quest, filters, selected) {
    if (filters.selectedOnly) return selected.has(questKey(quest))
    if (filters.status === "open") return quest.is_open
    if (filters.status === "closed") return !quest.is_open
    return true
}

// //// 按玩法, 活动, 难度和开放状态筛选救援关卡 [@x380kkm 2026-09-23] ////
export function filterRescueQuests(quests, filters, selected) {
    const search = filters.search.trim().toLocaleLowerCase("zh-CN")
    return quests.filter((quest) => {
        if (filters.category && String(quest.category_id) !== filters.category) return false
        if (filters.activity && (quest.activity_id ?? "permanent") !== filters.activity) return false
        if (filters.stage && questStageKey(quest) !== filters.stage) return false
        if (filters.difficulty && String(quest.difficulty_id) !== filters.difficulty) return false
        if (!isInAvailabilityScope(quest, filters, selected)) return false
        const text = [quest.name, quest.activity_name, quest.category_name, quest.difficulty, quest.quest_id]
            .join(" ").toLocaleLowerCase("zh-CN")
        return !search || text.includes(search)
    })
}
// //// /按玩法, 活动, 难度和开放状态筛选救援关卡 ////

// //// 读取, 编辑并保存机器人救援设置 [@x380kkm 2026-09-23] ////
export function createRescueController({ panel, requestApi, runAction }) {
    const document = panel.ownerDocument
    const elements = Object.fromEntries([
        "form", "settings-controls", "quest-controls", "enabled", "spawn-interval", "room-lifetime",
        "max-rooms", "state", "selection-state", "refresh", "disable", "category", "activity",
        "difficulty", "stage", "search", "status", "selected-only", "list", "page-state", "previous", "next",
        "select-page", "clear-page", "clear-all",
    ].map((name) => [name, panel.querySelector(`#rescue-${name}`)]))
    const state = {
        quests: [], selected: new Map(), settings: null, page: 0, busy: false, loaded: false,
        previousStatus: "open",
    }

    function createElement(tag, className, text) {
        const node = document.createElement(tag)
        if (className) node.className = className
        if (text !== undefined) node.textContent = text
        return node
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
        }
    }

    function filteredQuests() {
        return filterRescueQuests(state.quests, filters(), state.selected)
    }

    function availabilityQuests() {
        const currentFilters = filters()
        return state.quests.filter((quest) => isInAvailabilityScope(quest, currentFilters, state.selected))
    }

    function hasChanges() {
        if (!state.settings) return false
        const saved = state.settings
        return elements.enabled.checked !== saved.enabled
            || Number(elements["spawn-interval"].value) !== saved.spawn_interval_seconds
            || Number(elements["room-lifetime"].value) !== saved.room_lifetime_seconds
            || Number(elements["max-rooms"].value) !== saved.max_rooms
            || state.selected.size !== saved.quests.length
            || saved.quests.some((quest) => !state.selected.has(questKey(quest)))
    }

    function updateControls() {
        const disabled = state.busy || !state.loaded
        elements["settings-controls"].disabled = disabled
        elements["quest-controls"].disabled = disabled
        elements.refresh.disabled = state.busy
        elements.disable.disabled = disabled || !state.settings?.enabled
        elements.status.disabled = elements["selected-only"].checked
        panel.setAttribute("aria-busy", String(state.busy))
    }

    async function withBusy(action) {
        if (state.busy) return
        state.busy = true
        updateControls()
        try {
            return await action()
        } finally {
            state.busy = false
            updateControls()
        }
    }

    function renderSummary() {
        const selectedOpen = state.quests.filter((quest) => quest.is_open && state.selected.has(questKey(quest))).length
        const selectedClosed = state.selected.size - selectedOpen
        elements["selection-state"].textContent = `已选 ${state.selected.size} 个关卡, 当前可进入 ${selectedOpen} 个.`
            + (selectedClosed > 0 ? ` 暂不可进入的 ${selectedClosed} 个选择会保留.` : "")
            + (hasChanges() ? " 有修改, 保存后生效." : "")
        if (!state.settings) return
        elements.state.textContent = !state.settings.enabled
            ? "救援已关闭."
            : state.settings.quests.length === 0
                ? "救援已开启, 请先选择关卡."
                : state.settings.eligible_quest_count > 0
                    ? `救援已开启, 当前可用 ${state.settings.eligible_quest_count} 个关卡.`
                    : "救援已开启, 等待所选关卡开放."
    }

    function applySettings(settings) {
        state.settings = settings
        state.selected = new Map(settings.quests.map((quest) => [questKey(quest), {
            category_id: quest.category_id, quest_id: quest.quest_id,
        }]))
        elements.enabled.checked = settings.enabled
        elements["spawn-interval"].value = String(settings.spawn_interval_seconds)
        elements["room-lifetime"].value = String(settings.room_lifetime_seconds)
        elements["max-rooms"].value = String(settings.max_rooms)
    }

    function fillOptions(select, entries, allLabel) {
        const previous = select.value
        const all = createElement("option", "", allLabel)
        all.value = ""
        select.replaceChildren(all)
        for (const [value, label] of entries) {
            const option = createElement("option", "", label)
            option.value = String(value)
            select.append(option)
        }
        select.value = entries.some(([value]) => String(value) === previous) ? previous : ""
    }

    function fillActivityOptions() {
        const quests = availabilityQuests().filter((quest) => !elements.category.value
            || String(quest.category_id) === elements.category.value)
        const activities = new Map(quests.map((quest) => [
            quest.activity_id ?? "permanent",
            quest.activity_id ? `${quest.activity_name} (${quest.activity_id})` : "领主战",
        ]))
        fillOptions(elements.activity, [...activities], "全部活动")
    }

    function fillFilters() {
        fillOptions(elements.category,
            [...new Map(availabilityQuests().map((quest) => [quest.category_id, quest.category_name]))],
            "全部类型")
        fillActivityOptions()
        fillStageOptions()
        fillDifficultyOptions()
    }

    function fillStageOptions() {
        const quests = availabilityQuests().filter(quest =>
            (!elements.category.value || String(quest.category_id) === elements.category.value)
            && (!elements.activity.value || (quest.activity_id ?? "permanent") === elements.activity.value))
        fillOptions(elements.stage, [...new Map(quests.map(quest =>
            [questStageKey(quest), `${questStageName(quest)} (${questStageKey(quest)})`]))], "全部关卡")
    }

    function fillDifficultyOptions() {
        const quests = availabilityQuests().filter((quest) =>
            (!elements.category.value || String(quest.category_id) === elements.category.value)
            && (!elements.activity.value || (quest.activity_id ?? "permanent") === elements.activity.value)
            && (!elements.stage.value || questStageKey(quest) === elements.stage.value))
        const difficulties = [...new Map(quests.map((quest) => [quest.difficulty_id, quest.difficulty]))]
            .sort((left, right) => DIFFICULTY_ORDER.indexOf(left[0]) - DIFFICULTY_ORDER.indexOf(right[0]))
        fillOptions(elements.difficulty, difficulties, "全部难度")
    }

    function createQuestRow(quest) {
        const key = questKey(quest)
        const card = createElement("label", "rescue-quest")
        const checkbox = createElement("input")
        checkbox.type = "checkbox"
        checkbox.checked = state.selected.has(key)
        checkbox.disabled = !quest.is_open && !checkbox.checked
        card.classList.toggle("is-selected", checkbox.checked)
        const description = createElement("span", "rescue-quest-description")
        description.append(
            createElement("strong", "", quest.name),
            createElement("span", "meta", `${quest.category_name} · ${quest.activity_name ?? "领主战"}`),
        )
        const status = createElement("span", quest.is_open ? "badge" : "badge rescue-closed",
            quest.is_open ? "可进入" : "暂不可进入")
        checkbox.addEventListener("change", () => {
            if (checkbox.checked) state.selected.set(key, { category_id: quest.category_id, quest_id: quest.quest_id })
            else state.selected.delete(key)
            card.classList.toggle("is-selected", checkbox.checked)
            if (elements["selected-only"].checked) {
                fillFilters()
                renderList()
            } else renderSummary()
        })
        card.append(checkbox, description, status)
        return card
    }

    function renderList() {
        const quests = filteredQuests()
        state.page = Math.min(state.page, Math.max(0, Math.ceil(quests.length / PAGE_SIZE) - 1))
        const start = state.page * PAGE_SIZE
        elements.list.replaceChildren(...quests.slice(start, start + PAGE_SIZE).map(createQuestRow))
        if (!quests.length) {
            const message = !elements["selected-only"].checked && elements.status.value === "open"
                && availabilityQuests().length === 0
                ? "当前没有可进入的救援关卡, 可切换全部查看目录."
                : "没有符合筛选条件的关卡."
            elements.list.append(createElement("p", "empty-state", message))
        }
        elements["page-state"].textContent = quests.length
            ? `第 ${state.page + 1} / ${Math.ceil(quests.length / PAGE_SIZE)} 页, 共 ${quests.length} 个关卡`
            : "共 0 个关卡"
        elements.previous.disabled = state.page === 0
        elements.next.disabled = start + PAGE_SIZE >= quests.length
        elements["select-page"].disabled = quests.length === 0
        elements["clear-page"].disabled = quests.length === 0
        renderSummary()
    }

    async function load(change) {
        return withBusy(async () => {
            const preserveDraft = hasChanges()
            try {
                const [settings, catalog] = await Promise.all([
                    requestApi("/v1/rescue-settings"),
                    requestApi("/v1/rescue-settings/quests"),
                ])
                state.quests = catalog.quests
                if (preserveDraft) {
                    state.settings = settings
                    if (change) {
                        const key = questKey(change.quest)
                        if (change.checked) state.selected.set(key, {category_id: change.quest.category_id, quest_id: change.quest.quest_id})
                        else state.selected.delete(key)
                        if (change.checked) elements.enabled.checked = settings.enabled
                    }
                }
                else applySettings(settings)
                state.loaded = true
                fillFilters()
                renderList()
            } catch (error) {
                elements.state.textContent = "救援设置读取失败, 点击刷新重试."
                throw error
            }
        })
    }

    function settingInteger(name, label, min, max) {
        const value = Number(elements[name].value)
        if (!Number.isInteger(value) || value < min || value > max) {
            throw new Error(`${label}须为 ${min} 到 ${max} 的整数.`)
        }
        return value
    }

    async function save() {
        const body = {
            enabled: elements.enabled.checked,
            spawn_interval_seconds: settingInteger("spawn-interval", "生成间隔", 5, 3600),
            room_lifetime_seconds: settingInteger("room-lifetime", "空房保留时间", 30, 3600),
            max_rooms: settingInteger("max-rooms", "同时开放房间数", 1, 10),
            quests: [...state.selected.values()].sort((left, right) =>
                left.category_id - right.category_id || left.quest_id - right.quest_id),
        }
        applySettings(await requestApi("/v1/rescue-settings", { method: "PUT", body }))
        renderList()
        return "救援设置已保存."
    }

    async function disable() {
        const preserveDraft = hasChanges()
        const settings = await requestApi("/v1/rescue-settings", { method: "PUT", body: { enabled: false } })
        if (preserveDraft) {
            state.settings = settings
            elements.enabled.checked = false
        } else {
            applySettings(settings)
        }
        renderList()
        return "救援已关闭, 当前战斗可正常完成."
    }

    function selectPage(checked) {
        for (const quest of filteredQuests().slice(state.page * PAGE_SIZE, (state.page + 1) * PAGE_SIZE)) {
            if (checked && quest.is_open) state.selected.set(questKey(quest), { category_id: quest.category_id, quest_id: quest.quest_id })
            else if (checked) continue
            else state.selected.delete(questKey(quest))
        }
        if (elements["selected-only"].checked) fillFilters()
        renderList()
    }

    elements.form.addEventListener("submit", (event) => {
        event.preventDefault()
        runAction(null, () => withBusy(save))
    })
    elements.refresh.addEventListener("click", () => runAction(null, load))
    elements.disable.addEventListener("click", () => runAction(null, () => withBusy(disable)))
    elements["settings-controls"].addEventListener("input", renderSummary)
    elements["settings-controls"].addEventListener("change", renderSummary)
    for (const name of ["category", "activity", "stage", "difficulty", "status", "selected-only", "search"]) {
        elements[name].addEventListener(name === "search" ? "input" : "change", () => {
            state.page = 0
            if (name === "selected-only") {
                if (elements["selected-only"].checked) {
                    state.previousStatus = elements.status.value
                    elements.status.value = ""
                    for (const filter of ["category", "activity", "stage", "difficulty", "search"]) elements[filter].value = ""
                } else elements.status.value = state.previousStatus
                updateControls()
            }
            if (name === "status" || name === "selected-only") fillFilters()
            if (name === "category") {
                fillActivityOptions()
                fillStageOptions()
                fillDifficultyOptions()
            }
            if (name === "activity") { fillStageOptions(); fillDifficultyOptions() }
            if (name === "stage") fillDifficultyOptions()
            renderList()
        })
    }
    elements.previous.addEventListener("click", () => { state.page--; renderList() })
    elements.next.addEventListener("click", () => { state.page++; renderList() })
    elements["select-page"].addEventListener("click", () => selectPage(true))
    elements["clear-page"].addEventListener("click", () => selectPage(false))
    elements["clear-all"].addEventListener("click", () => {
        state.selected.clear()
        if (elements["selected-only"].checked) fillFilters()
        renderList()
    })
    updateControls()
    return { load }
}
// //// /读取, 编辑并保存机器人救援设置 ////
