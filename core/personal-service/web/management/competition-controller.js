// audience: internal
// # personal-service-competition-controller
// 管理操作绑定选定存档, 活动期别和参与轮次. 发奖和全服统计分别提交, 切换存档后忽略旧响应.

import { competitionFamilies, competitionPeriod, competitionSummary, selectedCompetitionTiers, renderCompetitionRewards } from "./competition-views.js"

// //// 管理竞赛目录, 开放状态和人工奖励结算 [@x380kkm 2026-09-28] ////
export function createCompetitionController({ requestApi, runAction, onSelect, onChange, document = globalThis.document, confirm = globalThis.confirm }) {
    const keys = ["save", "family", "search", "event", "state", "detail", "title", "meta", "summary", "policy", "policySave", "policyState", "open", "close", "always", "global", "globalForm", "hp", "kills", "statsSave", "rankingOptions", "tier", "rushOptions", "rank", "preview", "settle", "refresh"]
    const controls = Object.fromEntries(keys.map(key => [key, document.querySelector(`#competition-${key.replace(/[A-Z]/g, letter => `-${letter.toLowerCase()}`)}`)]))
    let slots = []
    let entries = []
    let selected = null
    let generation = 0
    let busy = false
    let active = false

    function option(value, text) {
        const node = document.createElement("option")
        node.value = String(value)
        node.textContent = text
        return node
    }

    function updateControls() {
        for (const key of ["save", "family", "search", "event", "refresh"]) controls[key].disabled = busy
        for (const key of ["policy", "policySave", "open", "close", "always", "statsSave", "hp", "kills", "tier", "rank"]) controls[key].disabled = busy || !selected
        const eligible = selected && (selected.family === "raid" || selected.has_record)
        controls.settle.disabled = busy || !eligible || (selected.family !== "raid" && selected.received)
            || !selectedCompetitionTiers(selected, controls.tier.value, controls.rank.value).length
    }

    function showRewards() {
        if (selected) renderCompetitionRewards(document, controls.preview, selectedCompetitionTiers(selected, controls.tier.value, controls.rank.value))
        updateControls()
    }

    function showPolicy() {
        controls.policyState.textContent = controls.policy.value === "manual"
            ? "手动发奖: 游戏内保留成绩和已领记录, 由本页结算奖励."
            : "游戏内领取: 按活动原有领奖流程发奖, 本页也可提前人工结算."
    }

    function selectEntry() {
        selected = entries.find(entry => entry.activity_id === controls.event.value) ?? null
        controls.detail.hidden = !selected
        if (!selected) { onSelect?.(null, null); updateControls(); return }
        controls.title.textContent = selected.name
        controls.meta.textContent = competitionPeriod(selected)
        controls.summary.textContent = competitionSummary(selected)
        controls.policy.value = selected.reward_mode ?? "automatic"
        showPolicy()
        controls.rankingOptions.hidden = selected.family !== "ranking"
        controls.rushOptions.hidden = selected.family !== "rush"
        controls.global.hidden = selected.family !== "raid"
        controls.tier.replaceChildren(option("", "按当前成绩计算"))
        for (const tier of selected.tiers ?? []) controls.tier.append(option(tier.id, tier.label ?? `档位 ${tier.id}`))
        controls.tier.value = ""
        controls.rank.value = String(selected.suggested_rank_number ?? 1)
        controls.hp.value = String(selected.global_state?.hp_percentage ?? 100)
        controls.kills.value = String(selected.global_state?.total_kill_count ?? 0)
        controls.settle.textContent = selected.received && selected.family !== "raid" ? "本轮已发放" : "结算并发放奖励"
        showRewards()
        onSelect?.(selected, slots.find(slot => String(slot.id) === controls.save.value))
    }

    function filterEntries() {
        const previous = controls.event.value
        const query = controls.search.value.trim().toLocaleLowerCase()
        const visible = entries.filter(entry => (!controls.family.value || entry.family === controls.family.value)
            && `${entry.name} ${entry.activity_id}`.toLocaleLowerCase().includes(query))
        controls.event.replaceChildren(option("", visible.length ? "选择活动" : "没有匹配的竞赛活动"))
        for (const entry of visible) controls.event.append(option(entry.activity_id, `${competitionFamilies[entry.family] ?? entry.family} / ${entry.name}`))
        controls.event.value = visible.some(entry => entry.activity_id === previous) ? previous : visible[0]?.activity_id ?? ""
        selectEntry()
    }

    async function load() {
        const requestGeneration = ++generation
        const slotId = controls.save.value
        entries = []
        selected = null
        controls.detail.hidden = true
        updateControls()
        onSelect?.(null, null)
        if (!slotId) { entries = []; filterEntries(); controls.state.textContent = "导入或创建存档后可管理竞赛奖励."; return }
        controls.state.textContent = "正在读取竞赛成绩和奖励."
        try {
            const payload = await requestApi(`/v1/local-saves/${slotId}/competitions`)
            if (requestGeneration !== generation || slotId !== controls.save.value) return
            entries = payload.entries
            filterEntries()
            controls.state.textContent = `${payload.slot_name}, 共 ${entries.length} 个竞赛活动. 发奖仅针对所选存档.`
        } catch (error) {
            if (requestGeneration !== generation) return
            entries = []
            filterEntries()
            controls.state.textContent = "竞赛信息读取失败, 请刷新后操作."
            throw error
        }
    }

    function context() {
        if (!selected || !controls.save.value) throw new Error("请先选择存档和竞赛活动.")
        return { entry: selected, slot: slots.find(slot => String(slot.id) === controls.save.value), path: `/v1/local-saves/${controls.save.value}/competitions/${encodeURIComponent(selected.activity_id)}` }
    }

    async function act(control, action) {
        if (busy || !selected) return
        const current = context()
        busy = true
        updateControls()
        try {
            await runAction(control, async () => {
                const message = await action(current)
                if (message) { await load(); await onChange?.(); return message }
            })
        } finally { busy = false; updateControls() }
    }

    function scope(entry) {
        return { expected_round: entry.participation_round, expected_period_start_ms: entry.period_start_ms ?? null }
    }

    async function settle({ entry, slot, path }) {
        const body = scope(entry)
        if (entry.family === "ranking" && controls.tier.value !== "") body.tier_id = Number(controls.tier.value)
        if (entry.family === "rush") {
            const rank = Number(controls.rank.value)
            if (!Number.isSafeInteger(rank) || rank < 1) throw new Error("排名必须是正整数.")
            body.rank_number = rank
        }
        const tiers = selectedCompetitionTiers(entry, controls.tier.value, controls.rank.value)
        if (!tiers.length || (entry.family !== "raid" && !entry.has_record)) return
        const rewards = tiers.map(tier => tier.label ?? `档位 ${tier.id}`).join(", ")
        if (!confirm(`向存档 "${slot.name}" 发放 "${entry.name}" 第 ${entry.participation_round + 1} 轮奖励?\n${rewards}\n奖励和统计结果会记录, 同一轮已发奖励保持原样.`)) return
        const result = await requestApi(`${path}/settle`, { method: "POST", body })
        return { delivered: "竞赛奖励已发放, 可在记录和游戏邮箱查看.", already_received: "此轮奖励已经发放, 已保留原领取记录.", no_reward: "当前成绩或全服统计没有满足新的奖励档位." }[result.status] ?? "竞赛结算已完成."
    }

    controls.event.addEventListener("change", selectEntry)
    controls.family.addEventListener("change", filterEntries)
    controls.search.addEventListener("input", filterEntries)
    controls.save.addEventListener("change", () => runAction(controls.refresh, load))
    controls.refresh.addEventListener("click", () => runAction(controls.refresh, load))
    controls.tier.addEventListener("change", showRewards)
    controls.rank.addEventListener("input", showRewards)
    controls.policy.addEventListener("change", showPolicy)
    controls.settle.addEventListener("click", () => act(controls.settle, settle))
    controls.policySave.addEventListener("click", () => act(controls.policySave, async ({ path }) => {
        await requestApi(`${path}/reward-mode`, { method: "PUT", body: { mode: controls.policy.value } })
        return "发奖方式已保存."
    }))
    for (const [key, endpoint, method, body, message] of [
        ["open", "temporary-open", "POST", {}, "活动已开放 24 小时."],
        ["close", "close", "POST", {}, "活动已关闭, 已有成绩和奖励记录保留."],
        ["always", "mode", "PUT", { mode: "always" }, "活动已设为始终开放."],
    ]) controls[key].addEventListener("click", () => act(controls[key], async ({ entry }) => {
        await requestApi(`/v1/activities/${encodeURIComponent(entry.activity_id)}/${endpoint}`, { method, body })
        return message
    }))
    controls.globalForm.addEventListener("submit", event => {
        event.preventDefault()
        return act(controls.statsSave, async ({ entry, path }) => {
            const hp = Number(controls.hp.value)
            const kills = Number(controls.kills.value)
            if (!Number.isInteger(hp) || hp < 0 || hp > 100 || !Number.isSafeInteger(kills) || kills < 0) throw new Error("请填写 0 到 100 的血量百分比及非负整数讨伐数.")
            if (!confirm(`保存 "${entry.name}" 当前期别的全服数据?\n总讨伐 ${kills} 次, 剩余血量 ${hp}%.\n本机所有存档共享这些统计值. 各存档按其发奖方式领取奖励.`)) return
            await requestApi(`${path}/server-state`, { method: "PUT", body: { expected_period_start_ms: entry.period_start_ms ?? null, hp_percentage: hp, total_kill_count: kills } })
            return "全服统计已保存."
        })
    })
    controls.detail.hidden = true
    updateControls()
    return {
        refresh: load,
        activate() { active = true; return runAction(controls.refresh, load) },
        setSaves(values, activeSlotId) {
            const previous = controls.save.value
            slots = values
            controls.save.replaceChildren(option("", "选择存档"))
            for (const slot of slots) controls.save.append(option(slot.id, `${slot.name} (#${slot.id})`))
            controls.save.value = slots.some(slot => String(slot.id) === previous) ? previous : String(slots.find(slot => slot.id === activeSlotId)?.id ?? slots[0]?.id ?? "")
            generation += 1
            entries = []
            selected = null
            controls.detail.hidden = true
            updateControls()
            return active ? runAction(controls.refresh, load) : undefined
        },
    }
}
