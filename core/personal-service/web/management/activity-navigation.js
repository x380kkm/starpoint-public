// audience: external
// # personal-service-activity-navigation
// 活动入口按玩法分类, 同一关卡的难度共用一个选择入口.

const COMMERCE_KINDS = new Set(["payment", "pass-card", "special-exchange", "start-dash-exchange", "treasure-vault-exchange", "payment-rebate"])
const DIFFICULTY_SUFFIX = /\s+(初级|中级|高级\+?|超级\+?|地狱级|地狱|超級|超級\+|极级|Lv\s*\d+)$/i

// //// 区分限时活动, 领主战, 日常和内购 [@x380kkm 2026-09-26] ////
export function activitySection(activity) {
    const id = String(activity.activity_id)
    if (id.startsWith("quest:2:")) return "boss"
    if (/^quest:(6|14):/.test(id) || /^(daily-week|daily-exp-mana):/.test(id) || activity.kind === "daily") return "daily"
    if (COMMERCE_KINDS.has(activity.kind)) return "commerce"
    return "events"
}

// //// 从关卡名称分离难度标签 [@x380kkm 2026-09-26] ////
export function questStageName(quest) {
    const name = String(quest.name || quest.quest_id || quest.activity_id)
    const suffix = quest.difficulty && ` ${quest.difficulty}`
    return suffix && name.endsWith(suffix) ? name.slice(0, -suffix.length) : name.replace(DIFFICULTY_SUFFIX, "")
}

// //// 按关卡编号区分同名领主与活动复刻 [@x380kkm 2026-09-26] ////
export function questStageKey(quest) {
    const match = /^quest:(\d+):(\d+)$/.exec(quest.activity_id || "")
    const category = Number(match?.[1] || quest.category_id)
    const id = Number(match?.[2] || quest.quest_id)
    return [2, 6, 14].includes(category) ? `${category}:${Math.floor(id / 1000)}`
        : `${category}:${quest.activity_id || ""}:${questStageName(quest)}`
}

// //// 按分类和活动归属组合关卡难度 [@x380kkm 2026-09-26] ////
export function activityGroups(items) {
    const groups = new Map()
    for (const activity of items) {
        const match = /^quest:(\d+):/.exec(activity.activity_id)
        const name = match ? questStageName(activity) : activity.name
        const key = match ? questStageKey(activity) : activity.activity_id
        if (!groups.has(key)) groups.set(key, { key, name, items: [] })
        groups.get(key).items.push(activity)
    }
    const result = [...groups.values()]
    for (const group of result) {
        const name = group.name
        if (result.some(other => other !== group && other.name === name)) {
            for (const other of result.filter(item => item.name === name)) other.name += ` (${other.key})`
        }
        group.items.sort((a, b) => a.activity_id.localeCompare(b.activity_id, "en", { numeric: true }))
    }
    return result.sort((a, b) => a.name.localeCompare(b.name, "zh-CN"))
}

function fillOptions(select, entries, value) {
    select.replaceChildren()
    for (const [id, label] of entries) {
        const option = document.createElement("option")
        option.value = id
        option.textContent = label
        select.append(option)
    }
    select.value = value || entries[0]?.[0] || ""
    select.disabled = entries.length === 0
}

// //// 呈现独立活动分类与关卡难度下拉框 [@x380kkm 2026-09-26] ////
export function renderActivityNavigation(state, elements, actions, items) {
    const select = elements.activityGroup
    if (!select) return
    const groups = activityGroups(items)
    const selected = groups.find(group => group.items.some(item => item.activity_id === state.selectedId))
        ?? groups.find(group => group.key === state.groupId) ?? groups[0]
    const section = state.section || "events"
    for (const tab of elements.activitySections) {
        const active = tab.dataset.activitySection === section
        tab.setAttribute("aria-selected", String(active))
        tab.tabIndex = active ? 0 : -1
        tab.onclick = () => actions.runAction(tab, () => actions.setActivitySection(tab.dataset.activitySection))
        tab.onkeydown = event => {
            const index = elements.activitySections.indexOf(tab)
            const next = event.key === "ArrowRight" ? index + 1 : event.key === "ArrowLeft" ? index - 1
                : event.key === "Home" ? 0 : event.key === "End" ? elements.activitySections.length - 1 : null
            if (next === null) return
            event.preventDefault()
            const target = elements.activitySections[(next + elements.activitySections.length) % elements.activitySections.length]
            target.focus()
            actions.runAction(target, () => actions.setActivitySection(target.dataset.activitySection))
        }
    }
    const calendar = section === "events"
    elements.activityCalendar.hidden = !calendar
    elements.activityCalendarToolbar.hidden = !calendar
    elements.activityCatalogList.hidden = section === "boss" || section === "daily"
    fillOptions(select, groups.map(group => [group.key, group.name]), selected?.key)
    const choices = selected?.items || []
    fillOptions(elements.activityDifficulty, choices.map(item => [item.activity_id,
        DIFFICULTY_SUFFIX.exec(item.name)?.[1] || (item.kind === "quest" ? item.name : "开放与轮换规则")]),
        choices.some(item => item.activity_id === state.selectedId) ? state.selectedId : choices[0]?.activity_id)
    elements.activityDifficultyLabel.hidden = choices.length === 1 && choices[0]?.kind !== "quest"
    state.groupId = selected?.key || null
    if (section !== "events" && state.selectedId !== elements.activityDifficulty.value) {
        actions.selectActivity(elements.activityDifficulty.value || null)
    }
    select.onchange = () => {
        const group = groups.find(candidate => candidate.key === select.value)
        if (group) actions.selectActivity(group.items[0].activity_id)
        actions.render()
    }
    elements.activityDifficulty.onchange = () => actions.selectActivity(elements.activityDifficulty.value)
}
// //// /呈现独立活动分类与关卡难度下拉框 ////
