// audience: external
// # personal-service-activity-rescue
// 每个关卡独立保存铃铛开关, 开放状态由服务端当前时窗和存档资格计算.

import { questStageName, questStageKey } from "/manage/activity-navigation.js"

// //// 管理活动内的协战关卡与铃铛开关 [@x380kkm 2026-09-26] ////
export function createActivityRescue({ panel, requestApi, runAction, onChange }) {
    const stage = panel.querySelector("#activity-rescue-stage")
    const difficulty = panel.querySelector("#activity-rescue-difficulty")
    const enabled = panel.querySelector("#activity-rescue-enabled")
    const status = panel.querySelector("#activity-rescue-state")
    let generation = 0
    let quests = []
    let settings = null
    let busy = false

    function key(quest) { return `${quest.category_id}:${quest.quest_id}` }
    function groupKey(quest) { return questStageKey(quest) }
    function options(select, entries) {
        const previous = select.value
        select.replaceChildren()
        for (const [value, label] of entries) {
            const option = panel.ownerDocument.createElement("option")
            option.value = value
            option.textContent = label
            select.append(option)
        }
        select.value = entries.some(([value]) => value === previous) ? previous : entries[0]?.[0] || ""
    }

    function renderStatus() {
        const quest = quests.find(item => key(item) === difficulty.value)
        const selected = quest && settings.quests.some(item => key(item) === key(quest))
        enabled.checked = Boolean(selected)
        enabled.disabled = busy || !quest || (!quest.is_open && !selected)
        stage.disabled = busy
        difficulty.disabled = busy
        status.textContent = !quest ? "此活动没有协战关卡."
            : !quest.is_open ? "关卡关闭或存档尚未解锁, 铃铛暂停."
            : !selected ? "铃铛已关闭."
            : settings.enabled ? "铃铛已开启, 跟随关卡开放时间生效."
            : "此关卡已选中, 救援总开关已关闭."
    }

    function renderDifficulties() {
        options(difficulty, quests.filter(quest => groupKey(quest) === stage.value)
            .map(quest => [key(quest), `${quest.difficulty} (${quest.is_open ? "可进入" : "关闭或未解锁"})`]))
        renderStatus()
    }

    async function selectActivity(activity) {
        const current = ++generation
        panel.hidden = true
        if (!activity) return
        const [catalog, policy] = await Promise.all([
            requestApi("/v1/rescue-settings/quests"), requestApi("/v1/rescue-settings"),
        ])
        if (current !== generation) return
        quests = catalog.quests.filter(quest => activity.activity_id === `quest:${key(quest)}`
            || quest.activity_id === activity.activity_id)
        settings = policy
        panel.hidden = quests.length === 0
        options(stage, [...new Map(quests.map(quest => [groupKey(quest), questStageName(quest)]))])
        renderDifficulties()
    }

    enabled.addEventListener("change", () => runAction(null, async () => {
        const quest = quests.find(item => key(item) === difficulty.value)
        if (!quest || busy) return
        const current = generation
        const checked = enabled.checked
        busy = true
        renderStatus()
        try {
            const result = await requestApi(`/v1/rescue-settings/quests/${quest.category_id}/${quest.quest_id}`, {
                method: "PUT", body: { enabled: checked },
            })
            if (current === generation) settings = result
            await onChange?.({ quest, checked })
            return checked ? "此关卡铃铛已开启." : "此关卡铃铛已关闭."
        } finally {
            busy = false
            renderStatus()
        }
    }))
    stage.addEventListener("change", renderDifficulties)
    difficulty.addEventListener("change", renderStatus)
    return { selectActivity }
}
// //// /管理活动内的协战关卡与铃铛开关 ////
